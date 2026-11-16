//! The zaf home's list of projects: every folder a verb has run in, or `zaf setup` has set up.
//!
//! The list is `projects.toml` in the zaf home, one `[[project]]` table with a `folder` for each
//! project. A project joins it the first time any verb runs from its root; a folder already on
//! it is not added twice. Reading marks an entry whose folder or manifest is gone as missing and
//! never fails; an entry can be dropped. Two zaf processes may write at the same time: every
//! write holds an exclusive lock on `projects.lock` and reads the list again under it, so one
//! never loses the entry the other added.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::manifest::ProjectFiles;

/// The list's file name in the zaf home.
pub const PROJECTS_FILE: &str = "projects.toml";

/// The lock file beside the list; writers take it exclusively.
pub const PROJECTS_LOCK: &str = "projects.lock";

/// One project on the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    /// The project's root folder, as it was added.
    pub folder: PathBuf,
    /// The project's name: its folder's last component.
    pub name: String,
    /// Whether the folder, or every manifest file in it, is gone.
    pub missing: bool,
    /// The version the hub's tick builds for this project, once one was given to it; none until
    /// then. The Engineer names it once and the hub remembers it here.
    pub version: Option<String>,
}

/// What the file holds.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Document {
    #[serde(default, rename = "project")]
    projects: Vec<Stored>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Stored {
    folder: PathBuf,
    /// The remembered version; a list written before it was kept reads as none, and an entry with
    /// none writes no key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

/// The list of projects, read and written through the zaf home.
#[derive(Debug, Clone, Copy)]
pub struct Projects;

impl Projects {
    /// Every project on the list, in the order they joined, each marked missing when its folder
    /// or its manifest is gone. A list that is not there, or cannot be read, is empty.
    pub fn read(home: &Path) -> Vec<ProjectEntry> {
        load(home)
            .unwrap_or_default()
            .projects
            .into_iter()
            .map(|stored| entry(home, stored))
            .collect()
    }

    /// Puts the project at `root` on the list; one already on it is left as it is. Makes the
    /// zaf home when it is missing. A list that exists but cannot be parsed is refused, never
    /// overwritten.
    pub fn add(home: &Path, root: &Path) -> io::Result<()> {
        let folder = canonical(root);
        locked(home, |document| {
            if document
                .projects
                .iter()
                .any(|stored| stored.folder == folder)
            {
                return false;
            }
            document.projects.push(Stored {
                folder,
                version: None,
            });
            true
        })
    }

    /// Remembers `version` on the entry of the project at `root`, in place of one it had. Refused
    /// with `NotFound` for a project that is not on the list: nothing is added by it.
    pub fn remember_version(home: &Path, root: &Path, version: &str) -> io::Result<()> {
        let folder = canonical(root);
        let given = root.to_owned();
        let mut found = false;
        locked(home, |document| {
            let Some(stored) = document
                .projects
                .iter_mut()
                .find(|stored| stored.folder == folder || stored.folder == given)
            else {
                return false;
            };
            found = true;
            if stored.version.as_deref() == Some(version) {
                return false;
            }
            stored.version = Some(version.to_owned());
            true
        })?;
        if found {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{} is not on the project list", root.display()),
            ))
        }
    }

    /// Takes the project at `root` off the list; one that is not on it is quietly nothing. The
    /// folder itself is never touched.
    pub fn drop(home: &Path, root: &Path) -> io::Result<()> {
        let given = root.to_owned();
        let folder = canonical(root);
        locked(home, |document| {
            let before = document.projects.len();
            document
                .projects
                .retain(|stored| stored.folder != folder && stored.folder != given);
            document.projects.len() != before
        })
    }
}

fn entry(home: &Path, stored: Stored) -> ProjectEntry {
    let Stored { folder, version } = stored;
    let missing = !folder.is_dir() || ProjectFiles::find(&folder, Some(home)).is_empty();
    let name = folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    ProjectEntry {
        folder,
        name,
        missing,
        version,
    }
}

/// The folder as the list keeps it: with symlinks and `..` resolved when it exists, as given
/// when it does not.
fn canonical(root: &Path) -> PathBuf {
    fs::canonicalize(root).unwrap_or_else(|_| root.to_owned())
}

/// The list as the file holds it: none when there is no file, an error when it cannot be read
/// or parsed.
fn load(home: &Path) -> io::Result<Document> {
    match fs::read_to_string(home.join(PROJECTS_FILE)) {
        Ok(text) => toml::from_str(&text).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is not a project list: {error}", PROJECTS_FILE),
            )
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Document::default()),
        Err(error) => Err(error),
    }
}

/// Runs `change` on the list under the exclusive lock, re-reading it first, and writes it back
/// (to a temporary file, then renamed over the list) when `change` says it changed something.
fn locked(home: &Path, change: impl FnOnce(&mut Document) -> bool) -> io::Result<()> {
    fs::create_dir_all(home)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join(PROJECTS_LOCK))?;
    lock.lock()?;
    let mut document = load(home)?;
    if !change(&mut document) {
        return Ok(());
    }
    let text = toml::to_string(&document)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let temp = home.join(format!("{PROJECTS_FILE}.{}.tmp", std::process::id()));
    File::create(&temp).and_then(|mut file| io::Write::write_all(&mut file, text.as_bytes()))?;
    fs::rename(&temp, home.join(PROJECTS_FILE)).inspect_err(|_| {
        let _ = fs::remove_file(&temp);
    })
    // The lock is released when `lock` drops, after the rename.
}

#[cfg(test)]
mod tests;
