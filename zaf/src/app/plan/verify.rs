//! `plan verify` (D24, D27, I9, I13): an epic's graph held to O3's structural checks.
//!
//! The graph is read from the epic's tickets ([`PlanGraph::read`]) and checked, with the epic's
//! plan records and the release's vocabulary, by [`check_shape`]; this is the use case that reads
//! them and nothing else. Nothing is written. An epic filed before 0.6.0 has no graph to hold to
//! the rules, and is refused with one line ([`VerifyError::Older`]).

use std::path::Path;

use crate::app::release::{VocabularyLoadError, release_vocabulary};
use crate::domain::plan::read::PlanGraph;
use crate::domain::plan::shape::{ShapeFailure, check_shape};
use crate::domain::{Epic, TaskBody, TicketId};
use crate::manifest::Manifest;
use crate::ports::{Board, PortError, Vcs};

/// Why an epic's graph is not verified.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    /// The ticket is no open epic.
    #[error("{0} is not an open epic")]
    NotAnEpic(TicketId),
    /// The epic has no ticket and no plan record.
    #[error(
        "epic {0} has no planned tickets: file its plan with `zaf plan file {0}`, or see `zaf epic show {0}`"
    )]
    NoTickets(TicketId),
    /// The epic has tickets and no plan record or planned ticket: it was filed before 0.6.0.
    #[error("planned before 0.6.0: plan verify checks graph epics")]
    Older,
    /// The epic's release vocabulary is not loaded.
    #[error(transparent)]
    Vocabulary(#[from] VocabularyLoadError),
    /// The board could not answer.
    #[error(transparent)]
    Board(PortError),
}

/// Every way the graph of `epic` breaks O3's structural rules, in the order [`check_shape`] gives
/// them; empty for a good graph.
///
/// Interface: `app::plan::verify(epic) -> Vec<ShapeFailure>`, read by the chair's header; the
/// root, the manifest, the board and the version control are what the function reads it with.
///
/// # Errors
/// [`VerifyError::NotAnEpic`] when `epic` is no open epic, [`VerifyError::NoTickets`] when it has
/// no ticket and no plan record, [`VerifyError::Older`] when it was filed before 0.6.0,
/// [`VerifyError::Vocabulary`] when its release's vocabulary does not load, and
/// [`VerifyError::Board`] when the board could not answer.
pub fn verify(
    root: &Path,
    manifest: &Manifest,
    board: &dyn Board,
    vcs: &dyn Vcs,
    epic: TicketId,
) -> Result<Vec<ShapeFailure>, VerifyError> {
    let epics = board.epics().map_err(VerifyError::Board)?;
    let found = epics
        .iter()
        .find(|candidate| candidate.id == epic)
        .ok_or(VerifyError::NotAnEpic(epic))?;
    if found.records.is_empty() && !planned(found) {
        return Err(if found.tasks.is_empty() {
            VerifyError::NoTickets(epic)
        } else {
            VerifyError::Older
        });
    }
    let vocabulary = release_vocabulary(root, manifest, vcs, found)?;
    let graph = PlanGraph::read(found, &epics);
    Ok(check_shape(&graph, &vocabulary, &found.records))
}

/// Whether one of the epic's tickets is a planned one (D4).
fn planned(epic: &Epic) -> bool {
    epic.tasks.iter().any(|task| {
        matches!(
            TaskBody::parse(task.body.as_str()),
            Ok(TaskBody::Planned(_))
        )
    })
}
