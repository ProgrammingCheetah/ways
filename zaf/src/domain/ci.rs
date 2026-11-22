//! Whether a pull request is expected to get a check.
//!
//! A forge runs a workflow on a pull request only when the workflow's `pull_request` trigger
//! lets the change through, so a pull request that touches nothing a workflow watches is never
//! checked, and a wait for a check that never comes would hang. [`checks_expected`] answers from
//! the workflow filters, the changed files and whether that list may be cut short. It leans to
//! yes: a filter it is not sure to read right expects a check, because a wrongly expected check
//! costs a wait that ends at its grace, and a wrongly dropped one lets an unchecked change merge.
//!
//! The workflow files are read in an adapter (the YAML stays there): this rule gets each one as
//! a [`WorkflowFilter`]. The glob matcher is a pure library and stays behind this module.

use globset::GlobBuilder;

use super::RepoPath;

/// The folders whose files are workflows a forge runs.
const WORKFLOW_DIRS: [&str; 2] = [".forgejo/workflows/", ".github/workflows/"];

/// What a workflow's `pull_request` trigger asks of a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullRequestFilter {
    /// The workflow has no trigger that reaches a pull request: it never runs on one.
    NoTrigger,
    /// A `pull_request` or `pull_request_target` trigger with no `paths` filter, or a `push`
    /// with no filter: any change runs it.
    AnyChange,
    /// A `pull_request` trigger with a `paths` filter: a change runs it when a changed file
    /// matches one of these patterns.
    Paths(Vec<String>),
    /// A `push` trigger with a `branches` filter: a push to the slice's branch runs it when the
    /// branch matches one of these patterns.
    Branches(Vec<String>),
    /// Several triggers that each narrow: the workflow runs when any of them lets the change
    /// through. Holds no `NoTrigger`, `AnyChange` or `Either`.
    Either(Vec<PullRequestFilter>),
    /// The read is not certain: the file is unreadable or not a workflow, or it filters by
    /// `paths-ignore` or by branch, or says something the reader does not understand.
    Unsure,
}

/// One workflow file, as the adapter read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowFilter {
    /// Where the file sits in the repository.
    pub path: RepoPath,
    /// What its `pull_request` trigger asks of a change.
    pub filter: PullRequestFilter,
}

/// Whether `path` is itself a workflow file: `.yml` or `.yaml` under a workflows folder.
fn is_workflow_file(path: &str) -> bool {
    WORKFLOW_DIRS.iter().any(|dir| path.starts_with(dir))
        && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

/// Whether the glob `pattern` is sure to read right and matches `path`; `None` when it is not
/// sure (a negation, a brace group a forge does not read, a pattern that does not compile).
fn glob_matches(pattern: &str, path: &str) -> Option<bool> {
    if pattern.starts_with('!') || pattern.contains(['{', '}']) {
        return None;
    }
    let glob = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .ok()?;
    Some(glob.compile_matcher().is_match(path))
}

/// Whether `filter` lets a pull request from `branch` that changes `changed` through. A branch
/// not known, or a pattern not sure to read right, lets it through.
fn runs_on(filter: &PullRequestFilter, changed: &[RepoPath], branch: Option<&str>) -> bool {
    match filter {
        PullRequestFilter::NoTrigger => false,
        PullRequestFilter::AnyChange | PullRequestFilter::Unsure => true,
        PullRequestFilter::Paths(patterns) => patterns.iter().any(|pattern| {
            changed
                .iter()
                .any(|file| glob_matches(pattern, file.as_str()).unwrap_or(true))
        }),
        PullRequestFilter::Branches(patterns) => branch.is_none_or(|branch| {
            patterns
                .iter()
                .any(|pattern| glob_matches(pattern, branch).unwrap_or(true))
        }),
        PullRequestFilter::Either(filters) => filters
            .iter()
            .any(|filter| runs_on(filter, changed, branch)),
    }
}

/// Whether a check is expected on a pull request from `branch` (the slice's branch, when it is
/// known) that changes `changed`.
///
/// `changed` lists a rename under both its names, so a file moved out of a watched path still
/// matches. `cut_short` says the forge may have cut the list short (its page limit), so an
/// unseen file may match.
pub fn checks_expected(
    workflows: &[WorkflowFilter],
    changed: &[RepoPath],
    cut_short: bool,
    branch: Option<&str>,
) -> bool {
    if cut_short || changed.iter().any(|file| is_workflow_file(file.as_str())) {
        return true;
    }
    workflows
        .iter()
        .any(|workflow| runs_on(&workflow.filter, changed, branch))
}

#[cfg(test)]
mod tests;
