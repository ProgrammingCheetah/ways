//! `zaf epic check`'s two checks (D25, M16).
//!
//! The first, [`behaviour_failures`]: the behaviours of the epic's body tickets closed as
//! completed are compared with those of every plan record, less what the records dropped, by key
//! and goal; each difference is one failure naming the behaviour. An epic with no plan record is
//! [`BehaviourCheck::NotPlannedAsGraph`], not a failure list.
//!
//! The second, [`done_when_failures`]: each `Done when:` line of a signed epic's body
//! runs the tests it names.
//!
//! A line's tests are its backticked spans. Each runs the way `zaf check` runs a task's named
//! tests: the name fills the `{name}` of the manifest's `[gate] tests` template as one shell word
//! ([`run_named_test`]), never a shell string of its own. A test that fails is one failure naming
//! the line and the test; a line that names no test is one "not checkable" failure. A body with
//! no `Done when:` line is refused, never passed.
//!
//! [`check`] is the verb's whole read (I9): it reads the epic from the board, refuses one that has
//! no `Done when:` line or is not signed, runs both checks and returns every failure as one line,
//! the behaviours check's first.

use std::collections::BTreeSet;
use std::fmt;

use crate::app::check::task::run_named_test;
use crate::app::check::{LineStatus, Shell};
use crate::domain::plan::NodeKind;
use crate::domain::plan::text::parse_plan;
use crate::domain::{Epic, EpicRecord, EpicTask, TaskBody, TicketId};
use crate::manifest::CommandLine;
use crate::ports::{Board, PortError};

/// One behaviour as the comparison reads it: the planner's key and its goal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BehaviourId {
    /// The planner's key: a plan section's `Node:`, a filed section's `Behaviour:` key (or its
    /// ticket's node, a part `A.a` read as `A`). A ticket of the older template has `#<n>`.
    pub key: String,
    /// The behaviour's goal, trimmed.
    pub goal: String,
}

impl fmt::Display for BehaviourId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.goal)
    }
}

/// Why the behaviours check failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviourFailure {
    /// A planned behaviour no ticket closed as completed built.
    NotBuilt(BehaviourId),
    /// A behaviour a completed ticket built that no plan asked for.
    NotPlanned(BehaviourId),
    /// A plan record whose plan does not read, so what it planned is unknown.
    UnreadablePlan {
        /// The record's number.
        k: u64,
        /// The faults the plan's reader found.
        reason: String,
    },
}

impl fmt::Display for BehaviourFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotBuilt(behaviour) => write!(f, "planned, not built: {behaviour}"),
            Self::NotPlanned(behaviour) => write!(f, "built, not planned: {behaviour}"),
            Self::UnreadablePlan { k, reason } => {
                write!(f, "plan record {k} does not read as a plan: {reason}")
            }
        }
    }
}

/// What the behaviours check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BehaviourCheck {
    /// The epic has no plan record: nothing to compare.
    NotPlannedAsGraph,
    /// The failures, planned ones first, each group in key order; empty when every behaviour held.
    Failures(Vec<BehaviourFailure>),
}

impl fmt::Display for BehaviourCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPlannedAsGraph => f.write_str("not planned as a graph"),
            Self::Failures(failures) => {
                let lines: Vec<String> = failures.iter().map(ToString::to_string).collect();
                f.write_str(&lines.join("\n"))
            }
        }
    }
}

/// The behaviours of a ticket body, read as the board holds it: none for an interface ticket or a
/// body that does not read.
fn behaviours_of(task: &EpicTask) -> BTreeSet<BehaviourId> {
    let Ok(body) = TaskBody::parse(task.body.as_str()) else {
        return BTreeSet::new();
    };
    match &body {
        TaskBody::Planned(planned) if planned.kind == NodeKind::Interface => BTreeSet::new(),
        TaskBody::Planned(planned) => {
            let node = planned.node.as_str();
            let part = node.split('.').next().unwrap_or(node);
            planned
                .sections
                .iter()
                .map(|section| BehaviourId {
                    key: section
                        .key
                        .as_ref()
                        .map_or_else(|| part.to_owned(), |key| key.as_str().to_owned()),
                    goal: section.template.goal.trim().to_owned(),
                })
                .collect()
        }
        TaskBody::Template(_) => body
            .behaviours()
            .into_iter()
            .map(|behaviour| BehaviourId {
                key: task.number.to_string(),
                goal: behaviour.goal.trim().to_owned(),
            })
            .collect(),
    }
}

/// What the plan records ask for, in the order posted: each record adds its plan's behaviours
/// after taking away those of the tickets it drops, which an earlier record filed.
fn planned(
    records: &[EpicRecord],
    tickets: &[EpicTask],
) -> Result<BTreeSet<BehaviourId>, Vec<BehaviourFailure>> {
    let mut planned = BTreeSet::new();
    let mut unreadable = Vec::new();
    for filed in records {
        let record = &filed.record;
        for dropped in &record.dropped {
            for task in tickets.iter().filter(|task| task.number == *dropped) {
                for behaviour in behaviours_of(task) {
                    planned.remove(&behaviour);
                }
            }
        }
        match parse_plan(&record.plan) {
            Ok(parsed) => {
                for node in parsed.input.nodes {
                    for behaviour in node.behaviours {
                        planned.insert(BehaviourId {
                            key: node.id.as_str().to_owned(),
                            goal: behaviour.text.trim().to_owned(),
                        });
                    }
                }
            }
            Err(faults) => unreadable.push(BehaviourFailure::UnreadablePlan {
                k: record.k,
                reason: faults.to_string(),
            }),
        }
    }
    if unreadable.is_empty() {
        Ok(planned)
    } else {
        Err(unreadable)
    }
}

/// Compares what the epic's plan records asked for with what its tickets closed as completed
/// built, by key and goal (D25).
///
/// Planned is every record's plan as written, less the behaviours of the tickets each record
/// names dropped (D29). Built is the behaviours of the tickets in `tickets` closed as completed,
/// counting each section of a merged ticket; interface tickets and tickets closed as not planned
/// are not counted. Each planned behaviour not built, and each built behaviour not planned, is one
/// failure. No record in `records` is [`BehaviourCheck::NotPlannedAsGraph`].
///
/// A record whose plan does not read is one failure of its own; the comparison is then left out,
/// since half a plan would blame every behaviour the unread one held.
///
/// Interface: `behaviour_failures(epic, records, tickets) -> BehaviourCheck`. It consumes the
/// epic's plan records (#1171), [`parse_plan`] (#1208) and [`TaskBody::behaviours`] (#1187).
/// `epic` is the epic the records and tickets belong to.
pub fn behaviour_failures(
    _epic: &Epic,
    records: &[EpicRecord],
    tickets: &[EpicTask],
) -> BehaviourCheck {
    if records.is_empty() {
        return BehaviourCheck::NotPlannedAsGraph;
    }
    let planned = match planned(records, tickets) {
        Ok(planned) => planned,
        Err(unreadable) => return BehaviourCheck::Failures(unreadable),
    };
    let built: BTreeSet<BehaviourId> = tickets
        .iter()
        .filter(|task| task.done())
        .flat_map(behaviours_of)
        .collect();
    let failures = planned
        .difference(&built)
        .cloned()
        .map(BehaviourFailure::NotBuilt)
        .chain(
            built
                .difference(&planned)
                .cloned()
                .map(BehaviourFailure::NotPlanned),
        )
        .collect();
    BehaviourCheck::Failures(failures)
}

/// Why a `Done when:` line failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DoneWhenFailure {
    /// A test the line names failed.
    TestFailed {
        /// The line, as the body has it, trimmed.
        line: String,
        /// The test's name.
        test: String,
        /// The last line the test printed, or why it could not run.
        reason: String,
    },
    /// The line names no test in backticks, so nothing runs for it.
    NotCheckable {
        /// The line, as the body has it, trimmed.
        line: String,
    },
}

impl fmt::Display for DoneWhenFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TestFailed { line, test, reason } => {
                write!(f, "`{test}` failed ({reason}): {line}")
            }
            Self::NotCheckable { line } => write!(f, "not checkable, it names no test: {line}"),
        }
    }
}

/// The epic's body holds no `Done when:` line, so there is nothing to check and nothing to pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the epic's body has no `Done when:` line, so nothing in it can be checked")]
pub struct NoDoneWhen;

/// The `Done when:` lines of `body`, trimmed: a line that, past its indent and a list marker
/// (`-`, `*`, `>`), starts with `Done when:` in any case.
pub fn done_when_lines(body: &str) -> Vec<&str> {
    body.lines()
        .map(str::trim)
        .filter(|line| {
            let rest = line.trim_start_matches(['-', '*', '>', ' ', '\t']);
            rest.get(..10)
                .is_some_and(|head| head.eq_ignore_ascii_case("done when:"))
        })
        .collect()
}

/// The backticked spans of a line, trimmed, empty ones left out.
fn tests_of(line: &str) -> Vec<&str> {
    line.split('`')
        .skip(1)
        .step_by(2)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect()
}

/// Runs the tests the `Done when:` lines of `body` name, each through `tests` (the `[gate] tests`
/// template) on `runner`, and returns one failure per test that fails and one per line that
/// names no test, in line order. An empty list means every line held.
///
/// A body with no `Done when:` line is [`NoDoneWhen`].
///
/// Interface: consumes [`run_named_test`] of `app/check/task.rs` and the manifest's `[gate]
/// tests` template (M16).
pub fn done_when_failures(
    body: &str,
    tests: &CommandLine,
    runner: &dyn Shell,
) -> Result<Vec<DoneWhenFailure>, NoDoneWhen> {
    let lines = done_when_lines(body);
    if lines.is_empty() {
        return Err(NoDoneWhen);
    }
    let mut failures = Vec::new();
    for line in lines {
        let names = tests_of(line);
        if names.is_empty() {
            failures.push(DoneWhenFailure::NotCheckable {
                line: line.to_owned(),
            });
        }
        for name in names {
            if let LineStatus::Fail(reason) = run_named_test(tests, runner, name) {
                failures.push(DoneWhenFailure::TestFailed {
                    line: line.to_owned(),
                    test: name.to_owned(),
                    reason,
                });
            }
        }
    }
    Ok(failures)
}

/// Why `zaf epic check` gave no verdict.
#[derive(Debug, thiserror::Error)]
pub enum EpicCheckError {
    /// The board failed.
    #[error(transparent)]
    Board(PortError),
    /// No open epic has the id.
    #[error("{0} is not an open epic")]
    NotAnEpic(TicketId),
    /// The epic's body holds no `Done when:` line.
    #[error("{epic}: {source}")]
    NoDoneWhen {
        /// The epic.
        epic: TicketId,
        /// What is missing.
        source: NoDoneWhen,
    },
    /// The epic is not signed as its body reads now, so its `Done when:` lines are not the
    /// Engineer's yet.
    #[error("{0} is not signed as its body reads now; sign it before it is checked")]
    NotSigned(TicketId),
    /// The manifest has no `[gate] tests` template, so no named test can run.
    #[error("the manifest has no `[gate] tests` template, so no `Done when:` test can run")]
    NoTestsTemplate,
}

/// `zaf epic check <epic>` (I9): the failures of both checks of the epic, one line each, the
/// behaviours check's first; none when everything holds.
///
/// Refused: an epic that is not open, one with no `Done when:` line, one not signed as its body
/// reads now, and a manifest with no `[gate] tests` template (`tests` is `None`).
///
/// Interface: consumes [`behaviour_failures`] (#1247) and [`done_when_failures`] (#1245).
pub fn check(
    board: &dyn Board,
    epic: TicketId,
    tests: Option<&CommandLine>,
    runner: &dyn Shell,
) -> Result<Vec<String>, EpicCheckError> {
    let read = match board.epic(epic) {
        Ok(read) => read,
        Err(PortError::NotFound(_)) => return Err(EpicCheckError::NotAnEpic(epic)),
        Err(error) => return Err(EpicCheckError::Board(error)),
    };
    if done_when_lines(read.body.as_str()).is_empty() {
        return Err(EpicCheckError::NoDoneWhen {
            epic,
            source: NoDoneWhen,
        });
    }
    if !read.signed() {
        return Err(EpicCheckError::NotSigned(epic));
    }
    let tests = tests.ok_or(EpicCheckError::NoTestsTemplate)?;
    let mut lines = match behaviour_failures(&read, &read.records, &read.tasks) {
        BehaviourCheck::NotPlannedAsGraph => vec![BehaviourCheck::NotPlannedAsGraph.to_string()],
        BehaviourCheck::Failures(failures) => failures.iter().map(ToString::to_string).collect(),
    };
    let done_when = done_when_failures(read.body.as_str(), tests, runner)
        .map_err(|source| EpicCheckError::NoDoneWhen { epic, source })?;
    lines.extend(done_when.iter().map(ToString::to_string));
    Ok(lines)
}

#[cfg(test)]
mod tests;
