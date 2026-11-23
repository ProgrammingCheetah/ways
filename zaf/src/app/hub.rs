//! `zaf hub`: the Engineer's live view of what is happening (docs/zaf-actions.md §5).
//!
//! The hub only reads: it starts nothing, changes nothing and costs no model turns.
//! [`Snapshot`] is what the hub shows, [`draw`] turns one into a frame and reads no state,
//! and [`gather`] reads the board, the ledger, the spawner's state, the phases and the last
//! reports into a snapshot. [`Watch`] redraws it when a report arrives, [`Report`] is what verbs
//! and the profiles' hook leave in `.zaf/hub/reports`, and [`ToolCall`] is the hook's payload.
//! [`Dashboard`] serves the frame as an HTML page on the LAN (`zaf hub --port <n>`);
//! [`guard::Key`] is the key the hub makes at start, which every control must carry, and
//! [`controls`] are the routes it then admits (`POST /api/stop`, `POST /api/tick`).
//! [`state::ProjectState`] is one project's whole state as one value that serializes to JSON.

use std::time::Duration;

use super::actions::Phase;
use crate::domain::EpicStage;

pub mod controls;
mod dashboard;
mod draw;
mod gather;
pub mod guard;
mod hook;
pub mod link;
mod live;
pub mod lock;
pub mod next;
mod phases;
mod reports;
pub mod service;
pub mod state;
mod watch;

pub use dashboard::{BindError, Dashboard, Page, StateReader, lan_address, lan_url};
pub use draw::draw;
pub use gather::{HubError, HubInputs, LineState, Places, gather, line_state, shown_state};
pub use hook::{PROGRAMMER, TEAM, TESTING, ToolCall, WAITING_ON_REVIEW, task_in_workspace};
pub use live::Liveness;
pub use phases::{PhaseError, PhasePlan, declare, enter, plans_dir, plans_of};
pub use reports::{MAX_LINES, Report, WORKING, append, last_report, record, reports_file};
pub use watch::{Change, Changes, Mark, Watch, Watched, Why};

/// Everything one frame shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// One row per open epic, in issue order.
    pub epics: Vec<EpicRow>,
    /// The board's claims when it was read, so a redraw can tell which belong to an instance
    /// that has ended; never drawn.
    pub claims: Vec<crate::ports::ClaimRecord>,
}

/// An open epic: the count of its tasks done, what it cost, what is free, ready and asked.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EpicRow {
    /// Its issue number.
    pub number: u64,
    /// Its title, whole: the frame cuts it to the width.
    pub title: String,
    /// Where it stands, derived from the board.
    pub stage: EpicStage,
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
    /// One row per model that declared phases on the epic's own number, by role.
    pub models: Vec<ModelRow>,
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
    /// Its title, whole: the frame cuts it to the width.
    pub title: String,
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
