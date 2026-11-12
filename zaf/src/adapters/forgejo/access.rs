//! What `zaf doctor` asks a Forgejo backend, the board and the forge alike: whether the instance
//! answers, and whether the token reads the repository and may write what the backend writes.
//!
//! A write is proved without writing: a create sent with an empty body, which Forgejo refuses as
//! invalid (422) only once the token's scope and the user's access both let it through, and which
//! opens nothing.

use serde_json::json;

use super::api::{ForgejoApi, Method};
use super::rows::{RepositoryRow, VersionRow};
use crate::ports::{AccessCheck, PortError};

/// The status Forgejo answers a write whose body fails validation: the token got past the
/// scope and permission checks, so it may write.
const UNPROCESSABLE: u16 = 422;

/// What a backend writes, each proved by its own empty create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Write {
    /// Issues and their comments: the board's tickets, the forge's hand-off comment.
    Issues,
    /// Pull requests: the forge's opens and merges.
    PullRequests,
}

impl Write {
    /// The repository endpoint its empty create goes to.
    fn endpoint(self) -> &'static str {
        match self {
            Self::Issues => "issues",
            Self::PullRequests => "pulls",
        }
    }

    /// What a refusal says the token cannot write, after the repository's name.
    fn of_repo(self) -> &'static str {
        match self {
            Self::Issues => "",
            Self::PullRequests => "'s pull requests",
        }
    }
}

/// The instance's version, then the token: whether it reads the repository and may make each of
/// `writes`. `granted` is what a passed token check says, after the variable's name.
pub(super) fn checks(api: &ForgejoApi, writes: &[Write], granted: &str) -> Vec<AccessCheck> {
    let url = api.url();
    let answered: Result<VersionRow, PortError> = api.instance("version");
    let instance = match &answered {
        Ok(version) => AccessCheck {
            what: "instance".to_owned(),
            ok: true,
            detail: format!("Forgejo {} at {url}", version.version),
        },
        Err(error) => AccessCheck {
            what: "instance".to_owned(),
            ok: false,
            detail: format!("no answer from {url}: {error}"),
        },
    };
    let checked = match answered {
        Ok(_) => token(api, writes, granted),
        Err(_) => Err(format!(
            "`{}` not checked: the instance does not answer",
            api.token_env()
        )),
    };
    vec![instance, token_check(checked)]
}

/// Only `writes`, for a backend beside one whose checks already asked the instance and read the
/// repository with the same token: one `token` check.
pub(super) fn writes_only(api: &ForgejoApi, writes: &[Write], granted: &str) -> AccessCheck {
    let checked = present(api).and_then(|()| {
        writes
            .iter()
            .try_for_each(|write| may_write(api, *write))
            .map(|()| format!("`{}` {granted}", api.token_env()))
    });
    token_check(checked)
}

/// The token check from what checking it found.
fn token_check(checked: Result<String, String>) -> AccessCheck {
    AccessCheck {
        what: "token".to_owned(),
        ok: checked.is_ok(),
        detail: checked.unwrap_or_else(|why| why),
    }
}

/// Whether the door holds a token at all.
fn present(api: &ForgejoApi) -> Result<(), String> {
    if api.has_token() {
        return Ok(());
    }
    Err(format!(
        "`{}` is not set: export a Forgejo access token in it, with read and write on the repository's issues and pull requests",
        api.token_env()
    ))
}

/// Whether the token may read and write the repository: it reads the repository and the
/// permissions it grants, then proves each write.
fn token(api: &ForgejoApi, writes: &[Write], granted: &str) -> Result<String, String> {
    present(api)?;
    let repo = api.repo();
    let var = api.token_env();
    let read: RepositoryRow = api
        .get("")
        .map_err(|error| format!("`{var}` cannot read {repo}: {error}"))?;
    let permissions = read.permissions;
    if !permissions.is_some_and(|permissions| permissions.pull) {
        return Err(format!("`{var}` cannot read {repo}'s issues"));
    }
    if !permissions.is_some_and(|permissions| permissions.push) {
        return Err(format!(
            "`{var}` cannot write {repo}: its user has no write access"
        ));
    }
    for write in writes {
        may_write(api, *write)?;
    }
    Ok(format!("`{var}` {granted}"))
}

/// Sends `write`'s create with an empty body: a 422 proves the token may make one.
fn may_write(api: &ForgejoApi, write: Write) -> Result<(), String> {
    let repo = api.repo();
    let var = api.token_env();
    let what = write.of_repo();
    let probe = api
        .probe(Method::Post, write.endpoint(), Some(&json!({})))
        .map_err(|error| format!("`{var}` cannot write {repo}{what}: {error}"))?;
    if probe.status != UNPROCESSABLE {
        return Err(format!(
            "`{var}` cannot write {repo}{what}: Forgejo answered {} {}",
            probe.status,
            probe.message()
        ));
    }
    Ok(())
}
