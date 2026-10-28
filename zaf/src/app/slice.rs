//! `zaf slice open|pr|merge|drop`: a task's slice from workspace to merge, each one call.
//!
//! Every slice verb first resolves its [`SliceTarget`] from the board and the
//! manifest's branch patterns ([`resolve`]), then runs on that target. The two
//! steps are apart because the caller needs the slice's name in between (the
//! light gate runs inside the slice's workspace).

use crate::domain::{IssueNumber, TaskLabel};
use crate::manifest::{BranchPattern, BranchesSection};
use crate::ports::{
    BaseMerge, BaseName, Board, CommitId, Forge, Gate, GateLevel, GateVerdict, PortError, PrMerge,
    PrNumber, PrRequest, Publication, SliceName, SliceOpening, Ticket, Vcs,
};

/// The placeholder for the task's bare number in a branch pattern.
const TASK_PLACEHOLDER: &str = "{task}";
/// The placeholder for the epic's bare number in a branch pattern.
const EPIC_PLACEHOLDER: &str = "{epic}";
/// The placeholder for the release version in a branch pattern.
const VERSION_PLACEHOLDER: &str = "{version}";

/// The step of a slice verb a port call failed in, so the caller can say which.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Step {
    /// Reading the task's ticket.
    Ticket,
    /// Opening the slice and its workspace.
    Open,
    /// Reading the slice's change.
    Diff,
    /// Checking for and committing uncommitted work.
    Commit,
    /// Merging the base into the slice.
    MergeBase,
    /// Running the light gate.
    Gate,
    /// Publishing the slice.
    Publish,
    /// Opening or finding the pull request.
    PullRequest,
    /// Merging the pull request.
    Merge,
    /// Labelling the task done.
    Label,
    /// Removing the slice's workspace.
    Drop,
}

/// Why a slice verb stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SliceError {
    /// The task pattern names `{epic}` and the ticket belongs to no epic.
    #[error("{task} belongs to no epic, and the branch pattern names `{{epic}}`")]
    NoEpic {
        /// The task.
        task: IssueNumber,
    },
    /// A branch pattern names `{version}` and the call gave none.
    #[error("the branch pattern names `{{version}}`, and no version was given")]
    NoVersion,
    /// Merging the base in stopped on conflicts; the slice is unchanged.
    #[error("merge-base: the base conflicts with the slice in {}", paths.join(", "))]
    Conflicted {
        /// The conflicting paths.
        paths: Vec<String>,
    },
    /// The light gate ran and failed.
    #[error("gate: the light gate failed")]
    GateFailed {
        /// What the gate printed.
        output: String,
    },
    /// A port call failed in a step.
    #[error("{step}: {error}")]
    Port {
        /// The step it failed in.
        step: Step,
        /// The port's error.
        #[source]
        error: PortError,
    },
}

impl SliceError {
    /// The step the verb stopped at.
    pub fn step(&self) -> Option<Step> {
        match self {
            Self::NoEpic { .. } | Self::NoVersion => None,
            Self::Conflicted { .. } => Some(Step::MergeBase),
            Self::GateFailed { .. } => Some(Step::Gate),
            Self::Port { step, .. } => Some(*step),
        }
    }
}

/// Tags a port error with the step it failed in.
fn at(step: Step) -> impl FnOnce(PortError) -> SliceError {
    move |error| SliceError::Port { step, error }
}

/// The query every slice verb starts from: the task, and the release version its branches name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceQuery {
    /// The task.
    pub task: IssueNumber,
    /// The release version, for patterns that name `{version}`.
    pub version: Option<String>,
}

/// A task's slice, resolved: its ticket, its slice name and the base it merges into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceTarget {
    /// The task's ticket as the board holds it.
    pub ticket: Ticket,
    /// The slice: the task branch pattern, filled.
    pub slice: SliceName,
    /// The base: the integration branch pattern, filled.
    pub base: BaseName,
}

/// Resolves a task's slice from its ticket and the manifest's branch patterns.
pub fn resolve(
    board: &dyn Board,
    branches: &BranchesSection,
    query: &SliceQuery,
) -> Result<SliceTarget, SliceError> {
    let ticket = board.task(query.task).map_err(at(Step::Ticket))?;
    let slice = fill(&branches.task, &ticket, query.version.as_deref())?;
    let base = fill(&branches.integration, &ticket, query.version.as_deref())?;
    let target = SliceTarget {
        ticket,
        slice: SliceName::from(slice),
        base: BaseName::from(base),
    };
    Ok(target)
}

/// Fills a branch pattern's placeholders; a placeholder without a value is an error.
fn fill(
    pattern: &BranchPattern,
    ticket: &Ticket,
    version: Option<&str>,
) -> Result<String, SliceError> {
    let mut name = pattern
        .as_ref()
        .replace(TASK_PLACEHOLDER, &bare(ticket.number));
    if name.contains(EPIC_PLACEHOLDER) {
        let epic = ticket.epic.ok_or(SliceError::NoEpic {
            task: ticket.number,
        })?;
        name = name.replace(EPIC_PLACEHOLDER, &bare(epic));
    }
    if name.contains(VERSION_PLACEHOLDER) {
        let version = version.ok_or(SliceError::NoVersion)?;
        name = name.replace(VERSION_PLACEHOLDER, version);
    }
    Ok(name)
}

/// An issue number without its `#`, as a branch name carries it.
fn bare(number: IssueNumber) -> String {
    number.get().to_string()
}

/// `zaf slice open`: opens the task's slice and workspace at its base.
pub fn open(vcs: &dyn Vcs, target: &SliceTarget) -> Result<SliceOpening, SliceError> {
    vcs.open_slice(&target.slice, &target.base)
        .map_err(at(Step::Open))
}

/// What `zaf slice pr` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReport {
    /// The pull request, new or the one already open.
    pub pr: PrNumber,
    /// The commit made from uncommitted work, if there was any.
    pub commit: Option<CommitId>,
    /// What publishing did.
    pub publication: Publication,
}

/// The pull request's title and the commit message: the ticket's title and its number.
pub fn pr_title(ticket: &Ticket) -> String {
    format!("{} ({})", ticket.title, ticket.number)
}

/// The pull request a slice opens: from the slice into its base, the task in its title.
fn pr_request(target: &SliceTarget) -> PrRequest {
    PrRequest {
        slice: target.slice.clone(),
        base: target.base.clone(),
        title: pr_title(&target.ticket),
        body: format!("Refs {}.", target.ticket.number),
    }
}

/// `zaf slice pr`: commits uncommitted work, publishes the slice, and opens its pull request.
///
/// Repeating it on a published slice with an open pull request changes nothing
/// and returns that pull request.
pub fn pr(vcs: &dyn Vcs, forge: &dyn Forge, target: &SliceTarget) -> Result<PrReport, SliceError> {
    let request = pr_request(target);
    let clean = vcs.is_clean(&target.slice).map_err(at(Step::Commit))?;
    let commit = if clean {
        None
    } else {
        let commit = vcs
            .commit(&target.slice, &request.title)
            .map_err(at(Step::Commit))?;
        Some(commit)
    };
    let publication = vcs.publish(&target.slice).map_err(at(Step::Publish))?;
    let pr = forge.open_pr(&request).map_err(at(Step::PullRequest))?;
    let report = PrReport {
        pr,
        commit,
        publication,
    };
    Ok(report)
}

/// The ports `zaf slice merge` drives. A project without a gate passes `None`
/// and the gate step is skipped.
#[derive(Clone, Copy)]
pub struct MergePorts<'a> {
    /// The board, to label the task done.
    pub board: &'a dyn Board,
    /// The VCS, to merge the base in, publish and drop.
    pub vcs: &'a dyn Vcs,
    /// The forge, to merge the pull request.
    pub forge: &'a dyn Forge,
    /// The gate, run in the slice's workspace; `None` when the project declares none.
    pub gate: Option<&'a dyn Gate>,
}

/// The command `zaf slice merge` runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeCommand {
    /// The resolved slice.
    pub target: SliceTarget,
    /// The pull request `zaf slice pr` recorded, if the caller has it; otherwise
    /// the slice's open pull request is looked up.
    pub pr: Option<PrNumber>,
}

/// What a merge that ran did, step by step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReport {
    /// What merging the base in did.
    pub base_merge: BaseMerge,
    /// The light gate's verdict; `None` when the project declares no gate.
    pub gate: Option<GateVerdict>,
    /// What publishing did.
    pub publication: Publication,
    /// The pull request merged.
    pub pr: PrNumber,
    /// What merging it did.
    pub merge: PrMerge,
}

/// What `zaf slice merge` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeOutcome {
    /// The steps ran to the end.
    Merged(MergeReport),
    /// The task was already done; only its workspace was dropped, if it was still there.
    AlreadyDone,
}

/// `zaf slice merge`: merges the base in, runs the light gate, publishes, merges
/// the pull request, labels the task done and drops the workspace.
///
/// It stops at the first failing step, and the error names it. A task already
/// labelled done succeeds quietly.
pub fn merge(ports: MergePorts<'_>, command: &MergeCommand) -> Result<MergeOutcome, SliceError> {
    let target = &command.target;
    if target.ticket.label == Some(TaskLabel::Done) {
        ports
            .vcs
            .drop_slice(&target.slice)
            .map_err(at(Step::Drop))?;
        return Ok(MergeOutcome::AlreadyDone);
    }
    let base_merge = ports
        .vcs
        .merge_base(&target.slice)
        .map_err(at(Step::MergeBase))?;
    if let BaseMerge::Conflicted { paths } = &base_merge {
        return Err(SliceError::Conflicted {
            paths: paths.clone(),
        });
    }
    let gate = match ports.gate {
        Some(gate) => {
            let run = gate.run(GateLevel::Light).map_err(at(Step::Gate))?;
            if run.verdict == GateVerdict::Failed {
                return Err(SliceError::GateFailed { output: run.output });
            }
            Some(run.verdict)
        }
        None => None,
    };
    let publication = ports
        .vcs
        .publish(&target.slice)
        .map_err(at(Step::Publish))?;
    let pr = match command.pr {
        Some(pr) => pr,
        None => ports
            .forge
            .open_pr(&pr_request(target))
            .map_err(at(Step::PullRequest))?,
    };
    let merge = ports.forge.merge(pr).map_err(at(Step::Merge))?;
    ports
        .board
        .label(target.ticket.number, TaskLabel::Done)
        .map_err(at(Step::Label))?;
    ports
        .vcs
        .drop_slice(&target.slice)
        .map_err(at(Step::Drop))?;
    let report = MergeReport {
        base_merge,
        gate,
        publication,
        pr,
        merge,
    };
    Ok(MergeOutcome::Merged(report))
}

/// `zaf slice drop`: removes the slice's workspace; a slice already gone succeeds quietly.
pub fn drop(vcs: &dyn Vcs, target: &SliceTarget) -> Result<(), SliceError> {
    vcs.drop_slice(&target.slice).map_err(at(Step::Drop))
}

#[cfg(test)]
mod tests;
