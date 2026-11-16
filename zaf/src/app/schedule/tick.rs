//! The tick's phase steps over the core's placement, and the one gate every phase start passes
//! (docs/zaf-actions.md §5.1).
//!
//! [`may_start`] is the gate: it asks the core where a ticket sits ([`place`]) and lets an action
//! start only when the ticket is in the lane at that action, its wait has not failed with nowhere
//! to go, and the action is not getting nowhere ([`gets_nowhere`]). The tick and `zaf spawn phase`
//! both start through it, so the two never disagree: a refusal carries the reason in its own
//! words ([`Refusal::reason`]).
//!
//! [`tick`] runs steps 3 to 6 on what [`super::gather::gather`] read: it places every ticket once
//! ([`Placement::of`]), names each one its epic or a question holds back ([`Held`]), then walks the
//! lane, lowest ticket first. Each entry the gate lets through goes to the step of its kind: step 3
//! runs a `zaf`-kind action inline ([`run_zaf_actions`]), step 4 launches a wait
//! ([`launch_waits`]), step 6 starts at most one model phase ([`start_planned`]). Step 3 runs at
//! most one merge that goes through (R29): once one exited 0 the base has moved, so every other
//! merge of the lane waits for the next tick ([`run_one_merge`]). An entry the
//! gate holds is step 5's: a failed wait with no `on-fail` raises its question, and an action
//! getting nowhere raises one too; neither runs. Step 5 also asks about the waiting tickets and
//! the open epics ([`waiting_causes`]): an overdue check wait, a dead dependency, a cycle, an
//! unchallenged epic. Every question goes through [`super::ask::ask`], each named with its cause
//! in [`TickOutcome::asked`]. The model phase is planned against the slots that the project's live model runs hold,
//! counted from the run folders whether or not their tickets read ([`Gathered::model_runs`]),
//! beside the launches of the plan: a dead process, or a wait, holds none. A failed model start
//! records nothing as started and tries no other entry; it is named in [`PhaseTick::failed`].
//!
//! Nothing is stored: each tick places every ticket again from the facts, and hands the placement
//! back ([`PhaseTick::placement`]) for the scheduler to keep as a cache, never to decide from.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::SystemTime;

use super::ask::{Cause, FailedWait, Hold, ask_outcome, hold_of, waiting_causes};
use super::gather::{Beside, Gathered};
use crate::app::actions::{ActionKind, ActionName, ListedPhase};
use crate::app::spawn::inline::run_zaf_actions;
use crate::app::spawn::outcome::{Ran, TickOutcome};
use crate::app::spawn::start::{InUse, PlannedModel, start_planned};
use crate::app::spawn::waits::launch_waits;
use crate::domain::schedule::{
    Failure, LaneEntry, Phase, PhaseEnd, PhaseStatus, Place, Placement, RunRecord, TicketFacts,
    WaitingOn, current_phase, gets_nowhere, latest_counted_run, place,
};
use crate::domain::{Budget, TicketId, Units};
use crate::ports::Board;

/// Why an action may not start now, as the gate found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The ticket is not among the open tickets with a phase list: done, or closed.
    NotOpen,
    /// A phase run of the ticket is live, running this action.
    Running(ActionName),
    /// Every phase of the ticket is done, or its pull request is merged.
    Done,
    /// The ticket waits: its current phase, and the reasons in words, the first that holds first.
    Waiting {
        /// The ticket's current phase.
        phase: ActionName,
        /// What it waits on, in words.
        reasons: Vec<String>,
    },
    /// The ticket is in the lane at another action.
    NotNext {
        /// The action it is in the lane at.
        next: ActionName,
        /// The phase that failed, when `next` is its `on-fail`.
        failed: Option<ActionName>,
    },
    /// The ticket's wait failed and names no `on-fail`, or failed on something its `on-fail`
    /// does not answer ([`Failure::for_on_fail`]): the Engineer is asked.
    FailedWait(Box<Failure>),
    /// The action got nowhere [`crate::domain::schedule::NOWHERE_LIMIT`] times at one head.
    Nowhere(Box<Hold>),
}

impl Refusal {
    /// The refusal in words, as `zaf spawn phase` gives it for `action` on `task`.
    pub fn reason(&self, task: TicketId, action: &str) -> String {
        let not_current = |why: String| format!("{action} is not {task}'s current phase: {why}");
        match self {
            Self::NotOpen => not_current(format!("{task} is not open, so it is done or closed")),
            Self::Running(running) => format!("{task}'s phase {running} already runs"),
            Self::Done => not_current("every phase is done".to_owned()),
            Self::Waiting { phase, reasons } => {
                not_current(format!("{phase} waits: {}", reasons.join("; ")))
            }
            Self::NotNext {
                next,
                failed: Some(failed),
            } => not_current(format!("{failed} failed, {next} is next")),
            Self::NotNext { next, failed: None } => not_current(format!("{next} is ready")),
            Self::FailedWait(failure) if failure.for_on_fail() => {
                not_current(format!("{action} failed and names no on-fail action"))
            }
            Self::FailedWait(_) => not_current(format!(
                "{action} failed on something no on-fail action answers"
            )),
            Self::Nowhere(hold) => format!(
                "{action} is not started on {task}: it got nowhere {} times {}; it starts again \
                 once the question on it is ruled",
                hold.codes.len(),
                hold.at()
            ),
        }
    }
}

/// The kind of `action` in `list`: the kind it declares, or a model's for an `on-fail` action the
/// list does not hold, which is a model action by its declaration.
pub fn kind_of(list: &[ListedPhase], action: &ActionName) -> ActionKind {
    list.iter()
        .find(|listed| &listed.action == action)
        .map_or(ActionKind::Model, |listed| listed.kind)
}

/// The gate: whether `action` may start on the ticket of `facts` at `now`, and as what kind.
///
/// It may when the core places the ticket in the lane at `action` ([`place`]), the ticket's
/// current phase is no wait that failed with no `on-fail` to answer it (that one is the
/// Engineer's: none named, or a failure the `on-fail` does not answer), and
/// `action` is not getting nowhere ([`gets_nowhere`]). Anything else is the [`Refusal`] that says
/// why. `beside` gives the kinds of the list's actions and the questions open on the epic, which
/// name a wait.
pub fn may_start(
    facts: &TicketFacts,
    beside: &Beside,
    action: &ActionName,
    now: SystemTime,
) -> Result<ActionKind, Refusal> {
    let current = current_phase(facts);
    match place(facts, now) {
        Place::Running(run) => Err(Refusal::Running(run.phase)),
        Place::Done => Err(Refusal::Done),
        Place::Waiting(on) => Err(Refusal::Waiting {
            phase: current.map_or_else(|| action.clone(), |current| current.phase.name.clone()),
            reasons: waiting_words(facts, beside, &on),
        }),
        Place::InLane { next, .. } => {
            let failed = current.as_ref().and_then(|current| match &current.status {
                PhaseStatus::Failed(failure) => Some((current.phase, failure)),
                PhaseStatus::Done | PhaseStatus::Open | PhaseStatus::Awaiting(_) => None,
            });
            if &next != action {
                return Err(Refusal::NotNext {
                    failed: failed
                        .filter(|(phase, _)| phase.name != next)
                        .map(|(phase, _)| phase.name.clone()),
                    next,
                });
            }
            if let Some((phase, failure)) = failed
                && (phase.on_fail.is_none() || !failure.for_on_fail())
                && kind_of(&beside.list, &phase.name) == ActionKind::Wait
            {
                return Err(Refusal::FailedWait(Box::new(failure.clone())));
            }
            if gets_nowhere(facts, action)
                && let Some(hold) = hold_of(facts, action)
            {
                return Err(Refusal::Nowhere(Box::new(hold)));
            }
            Ok(kind_of(&beside.list, action))
        }
    }
}

/// The ticket's current phase when it has failed at the head: the phase, what it failed on, and
/// the run that failed it (its latest counted run at the head; none when a check failed it
/// alone). What `zaf brief --phase <on-fail>` says the `on-fail` was started for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedPhase<'a> {
    /// The phase that failed.
    pub phase: &'a Phase,
    /// What it failed on.
    pub failure: Failure,
    /// The run that failed it; none when the checks alone did.
    pub run: Option<&'a RunRecord>,
}

/// The current phase of `facts` when it has failed ([`FailedPhase`]); none when it has not, or
/// every phase is done.
pub fn failed_phase(facts: &TicketFacts) -> Option<FailedPhase<'_>> {
    let current = current_phase(facts)?;
    let PhaseStatus::Failed(failure) = current.status else {
        return None;
    };
    Some(FailedPhase {
        phase: current.phase,
        failure,
        run: latest_counted_run(current.phase, facts),
    })
}

/// What a waiting ticket waits on, in words, the epic and the questions first in the order the
/// old gate named them: an unsigned epic, a question on the epic, a question on the ticket, an
/// unchallenged epic; then the tickets it depends on, the checks at its head, the ticket that
/// merges before it and the integration branch's checks.
fn waiting_words(facts: &TicketFacts, beside: &Beside, on: &BTreeSet<WaitingOn>) -> Vec<String> {
    let mut words: Vec<String> = held_words(facts, beside, on);
    for waiting_on in on {
        match waiting_on {
            WaitingOn::Ticket(ticket) => words.push(format!("{ticket} is not done")),
            WaitingOn::Check(_, head) => {
                words.push(format!("the checks at {head} have not ended"));
            }
            WaitingOn::Merge(ticket) => {
                words.push(format!("{ticket} is fresh from a fix and merges first"));
            }
            WaitingOn::Base(head) => words.push(format!(
                "the integration branch's checks at {head} have not ended"
            )),
            WaitingOn::Epic(_) | WaitingOn::Question(_) => {}
        }
    }
    words
}

/// The epic's and the questions' part of what a ticket waits on, in words, in the old gate's
/// order: `its epic #7 is not signed`, `a question is open on its epic #2`, `a question is open on
/// it`, `its epic #7 is not challenged yet`.
fn held_words(facts: &TicketFacts, beside: &Beside, on: &BTreeSet<WaitingOn>) -> Vec<String> {
    let mut words = Vec::new();
    let epic = facts
        .epic
        .filter(|epic| on.contains(&WaitingOn::Epic(epic.id)));
    if let Some(epic) = epic.filter(|epic| !epic.signed) {
        words.push(format!("its epic {} is not signed", epic.id));
    }
    let questions: Vec<TicketId> = on
        .iter()
        .filter_map(|waiting_on| match waiting_on {
            WaitingOn::Question(question) => Some(*question),
            _ => None,
        })
        .collect();
    if questions
        .iter()
        .any(|question| beside.on_epic.contains(question))
    {
        words.push(format!("a question is open on its epic {}", beside.epic));
    }
    if questions
        .iter()
        .any(|question| !beside.on_epic.contains(question))
    {
        words.push("a question is open on it".to_owned());
    }
    if let Some(epic) = epic.filter(|epic| epic.signed && !epic.challenged) {
        words.push(format!("its epic {} is not challenged yet", epic.id));
    }
    words
}

/// A ticket its epic or a question holds back, or a merge the tick left for the next one
/// ([`PhaseTick::held`]), with the reason the tick's block names it by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// The ticket.
    pub ticket: TicketId,
    /// Why it is held, the first reason that holds.
    pub reason: String,
}

impl Held {
    /// The line the tick's block shows: `#3 skipped: its epic #2 is not challenged yet`.
    pub fn line(&self) -> String {
        format!("{} skipped: {}", self.ticket, self.reason)
    }
}

/// Every ticket of `gathered` its epic or a question holds back, placed at `now`, lowest first,
/// each once with the first reason that holds.
pub fn held(gathered: &Gathered, now: SystemTime) -> Vec<Held> {
    let placement = Placement::of(&gathered.tickets, now);
    let mut waits: BTreeMap<TicketId, BTreeSet<WaitingOn>> = BTreeMap::new();
    for (waiting_on, tickets) in &placement.waiting {
        for ticket in tickets {
            waits.entry(*ticket).or_default().insert(waiting_on.clone());
        }
    }
    gathered
        .tickets
        .iter()
        .filter_map(|facts| {
            let on = waits.get(&facts.ticket)?;
            let beside = gathered.beside.get(&facts.ticket)?;
            let reason = held_words(facts, beside, on).into_iter().next()?;
            Some(Held {
                ticket: facts.ticket,
                reason,
            })
        })
        .collect()
}

/// What the tick acts through, each performing one act and returning once it is done or launched.
pub struct Steps<'a> {
    /// Step 3: runs `zaf do <action> <task>` and gives its exit code.
    pub run: &'a mut dyn FnMut(TicketId, &ActionName) -> u8,
    /// Step 4: launches a wait with `zaf spawn phase <task> <action>`, never waiting for it.
    pub launch: &'a mut dyn FnMut(TicketId, &ActionName) -> Result<(), String>,
    /// Step 6: starts the planned model phase with `zaf spawn phase`, never waiting for it.
    pub start: &'a mut dyn FnMut(&PlannedModel) -> Result<(), String>,
    /// Where a wait that could not launch is logged.
    pub log: &'a mut dyn FnMut(&str),
}

/// What the machine and the project hold besides the live model runs: the live use and E across
/// the machine, the project's planning and challenger launches (running, and reserved this tick),
/// and its `[spawn] slots`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Machine {
    /// The live use of the machine, `ports::live_use` of every live process carrying `ZAF_RUN`,
    /// an orphan of an ended run included.
    pub units: Units,
    /// Whether a live process carries `ZAF_SIZE=E`, `ports::e_is_live` of the same read.
    pub e_live: bool,
    /// The project's launches that hold a slot: those that run and those reserved this tick.
    pub launches_here: usize,
    /// The project's `[spawn] slots`.
    pub slots: usize,
}

/// What the tick's phase steps did: the merged outcome, every step that failed, and the tickets
/// held back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhaseTick {
    /// What steps 3 to 6 did.
    pub outcome: TickOutcome,
    /// What failed: the questions' board, the count of the live model runs, or the model phase's
    /// start. Any of them fails the tick.
    pub failed: Vec<String>,
    /// The tickets their epic or a question holds back ([`held`]), then the merges left for the
    /// next tick because one merged in this one.
    pub held: Vec<Held>,
    /// Where the tick placed every ticket before it acted: what the scheduler keeps between ticks.
    pub placement: Placement,
}

/// The lane of a placement as the gate sorts it: the entries it lets start, by the kind of their
/// action, each lowest ticket first, and the ones it holds for a question.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sorted {
    /// The entries at a `zaf`-kind action: step 3.
    pub zaf: Vec<LaneEntry>,
    /// The entries at a wait: step 4.
    pub waits: Vec<LaneEntry>,
    /// The entries at a model action: step 6.
    pub models: Vec<LaneEntry>,
    /// The waits that failed with no `on-fail`: step 5 asks.
    pub failed_waits: Vec<FailedWait>,
    /// The actions getting nowhere: step 5 asks.
    pub nowhere: Vec<Hold>,
}

/// Every lane entry of `placement`, lowest ticket first, passed through the gate ([`may_start`])
/// at `now` and sorted to the step that acts on it. An entry the gate refuses for any other reason
/// is no step's.
pub fn sort_lane(gathered: &Gathered, placement: &Placement, now: SystemTime) -> Sorted {
    let facts_of: BTreeMap<TicketId, &TicketFacts> = gathered
        .tickets
        .iter()
        .map(|facts| (facts.ticket, facts))
        .collect();
    let mut entries: Vec<&LaneEntry> = placement.lane.entries().iter().collect();
    entries.sort_by_key(|entry| entry.ticket);
    let mut sorted = Sorted::default();
    for entry in entries {
        let (Some(facts), Some(beside)) = (
            facts_of.get(&entry.ticket),
            gathered.beside.get(&entry.ticket),
        ) else {
            continue;
        };
        match may_start(facts, beside, &entry.next, now) {
            Ok(ActionKind::Zaf) => sorted.zaf.push(entry.clone()),
            Ok(ActionKind::Wait) => sorted.waits.push(entry.clone()),
            Ok(ActionKind::Model) => sorted.models.push(entry.clone()),
            Err(Refusal::FailedWait(failure)) => sorted.failed_waits.push(FailedWait {
                ticket: entry.ticket,
                action: entry.next.clone(),
                failure: *failure,
            }),
            Err(Refusal::Nowhere(hold)) => sorted.nowhere.push(*hold),
            Err(_) => {}
        }
    }
    sorted
}

/// Step 3 with one merge a tick (R29): runs each of `entries` in order ([`run_zaf_actions`]),
/// but once a merge has exited 0 it runs no other merge: the base just moved, and its checks are
/// not known until the next gather. An entry is a merge when its action is a phase of its ticket
/// that ends at `merged`. Each merge left for the next tick is named, as a held ticket is; an
/// entry that is no merge runs whatever came before, and so does a merge after one that failed,
/// which moved nothing.
fn run_one_merge(
    gathered: &Gathered,
    entries: &[LaneEntry],
    run: &mut dyn FnMut(TicketId, &ActionName) -> u8,
) -> (Vec<Ran>, Vec<Held>) {
    let merges = |entry: &LaneEntry| {
        gathered
            .tickets
            .iter()
            .filter(|facts| facts.ticket == entry.ticket)
            .flat_map(|facts| &facts.phases)
            .any(|phase| phase.name == entry.next && phase.ends.contains(&PhaseEnd::Merged))
    };
    let mut ran = Vec::new();
    let mut deferred = Vec::new();
    let mut merged: Option<TicketId> = None;
    for entry in entries {
        let a_merge = merges(entry);
        if let Some(merged) = merged.filter(|_| a_merge) {
            deferred.push(Held {
                ticket: entry.ticket,
                reason: format!(
                    "{merged} merged in this tick, so its {} waits for the next one",
                    entry.next
                ),
            });
            continue;
        }
        let done = run_zaf_actions(std::slice::from_ref(entry), run);
        if a_merge && done.iter().any(|ran| ran.exit == 0) {
            merged = Some(entry.ticket);
        }
        ran.extend(done);
    }
    (ran, deferred)
}

/// Runs steps 3 to 6 on `gathered` at `now`, in that order, and merges what each did (see the
/// module above). Step 5 raises each question under the spawner's lock of `state_dir`
/// ([`super::ask::ask`]). The model phase is planned against `machine` and the live model runs of
/// the run folders ([`Gathered::model_runs`]), within `budget`. A board that fails step 5, or a
/// lock it cannot take, is named in [`PhaseTick::failed`] and step 6 still runs; a start that
/// fails is named there too and tries no other entry, and so is a count of the live model runs
/// that failed, which starts no model.
pub fn tick(
    board: &dyn Board,
    state_dir: &Path,
    gathered: &Gathered,
    now: SystemTime,
    machine: &Machine,
    budget: &Budget,
    steps: &mut Steps<'_>,
) -> PhaseTick {
    let placement = Placement::of(&gathered.tickets, now);
    let sorted = sort_lane(gathered, &placement, now);
    let mut tick = PhaseTick {
        held: held(gathered, now),
        ..PhaseTick::default()
    };
    let (ran, deferred) = run_one_merge(gathered, &sorted.zaf, steps.run);
    tick.outcome.ran = ran;
    tick.held.extend(deferred);
    tick.outcome.waits = launch_waits(&sorted.waits, steps.launch, steps.log);
    let causes: Vec<Cause> = (sorted.failed_waits.iter().cloned().map(Cause::FailedWait))
        .chain(sorted.nowhere.iter().cloned().map(Cause::Nowhere))
        .chain(waiting_causes(gathered, &placement, now))
        .collect();
    match ask_outcome(board, state_dir, &causes) {
        Ok(asked) => {
            tick.outcome.questions = asked.questions;
            tick.outcome.asked = asked.asked;
        }
        Err(error) => tick
            .failed
            .push(format!("cannot raise a question: {error}")),
    }
    // The live model runs are counted from the run folders, so a run whose ticket could not be
    // read this tick still holds its slot; folders that cannot be counted start no model.
    let model_runs = match &gathered.model_runs {
        Ok(model_runs) => *model_runs,
        Err(error) => {
            tick.failed.push(format!(
                "no model starts: the live model runs cannot be counted: {error}"
            ));
            tick.placement = placement;
            return tick;
        }
    };
    let in_use = InUse {
        now,
        units: machine.units,
        e_live: machine.e_live,
        running_here: machine.launches_here.saturating_add(model_runs),
        slots: machine.slots,
    };
    // No epic launch is a candidate here: the tick's launcher is not wired to start one by `zaf
    // spawn phase` yet, so only the tickets' model phases are planned.
    match start_planned(&sorted.models, &[], &in_use, budget, steps.start) {
        Ok(started) => tick.outcome.started = started,
        Err(error) => tick.failed.push(error),
    }
    tick.placement = placement;
    tick
}

#[cfg(test)]
pub(crate) mod support;
#[cfg(test)]
mod tests;
