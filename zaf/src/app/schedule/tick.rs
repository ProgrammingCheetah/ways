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
//! ([`launch_waits`]), step 6 starts at most one model phase ([`start_planned`]). Step 3 lands at
//! most `[merge] per_tick` merges (D30): once that many merged, or the forge refused one after
//! another had merged, every other merge of the lane waits for the next tick
//! ([`run_merge_group`]). An entry the
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
use std::time::{Instant, SystemTime};

use super::ask::{Cause, FailedWait, Hold, ask_outcome, hold_of, is_red, waiting_causes};
use super::gather::{Beside, Gathered};
use crate::app::actions::{ActionKind, ActionName, ListedPhase};
use crate::app::spawn::inline::run_zaf_actions;
use crate::app::spawn::outcome::{Ran, TickOutcome};
use crate::app::spawn::start::{InUse, PlannedModel, start_planned};
use crate::app::spawn::waits::launch_waits;
use crate::domain::schedule::{
    Failure, GroupBase, LaneEntry, Phase, PhaseEnd, PhaseStatus, Place, Placement, RunRecord,
    TicketFacts, WaitingOn, current_phase, gets_nowhere, latest_counted_run, place, weight,
};
use crate::domain::{Budget, CommitId, TicketId, Units};
use crate::ports::{Board, PrNumber};

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
    /// Step 3: the number of the task's pull request, read before its merge runs, since a merge
    /// that lands forgets the record the number comes from; none when there is no record to read.
    pub pr_of: &'a mut dyn FnMut(TicketId) -> Option<PrNumber>,
    /// Step 3: whether the pull request of that number is merged, as the forge says now
    /// (`Forge::merged`); one it cannot find or read is not merged.
    pub merged: &'a mut dyn FnMut(PrNumber) -> bool,
    /// Step 3: the integration branch's head on the forge now, read once after each landed merge;
    /// none when it cannot be read.
    pub branch_head: &'a mut dyn FnMut() -> Option<CommitId>,
    /// Step 3: hands the merge runner the group's last commit with the group's base (D31, F30),
    /// or none when there is no such commit to judge at; it keeps them in memory for this tick
    /// only, for the re-check before the group's next merge.
    pub group: &'a mut dyn FnMut(Option<GroupBase>),
    /// Where a wait that could not launch, and each merge of step 3, is logged.
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
    /// The project's `[merge] per_tick`: the merges step 3 may land in one tick.
    pub merge_per_tick: u32,
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

/// The exit code of `zaf do` for an action that never ran.
const NOT_RUN: u8 = 125;

/// Whether `entry` is a merge: its action is a phase of its ticket that ends at `merged`.
fn is_merge(gathered: &Gathered, entry: &LaneEntry) -> bool {
    gathered
        .tickets
        .iter()
        .filter(|facts| facts.ticket == entry.ticket)
        .flat_map(|facts| &facts.phases)
        .any(|phase| phase.name == entry.next && phase.ends.contains(&PhaseEnd::Merged))
}

/// Joins to the tick's merge list the tickets held only by R30 behind a ticket that merges in it:
/// each goes right after that ticket, lowest first, so its merge runs after the fresh one has
/// merged and re-checks its hold then (F31). A ticket that waits on anything besides that one
/// merge, the integration branch's checks included, is not added, and nor is one behind a ticket
/// that does not merge in this tick. Nothing is gathered or sorted again. Gives each ticket added
/// with the ticket it follows.
fn follow_fresh(
    gathered: &Gathered,
    zaf: &mut Vec<LaneEntry>,
    now: SystemTime,
) -> BTreeMap<TicketId, TicketId> {
    let mut follows = BTreeMap::new();
    let mut behind: BTreeMap<TicketId, Vec<LaneEntry>> = BTreeMap::new();
    for facts in &gathered.tickets {
        let Place::Waiting(waits) = place(facts, now) else {
            continue;
        };
        let mut waits = waits.into_iter();
        let (Some(WaitingOn::Merge(first)), None) = (waits.next(), waits.next()) else {
            continue;
        };
        let (Some(current), Some(beside)) =
            (current_phase(facts), gathered.beside.get(&facts.ticket))
        else {
            continue;
        };
        let next = current.next().clone();
        let leads = zaf
            .iter()
            .any(|entry| entry.ticket == first && is_merge(gathered, entry));
        if !leads
            || kind_of(&beside.list, &next) != ActionKind::Zaf
            || gets_nowhere(facts, &next)
            || follows.contains_key(&facts.ticket)
        {
            continue;
        }
        let since = facts.ready_since.unwrap_or(now);
        let entry = LaneEntry {
            ticket: facts.ticket,
            next,
            since,
            size: facts.size,
            weight: weight(since, facts.priority, now),
        };
        if !is_merge(gathered, &entry) {
            continue;
        }
        follows.insert(facts.ticket, first);
        behind.entry(first).or_default().push(entry);
    }
    if behind.is_empty() {
        return follows;
    }
    let mut joined = Vec::with_capacity(zaf.len() + follows.len());
    for entry in zaf.drain(..) {
        let ticket = entry.ticket;
        joined.push(entry);
        if let Some(mut followers) = behind.remove(&ticket) {
            followers.sort_by_key(|follower| follower.ticket);
            joined.extend(followers);
        }
    }
    *zaf = joined;
    follows
}

/// Why `entry`'s merge waits: how many merged in this tick. One merge names its ticket.
fn left_over(landed: &[TicketId], entry: &LaneEntry) -> String {
    match landed {
        [only] => format!(
            "{only} merged in this tick, so its {} waits for the next one",
            entry.next
        ),
        _ => format!("{} merged in this tick", landed.len()),
    }
}

/// Step 3 with up to `per_tick` merges a tick (D30, `[merge] per_tick`): runs each of `entries` in
/// order ([`run_zaf_actions`]). After each merge it asks the forge whether the pull request number
/// read before it ran merged (`Steps::pr_of`, `Steps::merged`), whatever the action's exit code: a merge that landed but exited 2
/// because `board done` failed moved the branch, so it counts. Once `per_tick` have landed it runs
/// no other merge, and once a merge did not land after one had, the forge refused it and the group
/// ends (Q4): the others go next tick. Before any merge has landed a refusal ends nothing, because
/// the base has not moved: that is today's tick, which `per_tick = 1` keeps (F35). An entry is a
/// merge when its action is a phase of its ticket that ends at `merged`. Each merge left for the
/// next tick is named, as a held ticket is; an entry that is no merge runs whatever came before.
/// A ticket in `follows` is held by R30 behind the ticket it maps to and runs only once that one
/// has landed ([`follow_fresh`]). Each merge's exit and length, and the group's, go to the log
/// (F34).
///
/// After each merge that landed the tick reads the integration branch's head once
/// (`Steps::branch_head`) and hands it, with the branch as it read at its start, to the runner
/// (`Steps::group`): the group's last commit. The re-check before the next merge judges R29 at that
/// start while the branch is still at that commit, so checks pending at a commit the group just made
/// do not hold it; any other head is judged as it reads. It lives in memory for this tick, and is
/// handed back empty when the group ends (F30).
fn run_merge_group(
    gathered: &Gathered,
    entries: &[LaneEntry],
    per_tick: u32,
    follows: &BTreeMap<TicketId, TicketId>,
    steps: &mut Steps<'_>,
) -> (Vec<Ran>, Vec<Held>) {
    let began = Instant::now();
    let mut ran = Vec::new();
    let mut deferred = Vec::new();
    let mut landed: Vec<TicketId> = Vec::new();
    let mut ended = false;
    for entry in entries {
        // A ticket that follows the one fresh from a fix runs only once that one has merged: its
        // hold is over then, and is not while that one stands.
        if follows
            .get(&entry.ticket)
            .is_some_and(|first| !landed.contains(first))
        {
            continue;
        }
        let a_merge = is_merge(gathered, entry);
        let full = u32::try_from(landed.len()).unwrap_or(u32::MAX) >= per_tick;
        if a_merge && (full || ended) {
            deferred.push(Held {
                ticket: entry.ticket,
                reason: left_over(&landed, entry),
            });
            continue;
        }
        // The number is read now: a merge that lands deletes the record it comes from.
        let pr = if a_merge {
            (steps.pr_of)(entry.ticket)
        } else {
            None
        };
        let at = Instant::now();
        let done = run_zaf_actions(std::slice::from_ref(entry), steps.run);
        if a_merge {
            let exit = done.first().map_or(NOT_RUN, |ran| ran.exit);
            let did_land = exit != NOT_RUN && pr.is_some_and(|pr| (steps.merged)(pr));
            (steps.log)(&format!(
                "merge {} of {}: exit {exit}, {}, {:.1}s",
                entry.ticket,
                entry.next,
                if did_land { "merged" } else { "not merged" },
                at.elapsed().as_secs_f64()
            ));
            if did_land {
                landed.push(entry.ticket);
                // The group's own last commit, for the next merge's re-check (D31): the head the
                // branch is at now, with the branch as this tick read it at its start. A head or
                // a start that was not read leaves the next re-check as it always was.
                let own = (steps.branch_head)().zip(gathered.base.as_ref());
                (steps.group)(own.map(|(head, base)| GroupBase {
                    head,
                    base: base.facts.clone(),
                }));
            } else if !landed.is_empty() {
                ended = true;
            }
        }
        ran.extend(done);
    }
    (steps.group)(None);
    if !landed.is_empty() || !deferred.is_empty() {
        (steps.log)(&format!(
            "merges: {} landed, {} left for the next tick, in {:.1}s",
            landed.len(),
            deferred.len(),
            began.elapsed().as_secs_f64()
        ));
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
    let mut sorted = sort_lane(gathered, &placement, now);
    let follows = follow_fresh(gathered, &mut sorted.zaf, now);
    let mut tick = PhaseTick {
        held: held(gathered, now),
        ..PhaseTick::default()
    };
    // A red integration branch lands one merge a tick, its question ruled or not: any merge of a
    // group may be what broke it (D31, Q2). Read from the forge each tick; nothing is stored.
    let per_tick = if is_red(gathered) {
        1
    } else {
        machine.merge_per_tick
    };
    let (ran, deferred) = run_merge_group(gathered, &sorted.zaf, per_tick, &follows, steps);
    tick.outcome.ran = ran;
    tick.held.extend(deferred);
    tick.outcome.waits = launch_waits(&sorted.waits, steps.launch, steps.log);
    let causes: Vec<Cause> = (sorted.failed_waits.iter().cloned().map(Cause::FailedWait))
        .chain(sorted.nowhere.iter().cloned().map(Cause::Nowhere))
        .chain(waiting_causes(
            gathered,
            &placement,
            now,
            machine.merge_per_tick,
        ))
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
