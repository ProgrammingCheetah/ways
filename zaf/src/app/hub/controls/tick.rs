//! `POST /api/tick`: one spawner tick for a project, as `zaf spawn --once --no-wait` runs it.
//!
//! The body is `{"project": "<root>", "version": "<v>"}`, `project` the root as `GET /api/state`
//! names it and `version` only the first time. The project must be on the zaf home's list; no
//! other folder is ever ticked. The version is remembered on the project's entry
//! ([`Projects::remember_version`]) and asked for again only when none is remembered; one given
//! replaces the remembered one. The tick runs through the [`Ticker`] the hub was given, from the
//! project's root, and what it did is the answer in plain words.

use std::path::{Path, PathBuf};

use super::Answer;
use super::project::{self, Refusal};

/// What a tick's request came to, before it is written as the control's [`Answer`]: a status and
/// the JSON body, which is `{"refused": "<why>"}` for any refusal.
#[derive(Debug, Clone, PartialEq)]
struct Reply {
    status: &'static str,
    body: serde_json::Value,
}

impl Reply {
    fn done(body: serde_json::Value) -> Self {
        Self {
            status: "200 OK",
            body,
        }
    }

    fn refused(status: &'static str, why: impl Into<String>) -> Self {
        Self {
            status,
            body: serde_json::json!({ "refused": why.into() }),
        }
    }
}

/// Runs `POST /api/tick` on `body`, as the guard admitted it.
pub(super) fn run(home: Option<&Path>, ticker: &dyn Ticker, body: &[u8]) -> Answer {
    let reply = handle(home, ticker, body);
    Answer::done(reply.status, &reply.body)
}

/// What a [`Ticker`] came back with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ticked {
    /// The tick ended, exit `0`; what it printed, line by line: what it started, held back and
    /// skipped.
    Ran(Vec<String>),
    /// The tick has not ended: a model it launched still runs, and the hub does not wait for it.
    /// What it printed so far, which holds what it started.
    Running(Vec<String>),
    /// The tick refused, exit `1`: not ready, held, a profile out of date; why, as it said.
    Refused(String),
    /// The tick could not run, or a backend failed under it; why.
    Failed(String),
}

/// What runs one tick for a project. The hub's own runs the `zaf` binary (`zaf spawn --once
/// --no-wait --version <v>`) with the project's root as its folder, so the tick is the app code
/// the verb runs, whichever project the hub itself was started in.
pub trait Ticker: Send + Sync {
    /// Runs one tick of the project at `root` for `version`.
    fn tick(&self, root: &Path, version: &str) -> Ticked;
}

/// The body of a request, as far as it is read.
#[derive(Debug, serde::Deserialize)]
struct Asked {
    project: Option<PathBuf>,
    version: Option<String>,
}

/// The longest version a tick takes.
const MAX_VERSION: usize = 64;

/// Whether `version` is one a tick may be given: a milestone such as `0.4.0` or `1.0.0-rc.1`,
/// letters, digits, dots, hyphens, underscores and plus signs, never starting with a hyphen since
/// it is handed on as an argument.
pub fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= MAX_VERSION
        && !version.starts_with('-')
        && version
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// Answers a `POST /api/tick` with `body`.
fn handle(home: Option<&Path>, ticker: &dyn Ticker, body: &[u8]) -> Reply {
    const BAD: &str = "400 Bad Request";
    let Some(home) = home else {
        return Reply::refused(
            "503 Service Unavailable",
            "this machine has no zaf home to read the projects from",
        );
    };
    let Ok(asked) = serde_json::from_slice::<Asked>(body) else {
        return Reply::refused(
            BAD,
            "the body must be JSON with a project and, the first time, a version",
        );
    };
    let Some(project) = asked.project else {
        return Reply::refused(BAD, "name the project to tick: send its root as project");
    };
    let refused = |refusal: Refusal| Reply::refused(refusal.status, refusal.why);
    let entry = match project::listed(home, &project) {
        Ok(entry) => entry,
        Err(refusal) => return refused(refusal),
    };
    if entry.missing {
        return Reply::refused(
            "409 Conflict",
            format!(
                "{} is gone from this machine, or has no manifest: nothing to tick",
                entry.folder.display()
            ),
        );
    }
    let version = match project::version_for(home, &entry, asked.version) {
        Ok(version) => version,
        Err(refusal) => return refused(refusal),
    };
    match ticker.tick(&entry.folder, &version) {
        Ticked::Ran(said) => answered(&version, &said, false),
        Ticked::Running(said) => answered(&version, &said, true),
        Ticked::Refused(why) => Reply::refused("409 Conflict", why),
        Ticked::Failed(why) => Reply::refused("502 Bad Gateway", why),
    }
}

/// The answer to a tick that ran: the version it ran for, what it said, and whether it is still
/// running a model the hub does not wait for.
fn answered(version: &str, said: &[String], running: bool) -> Reply {
    let mut lines: Vec<&str> = said.iter().map(String::as_str).collect();
    if lines.is_empty() {
        lines.push("the tick ran and said nothing");
    }
    if running {
        lines.push("the tick has not ended; the hub does not wait for it or the models it started");
    }
    Reply::done(serde_json::json!({
        "version": version,
        "running": running,
        "tick": lines,
    }))
}

#[cfg(test)]
mod tests;
