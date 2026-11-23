//! `zaf epic wait <epic> [--minutes <n>]` (release 0.5.5, D19, Q7): block until the epic needs the
//! Engineer, a problem shows or the epic is done.
//!
//! It reads the project as the chair does (the hub's state turned into a [`ChairState`]) on an
//! interval and returns on the first read where:
//!
//! - a question waits on the Engineer, raised on the epic or on one of its tasks (an inbox item
//!   that carries a question and names one of those tickets);
//! - a problem shows: one naming the epic or one of its tasks, or a project-wide one that
//!   appeared after the call began (ruling on #1087: a standing project problem would make every
//!   call return at once);
//! - the epic is done: its stage is done, or it is closed since the first read.
//!
//! The condition is read from the state each time and nothing is stored, so a question already
//! waiting when the call is made returns at once. The gather and the clock are ports
//! ([`Gather`], [`Clock`]): a test drives them with a script and a clock that only advances when
//! it is slept on.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use crate::app::chair::{ChairState, build};
use crate::app::hub::next::next_for_project;
use crate::app::hub::state::gather_project;
use crate::domain::{EpicStage, TicketId};

/// What reads the project's state each time the wait looks.
pub trait Gather {
    /// The project as the chair reads it; the reason in plain words when the board could not be
    /// read.
    fn read(&self) -> Result<ChairState, String>;
}

/// What tells the time and lets it pass.
pub trait Clock {
    /// The time since the clock began.
    fn now(&self) -> Duration;
    /// Lets `time` pass.
    fn sleep(&self, time: Duration);
}

/// The project at `root`, read as the hub's page is.
#[derive(Debug, Clone)]
pub struct ProjectGather {
    /// The project's root.
    pub root: PathBuf,
}

impl Gather for ProjectGather {
    fn read(&self) -> Result<ChairState, String> {
        let project = gather_project(&self.root, SystemTime::now());
        if let Some(reason) = &project.reason {
            return Err(reason.clone());
        }
        Ok(build(&project, &next_for_project(&project)))
    }
}

/// Why the wait did not end in a return.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WaitError {
    /// The first read found no open epic with the id.
    #[error("{0} is not an open epic")]
    NotAnEpic(TicketId),
    /// The first read of the board failed.
    #[error("the board cannot be read: {0}")]
    Board(String),
}

/// Which of the three ended the wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// A question waits on the Engineer.
    Question,
    /// A problem shows.
    Problem,
    /// The epic is done.
    Done,
}

impl Reason {
    /// The line that names it.
    pub const fn line(self) -> &'static str {
        match self {
            Self::Question => "returned: a question waits on you",
            Self::Problem => "returned: a problem shows",
            Self::Done => "returned: the epic is done",
        }
    }
}

/// How the wait ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Waited {
    /// It returned for `reason`, on this state.
    Returned {
        /// What ended it.
        reason: Reason,
        /// The project as the last read had it.
        state: Box<ChairState>,
        /// The epic's open questions at that read, and its tasks'.
        questions: Vec<OpenQuestion>,
    },
    /// Nothing happened by the limit.
    NothingYet,
}

/// A question open on the epic or one of its tasks.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct OpenQuestion {
    /// The question's issue.
    pub question: TicketId,
    /// The ticket it was raised on, when its body says.
    pub on: Option<TicketId>,
    /// The question in full.
    pub text: String,
}

impl OpenQuestion {
    /// `#285 on #231: <text>`, the text's later lines indented under it.
    pub fn render(&self) -> String {
        let on = self.on.map(|on| format!(" on {on}")).unwrap_or_default();
        let mut lines = self.text.trim().lines();
        let first = lines.next().unwrap_or_default();
        let rest: String = lines.map(|line| format!("\n  {line}")).collect();
        format!("{}{on}: {first}{rest}", self.question)
    }
}

/// The epic and its tasks, as the state lists them.
fn tickets(state: &ChairState, epic: TicketId) -> BTreeSet<TicketId> {
    let mut tickets = BTreeSet::from([epic]);
    if let Some(view) = state.epic(epic) {
        tickets.extend(view.squares.iter().map(|square| square.ticket));
    }
    tickets
}

/// The questions open on the epic and its tasks, in the inbox's order.
pub fn open_questions(state: &ChairState, epic: TicketId) -> Vec<OpenQuestion> {
    let tickets = tickets(state, epic);
    state
        .inbox
        .iter()
        .filter_map(|item| {
            let question = item.question.as_ref()?;
            let number = item.number?;
            let names = tickets.contains(&number)
                || question.raised_on.is_some_and(|on| tickets.contains(&on));
            names.then(|| OpenQuestion {
                question: number,
                on: question.raised_on,
                text: question.text.clone(),
            })
        })
        .collect()
}

/// The project-wide problems of `state`, by headline: those that name no ticket.
fn standing(state: &ChairState) -> BTreeSet<&str> {
    state
        .problems
        .iter()
        .filter(|problem| problem.ticket.is_none())
        .map(|problem| problem.headline.as_str())
        .collect()
}

/// Whether a problem shows: one naming the epic or a task of it, or a project-wide one not in
/// `baseline`, the project-wide ones that stood when the call began.
fn problem_shows(state: &ChairState, epic: TicketId, baseline: &BTreeSet<String>) -> bool {
    let tickets = tickets(state, epic);
    state.problems.iter().any(|problem| match problem.ticket {
        Some(ticket) => tickets.contains(&ticket),
        None => !baseline.contains(&problem.headline),
    })
}

/// Why `state` ends the wait, when it does.
fn reason(state: &ChairState, epic: TicketId, baseline: &BTreeSet<String>) -> Option<Reason> {
    if !open_questions(state, epic).is_empty() {
        return Some(Reason::Question);
    }
    if problem_shows(state, epic, baseline) {
        return Some(Reason::Problem);
    }
    // An epic that is not open any more was closed since the first read, which found it open.
    let done = state
        .epic(epic)
        .is_none_or(|view| view.stage == EpicStage::Done);
    done.then_some(Reason::Done)
}

/// Waits on `epic` for at most `limit`, reading through `gather` every `every` by `clock`.
///
/// The first read decides the epic exists (an unknown one is [`WaitError::NotAnEpic`]) and that
/// the board can be read (else [`WaitError::Board`]); a later failed read is tried again at the
/// next interval.
pub fn wait(
    gather: &dyn Gather,
    clock: &dyn Clock,
    epic: TicketId,
    limit: Duration,
    every: Duration,
) -> Result<Waited, WaitError> {
    let began = clock.now();
    let first = gather.read().map_err(WaitError::Board)?;
    if first.epic(epic).is_none() {
        return Err(WaitError::NotAnEpic(epic));
    }
    let baseline: BTreeSet<String> = standing(&first).into_iter().map(str::to_owned).collect();
    let mut state = first;
    loop {
        if let Some(reason) = reason(&state, epic, &baseline) {
            let questions = open_questions(&state, epic);
            return Ok(Waited::Returned {
                reason,
                state: Box::new(state),
                questions,
            });
        }
        let left = limit.saturating_sub(clock.now().saturating_sub(began));
        if left.is_zero() {
            return Ok(Waited::NothingYet);
        }
        // A read that failed is the same read at the next interval.
        loop {
            clock.sleep(every.min(left));
            if let Ok(read) = gather.read() {
                state = read;
                break;
            }
            if clock.now().saturating_sub(began) >= limit {
                return Ok(Waited::NothingYet);
            }
        }
    }
}

#[cfg(test)]
mod tests;
