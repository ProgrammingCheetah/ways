//! The hub's wait in a terminal (docs/zaf-actions.md §5): it redraws when a report arrives, not
//! on a timer. A new report is the only thing that reads the board again; between reports only
//! the clocks move, from the time the last frame was gathered, and nothing reads the network.

use std::path::Path;
use std::time::{Duration, SystemTime};

use super::reports::reports_file;
use super::{HubError, Snapshot, draw};

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
pub struct Watch<G> {
    gather: G,
    snapshot: Snapshot,
    gathered_at: SystemTime,
    mark: Mark,
    width: usize,
    drawn_seconds: u64,
}

impl<G> Watch<G>
where
    G: FnMut(SystemTime) -> Result<Snapshot, HubError>,
{
    /// Gathers the first snapshot, at `now`, and draws it. `gather` is what reads the board; it
    /// is called once here and once per report after.
    pub fn start(
        mut gather: G,
        now: SystemTime,
        mark: Mark,
        width: usize,
    ) -> Result<(Self, String), HubError> {
        let snapshot = gather(now)?;
        let frame = draw(&snapshot, width);
        let watch = Self {
            gather,
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
    /// snapshot drawn again, older. The same mark in the same second draws nothing.
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
