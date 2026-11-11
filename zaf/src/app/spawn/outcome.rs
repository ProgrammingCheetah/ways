//! What one tick did: [`TickOutcome`] (docs/zaf-actions.md §5.1).
//!
//! The tick fills it in steps, each a task of its own: the `zaf`-kind actions it ran inline
//! ([`TickOutcome::ran`], [`super::inline`]), the model phase it started
//! ([`TickOutcome::started`]), the waits it launched ([`TickOutcome::waits`]) and the tasks whose
//! failed wait raised a question ([`TickOutcome::questions`]). The post-tick step reads the whole.
//! A tick with nothing in any of them is idle.
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

/// What one tick did, as the ticket states it (a task's id is [`TicketId`] here):
///
/// TickOutcome { started: Option<Started { task, action, units }>, waits: Vec<Launched { task, action }>, ran: Vec<Ran { task, action, exit }>, questions: Vec<TaskId> }
///
/// It is idle when all four are empty.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TickOutcome {
    /// The model phase it started, if any: at most one.
    pub started: Option<Started>,
    /// The waits it launched.
    pub waits: Vec<Launched>,
    /// The `zaf`-kind actions it ran inline, in the order they ran.
    pub ran: Vec<Ran>,
    /// The tasks whose failed wait raised a question.
    pub questions: Vec<TicketId>,
}

impl TickOutcome {
    /// Whether the tick did nothing at all.
    pub fn is_idle(&self) -> bool {
        self.started.is_none()
            && self.waits.is_empty()
            && self.ran.is_empty()
            && self.questions.is_empty()
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
        })
    }

    /// The outcome as text, one line each: `idle`, or what ran, what started and what waits.
    pub fn render_text(&self) -> String {
        if self.is_idle() {
            return "idle\n".to_owned();
        }
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
        text
    }
}
