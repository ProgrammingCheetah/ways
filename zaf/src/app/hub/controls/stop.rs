//! `POST /api/stop` with `{"project": <root or name>, "task": <number>}`: ends the task's running
//! instance and releases its claim, as `zaf spawn stop` does from that project's root.
//!
//! Answers `200` with what the verb's `--json` says (`task`, `stopped`, `released`). A task with
//! no live instance is `409` and changes nothing; a body that is not that, `400`; a project the
//! list does not hold or holds as missing, `404`; a backend that failed, `500`.

use std::path::{Path, PathBuf};

use super::{Answer, Failure, Stopper};
use crate::app::projects::{ProjectEntry, Projects};
use crate::app::spawn::stop::Stopped;
use crate::domain::TicketId;

/// What a body that is not a stop is told.
const NEEDS: &str =
    "a stop needs a JSON body with \"project\" (a listed project) and \"task\" (a number)";

pub(super) fn run(home: Option<&Path>, stop: &Stopper, body: &[u8]) -> Answer {
    let Some((project, task)) = parse(body) else {
        return Answer::refused("400 Bad Request", NEEDS);
    };
    let root = match resolve(home, &project) {
        Ok(root) => root,
        Err(why) => return Answer::refused("404 Not Found", &why),
    };
    answer(stop(&root, task))
}

/// The answer to what the stop came to.
fn answer(result: Result<Stopped, Failure>) -> Answer {
    match result {
        Ok(stopped) => Answer::done("200 OK", &report(&stopped)),
        Err(Failure::Refused(why)) => Answer::refused("409 Conflict", &why),
        Err(Failure::Failed(why)) => Answer::refused("500 Internal Server Error", &why),
    }
}

/// What the verb's `--json` prints.
fn report(stopped: &Stopped) -> serde_json::Value {
    serde_json::json!({
        "task": stopped.task.get(),
        "stopped": stopped.runs.iter().map(|run| serde_json::json!({
            "instance": run.instance,
            "pid": run.pid,
            "run": run.run_dir,
            "killed": run.killed,
        })).collect::<Vec<_>>(),
        "released": stopped.released.iter().map(|released| {
            released.instance.to_string()
        }).collect::<Vec<_>>(),
    })
}

/// The project and the task a body names; none for a body that is not an object with a string
/// `project` and a whole, positive `task`.
fn parse(body: &[u8]) -> Option<(String, TicketId)> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let project = value.get("project")?.as_str()?.to_owned();
    let task = value.get("task")?.as_u64().filter(|task| *task > 0)?;
    Some((project, TicketId::from(task)))
}

/// The root of the listed project that `project` names: its folder as the list holds it, or its
/// name when no other listed project has it. A project not on the list is never reached.
pub(super) fn resolve(home: Option<&Path>, project: &str) -> Result<PathBuf, String> {
    let home = home.ok_or("this machine has no zaf home to read the projects from")?;
    let listed = Projects::read(home);
    let by_folder = |entry: &&ProjectEntry| entry.folder == Path::new(project);
    let by_name = |entry: &&ProjectEntry| entry.name == project;
    let entry = if let Some(entry) = listed.iter().find(by_folder) {
        entry
    } else {
        let mut named = listed.iter().filter(by_name);
        match (named.next(), named.next()) {
            (Some(entry), None) => entry,
            (Some(_), Some(_)) => {
                return Err(format!(
                    "{project} names more than one listed project; name its folder"
                ));
            }
            _ => return Err(format!("{project} is not a listed project")),
        }
    };
    if entry.missing {
        return Err(format!("{project} is missing from this machine"));
    }
    Ok(entry.folder.clone())
}

#[cfg(test)]
mod tests;
