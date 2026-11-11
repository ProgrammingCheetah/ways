//! What a resumed phase's pack adds to today's pack: the diff of the task's recorded pull
//! request, the findings at its current head, and the hand-off comment the exiting model left.
//!
//! The diff is read through the VCS port in the recorded workspace against the base, as
//! `zaf changes` reads it; the findings and the hand-off come from the forge. Nothing is stored:
//! the sections live in the pack file and nowhere else.

use std::fmt::{self, Write as _};
use std::path::Path;

use super::BriefError;
use crate::ports::{BaseName, CheckState, Forge, PortError, PrNumber, Vcs};

/// The task's recorded pull request and where to read its change, for a resumed phase.
#[derive(Clone, Copy)]
pub struct PhaseView<'a> {
    /// The recorded pull request.
    pub pr: PrNumber,
    /// The recorded workspace; `None` when no workspace record holds, and then the pack says
    /// the diff cannot be read.
    pub workspace: Option<&'a Path>,
    /// The base the task merges into.
    pub base: &'a BaseName,
    /// The forge holding the pull request.
    pub forge: &'a dyn Forge,
    /// The VCS that reads the workspace's change.
    pub vcs: &'a dyn Vcs,
}

/// The `## Diff`, `## Findings` and `## Hand-off` sections of a resumed phase's pack.
pub(super) fn sections(view: &PhaseView<'_>) -> Result<String, BriefError> {
    let mut text = String::new();
    diff(&mut text, view)?;
    findings(&mut text, view)?;
    handoff(&mut text, view)?;
    Ok(text)
}

fn forge_error(error: PortError) -> BriefError {
    BriefError::Forge(error)
}

fn vcs_error(error: PortError) -> BriefError {
    BriefError::Vcs(error)
}

fn infallible(result: fmt::Result) {
    result.expect("writing to a String cannot fail");
}

/// The change against the base: the stat, then each file's hunks.
fn diff(text: &mut String, view: &PhaseView<'_>) -> Result<(), BriefError> {
    infallible(writeln!(
        text,
        "## Diff\n\nPull request {} against {}.\n",
        view.pr, view.base
    ));
    let Some(workspace) = view.workspace else {
        infallible(writeln!(
            text,
            "No workspace is recorded for the task, so its diff cannot be read.\n"
        ));
        return Ok(());
    };
    let stat = view
        .vcs
        .diff_stat(workspace, view.base)
        .map_err(vcs_error)?;
    if stat.files.is_empty() {
        infallible(writeln!(text, "The change holds no file.\n"));
        return Ok(());
    }
    infallible(writeln!(text, "```\n{}```\n", stat));
    for file in &stat.files {
        let hunks = view
            .vcs
            .diff_hunks(workspace, view.base, &file.path)
            .map_err(vcs_error)?;
        infallible(writeln!(
            text,
            "### {}\n\n```diff\n{}\n```\n",
            file.path,
            hunks.trim_end()
        ));
    }
    Ok(())
}

/// The review comments made at the pull request's current head, and its failed checks.
///
/// A comment counts only when the forge says it was made on the current head: one made on an
/// older head, or on none the forge names, is not new and is left out.
fn findings(text: &mut String, view: &PhaseView<'_>) -> Result<(), BriefError> {
    let head = view.forge.head(view.pr).map_err(forge_error)?;
    let comments = view.forge.review_comments(view.pr).map_err(forge_error)?;
    let checks = view.forge.checks(view.pr).map_err(forge_error)?;
    infallible(writeln!(text, "## Findings\n"));
    let Some(head) = head else {
        infallible(writeln!(
            text,
            "The forge names no head for the pull request, so no finding is pinned to it.\n"
        ));
        return Ok(());
    };
    infallible(writeln!(text, "At head {head}.\n"));
    let mut any = false;
    for comment in comments.iter().filter(|comment| {
        comment
            .head
            .as_ref()
            .is_some_and(|commit| commit.as_str() == head)
    }) {
        any = true;
        let place = match &comment.location {
            Some(location) => match location.line {
                Some(line) => format!(" on {}:{line}", location.path),
                None => format!(" on {}", location.path),
            },
            None => String::new(),
        };
        infallible(writeln!(
            text,
            "- review comment by {}{place}: {}",
            comment.author,
            comment.body.trim_end()
        ));
    }
    for check in checks
        .iter()
        .filter(|check| check.state == CheckState::Failed)
    {
        any = true;
        infallible(writeln!(text, "- failed check: {}", check.name));
    }
    if any {
        infallible(writeln!(text));
    } else {
        infallible(writeln!(text, "None at this head.\n"));
    }
    Ok(())
}

/// The hand-off comment, when the exiting model left one.
fn handoff(text: &mut String, view: &PhaseView<'_>) -> Result<(), BriefError> {
    let handoff = view.forge.handoff(view.pr).map_err(forge_error)?;
    infallible(writeln!(text, "## Hand-off\n"));
    match handoff {
        Some(handoff) => infallible(writeln!(text, "{}\n", handoff.trim_end())),
        None => infallible(writeln!(text, "None left.\n")),
    }
    Ok(())
}

#[cfg(test)]
mod tests;
