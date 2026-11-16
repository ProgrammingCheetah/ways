//! A fix brief's `## Why this run` (docs/zaf-actions.md §5.1): what an `on-fail` phase was started
//! for, and what was ruled since a model last worked on the ticket.
//!
//! [`why_this_run`] gathers the ticket as the tick does ([`gather_one`]) and reads, with nothing
//! stored: the current phase when it has failed at the head ([`failed_phase`]), with the exit code
//! of the run that failed it and that code's meaning as the action declares it, the checks that
//! failed, the questions the run raised, and what its run folder kept of its output (for an inline
//! `zaf`-kind run, its `FAIL` line and the last lines of its log: a merge whose base conflicts
//! names the paths there). Then every ruling made since the ticket's last model run started: each
//! ruled question raised on the ticket that a run since then recorded, closed after that start, or
//! closed at a time the board does not give, in full. With no model run before, every ruling is.
//! [`WhyThisRun::render`] writes the section.

use std::fmt::Write as _;
use std::time::SystemTime;

use crate::app::actions::{ActionKind, parse, resolve};
use crate::app::schedule::gather::{Beside, gather_one};
use crate::app::schedule::record::{PHASES_DIR, output_of};
use crate::app::schedule::tick::{failed_phase, kind_of};
use crate::app::spawn::phase::Ports;
use crate::domain::TicketId;
use crate::domain::schedule::{ActionName, TicketFacts};
use crate::ports::NewQuestion;

/// The heading of the section.
pub const WHY_HEADING: &str = "## Why this run";

/// What the section says: the phase that failed, if one did, and the rulings since the last model
/// run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WhyThisRun {
    /// The current phase, failed at the head; none when it has not failed.
    pub failed: Option<FailedRun>,
    /// The rulings made since the ticket's last model run started, oldest question first.
    pub rulings: Vec<Ruling>,
}

/// How the current phase failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedRun {
    /// The phase.
    pub phase: ActionName,
    /// The pull request's head it failed at; none without one.
    pub head: Option<String>,
    /// The exit code of the run that failed it; none when the checks alone did.
    pub exit: Option<i32>,
    /// What that code means, as the action's metadata declares it; none when it declares none.
    pub meaning: Option<String>,
    /// The checks that failed at the head.
    pub checks: Vec<String>,
    /// The questions the run raised, all ruled by now.
    pub asked: Vec<TicketId>,
    /// What the run folder kept of the run's output; none when it kept none.
    pub output: Option<String>,
}

/// One ruled question, in full.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ruling {
    /// The question's issue.
    pub question: TicketId,
    /// What it asked.
    pub text: String,
    /// The ruling.
    pub ruling: String,
}

/// Why `task`'s `on-fail` run was started, read as the tick reads the ticket. A ticket the tick
/// would not place (done, closed, of a type with no list) or one something about which cannot be
/// read is the error, in words.
pub fn why_this_run(ports: &Ports<'_>, task: TicketId) -> Result<WhyThisRun, String> {
    let (facts, beside) = gather_one(ports, task)
        .map_err(|skip| skip.reason().to_owned())?
        .ok_or_else(|| format!("{task} is not an open ticket with a phase list"))?;
    let failed = match failed_phase(&facts) {
        None => None,
        Some(failed) => {
            let exit = failed.failure.exit.map(|exit| exit.get());
            let output = match failed.run {
                Some(run) => output_of(&ports.state_dir.join(PHASES_DIR), task, run, ports.alive)?,
                None => None,
            };
            Some(FailedRun {
                phase: failed.phase.name.clone(),
                head: failed.failure.head.as_ref().map(ToString::to_string),
                exit,
                meaning: exit.and_then(|code| meaning(ports, &failed.phase.name, code)),
                checks: failed
                    .failure
                    .checks
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                asked: failed
                    .run
                    .map(|run| run.questions.iter().copied().collect())
                    .unwrap_or_default(),
                output,
            })
        }
    };
    let rulings = rulings_since(ports, task, &facts, &beside)?;
    Ok(WhyThisRun { failed, rulings })
}

/// What exit `code` of `action` means, as its metadata declares it; none when the action cannot be
/// found or read, or declares no such code.
fn meaning(ports: &Ports<'_>, action: &ActionName, code: i32) -> Option<String> {
    let code = u8::try_from(code).ok()?;
    let found = resolve(ports.root, ports.home, action.as_str()).ok()??;
    let text = std::fs::read_to_string(&found.path).ok()?;
    let parsed = parse(&found.path, &text).ok()?;
    parsed.metadata.codes.get(&code).cloned()
}

/// The rulings made since the last model run of `facts` started (see the module above).
fn rulings_since(
    ports: &Ports<'_>,
    task: TicketId,
    facts: &TicketFacts,
    beside: &Beside,
) -> Result<Vec<Ruling>, String> {
    let since: Option<SystemTime> = facts
        .runs
        .iter()
        .filter(|run| !run.live() && kind_of(&beside.list, &run.action) == ActionKind::Model)
        .map(|run| run.started_at)
        .max();
    let recorded_since: Vec<TicketId> = facts
        .runs
        .iter()
        .filter(|run| since.is_none_or(|since| run.started_at > since))
        .flat_map(|run| run.questions.iter().copied())
        .collect();
    let questions = ports
        .board
        .questions_on(task)
        .map_err(|error| error.to_string())?;
    let mut rulings = Vec::new();
    for (question, ruling) in questions {
        let Some(ruling) = ruling else {
            continue;
        };
        let counts = match since {
            None => true,
            Some(_) if recorded_since.contains(&question.id) => true,
            Some(since) => match ports.board.standing_at(question.id) {
                // A ruling closes the question; a time the board does not give is no reason to
                // leave a ruling out.
                Ok(at) => at.closed_at.is_none_or(|closed| closed > since),
                Err(_) => true,
            },
        };
        if counts {
            rulings.push(Ruling {
                question: question.id,
                text: NewQuestion::text_of(&question).to_owned(),
                ruling,
            });
        }
    }
    Ok(rulings)
}

impl WhyThisRun {
    /// The section, its heading first and a blank line after it.
    #[must_use]
    pub fn render(&self) -> String {
        let mut text = format!("{WHY_HEADING}\n\n");
        match &self.failed {
            None => text.push_str(
                "No phase has failed at the pull request's head: this run was started by hand, \
                 or for a ruling alone.\n\n",
            ),
            Some(failed) => failed.render(&mut text),
        }
        text.push_str("### Rulings since the last model run\n\n");
        if self.rulings.is_empty() {
            text.push_str("— none\n\n");
        }
        for ruling in &self.rulings {
            let _ = writeln!(
                text,
                "{} {}\nruling: {}\n",
                ruling.question, ruling.text, ruling.ruling
            );
        }
        text
    }

    /// The section when it cannot be read: why, so the programmer still learns a reason was
    /// looked for.
    #[must_use]
    pub fn unread(reason: &str) -> String {
        format!("{WHY_HEADING}\n\nzaf could not read why this run was started: {reason}\n\n")
    }
}

impl FailedRun {
    fn render(&self, text: &mut String) {
        let at = self.head.as_deref().map_or_else(
            || "with no pull request head".to_owned(),
            |head| format!("at {head}"),
        );
        let _ = write!(text, "Phase `{}` failed {at}", self.phase);
        match (self.exit, &self.meaning) {
            (Some(code), Some(meaning)) => {
                let _ = write!(text, ": exit {code}, {meaning}");
            }
            (Some(code), None) => {
                let _ = write!(text, ": exit {code}");
            }
            (None, _) => {}
        }
        text.push_str(".\n");
        if !self.checks.is_empty() {
            let _ = writeln!(text, "Failed checks: {}.", self.checks.join(", "));
        }
        if !self.asked.is_empty() {
            let asked: Vec<String> = self.asked.iter().map(ToString::to_string).collect();
            let _ = writeln!(
                text,
                "Its run raised {}, ruled since: the ruling is below.",
                asked.join(", ")
            );
        }
        text.push('\n');
        if let Some(output) = self.output.as_deref().map(str::trim_end)
            && !output.is_empty()
        {
            let _ = writeln!(text, "What its run printed last:\n\n```\n{output}\n```\n");
        }
    }
}

#[cfg(test)]
mod tests;
