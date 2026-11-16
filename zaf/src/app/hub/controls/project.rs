//! What the controls that run for a project share: the listed project a body names, and the
//! version it is run for, given or remembered on its entry (`POST /api/tick`, `POST /api/watch`).

use std::io;
use std::path::Path;

use super::tick::valid_version;
use crate::app::projects::{ProjectEntry, Projects};

/// A request refused before anything ran: the status and why, in plain words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Refusal {
    pub status: &'static str,
    pub why: String,
}

impl Refusal {
    pub(super) fn new(status: &'static str, why: impl Into<String>) -> Self {
        Self {
            status,
            why: why.into(),
        }
    }
}

/// The entry of the project whose root is `project`, or the refusal: a project not on the list is
/// never reached.
pub(super) fn listed(home: &Path, project: &Path) -> Result<ProjectEntry, Refusal> {
    let wanted = std::fs::canonicalize(project).unwrap_or_else(|_: io::Error| project.to_owned());
    Projects::read(home)
        .into_iter()
        .find(|entry| entry.folder == wanted || entry.folder == project)
        .ok_or_else(|| {
            Refusal::new(
                "404 Not Found",
                format!(
                    "{} is not on this machine's project list",
                    project.display()
                ),
            )
        })
}

/// The version a run for `entry` builds: the one `given`, which replaces the remembered one, else
/// the remembered one; refused when there is neither. A version that is not the remembered one is
/// remembered before the run, and a version that cannot be remembered is why nothing runs.
pub(super) fn version_for(
    home: &Path,
    entry: &ProjectEntry,
    given: Option<String>,
) -> Result<String, Refusal> {
    let root = entry.folder.display();
    let version = match (given, &entry.version) {
        (Some(given), _) => {
            if !valid_version(&given) {
                return Err(Refusal::new(
                    "400 Bad Request",
                    format!(
                        "{given:?} is not a version: use letters, digits, dots and hyphens, \
                         such as 0.4.0"
                    ),
                ));
            }
            given
        }
        (None, Some(remembered)) => remembered.clone(),
        (None, None) => {
            return Err(Refusal::new(
                "400 Bad Request",
                format!(
                    "a version is needed: none is remembered for {root}; send version once, \
                     the milestone its instances build"
                ),
            ));
        }
    };
    if entry.version.as_deref() != Some(version.as_str())
        && let Err(error) = Projects::remember_version(home, &entry.folder, &version)
    {
        return Err(Refusal::new(
            "500 Internal Server Error",
            format!("the version could not be remembered, so nothing ran: {error}"),
        ));
    }
    Ok(version)
}
