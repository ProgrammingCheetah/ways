//! The run folders: one per launch, the record of who ran what, and whether it still runs.
//!
//! `<state dir>/runs/<instance slug>/` holds, beside the brief and the report:
//!
//! - `launch.json`: the instance, its (now empty) tasks, when the tick reserved it and when it started.
//! - `pid`: the process holding the launch, as a [`Holder`]. The tick writes itself there
//!   when it reserves the launch, the runner writes the instance's own process once it
//!   starts, and the tick removes the file when the run ends.
//! - `exit`: how it ended: the exit code, `error`, `timeout`, or `lost` for a launch that
//!   ended with nobody there to record it.
//!
//! Nothing here is a count to keep in step. Whether a launch still runs is read from the
//! process table on every read ([`Run::running`]), so a process that dies frees its slot at
//! once, whoever was watching. A run folder written before launches were recorded has no
//! `launch.json`; it reads as a known instance that is not running.

use std::path::{Path, PathBuf};

use super::tick::PlannedLaunch;
use super::{RUNS_DIR, SpawnError, slug};
use crate::domain::Instance;
use crate::ports::Holder;

/// The launch record inside a run folder.
pub const LAUNCH_FILE: &str = "launch.json";

/// The process holding a launch, inside its run folder.
pub const HOLDER_FILE: &str = "pid";

/// How a launch ended, inside its run folder.
pub const EXIT_FILE: &str = "exit";

/// The exit status of a launch that ended with nobody there to record it.
pub const LOST: &str = "lost";

/// What `launch.json` holds: written once when the tick reserves the launch, and again when
/// it starts.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LaunchRecord {
    /// The instance, as `<role> instance <n> on #<epic>`.
    pub instance: String,
    /// The tasks it was launched with: none now, since no launch builds a task; kept so run
    /// folders written before then still read.
    pub tasks: Vec<u64>,
    /// When the tick reserved it, RFC 3339.
    pub reserved_at: String,
    /// When it started, RFC 3339; none while it waits for its runner.
    #[serde(default)]
    pub started_at: Option<String>,
}

/// One run folder as read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The folder's name, the instance's slug.
    pub slug: String,
    /// The folder.
    pub dir: PathBuf,
    /// Its launch record; none in a folder from before launches were recorded.
    pub record: Option<LaunchRecord>,
    /// The process holding it; none once the run ended.
    pub holder: Option<Holder>,
    /// How it ended; none while it runs, or when nobody recorded the end.
    pub exit: Option<String>,
}

impl Run {
    /// Whether the launch still runs: no exit is recorded and its holder is alive.
    pub fn running(&self, alive: &dyn Fn(&Holder) -> bool) -> bool {
        self.exit.is_none() && self.holder.as_ref().is_some_and(alive)
    }

    /// Whether the launch ended with nobody there to record it: it has a launch record and
    /// no exit, and it does not run.
    pub fn lost(&self, alive: &dyn Fn(&Holder) -> bool) -> bool {
        self.record.is_some() && self.exit.is_none() && !self.running(alive)
    }

    /// The instance, read from its launch record.
    pub fn instance(&self) -> Option<Instance> {
        self.record.as_ref()?.instance.parse().ok()
    }

    /// The instance's number: from its launch record, else from the slug's `instance-<n>-`.
    pub fn number(&self) -> Option<u32> {
        if let Some(instance) = self.instance() {
            return Some(instance.number.get());
        }
        let words: Vec<&str> = self.slug.split('-').collect();
        words
            .windows(2)
            .find(|pair| pair[0] == "instance")
            .and_then(|pair| pair[1].parse().ok())
    }
}

/// The folder holding every run folder.
fn runs_dir(state_dir: &Path) -> PathBuf {
    state_dir.join(RUNS_DIR)
}

/// Reads every run folder, in instance-number order (a folder with no number last, by name).
/// An absent runs folder is none.
pub fn read_all(state_dir: &Path) -> Result<Vec<Run>, SpawnError> {
    let dir = runs_dir(state_dir);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(SpawnError::Read {
                path: dir,
                reason: source.to_string(),
            });
        }
    };
    let mut runs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| SpawnError::Read {
            path: dir.clone(),
            reason: source.to_string(),
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        runs.push(read_one(&path)?);
    }
    runs.sort_by(|a, b| {
        (a.number().is_none(), a.number(), &a.slug).cmp(&(
            b.number().is_none(),
            b.number(),
            &b.slug,
        ))
    });
    Ok(runs)
}

/// Reads one run folder. A garbled launch record is an error naming it; a garbled holder
/// reads as none, so it holds nothing.
fn read_one(dir: &Path) -> Result<Run, SpawnError> {
    let record = match read_text(&dir.join(LAUNCH_FILE))? {
        Some(text) => Some(
            serde_json::from_str(&text).map_err(|error| SpawnError::Read {
                path: dir.join(LAUNCH_FILE),
                reason: error.to_string(),
            })?,
        ),
        None => None,
    };
    let holder = read_text(&dir.join(HOLDER_FILE))?.and_then(|text| Holder::parse(&text));
    let exit = read_text(&dir.join(EXIT_FILE))?
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty());
    Ok(Run {
        slug: dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        dir: dir.to_owned(),
        record,
        holder,
        exit,
    })
}

/// A file's text; none when it is absent.
fn read_text(path: &Path) -> Result<Option<String>, SpawnError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(SpawnError::Read {
            path: path.to_owned(),
            reason: source.to_string(),
        }),
    }
}

fn write(path: &Path, text: &str) -> Result<(), SpawnError> {
    std::fs::write(path, text).map_err(|source| SpawnError::Write {
        path: path.to_owned(),
        source,
    })
}

/// Creates a launch's run folder with its record and `holder` (the tick) in `pid`, and
/// returns the folder. A folder already there is an error: an instance number is used once.
pub fn reserve(
    state_dir: &Path,
    planned: &PlannedLaunch,
    holder: &Holder,
    at: &str,
) -> Result<PathBuf, SpawnError> {
    let parent = runs_dir(state_dir);
    std::fs::create_dir_all(&parent).map_err(|source| SpawnError::Write {
        path: parent.clone(),
        source,
    })?;
    let dir = parent.join(slug(&planned.instance));
    std::fs::create_dir(&dir).map_err(|source| SpawnError::Write {
        path: dir.clone(),
        source,
    })?;
    let record = LaunchRecord {
        instance: planned.instance.to_string(),
        tasks: Vec::new(),
        reserved_at: at.to_owned(),
        started_at: None,
    };
    write_record(&dir, &record)?;
    write(&dir.join(HOLDER_FILE), &format!("{holder}\n"))?;
    Ok(dir)
}

fn write_record(dir: &Path, record: &LaunchRecord) -> Result<(), SpawnError> {
    let text = serde_json::to_string_pretty(record).unwrap_or_default();
    write(&dir.join(LAUNCH_FILE), &format!("{text}\n"))
}

/// Records in the run folder's launch record that it started at `at`.
pub fn mark_started(dir: &Path, at: &str) -> Result<(), SpawnError> {
    let Some(mut record) = read_one(dir)?.record else {
        return Ok(());
    };
    record.started_at = Some(at.to_owned());
    write_record(dir, &record)
}

/// Records how a launch ended and removes its holder, so it no longer runs.
pub fn finish(dir: &Path, status: &str) -> Result<(), SpawnError> {
    write(&dir.join(EXIT_FILE), &format!("{status}\n"))?;
    match std::fs::remove_file(dir.join(HOLDER_FILE)) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SpawnError::Write {
            path: dir.join(HOLDER_FILE),
            source,
        }),
    }
}

#[cfg(test)]
mod tests;
