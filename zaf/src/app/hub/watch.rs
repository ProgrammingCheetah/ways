//! The hub's wait in a terminal (docs/zaf-actions.md §5): it redraws when a report arrives, not
//! on a timer. A new report is the only thing that reads the board again; between reports only
//! the clocks move, from the time the last frame was gathered, and nothing reads the network.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::reports::reports_file;
use super::{HubError, Snapshot, draw};
use crate::app::spawn::runs;
use crate::composition::STATE_DIR;
use crate::ports::Holder;

/// What the reports file looked like: its size and when it last changed. A report appended, or
/// the file trimmed after one, changes it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mark {
    len: u64,
    modified: Option<SystemTime>,
}

impl Mark {
    /// The mark of the reports file under `state_dir`; a missing file is the empty mark.
    pub fn read(state_dir: &Path) -> Self {
        std::fs::metadata(reports_file(state_dir)).map_or_else(
            |_| Self::default(),
            |meta| Self {
                len: meta.len(),
                modified: meta.modified().ok(),
            },
        )
    }
}

/// The live frame: the snapshot gathered at the last report and the frame drawn from it.
pub struct Watch<G, L> {
    gather: G,
    live: L,
    snapshot: Snapshot,
    gathered_at: SystemTime,
    mark: Mark,
    width: usize,
    drawn_seconds: u64,
}

impl<G, L> Watch<G, L>
where
    G: FnMut(SystemTime) -> Result<Snapshot, HubError>,
    L: FnMut(&mut Snapshot) -> Result<(), HubError>,
{
    /// Gathers the first snapshot, at `now`, and draws it. `gather` is what reads the board; it
    /// is called once here and once per report after. `live` brings a snapshot's slots and
    /// claims up to the process table (see [`Liveness`](super::Liveness)); it reads no network and
    /// is called before every redraw that is not a report's.
    pub fn start(
        mut gather: G,
        live: L,
        now: SystemTime,
        mark: Mark,
        width: usize,
    ) -> Result<(Self, String), HubError> {
        let snapshot = gather(now)?;
        let frame = draw(&snapshot, width);
        let watch = Self {
            gather,
            live,
            snapshot,
            gathered_at: now,
            mark,
            width,
            drawn_seconds: 0,
        };
        Ok((watch, frame))
    }

    /// Looks at the reports file's `mark` at `now`. A mark that changed is a new report: the board
    /// is read again and the frame drawn. The same mark in a later second is the clocks: the last
    /// snapshot drawn again, older, its slots and claims read again from the process table (a
    /// run that ended frees its slot here). The same mark in the same second draws nothing.
    pub fn step(&mut self, now: SystemTime, mark: Mark) -> Result<Option<String>, HubError> {
        if mark != self.mark {
            // The mark is taken first: a board that fails is tried again at the next report, not
            // at every look.
            self.mark = mark;
            self.snapshot = (self.gather)(now)?;
            self.gathered_at = now;
            self.drawn_seconds = 0;
            return Ok(Some(draw(&self.snapshot, self.width)));
        }
        let age = now.duration_since(self.gathered_at).unwrap_or_default();
        if age.as_secs() == self.drawn_seconds {
            return Ok(None);
        }
        // Read first: a read that fails leaves the last frame up and is tried at the next look.
        (self.live)(&mut self.snapshot)?;
        self.drawn_seconds = age.as_secs();
        Ok(Some(draw(&self.snapshot.aged(age), self.width)))
    }
}

impl Snapshot {
    /// The snapshot `by` later: every ticket's time since its last report is that much longer.
    /// A ticket with no report stays without one.
    pub fn aged(&self, by: Duration) -> Self {
        let mut aged = self.clone();
        for epic in &mut aged.epics {
            let groups = epic.groups.iter_mut().flat_map(|group| &mut group.tickets);
            for ticket in epic.tickets.iter_mut().chain(groups) {
                ticket.since = ticket.since.map(|since| since + by);
            }
        }
        aged
    }
}

/// Why the hub says a project changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// A report arrived.
    Report,
    /// A run folder appeared: an instance started.
    Started,
    /// A run's process died, or its folder went: an instance ended.
    Ended,
}

impl Why {
    /// The word the event carries.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Report => "report",
            Self::Started => "started",
            Self::Ended => "ended",
        }
    }
}

/// One change in one project: the event `GET /api/events` sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The project's folder.
    pub project: PathBuf,
    /// What changed.
    pub why: Why,
}

/// A listed project, as far as the watch needs it: its folder and the state folder it keeps its
/// reports and run folders in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    /// The project's folder, which names it in an event.
    pub project: PathBuf,
    /// Its state folder.
    pub state_dir: PathBuf,
}

impl Watched {
    /// The project at `folder`, its state folder the usual `.zaf` in it.
    pub fn of(folder: &Path) -> Self {
        Self {
            project: folder.to_owned(),
            state_dir: folder.join(STATE_DIR),
        }
    }
}

/// What the watch last saw of one project.
#[derive(Debug)]
struct Seen {
    mark: Mark,
    /// Each run folder's slug, and whether its process ran.
    runs: BTreeMap<String, bool>,
}

impl Seen {
    fn of(state_dir: &Path, alive: &dyn Fn(&Holder) -> bool) -> Self {
        Self {
            mark: Mark::read(state_dir),
            runs: running(state_dir, alive).unwrap_or_default(),
        }
    }
}

/// Every run folder of `state_dir` and whether it runs; none when they cannot be read.
fn running(state_dir: &Path, alive: &dyn Fn(&Holder) -> bool) -> Option<BTreeMap<String, bool>> {
    runs::read_all(state_dir).ok().map(|all| {
        all.into_iter()
            .map(|run| {
                let running = run.running(alive);
                (run.slug, running)
            })
            .collect()
    })
}

/// The change watch over every listed project: it looks at each one's reports file and run
/// folders, and reads neither a board nor the network. The first look at a project takes its
/// state as the one to compare with and says nothing.
#[derive(Debug, Default)]
pub struct Changes {
    seen: BTreeMap<PathBuf, Seen>,
}

impl Changes {
    /// A watch that has seen nothing; the first [`poll`](Self::poll) takes the state as it is.
    pub fn new() -> Self {
        Self::default()
    }

    /// Looks at `projects` and returns what changed since the last look, project by project in
    /// the order given: a report is `Report`, a run folder that appeared `Started` (and `Ended`
    /// too when its process is already gone), a run whose process died or whose folder went
    /// `Ended`. A project no longer listed is forgotten; run folders that cannot be read leave
    /// that project's runs as they were.
    pub fn poll(&mut self, projects: &[Watched], alive: &dyn Fn(&Holder) -> bool) -> Vec<Change> {
        self.seen
            .retain(|folder, _| projects.iter().any(|watched| &watched.project == folder));
        let mut changes = Vec::new();
        for watched in projects {
            let Some(before) = self.seen.get_mut(&watched.project) else {
                self.seen
                    .insert(watched.project.clone(), Seen::of(&watched.state_dir, alive));
                continue;
            };
            let mut push = |why| {
                changes.push(Change {
                    project: watched.project.clone(),
                    why,
                });
            };
            let mark = Mark::read(&watched.state_dir);
            if mark != before.mark {
                before.mark = mark;
                push(Why::Report);
            }
            let Some(now) = running(&watched.state_dir, alive) else {
                continue;
            };
            for (slug, runs_now) in &now {
                match before.runs.get(slug) {
                    None => {
                        push(Why::Started);
                        if !runs_now {
                            push(Why::Ended);
                        }
                    }
                    Some(true) if !runs_now => push(Why::Ended),
                    Some(_) => {}
                }
            }
            for (slug, was_running) in &before.runs {
                if *was_running && !now.contains_key(slug) {
                    push(Why::Ended);
                }
            }
            before.runs = now;
        }
        changes
    }
}

#[cfg(test)]
mod tests;
