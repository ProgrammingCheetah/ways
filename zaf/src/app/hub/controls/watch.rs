//! `POST /api/watch` with `{"project": <root>, "action": "start" | "stop", "version": <v>}`:
//! starts or stops the project's watch, the loop `zaf spawn --watch` runs, inside the hub.
//!
//! A start runs the loop for the project on the version the body gives, which is remembered as for
//! `POST /api/tick`, or the one remembered; a tick runs at once. A project that a scheduler already
//! runs for, the hub's own or a `zaf spawn --watch` in a terminal, is refused with the reason and
//! nothing is changed. A stop ends the hub's own watch after its current tick; a scheduler the hub
//! did not start is never stopped from here. Every watch the hub started ends with the hub
//! ([`crate::app::spawn::watch`]).
//!
//! Answers `200` with what happened (`project`, `action`, `watching`, and for a start its
//! `version` and `since`). A stop whose tick still runs is `202`: the watch ends once it has. A
//! refusal of the state is `409`, a body that is not that `400`, a project the list does not hold
//! `404`, a machine that could not do it `500`; every refusal is `{"refused": <why>}`.

use std::path::{Path, PathBuf};

use super::project::{self, Refusal};
use super::{Answer, Failure, Watcher};
use crate::app::spawn::watch::Stopped;

/// What a body that is not a watch is told.
const NEEDS: &str = "a watch needs a JSON body with \"project\" (a listed project) and \"action\" \
                     (\"start\" or \"stop\")";

/// What the body asks for.
enum Action {
    Start(Option<String>),
    Stop,
}

pub(super) fn run(home: Option<&Path>, watcher: &dyn Watcher, body: &[u8]) -> Answer {
    let Some((project, action)) = parse(body) else {
        return Answer::refused("400 Bad Request", NEEDS);
    };
    let Some(home) = home else {
        return Answer::refused(
            "503 Service Unavailable",
            "this machine has no zaf home to read the projects from",
        );
    };
    let entry = match project::listed(home, &project) {
        Ok(entry) => entry,
        Err(Refusal { status, why }) => return Answer::refused(status, &why),
    };
    let root = entry.folder.display().to_string();
    match action {
        Action::Start(given) => {
            if entry.missing {
                return Answer::refused(
                    "409 Conflict",
                    &format!("{root} is gone from this machine, or has no manifest: no watch"),
                );
            }
            let version = match project::version_for(home, &entry, given) {
                Ok(version) => version,
                Err(Refusal { status, why }) => return Answer::refused(status, &why),
            };
            match watcher.start(&entry.folder, &version) {
                Ok(started) => Answer::done(
                    "200 OK",
                    &serde_json::json!({
                        "project": root,
                        "action": "start",
                        "watching": true,
                        "version": started.version,
                        "since": started.since,
                        "tick": "the first tick runs at once",
                    }),
                ),
                Err(failure) => failed(failure),
            }
        }
        Action::Stop => match watcher.stop(&entry.folder) {
            Ok(Stopped::Ended) => Answer::done(
                "200 OK",
                &serde_json::json!({ "project": root, "action": "stop", "watching": false }),
            ),
            Ok(Stopped::Ending) => Answer::done(
                "202 Accepted",
                &serde_json::json!({
                    "project": root,
                    "action": "stop",
                    "watching": true,
                    "stopping": "it ends once its current tick has",
                }),
            ),
            Err(failure) => failed(failure),
        },
    }
}

fn failed(failure: Failure) -> Answer {
    match failure {
        Failure::Refused(why) => Answer::refused("409 Conflict", &why),
        Failure::Failed(why) => Answer::refused("500 Internal Server Error", &why),
    }
}

/// The project and the action a body names; none for a body that is not an object with a string
/// `project`, an `action` of `start` or `stop`, and for a start a string `version` when it has one.
fn parse(body: &[u8]) -> Option<(PathBuf, Action)> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let project = PathBuf::from(value.get("project")?.as_str()?);
    let action = match value.get("action")?.as_str()? {
        "start" => Action::Start(match value.get("version") {
            None | Some(serde_json::Value::Null) => None,
            Some(version) => Some(version.as_str()?.to_owned()),
        }),
        "stop" => Action::Stop,
        _ => return None,
    };
    Some((project, action))
}

#[cfg(test)]
mod tests;
