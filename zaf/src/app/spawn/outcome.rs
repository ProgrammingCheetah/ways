//! What one tick did: [`TickOutcome`] (docs/zaf-actions.md §5.1).
//!
//! The tick fills it in steps, each a task of its own: the `zaf`-kind actions it ran inline
//! ([`TickOutcome::ran`], [`super::inline`]), the model phase it started
//! ([`TickOutcome::started`]), the waits it launched ([`TickOutcome::waits`]), the issues it
//! raised a question on ([`TickOutcome::questions`]) and each question with its cause
//! ([`TickOutcome::asked`], [`crate::app::schedule::ask`]). The post-tick step reads the whole. A
//! tick with nothing in any of them is idle.
//!
//! The outcome is a record of what was done, never a store: nothing reads it back to decide what
//! the next tick does, which is derived again from the forge and the run folders.

use crate::app::actions::ActionName;
use crate::domain::TicketId;

/// The model phase a tick started, and what it holds of the budget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    /// The task.
    pub task: TicketId,
    /// The action it started: the first of a run of adjacent model actions.
    pub action: ActionName,
    /// The units the run costs: the task's size.
    pub units: u32,
}

/// A wait the tick launched, which costs nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launched {
    /// The task.
    pub task: TicketId,
    /// The wait's action.
    pub action: ActionName,
}

/// A `zaf`-kind action the tick ran inline with `zaf do`.
///
/// `exit` is the code `zaf do` gave: 0 when it passed, 125 when it never ran (`NOT_RUN`), and any
/// other code is the action's own failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The task it ran for.
    pub task: TicketId,
    /// The action.
    pub action: ActionName,
    /// The exit code of `zaf do <action> <task>`.
    pub exit: u8,
}

impl Ran {
    /// Whether the action passed.
    pub fn passed(&self) -> bool {
        self.exit == 0
    }
}

/// A question the tick raised, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The issue it was raised on: a task, or an epic.
    pub task: TicketId,
    /// The question.
    pub question: TicketId,
    /// Its cause in a few words: `merge got nowhere three times at d962227`.
    pub cause: String,
}

impl Asked {
    /// The line the tick's output names it by: `asked #61 on #7: merge got nowhere three times at
    /// d962227`.
    pub fn line(&self) -> String {
        format!("asked {} on {}: {}", self.question, self.task, self.cause)
    }
}

/// What one tick did, as the ticket states it (a task's id is [`TicketId`] here):
///
/// TickOutcome { started: Option<Started { task, action, units }>, waits: Vec<Launched { task, action }>, ran: Vec<Ran { task, action, exit }>, questions: Vec<TaskId>, asked: Vec<Asked { task, question, cause }> }
///
/// It is idle when all of them are empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickOutcome {
    /// The model phase it started, if any: at most one.
    pub started: Option<Started>,
    /// The waits it launched.
    pub waits: Vec<Launched>,
    /// The `zaf`-kind actions it ran inline, in the order they ran.
    pub ran: Vec<Ran>,
    /// The issues a question was raised on, in order.
    pub questions: Vec<TicketId>,
    /// The questions raised, each with its cause, in order: the issue of each is in
    /// [`TickOutcome::questions`].
    pub asked: Vec<Asked>,
}

impl TickOutcome {
    /// Whether the tick did nothing at all.
    pub fn is_idle(&self) -> bool {
        self.started.is_none()
            && self.waits.is_empty()
            && self.ran.is_empty()
            && self.questions.is_empty()
            && self.asked.is_empty()
    }

    /// The units the tick added to the budget in use: those of the phase it started. A wait and a
    /// `zaf`-kind action add none.
    pub fn units(&self) -> u32 {
        self.started.as_ref().map_or(0, |started| started.units)
    }

    /// The outcome as `--json` prints it.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "idle": self.is_idle(),
            "started": self.started.as_ref().map(|started| serde_json::json!({
                "task": started.task.get(),
                "action": started.action.as_str(),
                "units": started.units,
            })),
            "waits": self.waits.iter().map(|wait| serde_json::json!({
                "task": wait.task.get(),
                "action": wait.action.as_str(),
            })).collect::<Vec<_>>(),
            "ran": self.ran.iter().map(|ran| serde_json::json!({
                "task": ran.task.get(),
                "action": ran.action.as_str(),
                "exit": ran.exit,
            })).collect::<Vec<_>>(),
            "questions": self.questions.iter().map(|task| task.get()).collect::<Vec<_>>(),
            "asked": self.asked.iter().map(|asked| serde_json::json!({
                "task": asked.task.get(),
                "question": asked.question.get(),
                "cause": asked.cause,
            })).collect::<Vec<_>>(),
        })
    }

    /// The outcome as text, one line each: what ran, what started, what waits and what was asked,
    /// then each question with its cause ([`Asked::line`]). An idle tick gives no text: the tick's
    /// block says `nothing to do` ([`super::digest`]).
    pub fn render_text(&self) -> String {
        let mut text = String::new();
        for ran in &self.ran {
            text.push_str(&format!(
                "ran {} {}: exit {}\n",
                ran.action, ran.task, ran.exit
            ));
        }
        if let Some(started) = &self.started {
            text.push_str(&format!(
                "started {} {}: {} units\n",
                started.action, started.task, started.units
            ));
        }
        for wait in &self.waits {
            text.push_str(&format!("wait {} {}\n", wait.action, wait.task));
        }
        for task in &self.questions {
            text.push_str(&format!("question on {task}\n"));
        }
        for asked in &self.asked {
            text.push_str(&format!("{}\n", asked.line()));
        }
        text
    }
}
