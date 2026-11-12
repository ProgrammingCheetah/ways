//! What both issue boards (GitHub's and Forgejo's) keep the same way: the kind labels a ticket
//! carries, and the rules that read a task's states when a question, a ruling or a release
//! touches it.
//!
//! Where each board keeps a task's state differs (a label, a scoped label, a Project field);
//! these rules take the states as read and decide the same way on every board.

use crate::domain::TicketState;

/// The label every task carries.
pub(super) const TASK_LABEL: &str = "task";
/// The label every epic carries.
pub(super) const EPIC_LABEL: &str = "epic";
/// The label every question ticket carries, beside the state it holds.
pub(super) const QUESTION_LABEL: &str = "question";
/// The label a question raised as a one-way decision carries beside `question`.
pub(super) const DOOR_LABEL: &str = "door";
/// The prefix of the label that puts a task in a group.
pub(super) const GROUP_PREFIX: &str = "group:";
/// The colour a group label is created in.
pub(super) const GROUP_COLOR: &str = "ededed";

/// The name in the first `group:<name>` label among `labels`.
pub(super) fn group<'a>(labels: impl IntoIterator<Item = &'a str>) -> Option<String> {
    labels
        .into_iter()
        .find_map(|label| label.strip_prefix(GROUP_PREFIX))
        .map(str::to_owned)
}

/// Whether a question raised on an issue in `states` makes it wait: a task that is `claimed`
/// or `ready`. An epic, and a task that was never ready, carry no state a ruling could restore.
pub(super) fn can_wait(epic: bool, states: &[TicketState]) -> bool {
    !epic
        && [TicketState::Claimed, TicketState::Ready]
            .iter()
            .any(|state| states.contains(state))
}

/// Whether a ruling returns the issue in `states` to `ready`: a task that waits on a question
/// and is not `done`.
pub(super) fn waits_on_ruling(epic: bool, states: &[TicketState]) -> bool {
    !epic && !states.contains(&TicketState::Done) && states.contains(&TicketState::Question)
}

/// Whether a released task in `states` keeps its state instead of returning to `ready`: it
/// waits on a question or is `done`.
pub(super) fn keeps_state_on_release(states: &[TicketState]) -> bool {
    [TicketState::Question, TicketState::Done]
        .iter()
        .any(|state| states.contains(state))
}
