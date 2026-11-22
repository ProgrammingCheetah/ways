//! The tick's label sync (R5, docs/zaf-actions.md §5.1): the board's state labels are output. Each
//! tick, once the core has placed the tickets and before any phase starts, every open ticket's
//! label is set from its place and its claim ([`label_for`]), so a person reading the board reads
//! what the core decided; nothing reads a label back to decide where a ticket sits.
//!
//! Three cases are written, and only when the label differs:
//!
//! - a ticket that waits on a question and carries `ready` gets `question`;
//! - a ticket that waits on another ticket, and on no question, gets `blocked`;
//! - a ticket that carries `question` or `blocked` and waits on neither gets `ready`. A `blocked`
//!   one going `ready` is the old unblocking, worded as it was: `#26 unblocked: #25 is done`
//!   ([`Unblocked`]).
//!
//! The claim owns `ready` and `claimed`: the sync never sets either over the other, never writes
//! over `claimed` (on GitHub and Forgejo the label is the claim) and never sets or clears `done`.
//! A running ticket and a done one are left as they are. A ticket that carries `blocked` and waits
//! on a question keeps `blocked`: the boards set `question` only over `ready` or `claimed`, so a
//! ruling never hands it out early.
//!
//! A ticket with a phase list is placed by the core ([`shadow::places`]). A ticket the tick does
//! not place (its type has no phase list, or the project has no `[phases]`) waits on the open
//! questions raised on it or on its epic and on each ticket its `Blocked by:` line names that is
//! not done ([`Board::standing`], each read once); a ticket the gather could not read is left as
//! it is.
//!
//! [`read`] says what the sync would write, writing nothing (the dry run); [`sync`] writes it.
//! Both read the same, so the dry run names the same changes. A read or a write that fails is
//! named in [`Labelling::failed`] and the others still go; the tick's other steps run either way.

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use serde_json::{Value, json};

use super::gather::{Gathered, open_on};
use super::shadow;
use crate::app::spawn::blockers::{blockers, finished_epic_standing};
use crate::domain::schedule::{Place, WaitingOn, label_for};
use crate::domain::{Standing, Ticket, TicketId, TicketState};
use crate::ports::{Board, PortError};

/// A task set to `ready` from `blocked` because it waits on no ticket: the blockers it names are
/// all done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unblocked {
    /// The task.
    pub task: TicketId,
    /// The blockers its `Blocked by:` line names, each done, the lowest first; none when it names
    /// none.
    pub blockers: Vec<TicketId>,
}

impl Unblocked {
    /// The line of the tick's block and of the dry run: `#26 unblocked: #25 is done`,
    /// `#9 unblocked: #5, #6 and #7 are done`, or `#9 unblocked: it names no blocker`.
    pub fn line(&self) -> String {
        let task = self.task;
        match self.blockers.as_slice() {
            [] => format!("{task} unblocked: it names no blocker"),
            [one] => format!("{task} unblocked: {one} is done"),
            [rest @ .., last] => format!("{task} unblocked: {} and {last} are done", listed(rest)),
        }
    }
}

/// Any other label the sync set: `to` in place of `from`, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relabelled {
    /// The task.
    pub task: TicketId,
    /// The label it carried; none when it carried none.
    pub from: Option<TicketState>,
    /// The label it got.
    pub to: TicketState,
    /// Why, in words: `it waits on question #45`, `it waits on #5`, `no question is open on it`.
    pub why: String,
}

impl Relabelled {
    /// The line of the tick's block and of the dry run: `#26 labelled question: it waits on
    /// question #45`.
    pub fn line(&self) -> String {
        format!("{} labelled {}: {}", self.task, self.to, self.why)
    }
}

/// What the sync did, or would do in a dry run: the tasks unblocked, the other labels set, each in
/// task order, and every failure met on the way.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Labelling {
    /// The tasks set to `ready` from `blocked`, the lowest first.
    pub unblocked: Vec<Unblocked>,
    /// The other labels set, the lowest task first.
    pub relabelled: Vec<Relabelled>,
    /// What failed, one line each: the list of open tasks, a blocker's standing or a label's
    /// write. Each fails the tick, whose other steps still run.
    pub failed: Vec<String>,
}

impl Labelling {
    /// One line per task unblocked ([`Unblocked::line`]), then one per other label set
    /// ([`Relabelled::line`]).
    pub fn lines(&self) -> Vec<String> {
        self.unblocked
            .iter()
            .map(Unblocked::line)
            .chain(self.relabelled.iter().map(Relabelled::line))
            .collect()
    }

    /// The tasks unblocked as `--json` prints them: `[{"task": 26, "blockers": [25]}]`.
    pub fn to_json(&self) -> Value {
        self.unblocked
            .iter()
            // A ticket id serializes as its bare number.
            .map(|unblocked| json!({ "task": unblocked.task, "blockers": unblocked.blockers }))
            .collect()
    }

    /// The other labels set as `--json` prints them:
    /// `[{"task": 26, "from": "ready", "to": "question"}]`, `from` null for a task that carried none.
    pub fn relabelled_json(&self) -> Value {
        self.relabelled
            .iter()
            .map(|relabelled| {
                json!({ "task": relabelled.task, "from": relabelled.from, "to": relabelled.to })
            })
            .collect()
    }
}

/// One label to write, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Change {
    /// `blocked` to `ready`.
    Unblock(Unblocked),
    /// Any other.
    Relabel(Relabelled),
}

impl Change {
    fn task(&self) -> TicketId {
        match self {
            Self::Unblock(unblocked) => unblocked.task,
            Self::Relabel(relabelled) => relabelled.task,
        }
    }

    fn to(&self) -> TicketState {
        match self {
            Self::Unblock(_) => TicketState::Ready,
            Self::Relabel(relabelled) => relabelled.to,
        }
    }
}

/// What [`sync`] would write on `board` for `gathered` at `now`, writing nothing.
pub fn read(board: &dyn Board, gathered: &Gathered, now: SystemTime) -> Labelling {
    let (changes, failed) = changes(board, gathered, now);
    let mut labelling = Labelling {
        failed,
        ..Labelling::default()
    };
    for change in changes {
        labelling.push(change);
    }
    labelling
}

/// Sets every label [`read`] finds on `board`. A label that cannot be written is named in
/// [`Labelling::failed`] and left out of the rest; the others still go.
pub fn sync(board: &dyn Board, gathered: &Gathered, now: SystemTime) -> Labelling {
    let (changes, failed) = changes(board, gathered, now);
    let mut labelling = Labelling {
        failed,
        ..Labelling::default()
    };
    for change in changes {
        match board.set_state(change.task(), change.to()) {
            Ok(()) => labelling.push(change),
            Err(error) => labelling.failed.push(match &change {
                Change::Unblock(unblocked) => format!("cannot unblock {}: {error}", unblocked.task),
                Change::Relabel(relabelled) => format!(
                    "cannot label {} {}: {error}",
                    relabelled.task, relabelled.to
                ),
            }),
        }
    }
    labelling
}

impl Labelling {
    fn push(&mut self, change: Change) {
        match change {
            Change::Unblock(unblocked) => self.unblocked.push(unblocked),
            Change::Relabel(relabelled) => self.relabelled.push(relabelled),
        }
    }
}

/// Every label to write, in task order, and what could not be read.
fn changes(board: &dyn Board, gathered: &Gathered, now: SystemTime) -> (Vec<Change>, Vec<String>) {
    let labels: BTreeMap<TicketId, Option<TicketState>> = gathered
        .open
        .iter()
        .map(|ticket| (ticket.id, ticket.state))
        .collect();
    let mut changes = Vec::new();
    let mut failed = Vec::new();
    // The tickets the core places.
    let named: BTreeMap<TicketId, Vec<TicketId>> = gathered
        .tickets
        .iter()
        .map(|facts| {
            let named = facts
                .dependencies
                .iter()
                .map(|dependency| dependency.ticket);
            (facts.ticket, sorted(named))
        })
        .collect();
    for placed in shadow::places(gathered, now) {
        if matches!(placed.place, Place::Running(_) | Place::Done) {
            continue;
        }
        let Some(&label) = labels.get(&placed.ticket) else {
            continue;
        };
        let named = named.get(&placed.ticket).cloned().unwrap_or_default();
        let waits = match &placed.place {
            Place::Waiting(waits) => waits.clone(),
            _ => BTreeSet::new(),
        };
        changes.extend(change(placed.ticket, label, &waits, named));
    }
    // The tickets it does not: each standing read once, those the gather read included.
    let mut standings: BTreeMap<TicketId, Standing> = gathered
        .tickets
        .iter()
        .flat_map(|facts| facts.dependencies.iter())
        .map(|dependency| (dependency.ticket, dependency.standing))
        .collect();
    let skipped: BTreeSet<TicketId> = gathered
        .tickets
        .iter()
        .map(|facts| facts.ticket)
        .chain(
            gathered
                .unreadable
                .iter()
                .map(|unreadable| unreadable.ticket),
        )
        .collect();
    for ticket in gathered
        .open
        .iter()
        .filter(|ticket| !skipped.contains(&ticket.id))
    {
        match unplaced_waits(board, gathered, ticket, &mut standings) {
            Ok((waits, named)) => changes.extend(change(ticket.id, ticket.state, &waits, named)),
            Err(error) if ticket.state == Some(TicketState::Blocked) => {
                failed.push(format!("cannot unblock {}: {error}", ticket.id));
            }
            Err(error) => failed.push(format!("cannot label {}: {error}", ticket.id)),
        }
    }
    changes.sort_by_key(Change::task);
    (changes, failed)
}

/// What a ticket the core does not place waits on: each open question raised on it or on its
/// epic, and each ticket its `Blocked by:` line names that is not done; with the tickets it names,
/// the lowest first.
fn unplaced_waits(
    board: &dyn Board,
    gathered: &Gathered,
    ticket: &Ticket,
    standings: &mut BTreeMap<TicketId, Standing>,
) -> Result<(BTreeSet<WaitingOn>, Vec<TicketId>), PortError> {
    let mut waits: BTreeSet<WaitingOn> = open_on(&gathered.questions, ticket)
        .into_iter()
        .map(WaitingOn::Question)
        .collect();
    let named = sorted(blockers(ticket.body.as_str()));
    for &blocker in &named {
        let standing = match standings.get(&blocker) {
            Some(&standing) => standing,
            None => {
                let standing = finished_epic_standing(
                    blocker,
                    board.standing(blocker)?.into(),
                    &gathered.epics,
                )
                .standing;
                standings.insert(blocker, standing);
                standing
            }
        };
        if standing != Standing::Done {
            waits.insert(WaitingOn::Ticket(blocker));
        }
    }
    Ok((waits, named))
}

/// The label `task`, carrying `label` and waiting on `waits`, is to get, if any (see the module
/// above); `named` are the tickets its `Blocked by:` line names, the lowest first.
fn change(
    task: TicketId,
    label: Option<TicketState>,
    waits: &BTreeSet<WaitingOn>,
    named: Vec<TicketId>,
) -> Option<Change> {
    if matches!(label, Some(TicketState::Claimed | TicketState::Done)) {
        return None;
    }
    // Not claimed: the claim's label is never written over.
    let target = if waits.is_empty() {
        TicketState::Ready
    } else {
        label_for(&Place::Waiting(waits.clone()), false)
    };
    let relabel = |why: String| {
        Some(Change::Relabel(Relabelled {
            task,
            from: label,
            to: target,
            why,
        }))
    };
    match (target, label) {
        (TicketState::Question, Some(TicketState::Ready)) => {
            let questions: Vec<TicketId> = waits
                .iter()
                .filter_map(|waiting_on| match waiting_on {
                    WaitingOn::Question(question) => Some(*question),
                    _ => None,
                })
                .collect();
            let noun = if questions.len() == 1 {
                "question"
            } else {
                "questions"
            };
            relabel(format!("it waits on {noun} {}", and_listed(&questions)))
        }
        (TicketState::Blocked, label) if label != Some(TicketState::Blocked) => {
            let tickets: Vec<TicketId> = waits
                .iter()
                .filter_map(|waiting_on| match waiting_on {
                    WaitingOn::Ticket(ticket) => Some(*ticket),
                    _ => None,
                })
                .collect();
            relabel(format!("it waits on {}", and_listed(&tickets)))
        }
        (TicketState::Ready, Some(TicketState::Blocked)) => Some(Change::Unblock(Unblocked {
            task,
            blockers: named,
        })),
        (TicketState::Ready, Some(TicketState::Question)) => {
            relabel("no question is open on it".to_owned())
        }
        _ => None,
    }
}

/// `tickets` sorted, each once.
fn sorted(tickets: impl IntoIterator<Item = TicketId>) -> Vec<TicketId> {
    let mut tickets: Vec<TicketId> = tickets.into_iter().collect();
    tickets.sort();
    tickets.dedup();
    tickets
}

/// `#5, #6`: each ticket, joined by commas.
fn listed(tickets: &[TicketId]) -> String {
    let words: Vec<String> = tickets.iter().map(ToString::to_string).collect();
    words.join(", ")
}

/// `#5`, `#5 and #6`, `#5, #6 and #7`.
fn and_listed(tickets: &[TicketId]) -> String {
    match tickets {
        [] => String::new(),
        [one] => one.to_string(),
        [rest @ .., last] => format!("{} and {last}", listed(rest)),
    }
}

#[cfg(test)]
mod tests;
