//! The machine budget of #419: whether a launch fits the machine and its project.
//!
//! The machine has a budget in units, `[spawn] budget` in the zaf home, and each task size
//! costs a number of units from a table a project may override. A launch fits when the
//! project has a free slot, its `[spawn] slots` counted in instances, and the live use read
//! on that tick plus the cost of the launch's size stays inside the budget. With no budget
//! there is no machine cap and only the slots decide. Nothing here reads the machine: the
//! live use is handed in, so the rule is pure and every model launch asks the same one.

use super::{TaskSize, Units};

/// What each size short of E costs, in units: XS 1, S 2, M 4, L 8 and XL 16 unless a project
/// overrides one. E has no entry: it costs the whole budget until #434 leaves a reserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct SizeTable {
    /// The cost of an XS.
    #[serde(rename = "XS")]
    xs: u32,
    /// The cost of an S.
    #[serde(rename = "S")]
    s: u32,
    /// The cost of an M.
    #[serde(rename = "M")]
    m: u32,
    /// The cost of an L.
    #[serde(rename = "L")]
    l: u32,
    /// The cost of an XL.
    #[serde(rename = "XL")]
    xl: u32,
}

impl Default for SizeTable {
    /// The defaults of #419: XS 1, S 2, M 4, L 8, XL 16.
    fn default() -> Self {
        Self {
            xs: 1,
            s: 2,
            m: 4,
            l: 8,
            xl: 16,
        }
    }
}

impl SizeTable {
    /// The table with `size` costing `units`; an E is left as it is, since it has no entry.
    #[must_use]
    pub const fn with(mut self, size: TaskSize, units: u32) -> Self {
        match size {
            TaskSize::XS => self.xs = units,
            TaskSize::S => self.s = units,
            TaskSize::M => self.m = units,
            TaskSize::L => self.l = units,
            TaskSize::XL => self.xl = units,
            TaskSize::E => {}
        }
        self
    }

    /// The units `size` costs; none for an E, whose cost is the budget's.
    #[must_use]
    pub const fn units_of(&self, size: TaskSize) -> Option<u32> {
        match size {
            TaskSize::XS => Some(self.xs),
            TaskSize::S => Some(self.s),
            TaskSize::M => Some(self.m),
            TaskSize::L => Some(self.l),
            TaskSize::XL => Some(self.xl),
            TaskSize::E => None,
        }
    }
}

/// Whether a launch may start now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, strum::Display)]
pub enum Fit {
    /// It fits: a slot is free and the budget holds its size.
    Fits,
    /// A slot is free, but the live use plus its size's cost passes the machine budget.
    NoBudget,
    /// The project already runs as many instances as its `[spawn] slots`, whatever the
    /// budget has left.
    NoFreeSlot,
}

/// The machine budget and the cost of each size: `Budget { units: Option<u32>, sizes: SizeTable }`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct Budget {
    /// The machine's `[spawn] budget`, in units, from the zaf home; none means no machine cap.
    pub units: Option<u32>,
    /// The cost of each size, the project's overrides over the defaults.
    pub sizes: SizeTable,
}

impl Budget {
    /// What a launch of `size` costs, `cost(size) -> Units`: its entry in the table, and for
    /// an E the whole budget (nothing with no budget, which has no cap to take) until #434
    /// leaves a reserve.
    #[must_use]
    pub fn cost(&self, size: TaskSize) -> Units {
        let units = self.sizes.units_of(size).or(self.units).unwrap_or_default();
        Units::whole(u64::from(units))
    }

    /// Whether a launch of `size` fits,
    /// `fits(in_use: Units, size: TaskSize, running_here: usize, slots: usize) -> Fit`.
    ///
    /// It fits iff `running_here < slots`, the project running fewer instances than its
    /// slots, and `in_use + cost(size) <= units`, the live use of the machine plus the
    /// launch's cost staying inside the budget. A full project is [`Fit::NoFreeSlot`] before
    /// the budget is asked; with no budget every launch under the slots fits.
    #[must_use]
    pub fn fits(&self, in_use: Units, size: TaskSize, running_here: usize, slots: usize) -> Fit {
        if running_here >= slots {
            return Fit::NoFreeSlot;
        }
        let Some(units) = self.units else {
            return Fit::Fits;
        };
        if in_use + self.cost(size) <= Units::whole(u64::from(units)) {
            Fit::Fits
        } else {
            Fit::NoBudget
        }
    }
}

#[cfg(test)]
mod tests;
