//! Step 6 of the tick: start at most one model phase, the longest ready that fits
//! (docs/zaf-actions.md §5.1).
//!
//! The items are each task's `Ready` model phase and each `Failed` wait that declares an
//! `on-fail`, whose model action is the phase to start, at the size of the task (the #425
//! ruling). [`plan`] orders them by weight, how long each has been ready (`since`, the earliest
//! first, #421 rulings 19 and 20), and returns the first that [`Budget::fits`] accepts at its
//! size, with `units` its [`Budget::cost`] (#421 ruling 14). An item passed for
//! [`Fit::NoBudget`] or [`Fit::NoFreeSlot`] keeps its weight: nothing is stored, so the next
//! tick orders it by the same `since` and it comes first again once it fits. One that has been
//! ready for [`STARVE_AFTER`] and still does not fit is starved (the starvation rule of #419,
//! #421 ruling 20): no other model phase starts while it is, so the budget frees up for it.
//!
//! [`plan`] is pure: the caller hands in the standings, what the fit rule reads of the machine
//! ([`InUse`]) and the budget. [`start_planned`] hands the planned phase to a `start` the caller
//! gives, which launches it detached (`zaf spawn phase`) and never waits for it, and records it
//! in [`TickOutcome::started`]. A wait, a `zaf`-kind action and a failed wait with no `on-fail`
//! are other steps' ([`super::inline`], [`super::question`]).

use std::time::{Duration, SystemTime};

use super::inline::TickTask;
use super::outcome::{Started, TickOutcome};
use crate::app::actions::{ActionKind, ActionName, ListedPhase, Standing, derive_standing};
use crate::domain::{Budget, Fit, MEBIBYTES_PER_UNIT, TaskSize, TicketId, Units};

/// How long a model phase may be ready, passed over, before it holds the budget: 30 minutes,
/// the starvation rule of #419. A constant and not a setting, since the budget it holds is the
/// machine's.
pub const STARVE_AFTER: Duration = Duration::from_secs(30 * 60);

/// One task as the plan reads it: its size, its type's phase list and where it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStanding {
    /// The task.
    pub task: TicketId,
    /// Its size, from its ticket: what a model phase of it costs.
    pub size: TaskSize,
    /// Its type's phase list, which says the kind of each action.
    pub list: Vec<ListedPhase>,
    /// Where it stands this tick.
    pub standing: Standing,
}

impl TaskStanding {
    /// The standing of `task`, derived from its list and facts, at `size`.
    pub fn of(task: &TickTask, size: TaskSize) -> Self {
        Self {
            task: task.task,
            size,
            list: task.list.clone(),
            standing: derive_standing(&task.list, &task.facts),
        }
    }

    /// The model action the task may start and how long it has waited: its `Ready` action when
    /// that is model-kind, or the `on-fail` of its failed wait.
    fn model_item(&self) -> Option<(ActionName, SystemTime)> {
        match &self.standing {
            Standing::Ready { action, since } => self
                .list
                .iter()
                .any(|phase| phase.action == *action && phase.kind == ActionKind::Model)
                .then(|| (action.clone(), *since)),
            Standing::Failed {
                on_fail: Some(on_fail),
                since,
                ..
            } => Some((on_fail.clone(), *since)),
            _ => None,
        }
    }
}

/// What the fit rule reads of the machine and the project on this tick, besides the budget:
/// the time of the tick, the live use of #472, whether an E is live, and the project's running
/// count and slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InUse {
    /// The time of the tick: a phase ready for [`STARVE_AFTER`] before it is starved.
    pub now: SystemTime,
    /// The live use of the machine, `ports::live_use` of the live instances.
    pub units: Units,
    /// Whether an instance with `ZAF_SIZE=E` is live, `ports::e_is_live`.
    pub e_live: bool,
    /// The instances the project runs.
    pub running_here: usize,
    /// The project's `[spawn] slots`.
    pub slots: usize,
}

/// The model phase a tick plans to start: `PlannedPhase { task, action, units }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedPhase {
    /// The task.
    pub task: TicketId,
    /// The model action to start.
    pub action: ActionName,
    /// What it costs: [`Budget::cost`] at the size of the task, in whole units.
    pub units: u32,
}

/// The whole units of `cost`; a cost from the size table is always whole.
fn whole_units(cost: Units) -> u32 {
    u32::try_from(cost.mebibytes().get() / MEBIBYTES_PER_UNIT).unwrap_or(u32::MAX)
}

/// The model phase to start this tick, if any,
/// `plan(standings, in_use, budget) -> Option<PlannedPhase { task, action, units }>`.
///
/// The `Ready` model phases and the failed waits with an `on-fail` are ordered by how long each
/// has been ready, the earliest `since` first; items with the same `since` keep the order of
/// `standings`. The first for which [`Budget::fits`] is [`Fit::Fits`] at the size of its task is
/// returned with `units` its cost at that size; none when nothing fits. An item that does not
/// fit and has been ready [`STARVE_AFTER`] or more before `in_use.now` is starved: the walk
/// stops there and returns none, so no smaller phase starts until it fits. A younger one that
/// does not fit is passed.
pub fn plan(standings: &[TaskStanding], in_use: &InUse, budget: &Budget) -> Option<PlannedPhase> {
    let mut items: Vec<(&TaskStanding, ActionName, SystemTime)> = standings
        .iter()
        .filter_map(|standing| {
            let (action, since) = standing.model_item()?;
            Some((standing, action, since))
        })
        .collect();
    items.sort_by_key(|(_, _, since)| *since);
    for (standing, action, since) in items {
        let fit = budget.fits(
            in_use.units,
            in_use.e_live,
            standing.size,
            in_use.running_here,
            in_use.slots,
        );
        if fit == Fit::Fits {
            return Some(PlannedPhase {
                task: standing.task,
                action,
                units: whole_units(budget.cost(standing.size)),
            });
        }
        if starved(since, in_use.now) {
            return None;
        }
    }
    None
}

/// Whether a phase ready since `since` is starved at `now`: ready [`STARVE_AFTER`] or more. A
/// `since` after `now`, a clock that stepped back, is not.
fn starved(since: SystemTime, now: SystemTime) -> bool {
    now.duration_since(since)
        .is_ok_and(|ready_for| ready_for >= STARVE_AFTER)
}

/// Plans the tick's model phase and starts it: `start(&planned)` launches it detached and
/// returns once it is launched, never when it ends. The started phase is returned as the
/// outcome records it; none, with `start` never called, when nothing fits. A failed launch is
/// `start`'s error, and nothing is recorded as started.
pub fn start_planned<E>(
    standings: &[TaskStanding],
    in_use: &InUse,
    budget: &Budget,
    start: &mut dyn FnMut(&PlannedPhase) -> Result<(), E>,
) -> Result<Option<Started>, E> {
    let Some(planned) = plan(standings, in_use, budget) else {
        return Ok(None);
    };
    start(&planned)?;
    Ok(Some(Started {
        task: planned.task,
        action: planned.action,
        units: planned.units,
    }))
}

/// The outcome of a tick that did step 6 alone: [`TickOutcome::started`] filled, the rest empty
/// for the steps that fill them.
pub fn start_outcome<E>(
    standings: &[TaskStanding],
    in_use: &InUse,
    budget: &Budget,
    start: &mut dyn FnMut(&PlannedPhase) -> Result<(), E>,
) -> Result<TickOutcome, E> {
    Ok(TickOutcome {
        started: start_planned(standings, in_use, budget, start)?,
        ..TickOutcome::default()
    })
}

#[cfg(test)]
mod tests;
