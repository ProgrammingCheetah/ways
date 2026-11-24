//! The scheduling core (docs/zaf-actions.md §5.2; the rulings R12 to R22): where each ticket sits,
//! worked out from facts alone, and what a run meant.
//!
//! Pure: it reads no file, no forge, no clock (the time is handed in) and no process table, and
//! it writes nothing. The caller gathers each ticket's [`TicketFacts`]; [`place()`] says where the
//! ticket sits, in exactly one place, and [`Placement::of`] places them all. Nothing calls it
//! yet: later slices of the scheduler rebuild gather the facts, wire the tick and the start of a
//! phase to it, and write the state labels from [`label_for`].
//!
//! - `facts`: [`TicketFacts`], the [`RunRecord`] a phase run folder reads as and its [`Report`].
//! - `phase`: [`Phase`], its end facts ([`end_fact_holds`]), [`phase_status`] and
//!   [`current_phase`].
//! - `outcome`: [`Outcome`] of a run ([`outcome()`]), getting nowhere ([`nowhere`]) and
//!   [`is_machinery`].
//! - `place`: [`Place`], [`WaitingOn`], [`place()`] and [`Placement`].
//! - `merge`: the order of merges on one integration branch: [`at_merge`], [`fresh_from_fix`]
//!   and [`order_merges`] (R29, R30).
//! - `lane`: the ordered [`Lane`], [`weight`] and [`Priority`], read from its label.
//! - `wait`: an [`overdue`] check wait, [`dead_dependencies`] and an epic [`unchallenged`] past
//!   its deadline.
//! - `label`: [`label_for`], the state label as output.

mod facts;
mod label;
mod lane;
mod merge;
mod outcome;
mod phase;
mod place;
mod wait;

pub use facts::{
    BaseFacts, CheckFact, CheckName, CheckResult, Dependency, EpicFacts, EpicState, ExitCode,
    GroupBase, HeadEnd, MergeFacts, OutputLine, PullFacts, Report, ReviewFact, RuledMarker,
    RunExit, RunRecord, TicketFacts, Trees,
};
pub use label::label_for;
pub use lane::{
    Lane, LaneEntry, PRIORITY_AGE, PRIORITY_LABEL_PREFIX, Priority, PriorityError, PriorityIgnored,
    Weight, weight,
};
pub use merge::{at_merge, fresh_from_fix, merges_first, order_merges};
pub use outcome::{Failure, NOWHERE_LIMIT, Outcome, gets_nowhere, is_machinery, nowhere, outcome};
pub use phase::{
    ActionName, CurrentPhase, Phase, PhaseEnd, PhaseStatus, current_phase, end_fact_holds,
    latest_counted_run, phase_status,
};
pub use place::{Place, Placement, Run, WaitingOn, place};
pub use wait::{
    Awaited, CHALLENGE_DEADLINE, CHECK_DEADLINE, DeadDependency, DeadReason, DependencyGraph,
    NO_CHECK_DEADLINE, OverdueChallenge, OverdueCheck, base_clock, check_clock, dead_dependencies,
    dependency_graph, overdue, unchallenged,
};

#[cfg(test)]
mod tests;
