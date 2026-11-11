//! A task's standing in its phase list (docs/zaf-actions.md §5.1, "The standing").
//!
//! [`derive_standing`] is a pure rule table in the pattern of [`derive_phase`](super::derive_phase):
//! it reads no file, no forge and no clock, only the [`ListedPhase`]s and the [`StandingFacts`] it
//! is handed, and it writes nothing. The current phase is the first listed action whose `ends`
//! fact does not hold for the pull request's current head (the ruling on #424), so a push that
//! moves the head reopens a review posted at the old head. [`gather_standing_facts`] collects the
//! facts from the forge and the phase run folders; a read that fails is a
//! [`StandingUnreadable`], never a guess.
//!
//! # The phase run folder
//!
//! The launcher (#438) writes one folder for each run of a phase action for a task, in
//! `.zaf/phases/`; its name is the launcher's to choose, since the reader reads files and never
//! a name. A folder holds:
//!
//! - `task`: the task's number. Written last of the files a folder starts with; its modified time
//!   is when the run started.
//! - `action`: the action's name.
//! - `head`: the pull request's head when the run started; empty when the task has no pull
//!   request.
//! - `pid`: the process, as a [`Holder`](crate::ports::Holder): the pid, a space, the start the OS
//!   reported. Written once the process starts.
//! - `exit`: the process's exit code as a decimal number, written once the process has ended (the
//!   ruling on #451); its modified time is when the run ended. Absent while the run goes.
//!
//! A run is live while its `pid` names a live process and no `exit` is there. A folder with a dead
//! pid and no `exit` has ended with no exit code, which is never read as 0.

use std::time::SystemTime;

use crate::app::actions::{ActionKind, ActionName, PhaseEnd};

mod gather;

pub use gather::{PHASES_DIR, StandingUnreadable, gather_standing_facts, running_tasks};

/// One entry of a task type's phase list, with what its action declares: the `kind`, `ends` and
/// `on-fail` of its metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedPhase {
    /// The action's name.
    pub action: ActionName,
    /// Whether it starts a model, waits on the outside or is run by zaf alone.
    pub kind: ActionKind,
    /// The fact that ends the phase.
    pub ends: PhaseEnd,
    /// The model action to start when a wait fails; none when it declares none.
    pub on_fail: Option<ActionName>,
}

/// Where a task stands in its phase list: `Running { action }`, `Ready { action, since }`,
/// `Waiting { action }`, `Failed { action, on_fail: Option<ActionName>, since }` or `Done`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Standing {
    /// A phase process of the task is live, whatever the forge says.
    Running {
        /// The action of the live process.
        action: ActionName,
    },
    /// The phase may start now.
    Ready {
        /// The phase's action: the first of a run of adjacent model actions.
        action: ActionName,
        /// When the previous phase's fact came to hold, or when the task became ready for the
        /// first phase.
        since: SystemTime,
    },
    /// A wait that has run at the current head and whose fact does not hold yet.
    Waiting {
        /// The wait's action.
        action: ActionName,
    },
    /// A wait that failed: a check failed at the head, or its run at the head exited non-zero.
    Failed {
        /// The wait's action.
        action: ActionName,
        /// The wait's `on-fail`; none when it declares none.
        on_fail: Option<ActionName>,
        /// When it failed.
        since: SystemTime,
    },
    /// Every phase's fact holds: the pull request is merged.
    Done,
}

/// How one check run at the pull request's head stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    /// Queued or running.
    Pending,
    /// Finished green, at this time.
    Passed(SystemTime),
    /// Finished red, at this time.
    Failed(SystemTime),
}

/// One review posted on the pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewFact {
    /// The commit it was made on; none when the forge did not say, which is no head at all.
    pub head: Option<String>,
    /// When it was posted.
    pub posted_at: SystemTime,
}

/// What the forge says of the task's recorded pull request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullFacts {
    /// When it opened.
    pub opened_at: SystemTime,
    /// When it merged; none while it is not merged.
    pub merged_at: Option<SystemTime>,
    /// Its current head; none when the forge reports none.
    pub head: Option<String>,
    /// Its check runs at that head.
    pub checks: Vec<CheckResult>,
    /// The names of the checks among `checks` that failed, in the order the forge lists them.
    pub failed_checks: Vec<String>,
    /// Its reviews, at whatever head each was made.
    pub reviews: Vec<ReviewFact>,
}

/// How a run's process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunExit {
    /// The exit code.
    pub code: i32,
    /// When the code was recorded.
    pub at: SystemTime,
}

/// One phase run folder of the task, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseRun {
    /// The action it ran.
    pub action: ActionName,
    /// The pull request's head when it started; none without one.
    pub head: Option<String>,
    /// When it started.
    pub started_at: SystemTime,
    /// Its process is alive and no exit is recorded.
    pub alive: bool,
    /// How it ended; none while it runs, and none for a dead process that recorded no code.
    pub exit: Option<RunExit>,
}

/// Everything the rule table reads about one task. The caller gathers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandingFacts {
    /// When the task became ready for its first phase.
    pub ready_since: SystemTime,
    /// The recorded pull request, when the task has one.
    pub pull: Option<PullFacts>,
    /// Every phase run folder of the task.
    pub runs: Vec<PhaseRun>,
}

impl StandingFacts {
    /// The pull request's current head.
    fn head(&self) -> Option<&str> {
        self.pull.as_ref()?.head.as_deref()
    }

    /// The latest run of `action`, by start.
    fn latest_run(&self, action: &ActionName) -> Option<&PhaseRun> {
        self.runs
            .iter()
            .filter(|run| &run.action == action)
            .max_by_key(|run| run.started_at)
    }

    /// The latest run of `action` that started at the pull request's current head.
    fn run_at_head(&self, action: &ActionName) -> Option<&PhaseRun> {
        let head = self.head();
        self.runs
            .iter()
            .filter(|run| &run.action == action && run.head.as_deref() == head)
            .max_by_key(|run| run.started_at)
    }
}

/// What a failed wait failed on: the pull request's head, the checks that failed there and the
/// exit code of the wait's own run at that head when it was not zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitFailure {
    /// The pull request's current head; none when the forge reports none.
    pub head: Option<String>,
    /// The names of the checks that failed.
    pub checks: Vec<String>,
    /// The non-zero exit code of the wait's run at the head.
    pub exit: Option<i32>,
}

/// What the wait `action` failed on, read from the same facts [`derive_standing`] read.
#[must_use]
pub fn wait_failure(action: &ActionName, facts: &StandingFacts) -> WaitFailure {
    WaitFailure {
        head: facts.head().map(str::to_owned),
        checks: facts
            .pull
            .iter()
            .flat_map(|pull| pull.failed_checks.iter().cloned())
            .collect(),
        exit: facts
            .run_at_head(action)
            .and_then(|run| run.exit)
            .map(|exit| exit.code)
            .filter(|code| *code != 0),
    }
}

/// The task's standing in `list`, its type's phase list: `derive_standing(list, facts) -> Standing`.
///
/// A live phase process makes it [`Standing::Running`], whatever the forge says. Otherwise a
/// merged pull request is [`Standing::Done`], and the first listed action whose fact does not hold
/// is the current phase. Adjacent `model` actions form one run, so the standing names the first of
/// them; a wait is [`Standing::Ready`] until it has run at the current head, then
/// [`Standing::Waiting`], and [`Standing::Failed`] when a check failed at the head or its run at
/// the head exited non-zero.
pub fn derive_standing(list: &[ListedPhase], facts: &StandingFacts) -> Standing {
    if let Some(run) = facts.runs.iter().find(|run| run.alive) {
        return Standing::Running {
            action: run.action.clone(),
        };
    }
    if facts
        .pull
        .as_ref()
        .is_some_and(|pull| pull.merged_at.is_some())
    {
        return Standing::Done;
    }
    let mut previous: Option<SystemTime> = None;
    for group in runs_of(list) {
        let lead = &group[0];
        let mut last = previous;
        let mut holds = true;
        for phase in group {
            match fact_time(phase, lead, facts, previous) {
                Some(time) => last = Some(time),
                None => {
                    holds = false;
                    break;
                }
            }
        }
        if !holds {
            return current(lead, facts, previous);
        }
        previous = last;
    }
    Standing::Done
}

/// The list split into runs: adjacent `model` actions together, every other action alone.
fn runs_of(list: &[ListedPhase]) -> Vec<&[ListedPhase]> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < list.len() {
        let mut end = start + 1;
        if list[start].kind == ActionKind::Model {
            while end < list.len() && list[end].kind == ActionKind::Model {
                end += 1;
            }
        }
        runs.push(&list[start..end]);
        start = end;
    }
    runs
}

/// When `phase`'s fact came to hold for the pull request's current head; none when it does not.
///
/// The `exit` fact of a run of model actions is read from the run's first action, since one
/// process covers them all. `previous` is when the fact of the phase before the run came to hold.
fn fact_time(
    phase: &ListedPhase,
    lead: &ListedPhase,
    facts: &StandingFacts,
    previous: Option<SystemTime>,
) -> Option<SystemTime> {
    let pull = facts.pull.as_ref();
    match phase.ends {
        PhaseEnd::PrOpen => pull.map(|pull| pull.opened_at),
        PhaseEnd::Merged => pull?.merged_at,
        PhaseEnd::ReviewPosted => {
            let head = facts.head()?;
            pull?
                .reviews
                .iter()
                .filter(|review| review.head.as_deref() == Some(head))
                .map(|review| review.posted_at)
                .min()
        }
        PhaseEnd::CiDone => checks_done(&pull?.checks),
        PhaseEnd::Exit => {
            let run = facts.latest_run(&lead.action)?;
            let exit = run.exit?;
            let after = previous.is_none_or(|previous| run.started_at > previous);
            (after && exit.code == 0).then_some(exit.at)
        }
    }
}

/// When the checks finished, if they are all done and green: there is at least one, none is
/// pending and none failed. No check at all is not done, since nothing says it will never come.
fn checks_done(checks: &[CheckResult]) -> Option<SystemTime> {
    let mut latest = None;
    for check in checks {
        match check {
            CheckResult::Passed(at) => latest = latest.max(Some(*at)),
            CheckResult::Pending | CheckResult::Failed(_) => return None,
        }
    }
    latest
}

/// The standing of the phase whose fact does not hold, `lead` being its action (the first of a
/// run of model actions).
fn current(lead: &ListedPhase, facts: &StandingFacts, previous: Option<SystemTime>) -> Standing {
    let action = lead.action.clone();
    if lead.kind != ActionKind::Wait {
        return Standing::Ready {
            action,
            since: previous.unwrap_or(facts.ready_since),
        };
    }
    let at_head = facts.run_at_head(&action);
    let failed_check = facts
        .pull
        .iter()
        .flat_map(|pull| &pull.checks)
        .filter_map(|check| match check {
            CheckResult::Failed(at) => Some(*at),
            CheckResult::Pending | CheckResult::Passed(_) => None,
        })
        .min();
    let failed_run = at_head
        .and_then(|run| run.exit)
        .filter(|exit| exit.code != 0)
        .map(|exit| exit.at);
    match (failed_check, failed_run) {
        (None, None) if at_head.is_some() => Standing::Waiting { action },
        (None, None) => Standing::Ready {
            action,
            since: previous.unwrap_or(facts.ready_since),
        },
        (check, run) => Standing::Failed {
            action,
            on_fail: lead.on_fail.clone(),
            since: check
                .into_iter()
                .chain(run)
                .min()
                .unwrap_or(facts.ready_since),
        },
    }
}

#[cfg(test)]
mod tests;
