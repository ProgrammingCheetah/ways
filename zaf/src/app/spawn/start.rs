//! Step 6 of the tick: start at most one model, the longest waiting that fits
//! (docs/zaf-actions.md §5.1).
//!
//! The items are each task's `Ready` model phase, each `Failed` wait that declares an
//! `on-fail`, whose model action is the phase to start, at the size of the task (the #425
//! ruling), and each epic launch, planning or the challenger, charged XS (#419, #427), its
//! weight how long its stage has waited ([`EpicCandidate::since`]).
//! [`plan`] orders them by weight, how long each has been ready (`since`, the earliest
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
use super::tick::EpicCandidate;
use crate::app::actions::{ActionKind, ActionName, ListedPhase, Standing, derive_standing};
use crate::domain::{Budget, Fit, MEBIBYTES_PER_UNIT, RoleName, TaskSize, TicketId, Units};

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

/// The size an epic launch is charged: planning and the challenger are XS (the rulings on #419
/// and #427).
pub const EPIC_SIZE: TaskSize = TaskSize::XS;

/// The one model a tick plans to start: a task's phase or an epic's launch,
/// `PlannedModel { Phase { task, action, units } | Epic { epic, role, units } }`.
///
/// It is what `PlannedPhase` was before the epic launches joined the order: `plan()` returns at
/// most one, each epic launch an [`EpicCandidate { epic, role, since }`](EpicCandidate).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedModel {
    /// A model phase of a task.
    Phase {
        /// The task.
        task: TicketId,
        /// The model action to start.
        action: ActionName,
        /// What it costs: [`Budget::cost`] at the size of the task, in whole units.
        units: u32,
    },
    /// The launch of an epic's planning instance or challenger.
    Epic {
        /// The epic.
        epic: TicketId,
        /// The role to launch.
        role: RoleName,
        /// What it costs: [`Budget::cost`] at [`EPIC_SIZE`], in whole units.
        units: u32,
    },
}

impl PlannedModel {
    /// What it holds of the budget while it runs, in whole units.
    pub const fn units(&self) -> u32 {
        match self {
            Self::Phase { units, .. } | Self::Epic { units, .. } => *units,
        }
    }

    /// The model as the outcome records it: an epic's launch is recorded under the epic's id,
    /// its role standing for the action.
    fn started(self) -> Started {
        match self {
            Self::Phase {
                task,
                action,
                units,
            } => Started {
                task,
                action,
                units,
            },
            Self::Epic { epic, role, units } => Started {
                task: epic,
                action: ActionName::from(role.as_str()),
                units,
            },
        }
    }
}

/// The whole units of `cost`; a cost from the size table is always whole.
fn whole_units(cost: Units) -> u32 {
    u32::try_from(cost.mebibytes().get() / MEBIBYTES_PER_UNIT).unwrap_or(u32::MAX)
}

/// The model to start this tick, if any,
/// `plan(standings, epics, in_use, budget) -> Option<PlannedModel>`.
///
/// The `Ready` model phases, the failed waits with an `on-fail` and the epic launches are one
/// order (the ruling on #427): each weighed by how long it has waited, the earliest `since`
/// first, an epic's being how long its stage has waited. Items with the same `since` keep the
/// order of `standings`, then of `epics`. The first for which [`Budget::fits`] is [`Fit::Fits`]
/// at its size, that of its task or [`EPIC_SIZE`], is returned with `units` its cost at that
/// size; none when nothing fits. An item that does not fit and has been ready [`STARVE_AFTER`]
/// or more before `in_use.now` is starved: the walk stops there and returns none, so no smaller
/// item starts until it fits. A younger one that does not fit is passed.
pub fn plan(
    standings: &[TaskStanding],
    epics: &[EpicCandidate],
    in_use: &InUse,
    budget: &Budget,
) -> Option<PlannedModel> {
    let phases = standings.iter().filter_map(|standing| {
        let (action, since) = standing.model_item()?;
        let model = PlannedModel::Phase {
            task: standing.task,
            action,
            units: whole_units(budget.cost(standing.size)),
        };
        Some((since, standing.size, model))
    });
    let launches = epics.iter().map(|candidate| {
        let model = PlannedModel::Epic {
            epic: candidate.epic,
            role: candidate.role.clone(),
            units: whole_units(budget.cost(EPIC_SIZE)),
        };
        (candidate.since, EPIC_SIZE, model)
    });
    let mut items: Vec<(SystemTime, TaskSize, PlannedModel)> = phases.chain(launches).collect();
    items.sort_by_key(|(since, _, _)| *since);
    for (since, size, model) in items {
        let fit = budget.fits(
            in_use.units,
            in_use.e_live,
            size,
            in_use.running_here,
            in_use.slots,
        );
        if fit == Fit::Fits {
            return Some(model);
        }
        if starved(since, in_use.now) {
            return None;
        }
    }
    None
}

/// Whether an item ready since `since` is starved at `now`: ready [`STARVE_AFTER`] or more. A
/// `since` after `now`, a clock that stepped back, is not.
fn starved(since: SystemTime, now: SystemTime) -> bool {
    now.duration_since(since)
        .is_ok_and(|ready_for| ready_for >= STARVE_AFTER)
}

/// Plans the tick's model and starts it: `start(&planned)` launches it detached and returns once
/// it is launched, never when it ends. The started model is returned as the outcome records it;
/// none, with `start` never called, when nothing fits. A failed launch is `start`'s error, and
/// nothing is recorded as started.
pub fn start_planned<E>(
    standings: &[TaskStanding],
    epics: &[EpicCandidate],
    in_use: &InUse,
    budget: &Budget,
    start: &mut dyn FnMut(&PlannedModel) -> Result<(), E>,
) -> Result<Option<Started>, E> {
    let Some(planned) = plan(standings, epics, in_use, budget) else {
        return Ok(None);
    };
    start(&planned)?;
    Ok(Some(planned.started()))
}

/// The outcome of a tick that did step 6 alone: [`TickOutcome::started`] filled, the rest empty
/// for the steps that fill them.
pub fn start_outcome<E>(
    standings: &[TaskStanding],
    epics: &[EpicCandidate],
    in_use: &InUse,
    budget: &Budget,
    start: &mut dyn FnMut(&PlannedModel) -> Result<(), E>,
) -> Result<TickOutcome, E> {
    Ok(TickOutcome {
        started: start_planned(standings, epics, in_use, budget, start)?,
        ..TickOutcome::default()
    })
}

#[cfg(test)]
mod tests;
