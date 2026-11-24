//! What both issue boards (GitHub's and Forgejo's) keep the same way: the kind labels a ticket
//! carries, and the rules that read a task's states when a question, a ruling or a release
//! touches it.
//!
//! Where each board keeps a task's state differs (a label, a scoped label, a Project field);
//! these rules take the states as read and decide the same way on every board.

use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Deserializer};

use crate::domain::TicketState;
use crate::ports::{Login, PortError};

/// The login in a comment's author object (`{"login": "…"}`, as both forges print one), read
/// by a comment row's `author`: none when the forge printed no author (a deleted account) or an
/// empty login, which no trust check may match.
pub(super) fn author_login<'de, D>(deserializer: D) -> Result<Option<Login>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    struct Named {
        #[serde(default)]
        login: Option<String>,
    }
    let named = Option::<Named>::deserialize(deserializer)?;
    Ok(named
        .and_then(|named| named.login)
        .filter(|login| !login.trim().is_empty())
        .map(Login::from))
}

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

/// The error of a board that lists no trusted login and could not read its own: what the forge
/// said, then the two ways out. A forge that is not there, or did not answer in time, stays as
/// it is: neither way out mends it.
fn own_login_unread(error: PortError) -> PortError {
    match error {
        PortError::NotFound(said)
        | PortError::Conflict(said)
        | PortError::Rejected(said)
        | PortError::Backend(said) => PortError::Backend(format!(
            "the board cannot read the login its token posts as, whose zaf comments it trusts \
             ({said}): set `[board] trusted_logins` in the manifest, or give the token the right \
             to read its user"
        )),
        other @ (PortError::Unavailable(_) | PortError::TimedOut { .. }) => other,
    }
}

/// Whose zaf comments a board trusts: its own login, asked of the forge once and kept (a clone
/// of the board shares the answer), and the logins the manifest lists.
#[derive(Debug, Clone, Default)]
pub(super) struct TrustedLogins {
    listed: Vec<Login>,
    /// The own login once the forge was asked; `None` inside for a forge that could not tell
    /// it while the manifest lists logins, which is then not asked again.
    own: Arc<OnceLock<Option<Login>>>,
}

impl PartialEq for TrustedLogins {
    fn eq(&self, other: &Self) -> bool {
        self.listed == other.listed
    }
}

impl Eq for TrustedLogins {}

impl TrustedLogins {
    /// The set the manifest lists, trusted beside the board's own login.
    pub(super) fn listed(logins: Vec<Login>) -> Self {
        Self {
            listed: logins,
            own: Arc::default(),
        }
    }

    /// The trusted logins: the one `own` answers, called only the first time, then the listed
    /// ones. With none listed a failed ask is the error, which names both ways out
    /// ([`own_login_unread`]), and is not kept. With some listed a failed ask leaves the listed
    /// ones alone, the set the manifest states, and is kept.
    pub(super) fn get(
        &self,
        own: impl FnOnce() -> Result<Login, PortError>,
    ) -> Result<Vec<Login>, PortError> {
        if let Some(known) = self.own.get() {
            return Ok(self.beside(known.as_ref()));
        }
        let asked = match own() {
            Ok(login) => Some(login),
            Err(_) if !self.listed.is_empty() => None,
            Err(error) => return Err(own_login_unread(error)),
        };
        Ok(self.beside(self.own.get_or_init(|| asked).as_ref()))
    }

    /// `own`, when the forge told it, then each listed login that is not it.
    fn beside(&self, own: Option<&Login>) -> Vec<Login> {
        let listed = self.listed.iter().filter(|login| Some(*login) != own);
        own.into_iter().chain(listed).cloned().collect()
    }
}
