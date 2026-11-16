//! The lane: the ordered queue of tickets that may run, heaviest first (R4, R6, R20).
//!
//! A ticket's weight is computed each tick and never stored: how long it has been ready plus,
//! when the Engineer set one, its [`Priority`]'s worth in age: level 1, the most urgent, is worth
//! three times [`PRIORITY_AGE`], level 2 twice and level 3 once. A priority is a fixed amount of
//! age, so age always wins in the end and nothing starves. The budget walk over the lane (sizes,
//! the machine budget, slots, starvation) stays the caller's.
//!
//! The Engineer sets a priority with a label on the ticket, `priority/1`, `priority/2` or
//! `priority/3` ([`Priority::from_labels`]). It is the Engineer's input, not a state label: zaf
//! reads it and never writes or removes it.

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime};

use super::phase::ActionName;
use crate::domain::{TaskSize, TicketId};

/// What one step of priority is worth: 60 minutes of age (the plan's proposal).
pub const PRIORITY_AGE: Duration = Duration::from_secs(60 * 60);

/// The least urgent priority level; level 1 is the most urgent.
const LAST_LEVEL: u8 = 3;

/// What a priority label starts with: `priority/1`, `priority/2`, `priority/3`.
pub const PRIORITY_LABEL_PREFIX: &str = "priority/";

/// A priority the Engineer set on a ticket: a level from 1 to 3, the label `priority/<n>`. Level 1
/// is the most urgent: level `n` is worth `4 - n` times [`PRIORITY_AGE`], so 1 is worth three
/// steps, 2 two and 3 one (the lead's ruling, 2026-11-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, derive_more::Display)]
pub struct Priority(u8);

impl Priority {
    /// The priority at `level`; none outside 1 to 3.
    #[must_use]
    pub fn new(level: u8) -> Option<Self> {
        (1..=LAST_LEVEL).contains(&level).then_some(Self(level))
    }

    /// Its level, 1 (the most urgent) to 3.
    #[must_use]
    pub const fn level(self) -> u8 {
        self.0
    }

    /// What it is worth in age: [`PRIORITY_AGE`] once for level 3, twice for level 2 and three
    /// times for level 1.
    #[must_use]
    pub fn age(self) -> Duration {
        PRIORITY_AGE * u32::from(LAST_LEVEL + 1 - self.0)
    }

    /// The label that sets it: `priority/<level>`.
    #[must_use]
    pub fn label(self) -> String {
        format!("{PRIORITY_LABEL_PREFIX}{}", self.0)
    }

    /// What a ticket's labels say of its priority: none when no label starts with `priority/`, the
    /// priority of the one that does, or why they are ignored: the one names no level from 1 to 3,
    /// or more than one label names a priority. Labels that are no priority's are passed by.
    pub fn from_labels<'a>(
        labels: impl IntoIterator<Item = &'a str>,
    ) -> Result<Option<Self>, PriorityIgnored> {
        let named: Vec<&str> = labels
            .into_iter()
            .filter(|label| label.starts_with(PRIORITY_LABEL_PREFIX))
            .collect();
        match named.as_slice() {
            [] => Ok(None),
            [label] => label
                .strip_prefix(PRIORITY_LABEL_PREFIX)
                .and_then(|level| level.parse().ok())
                .map(Some)
                .ok_or_else(|| PriorityIgnored::Unknown((*label).to_owned())),
            more => Err(PriorityIgnored::Doubled(
                more.iter().map(|label| (*label).to_owned()).collect(),
            )),
        }
    }
}

/// Text that is not a priority level from 1 to 3.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{0}` is not a priority from 1 to 3")]
pub struct PriorityError(String);

impl FromStr for Priority {
    type Err = PriorityError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        text.trim()
            .parse::<u8>()
            .ok()
            .and_then(Self::new)
            .ok_or_else(|| PriorityError(text.to_owned()))
    }
}

/// Why a ticket's priority labels are ignored: the ticket then has no priority, and the caller
/// names it in a warning, never a failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PriorityIgnored {
    /// Its one priority label names no level from 1 to 3.
    #[error("the label `{0}` is not priority/1, priority/2 or priority/3")]
    Unknown(String),
    /// More than one label names a priority, so none is read.
    #[error("it carries more than one priority label ({})", .0.join(", "))]
    Doubled(Vec<String>),
}

/// A ticket's weight in the lane: the age it counts as. Heavier goes first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Weight(Duration);

impl Weight {
    /// The age it counts as.
    #[must_use]
    pub const fn age(self) -> Duration {
        self.0
    }
}

impl fmt::Display for Weight {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} s", self.0.as_secs())
    }
}

/// The weight at `now` of a ticket ready since `since`: its age, plus its priority's worth when
/// it has one. A `since` after `now`, a clock that stepped back, is no age.
#[must_use]
pub fn weight(since: SystemTime, priority: Option<Priority>, now: SystemTime) -> Weight {
    let age = now.duration_since(since).unwrap_or_default();
    Weight(age + priority.map(Priority::age).unwrap_or_default())
}

/// One ticket in the lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneEntry {
    /// The ticket.
    pub ticket: TicketId,
    /// The action its next run starts.
    pub next: ActionName,
    /// Since when it is ready.
    pub since: SystemTime,
    /// What a model run of it costs.
    pub size: TaskSize,
    /// Its weight this tick.
    pub weight: Weight,
}

/// The tickets that may run, the heaviest first; of equal weight, the lower ticket first (the
/// #497 ruling on ties).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Lane {
    entries: Vec<LaneEntry>,
}

impl Lane {
    /// Places `entry` at its turn. A ticket already in the lane is not placed again, and the
    /// answer is false.
    pub fn enqueue(&mut self, entry: LaneEntry) -> bool {
        if self.contains(entry.ticket) {
            return false;
        }
        let at = self
            .entries
            .partition_point(|queued| Self::goes_before(queued, &entry));
        self.entries.insert(at, entry);
        true
    }

    /// Whether `first` goes before `second`: heavier, or as heavy and the lower ticket.
    fn goes_before(first: &LaneEntry, second: &LaneEntry) -> bool {
        (first.weight, std::cmp::Reverse(first.ticket))
            > (second.weight, std::cmp::Reverse(second.ticket))
    }

    /// The entries, the front first.
    #[must_use]
    pub fn entries(&self) -> &[LaneEntry] {
        &self.entries
    }

    /// The entry that gets the next free slot.
    #[must_use]
    pub fn front(&self) -> Option<&LaneEntry> {
        self.entries.first()
    }

    /// Whether `ticket` is in the lane.
    #[must_use]
    pub fn contains(&self, ticket: TicketId) -> bool {
        self.entries.iter().any(|entry| entry.ticket == ticket)
    }

    /// How many tickets are in the lane.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the lane is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests;
