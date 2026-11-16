//! `POST /api/drop` with `{"project": "<folder>"}`: takes a project off the zaf home's list through
//! [`Projects::drop`], only when its entry reads missing. The folder itself is never touched.
//!
//! The project is its root as `GET /api/state` shows it. Answers `200` with `{"dropped": <folder>}`.
//! A project that is present is `409` with the reason and stays; a folder the list does not hold,
//! `404`; a body that is not that, `400`; a machine with no zaf home, `503`; a list that could not
//! be changed, `500`.

use std::path::Path;

use super::Answer;
use super::project::listed;
use crate::app::projects::Projects;

/// What a body that is not a drop is told.
const NEEDS: &str = "a drop needs a JSON body with \"project\" (the project's folder)";

pub(super) fn run(home: Option<&Path>, body: &[u8]) -> Answer {
    let Some(project) = parse(body) else {
        return Answer::refused("400 Bad Request", NEEDS);
    };
    let Some(home) = home else {
        return Answer::refused(
            "503 Service Unavailable",
            "this machine has no zaf home to drop the project from",
        );
    };
    // The entry is read now, not trusted from an earlier state, so a folder that came back since the
    // page was drawn is present and stays.
    // The folder is matched as the list keeps it (symlinks resolved when it exists) or as given,
    // as every other control finds its project.
    let entry = match listed(home, Path::new(&project)) {
        Ok(entry) => entry,
        Err(refusal) => return Answer::refused(refusal.status, &refusal.why),
    };
    if !entry.missing {
        return Answer::refused(
            "409 Conflict",
            &format!("{project} is present: only a project marked missing can be dropped"),
        );
    }
    match Projects::drop(home, &entry.folder) {
        Ok(()) => Answer::done("200 OK", &serde_json::json!({ "dropped": project })),
        Err(error) => Answer::refused(
            "500 Internal Server Error",
            &format!("the list could not be changed: {error}"),
        ),
    }
}

/// The folder a body names; none for a body that is not an object with a string `project`.
fn parse(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    Some(value.get("project")?.as_str()?.to_owned())
}

#[cfg(test)]
mod tests;
