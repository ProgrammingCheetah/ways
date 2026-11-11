//! Memory as the budget counts it: mebibytes read from the machine, and the units the
//! budget is written in.
//!
//! The budget of #419 is a number of units, one unit being 1 GiB, the "about 1 GB of allowed
//! RAM" of its ruling. A read of the process table gives mebibytes, so [`Units`] holds
//! mebibytes and nothing is rounded until a count is printed, with one decimal.

use std::fmt;
use std::iter::Sum;
use std::ops::Add;

/// The mebibytes in one budget unit: 1 GiB.
pub const MEBIBYTES_PER_UNIT: u64 = 1024;

/// An amount of memory in mebibytes (MiB, 1024 KiB).
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Mebibytes(u64);

impl Mebibytes {
    /// An amount of `count` mebibytes.
    #[must_use]
    pub const fn new(count: u64) -> Self {
        Self(count)
    }

    /// The amount of `count` kibibytes, rounded to the nearest mebibyte (a half rounds up),
    /// as `/proc` and `ps` report resident memory.
    #[must_use]
    pub const fn from_kibibytes(count: u64) -> Self {
        Self(count.saturating_add(512) / 1024)
    }

    /// The mebibytes.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl Add for Mebibytes {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }
}

impl Sum for Mebibytes {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), Add::add)
    }
}

impl fmt::Display for Mebibytes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} MiB", self.0)
    }
}

/// An amount of budget units; one unit is 1024 MiB. It holds the mebibytes, so adding
/// amounts loses nothing, and it prints with one decimal.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
pub struct Units(Mebibytes);

impl Units {
    /// The units that `memory` is worth.
    #[must_use]
    pub const fn of(memory: Mebibytes) -> Self {
        Self(memory)
    }

    /// Whole units: `count` of 1024 MiB each.
    #[must_use]
    pub const fn whole(count: u64) -> Self {
        Self(Mebibytes::new(count.saturating_mul(MEBIBYTES_PER_UNIT)))
    }

    /// The mebibytes this amount holds.
    #[must_use]
    pub const fn mebibytes(self) -> Mebibytes {
        self.0
    }

    /// The count of units, fractions kept.
    #[must_use]
    #[allow(clippy::cast_precision_loss)]
    pub fn as_f64(self) -> f64 {
        self.0.get() as f64 / MEBIBYTES_PER_UNIT as f64
    }
}

impl From<Mebibytes> for Units {
    fn from(memory: Mebibytes) -> Self {
        Self::of(memory)
    }
}

impl Add for Units {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sum for Units {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), Add::add)
    }
}

/// The count with one decimal, as `0.5`; no unit word follows.
impl fmt::Display for Units {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:.1}", self.as_f64())
    }
}

#[cfg(test)]
mod tests;
