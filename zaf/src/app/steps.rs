//! The step report: how a use case says a step started and how it ended.
//!
//! A reporter is built with the rows of a plan; `row` indexes that list. The terminal's
//! adapter (`cli::steps`) animates the row's line between the two calls; [`Recorder`] keeps
//! the calls for tests.

use crate::domain::screen::StepEnd;

/// The port a use case reports its steps through.
pub trait StepReport {
    /// The step of plan row `row` began.
    fn started(&mut self, row: usize);
    /// The step of plan row `row` ended as `end` says.
    fn ended(&mut self, row: usize, end: &StepEnd);
}

/// One call a [`Recorder`] heard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// `started(row)`.
    Started(usize),
    /// `ended(row, end)`.
    Ended(usize, StepEnd),
}

/// A [`StepReport`] that keeps every call, in order.
#[derive(Debug, Default)]
pub struct Recorder {
    /// The calls so far, oldest first.
    pub calls: Vec<Call>,
}

impl StepReport for Recorder {
    fn started(&mut self, row: usize) {
        self.calls.push(Call::Started(row));
    }

    fn ended(&mut self, row: usize, end: &StepEnd) {
        self.calls.push(Call::Ended(row, end.clone()));
    }
}

#[cfg(test)]
mod tests;
