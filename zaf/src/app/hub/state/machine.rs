//! Every project on the zaf home's list as one machine state.
//!
//! [`gather_machine`] reads the list ([`Projects`]) and each project's state
//! ([`gather_project`]), headed by the [`MachineLine`]: the instances running, the projects, the
//! units in use and the budget, across all projects. It only reads, and it never fails.
//!
//! A project whose folder or manifest is gone is kept, marked `missing`, and nothing of it is
//! read. Every other project is read on a thread of its own, all at once, and the one deadline
//! of [`READ_BOUND`] from the start covers them all: a project whose read fails shows its reason
//! like any project, and one whose read has not answered by then shows that, so no project holds
//! the others up. A read that hangs is left on its thread, which ends with the process or when
//! the read does; while it is there ([`Reading`]) no other read of that project starts, so a
//! project that hangs holds one thread however often the machine is read.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use super::{Problem, ProblemKind, ProjectState, gather_project, unreadable};
use crate::adapters::process;
use crate::app::projects::{ProjectEntry, Projects};
use crate::domain::{Budget, Units};
use crate::manifest::{HOME_FILE, SpawnSection};
use crate::ports::live_use;

/// How long the reads of the projects may take together, from the moment they start.
pub const READ_BOUND: Duration = Duration::from_secs(30);

/// The machine as the hub reads it: serialize it with `serde_json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MachineState {
    /// The headline, across all projects.
    pub machine: MachineLine,
    /// Every project on the list, in the order they joined, missing ones too.
    pub projects: Vec<ProjectState>,
}

/// The machine line: what runs on the machine and how much of its budget that uses.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MachineLine {
    /// The instances running now: the sum of every project's, read from its run folders.
    pub instances: usize,
    /// The projects on the list, missing ones included.
    pub projects: usize,
    /// The live use of the machine in units, read from the process table: every instance's
    /// memory, from any project, summed.
    #[serde(rename = "in_use_units", serialize_with = "units_as_number")]
    pub in_use: Units,
    /// The zaf home's budget (`[spawn] budget`; none is no machine cap), its reserve and the
    /// size table.
    pub budget: Budget,
}

/// Units as a plain number of units, `34.5`, rather than the mebibytes they hold.
fn units_as_number<S: serde::Serializer>(units: &Units, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(units.as_f64())
}

/// What reads one project's state: its root and the time the state is of.
pub type Reader = dyn Fn(&Path, SystemTime) -> ProjectState + Send + Sync;

/// Reads the machine of the zaf home `home` at `now`: the process table for the units in use,
/// the home's `config.toml` for the budget, and every listed project's board.
pub fn gather_machine(home: &Path, now: SystemTime) -> MachineState {
    gather_machine_with(
        home,
        now,
        READ_BOUND,
        live_use(&process::displayed()),
        budget_of(home),
        Arc::new(gather_project),
    )
}

/// [`gather_machine`] over a reader, a bound and the machine's use and budget, which a test or
/// another caller gives itself.
pub fn gather_machine_with(
    home: &Path,
    now: SystemTime,
    bound: Duration,
    in_use: Units,
    budget: Budget,
    reader: Arc<Reader>,
) -> MachineState {
    let entries = Projects::read(home);
    let projects = read_projects(&entries, now, bound, &reader);
    MachineState {
        machine: MachineLine {
            instances: projects.iter().map(|state| state.instances.len()).sum(),
            projects: projects.len(),
            in_use,
            budget,
        },
        projects,
    }
}

/// One project's state, read as [`gather_machine_with`] reads each project: on a thread of its
/// own, under `bound`, and never while an earlier read of it is still on its thread. A missing
/// entry is marked and not read; a read that has not answered by the bound comes back with that
/// reason and is left on its thread. What the captain's chair's gatherers call, one per project.
pub fn gather_one(
    entry: &ProjectEntry,
    now: SystemTime,
    bound: Duration,
    reader: &Arc<Reader>,
) -> ProjectState {
    read_projects(std::slice::from_ref(entry), now, bound, reader)
        .pop()
        .unwrap_or_else(|| unread(entry, now, "its read ended without an answer".to_owned()))
}

/// The project roots whose read is on a thread now, in this process.
static READING: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// One project's read while it runs: its root is in [`READING`] until this drops, which is when
/// the read's thread ends, however it ends.
struct Reading(PathBuf);

impl Reading {
    /// Marks `root` as being read; none when a read of it is still on its thread.
    fn begin(root: &Path) -> Option<Self> {
        READING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(root.to_owned())
            .then(|| Self(root.to_owned()))
    }
}

impl Drop for Reading {
    fn drop(&mut self) {
        READING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// Every entry's state, in the entries' order.
fn read_projects(
    entries: &[ProjectEntry],
    now: SystemTime,
    bound: Duration,
    reader: &Arc<Reader>,
) -> Vec<ProjectState> {
    let (sender, receiver) = mpsc::channel();
    let mut states: Vec<Option<ProjectState>> = vec![None; entries.len()];
    let mut asked = 0;
    let mut failed_to_start: HashMap<usize, String> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.missing {
            states[index] = Some(missing(entry, now));
            continue;
        }
        let Some(reading) = Reading::begin(&entry.folder) else {
            failed_to_start.insert(index, "its earlier board read has not ended".to_owned());
            continue;
        };
        let sender = sender.clone();
        let reader = Arc::clone(reader);
        let root = entry.folder.clone();
        let started = std::thread::Builder::new()
            .name(format!("hub-read-{}", entry.name))
            .spawn(move || {
                // Dropped when the thread ends, or with this closure when it never starts.
                let _reading = reading;
                // The receiver may be gone by the time a slow read ends: nobody is left to hear it.
                let _ = sender.send((index, reader(&root, now)));
            });
        match started {
            Ok(_) => asked += 1,
            Err(error) => {
                failed_to_start.insert(index, format!("its read could not start: {error}"));
            }
        }
    }
    drop(sender);
    let deadline = Instant::now() + bound;
    let mut timed_out = false;
    while asked > 0 {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok((index, state)) => {
                states[index] = Some(state);
                asked -= 1;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                timed_out = true;
                break;
            }
            // Every thread has ended, and some without an answer: a read that panicked.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    entries
        .iter()
        .zip(states)
        .enumerate()
        .map(|(index, (entry, state))| {
            state.unwrap_or_else(|| {
                let reason = failed_to_start.remove(&index).unwrap_or_else(|| {
                    if timed_out {
                        format!(
                            "its board read did not answer within {}",
                            humantime::format_duration(bound)
                        )
                    } else {
                        "its read ended without an answer".to_owned()
                    }
                });
                unread(entry, now, reason)
            })
        })
        .collect()
}

/// A project of the list whose folder or manifest is gone.
fn missing(entry: &ProjectEntry, now: SystemTime) -> ProjectState {
    let what = if entry.folder.is_dir() {
        "its manifest is gone"
    } else {
        "its folder is gone"
    };
    let mut state = ProjectState {
        missing: true,
        ..unread(entry, now, what.to_owned())
    };
    state.problems.push(Problem {
        kind: ProblemKind::Missing,
        detail: what.to_owned(),
        fix: None,
        verb: None,
    });
    state
}

/// An entry with nothing read and the reason why, named as the list names it.
fn unread(entry: &ProjectEntry, now: SystemTime, reason: String) -> ProjectState {
    ProjectState {
        project: entry.name.clone(),
        ..unreadable(&entry.folder, now, reason)
    }
}

/// The machine's budget: the `[spawn]` table of the zaf home's `config.toml`. A file that is not
/// there, or whose `[spawn]` cannot be read, is no machine cap with the default reserve and sizes
/// (every project's own load names the fault).
pub fn budget_of(home: &Path) -> Budget {
    let spawn = std::fs::read_to_string(home.join(HOME_FILE))
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|mut table| table.remove("spawn"))
        .and_then(|spawn| spawn.try_into::<SpawnSection>().ok());
    spawn.map(|spawn| spawn.budget()).unwrap_or_default()
}

#[cfg(test)]
mod tests;
