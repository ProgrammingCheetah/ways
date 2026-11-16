//! Every question the scheduler raises (docs/zaf-actions.md §5.1, R18, R19): step 5 of the tick.
//!
//! The scheduler asks the Engineer when a ticket can get nowhere without them. Each [`Cause`] is
//! one reason, and each raises one question on the issue it names ([`Cause::on`]), through
//! [`board::question_among`], which makes that issue wait. The causes:
//!
//! - **Getting nowhere** ([`Hold`]): the action a ticket is in the lane at got nowhere
//!   [`NOWHERE_LIMIT`] times at one head (the core's [`nowhere`]); the gate holds it
//!   ([`crate::app::schedule::tick::may_start`]). Machinery failures never count.
//! - **A failed wait with no `on-fail`** ([`FailedWait`]): the ticket has nowhere to go.
//! - **An overdue check wait** ([`overdue`]): the ticket has waited on the checks at one head for
//!   [`CHECK_DEADLINE`](crate::domain::schedule::CHECK_DEADLINE) and they have not ended: none registered, some still pending, or the
//!   phase awaits something from outside that has not come.
//! - **A dead dependency** ([`dead_dependencies`]): a ticket its `Blocked by:` line names is closed
//!   as not planned, or is no ticket at all. Asked at once.
//! - **A dependency cycle** ([`cycles`]): waiting tickets that wait on each other. Asked at once,
//!   once, on the cycle's lowest ticket, naming the whole cycle.
//! - **An unchallenged epic** ([`unchallenged`]): planned, and not challenged for
//!   [`CHALLENGE_DEADLINE`](crate::domain::schedule::CHALLENGE_DEADLINE). Asked on the epic.
//!
//! A wait on a question or on an unsigned epic has no clock: the Engineer already has it.
//!
//! Nothing is stored. Every question ends with a marker, its last sentence, that the scheduler
//! reads back from the board ([`Cause::marker`]): an open question on the same issue that ends
//! with the cause's marker is the same question, so no cause has two open questions. A cause on a
//! ticket in the lane (getting nowhere, a failed wait) is not asked over any open question on the
//! ticket, as before. A ruling resets what it should:
//!
//! - getting nowhere: the ruled question's `Last run: <n>.` marks the run the count starts again
//!   after, so three more runs that get nowhere ask again (read by the gather, [`marked_run`]);
//! - a failed wait: `Failed wait: <action> at <head>, <n>.` names the head and the failure's time,
//!   so a ruling at that head and failure is not asked again; a new head or a new failure is;
//! - an overdue check wait: `Check deadline: <head>.`, once per head; a new head starts the clock
//!   again;
//! - a dead dependency: `Dead dependency: <ticket>.`, once per ticket and dependency;
//! - a cycle: `Dependency cycle: <tickets>.`, once per cycle;
//! - an unchallenged epic: `Challenge deadline: <n>.`, the time its clock started, so once per
//!   planning.
//!
//! The ruled questions on an issue are read only when a cause other than getting nowhere comes
//! up for it, once per issue a tick.
//!
//! Two ticks can overlap (a `zaf spawn --once` that meets the tick a phase end started, or the
//! moment of a scheduler's restart), and both can find a cause with no open question. So each
//! question is raised under the spawner's lock (`spawn.lock`, waited for as an inline run's
//! opening waits for it), after the open questions are read again under it: a question another
//! tick raised since this tick's first read is found there, nothing is raised, and it is not
//! counted as this tick's. The lock is held for that read and the one question's write, never
//! across the step.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::gather::Gathered;
use crate::app::actions::ActionName;
use crate::app::board::{self, BoardError};
use crate::app::spawn::blockers::{cycles, render_cycle};
use crate::app::spawn::outcome::{Asked, TickOutcome};
use crate::app::spawn::{SpawnError, lock};
use crate::domain::schedule::{
    Awaited, DeadDependency, DeadReason, Failure, NOWHERE_LIMIT, OverdueChallenge, OverdueCheck,
    Placement, RunRecord, TicketFacts, dead_dependencies, dependency_graph, nowhere, overdue,
    unchallenged,
};
use crate::domain::{CommitId, Ticket, TicketId};
use crate::ports::{Board, NewQuestion};

/// What closes a getting-nowhere question: the start of the last run it counted, which a ruling
/// then marks as the point the count starts again from.
const MARKER: &str = "Last run: ";

/// How many characters of a head a cause's short line shows.
const SHORT_HEAD: usize = 7;

/// A task whose next action got nowhere [`NOWHERE_LIMIT`] times at one head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hold {
    /// The task.
    pub task: TicketId,
    /// The action that is not run.
    pub action: ActionName,
    /// The head it got nowhere at: the pull request's head, or none without one.
    pub head: Option<String>,
    /// Whether the task has a pull request, to name a missing head.
    pub has_pull: bool,
    /// The exit codes of its last [`NOWHERE_LIMIT`] counted runs, oldest first.
    pub codes: Vec<i32>,
    /// The last line of the last run's output; none when it left none.
    pub last_line: Option<String>,
    /// When the last run started, as the question marks it.
    last_run: String,
}

impl Hold {
    /// Where it got nowhere, in words: `at <head>`, `at no head` when the forge gives the pull
    /// request none, or `with no pull request`.
    pub fn at(&self) -> String {
        self.at_with(|head| head.to_owned())
    }

    /// [`Hold::at`] with the head as `show` gives it.
    fn at_with(&self, show: impl Fn(&str) -> String) -> String {
        match (&self.head, self.has_pull) {
            (Some(head), _) => format!("at {}", show(head)),
            (None, true) => "at no head".to_owned(),
            (None, false) => "with no pull request".to_owned(),
        }
    }
}

/// A time as a question marks it: nanoseconds since the epoch.
fn stamp(at: SystemTime) -> String {
    at.duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
        .to_string()
}

/// How many times, in words.
fn times(count: usize) -> String {
    match count {
        1 => "once".to_owned(),
        2 => "twice".to_owned(),
        3 => "three times".to_owned(),
        _ => format!("{count} times"),
    }
}

/// The first [`SHORT_HEAD`] characters of a head.
fn short(head: &str) -> String {
    head.chars().take(SHORT_HEAD).collect()
}

/// The whole hours of `waited`.
fn hours(waited: Duration) -> u64 {
    waited.as_secs() / 3600
}

/// A time as a question shows it: RFC 3339, to the second.
fn shown(at: SystemTime) -> String {
    humantime::format_rfc3339_seconds(at).to_string()
}

/// The question a held action raises:
///
/// `<action> got nowhere three times at <head> (exit codes <a>, <b>, <c>); the last line of its
/// log: "<line>". It is not started again until this is ruled. Last run: <stamp>.`
///
/// With no pull request it got nowhere `with no pull request`, and at `no head` when the forge
/// gives the pull request none.
pub fn nowhere_text(hold: &Hold) -> String {
    let codes: Vec<String> = hold.codes.iter().map(i32::to_string).collect();
    let last = hold.last_line.as_ref().map_or_else(
        || "none was recorded".to_owned(),
        |line| format!("\"{line}\""),
    );
    let at = hold.at();
    format!(
        "{} got nowhere {} {at} (exit codes {}); the last line of its log: {last}. \
         It is not started again until this is ruled. {MARKER}{}.",
        hold.action,
        times(codes.len()),
        codes.join(", "),
        hold.last_run
    )
}

/// Whether `text` is a getting-nowhere question ([`nowhere_text`]): `<action> got nowhere …`,
/// ending with the run marker.
pub fn is_nowhere(text: &str) -> bool {
    let text = text.trim();
    text.split_once(" got nowhere ")
        .is_some_and(|(action, _)| !action.is_empty() && !action.contains(char::is_whitespace))
        && text
            .strip_suffix('.')
            .is_some_and(|rest| rest.rsplit_once(MARKER).is_some())
}

/// The run a ruled question of `action` counted up to, read from its text: a getting-nowhere
/// question, or a failure question raised before getting nowhere counted.
pub fn marked_run<'a>(text: &'a str, action: &ActionName) -> Option<&'a str> {
    let text = text.trim();
    let named = [
        format!("{action} got nowhere "),
        format!("{action} failed "),
    ]
    .iter()
    .any(|opening| text.starts_with(opening.as_str()));
    if !named {
        return None;
    }
    let (_, run) = text.strip_suffix('.')?.rsplit_once(MARKER)?;
    Some(run)
}

/// The hold of `action` on the ticket of `facts`, its ruled markers read: none while it got
/// nowhere fewer than [`NOWHERE_LIMIT`] times since the last one.
pub fn hold_of(facts: &TicketFacts, action: &ActionName) -> Option<Hold> {
    let counted = nowhere(facts, action);
    if counted.len() < NOWHERE_LIMIT {
        return None;
    }
    let last: &[&RunRecord] = &counted[counted.len() - NOWHERE_LIMIT..];
    let newest = last[last.len() - 1];
    Some(Hold {
        task: facts.ticket,
        action: action.clone(),
        head: facts.head().map(|head| head.as_str().to_owned()),
        has_pull: facts.pull.is_some(),
        codes: last
            .iter()
            .filter_map(|run| run.exit.map(|exit| exit.code.get()))
            .collect(),
        last_line: newest
            .last_line
            .as_ref()
            .map(|line| line.as_str().to_owned()),
        last_run: stamp(newest.started_at),
    })
}

/// A wait that failed on a task and names no `on-fail`, as the gate holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedWait {
    /// The task.
    pub ticket: TicketId,
    /// The wait's action.
    pub action: ActionName,
    /// What failed, as the scheduling core worked it out.
    pub failure: Failure,
}

impl FailedWait {
    /// The head it failed at, in full; `no head` when the forge gave none.
    fn head(&self) -> &str {
        self.failure
            .head
            .as_ref()
            .map_or("no head", CommitId::as_str)
    }
}

/// The question a failed wait with no `on-fail` raises, in this form:
///
/// `<action> failed at <head>: <what failed>; it declares no on-fail. Failed wait: <action> at
/// <head>, <stamp>.`
///
/// What failed is `failed checks <names>` when checks failed at the head, else `exit code <n>` of
/// the wait's run; both when both, as the scheduling core worked the failure out. A wait the forge
/// gave no head for reads `no head`. The marker names the head and when it failed (nanoseconds
/// since the epoch, 0 with no time), so a ruling at that head and failure is not asked again.
pub fn failed_wait_text(failed: &FailedWait) -> String {
    let failure = &failed.failure;
    let mut what = Vec::new();
    if !failure.checks.is_empty() {
        let names: Vec<&str> = failure.checks.iter().map(|check| check.as_str()).collect();
        what.push(format!("failed checks {}", names.join(", ")));
    }
    if let Some(code) = failure.exit.filter(|code| !code.is_zero()) {
        what.push(format!("exit code {code}"));
    }
    if what.is_empty() {
        what.push("a failure with no check or exit code named".to_owned());
    }
    format!(
        "{} failed at {}: {}; it declares no on-fail. {}",
        failed.action,
        failed.head(),
        what.join(", "),
        failed_wait_marker(failed)
    )
}

/// The marker of a failed wait's question: `Failed wait: <action> at <head>, <stamp>.`
fn failed_wait_marker(failed: &FailedWait) -> String {
    format!(
        "Failed wait: {} at {}, {}.",
        failed.action,
        failed.head(),
        failed.failure.at.map_or_else(|| "0".to_owned(), stamp)
    )
}

/// Why the scheduler asks the Engineer: one cause, one question (see the module above).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cause {
    /// The action a ticket is in the lane at got nowhere [`NOWHERE_LIMIT`] times at one head.
    Nowhere(Hold),
    /// A wait failed and names no `on-fail`.
    FailedWait(FailedWait),
    /// The ticket has waited on the checks at one head past
    /// [`CHECK_DEADLINE`](crate::domain::schedule::CHECK_DEADLINE).
    OverdueCheck {
        /// The ticket.
        ticket: TicketId,
        /// The overdue wait, as the core found it.
        overdue: OverdueCheck,
        /// How long it has waited.
        waited: Duration,
    },
    /// A ticket its `Blocked by:` line names can never complete.
    DeadDependency {
        /// The ticket that waits.
        ticket: TicketId,
        /// The dependency, and why it never completes.
        dead: DeadDependency,
    },
    /// Waiting tickets wait on each other.
    Cycle {
        /// The lowest of them, which the question is raised on.
        lowest: TicketId,
        /// The tickets, each waiting on the next and the last on the first, the lowest first.
        cycle: Vec<TicketId>,
    },
    /// An epic planned and not challenged past
    /// [`CHALLENGE_DEADLINE`](crate::domain::schedule::CHALLENGE_DEADLINE).
    Unchallenged {
        /// The epic, and when its clock started.
        overdue: OverdueChallenge,
        /// How long it has waited.
        waited: Duration,
    },
}

impl Cause {
    /// The issue the question is raised on: the ticket, the cycle's lowest ticket, or the epic.
    pub fn on(&self) -> TicketId {
        match self {
            Self::Nowhere(hold) => hold.task,
            Self::FailedWait(failed) => failed.ticket,
            Self::OverdueCheck { ticket, .. } | Self::DeadDependency { ticket, .. } => *ticket,
            Self::Cycle { lowest, .. } => *lowest,
            Self::Unchallenged { overdue, .. } => overdue.epic,
        }
    }

    /// Whether any open question on the issue stands for this one: a cause on a ticket in the
    /// lane, which the Engineer already has if any question is open on it.
    fn any_open_question_stands(&self) -> bool {
        matches!(self, Self::Nowhere(_) | Self::FailedWait(_))
    }

    /// Whether one of `open`, each open question's issue and text, stands for this cause's
    /// question: one on its issue that ends with its marker, or any on its issue for a cause on a
    /// ticket in the lane.
    fn is_open_among(&self, open: &[(TicketId, String)]) -> bool {
        let on = self.on();
        let marker = self.marker();
        open.iter().any(|(issue, text)| {
            *issue == on && (self.any_open_question_stands() || text.ends_with(&marker))
        })
    }

    /// Whether a ruled question with this cause's marker means it is not asked again. A
    /// getting-nowhere question's ruling is read by the gather instead, and lets the count start
    /// again.
    fn ruling_settles(&self) -> bool {
        !matches!(self, Self::Nowhere(_))
    }

    /// The question's last sentence, which the scheduler reads back to know the question is this
    /// cause's: `Last run: <n>.`, `Failed wait: <action> at <head>, <n>.`, `Check deadline:
    /// <head>.`, `Dead dependency: <ticket>.`, `Dependency cycle: <tickets>.` or `Challenge
    /// deadline: <n>.`
    pub fn marker(&self) -> String {
        match self {
            Self::Nowhere(hold) => format!("{MARKER}{}.", hold.last_run),
            Self::FailedWait(failed) => failed_wait_marker(failed),
            Self::OverdueCheck { overdue, .. } => format!("Check deadline: {}.", overdue.head),
            Self::DeadDependency { dead, .. } => {
                format!("Dead dependency: {}.", dead.dependency)
            }
            Self::Cycle { cycle, .. } => format!("Dependency cycle: {}.", listed(cycle)),
            Self::Unchallenged { overdue, .. } => {
                format!("Challenge deadline: {}.", stamp(overdue.since))
            }
        }
    }

    /// The question's text, ending with its [`Cause::marker`].
    pub fn text(&self) -> String {
        match self {
            Self::Nowhere(hold) => nowhere_text(hold),
            Self::FailedWait(failed) => failed_wait_text(failed),
            Self::OverdueCheck {
                overdue, waited, ..
            } => {
                let at = short(overdue.head.as_str());
                let hours = hours(*waited);
                let since = shown(overdue.since);
                let body = match &overdue.awaited {
                    Awaited::NoCheck => format!(
                        "No check has registered at {at} for {hours} h (since {since}): the \
                         runner may be down, or the workflow's path filter may have skipped this \
                         change. It waits on its checks until one ends there."
                    ),
                    Awaited::Pending(checks) => format!(
                        "The checks {} at {at} are still pending after {hours} h (since \
                         {since}): the runner may be down. It waits until they end there.",
                        checks
                            .iter()
                            .map(|check| check.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    Awaited::Outside { phase, ends } => format!(
                        "{phase} has waited {hours} h at {at} (since {since}) for {}, and \
                         nothing has come. It waits until it comes.",
                        ends.iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                };
                format!("{body} {}", self.marker())
            }
            Self::DeadDependency { ticket, dead } => {
                let dependency = dead.dependency;
                let why = match dead.reason {
                    DeadReason::Missing => {
                        format!("{dependency} is not a ticket (no such issue, or a pull request)")
                    }
                    DeadReason::ClosedNotPlanned | DeadReason::Cycle => {
                        format!("{dependency} is {}", dead.reason)
                    }
                };
                format!(
                    "{ticket} waits on {dependency}, which can never complete: {why}. {ticket} \
                     waits until {dependency} is done or leaves its Blocked by: line. {}",
                    self.marker()
                )
            }
            Self::Cycle { cycle, .. } => {
                let body = match cycle.as_slice() {
                    [only] => format!(
                        "{only} waits on itself: {}, so it can never complete. It waits until \
                         its Blocked by: line no longer names it.",
                        render_cycle(cycle)
                    ),
                    _ => format!(
                        "These tickets wait on each other: {}, so none of them can ever \
                         complete. They wait until a Blocked by: line breaks the cycle.",
                        render_cycle(cycle)
                    ),
                };
                format!("{body} {}", self.marker())
            }
            Self::Unchallenged { overdue, waited } => format!(
                "Epic {} has tasks but no challenge after {} h (since {}): the challenger may \
                 have ended without a report, or never got a slot. Its tasks wait until it is \
                 challenged. {}",
                overdue.epic,
                hours(*waited),
                shown(overdue.since),
                self.marker()
            ),
        }
    }

    /// The cause in a few words, as the tick's output names the question: `merge got nowhere
    /// three times at d962227`, `ci failed at d962227 and names no on-fail`, `no check has
    /// registered at d962227 for 2 h`, `its dependency #9 is closed as not planned`, `its
    /// dependencies form a cycle: #3 → #5 → #3`, `it has had no challenge for 24 h`.
    pub fn line(&self) -> String {
        match self {
            Self::Nowhere(hold) => format!(
                "{} got nowhere {} {}",
                hold.action,
                times(hold.codes.len()),
                hold.at_with(short)
            ),
            Self::FailedWait(failed) => format!(
                "{} failed at {} and names no on-fail",
                failed.action,
                short(failed.head())
            ),
            Self::OverdueCheck {
                overdue, waited, ..
            } => {
                let at = short(overdue.head.as_str());
                let hours = hours(*waited);
                match &overdue.awaited {
                    Awaited::NoCheck => format!("no check has registered at {at} for {hours} h"),
                    Awaited::Pending(_) => {
                        format!("the checks at {at} are still pending after {hours} h")
                    }
                    Awaited::Outside { phase, .. } => {
                        format!("{phase} has waited {hours} h at {at}")
                    }
                }
            }
            Self::DeadDependency { dead, .. } => {
                format!("its dependency {} is {}", dead.dependency, dead.reason)
            }
            Self::Cycle { cycle, .. } => {
                format!("its dependencies form a cycle: {}", render_cycle(cycle))
            }
            Self::Unchallenged { waited, .. } => {
                format!("it has had no challenge for {} h", hours(*waited))
            }
        }
    }
}

/// Tickets as a marker lists them: `#3, #5, #7`.
fn listed(tickets: &[TicketId]) -> String {
    tickets
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// The causes the waiting tickets and the open epics of `gathered` raise at `now`, as `placement`
/// places the tickets: each waiting ticket's overdue check wait, then each of its dependencies
/// that is closed as not planned or no ticket, in ticket order; then each cycle among the waiting
/// tickets' dependencies, its lowest ticket first; then each open epic planned and not challenged
/// past its deadline that has no open question on it.
pub fn waiting_causes(gathered: &Gathered, placement: &Placement, now: SystemTime) -> Vec<Cause> {
    let waiting_tickets: BTreeSet<TicketId> =
        placement.waiting.values().flatten().copied().collect();
    let waiting: Vec<TicketFacts> = gathered
        .tickets
        .iter()
        .filter(|facts| waiting_tickets.contains(&facts.ticket))
        .cloned()
        .collect();
    let graph = dependency_graph(&waiting);
    let mut causes = Vec::new();
    for facts in &waiting {
        if let Some(found) = overdue(facts, now) {
            causes.push(Cause::OverdueCheck {
                ticket: facts.ticket,
                waited: now.duration_since(found.since).unwrap_or_default(),
                overdue: found,
            });
        }
        causes.extend(
            dead_dependencies(facts, &graph)
                .into_iter()
                .filter(|dead| dead.reason != DeadReason::Cycle)
                .map(|dead| Cause::DeadDependency {
                    ticket: facts.ticket,
                    dead,
                }),
        );
    }
    causes.extend(cycles(&graph).into_iter().filter_map(|cycle| {
        let lowest = cycle.iter().copied().min()?;
        Some(Cause::Cycle { lowest, cycle })
    }));
    for epic in &gathered.epics {
        let questioned = gathered
            .questions
            .get(&epic.id)
            .is_some_and(|open| !open.is_empty());
        if let Some(found) = unchallenged(epic, questioned, now) {
            causes.push(Cause::Unchallenged {
                waited: now.duration_since(found.since).unwrap_or_default(),
                overdue: found,
            });
        }
    }
    causes
}

/// Why step 5 stopped.
#[derive(Debug, thiserror::Error)]
pub enum AskError {
    /// The board failed.
    #[error(transparent)]
    Board(#[from] BoardError),
    /// The spawner's lock, which each question is raised under, could not be taken.
    #[error(transparent)]
    Lock(SpawnError),
}

impl From<crate::ports::PortError> for AskError {
    fn from(error: crate::ports::PortError) -> Self {
        Self::Board(BoardError::Port(error))
    }
}

/// The open questions of `questions`, each with the issue it was raised on and its text.
fn issues_and_texts(questions: &[Ticket]) -> Vec<(TicketId, String)> {
    questions
        .iter()
        .filter_map(|question| {
            let on = NewQuestion::raised_on(question.body.as_str())?;
            Some((on, NewQuestion::text_of(question).trim().to_owned()))
        })
        .collect()
}

/// Raises one question for each cause that has none, in order, and lists what it raised.
///
/// A cause has one already when an open question on its issue ends with its marker; a cause on a
/// ticket in the lane when any question is open on the ticket ([`Cause`]). Every cause but getting
/// nowhere is also passed when a ruled question on its issue ends with its marker: the Engineer
/// ruled on that one. A cause that passes is raised under the spawner's lock of `state_dir`, held
/// for that question alone: the open questions are read again under it, and a cause another tick
/// has raised since the first read raises nothing and is not listed as raised here. Each question
/// is raised with [`board::question_among`], which also makes the issue wait. Nothing is started
/// and nothing is charged. A board that fails, or a lock that cannot be taken, stops the step with
/// its error; the questions raised before it stand, and the next tick finds them open.
pub fn ask(board: &dyn Board, state_dir: &Path, causes: &[Cause]) -> Result<Vec<Asked>, AskError> {
    let mut raised = Vec::new();
    if causes.is_empty() {
        return Ok(raised);
    }
    let mut open = issues_and_texts(&board.questions()?);
    let mut ruled: BTreeMap<TicketId, Vec<String>> = BTreeMap::new();
    for cause in causes {
        let on = cause.on();
        let marker = cause.marker();
        if cause.is_open_among(&open) {
            continue;
        }
        if cause.ruling_settles() {
            let texts = match ruled.entry(on) {
                std::collections::btree_map::Entry::Occupied(found) => found.into_mut(),
                std::collections::btree_map::Entry::Vacant(slot) => slot.insert(
                    board
                        .questions_on(on)?
                        .iter()
                        .filter(|(_, ruling)| ruling.is_some())
                        .map(|(question, _)| NewQuestion::text_of(question).trim().to_owned())
                        .collect(),
                ),
            };
            if texts.iter().any(|text| text.ends_with(&marker)) {
                continue;
            }
        }
        // Held for this question alone, from the read below to its write: a tick that overlaps
        // this one may have raised it since the first read.
        let _held = lock(state_dir).map_err(AskError::Lock)?;
        let questions = board.questions()?;
        open = issues_and_texts(&questions);
        if cause.is_open_among(&open) {
            continue;
        }
        let text = cause.text();
        let question = board::question_among(
            board,
            &NewQuestion {
                on,
                text: text.clone(),
                door: false,
            },
            &questions,
        )?;
        open.push((on, text));
        raised.push(Asked {
            task: on,
            question,
            cause: cause.line(),
        });
    }
    Ok(raised)
}

/// The outcome of a tick that did step 5 alone: [`TickOutcome::questions`] and
/// [`TickOutcome::asked`] filled from [`ask`], under the spawner's lock of `state_dir`, the rest
/// empty for the steps that fill them.
pub fn ask_outcome(
    board: &dyn Board,
    state_dir: &Path,
    causes: &[Cause],
) -> Result<TickOutcome, AskError> {
    let asked = ask(board, state_dir, causes)?;
    Ok(TickOutcome {
        questions: asked.iter().map(|asked| asked.task).collect(),
        asked,
        ..TickOutcome::default()
    })
}

#[cfg(test)]
mod tests;
