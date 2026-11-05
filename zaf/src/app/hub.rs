//! `zaf hub`: the Engineer's live view of what is happening (docs/zaf-actions.md §5).
//!
//! The hub only reads: it starts nothing, changes nothing and costs no model turns.
//! [`Snapshot`] is what the hub shows, [`draw`] turns one into a frame and reads no state,
//! and [`gather`] reads the board, the ledger, the spawner's state, the phases and the last
//! reports into a snapshot. [`Watch`] redraws it when a report arrives, [`Report`] is what verbs
//! and the profiles' hook leave in `.zaf/hub/reports`, and [`ToolCall`] is the hook's payload.
//! [`Dashboard`] serves the frame as an HTML page on the LAN (`zaf hub --port <n>`).

use std::time::Duration;

use super::actions::Phase;

mod dashboard;
mod draw;
mod gather;
mod hook;
mod phases;
mod reports;
mod watch;

pub use dashboard::{BindError, Dashboard, Page, lan_address, lan_url};
pub use draw::draw;
pub use gather::{HubError, HubInputs, gather};
pub use hook::{PROGRAMMER, TEAM, TESTING, ToolCall, WAITING_ON_REVIEW, task_in_workspace};
pub use phases::{PhaseError, PhasePlan, declare, enter, plans_dir, plans_of};
pub use reports::{MAX_LINES, Report, WORKING, append, last_report, record, reports_file};
pub use watch::{Mark, Watch};

/// Everything one frame shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// One row per open epic, in issue order.
    pub epics: Vec<EpicRow>,
}

/// An open epic: the count of its tasks done, what it cost, what is free, ready and asked.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EpicRow {
    /// Its issue number.
    pub number: u64,
    /// Its tasks done.
    pub done: u32,
    /// Its tasks, done ones included.
    pub total: u32,
    /// What its runs cost in the ledger, estimated, in dollars.
    pub dollars: f64,
    /// The spawner's slots with no instance in them.
    pub slots_free: u32,
    /// All the spawner's slots.
    pub slots_total: u32,
    /// Its tasks open for a claim.
    pub ready: u32,
    /// Its open questions for the Engineer.
    pub questions: u32,
    /// The claimed tasks that carry no group, shown directly under the epic.
    pub tickets: Vec<TicketRow>,
    /// Its groups, each with the claimed tasks in it.
    pub groups: Vec<GroupRow>,
}

/// A `group:<name>` label and the epic's tasks that carry it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GroupRow {
    /// The name in the label.
    pub name: String,
    /// Its tasks done.
    pub done: u32,
    /// Its tasks, done ones included.
    pub total: u32,
    /// The claimed tasks in it.
    pub tickets: Vec<TicketRow>,
}

/// A claimed task and the instance on it.
#[derive(Debug, Clone, PartialEq)]
pub struct TicketRow {
    /// Its issue number.
    pub number: u64,
    /// Who is on it: the model and the role, as `sonnet programmer`.
    pub who: String,
    /// Where it stands, one of the Team Phases.
    pub phase: Phase,
    /// One general word for what the instance is doing.
    pub word: String,
    /// Since its last report; none when it has made none.
    pub since: Option<Duration>,
    /// One row per model that declared phases on it, by role.
    pub models: Vec<ModelRow>,
}

/// A model's own phases on a task: the ones it declared and where it is among them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRow {
    /// The role that declared them, as `programmer`.
    pub role: String,
    /// The declared phases, in order.
    pub phases: Vec<String>,
    /// The phase it reported last; none before its first report.
    pub current: Option<String>,
    /// How often it has entered the current phase: 1 the first time, 2 on the second, zero before
    /// its first report.
    pub iteration: u32,
}

#[cfg(test)]
mod tests;
