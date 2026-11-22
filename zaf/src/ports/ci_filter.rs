//! The CI workflows of a repository as they stand at a pull request's base.
//!
//! Whether a pull request is expected to get a check depends on which workflows run on it. A
//! pull request may add or change a workflow, and its head is the author's to write, so the
//! workflows are read at the base, never at the head: every change to the base's workflows came
//! through a pull request that touched a workflow file, which always expects a check.

use super::{BranchName, PortError};
use crate::domain::WorkflowFilter;

/// The workflow files a forge runs, read at a branch.
pub trait CiFilter {
    /// Every workflow file of the forge's own folder (`.forgejo/workflows` or `.github/workflows`)
    /// at `base`, `.yml` and `.yaml` both, each as its `pull_request` filter, in name order.
    /// A file the forge refuses or that cannot be read is
    /// [`crate::domain::PullRequestFilter::Unsure`], never left out, so the rule expects a check.
    /// A base with no such folder has no workflows; a base the forge does not have, or a list the
    /// forge fails to give, is an error.
    fn workflows(&self, base: &BranchName) -> Result<Vec<WorkflowFilter>, PortError>;
}

#[cfg(test)]
mod tests;
