//! `plan file` with empty stdin (D17, I9): the epic's last plan record that has no filing mark,
//! finished from the record.
//!
//! The record's plan is read as it was written and checked as `plan file` checks it, then steps 4
//! to 6 of D17 are repeated: each ticket the epic lacks is filed once, each dropped ticket still
//! open is closed, and the mark is posted. The design is hashed first: one that differs from the
//! record's refuses the finish, raises a question on the epic naming both hashes, and files
//! nothing. An epic whose last record has its mark has nothing to finish.

use std::path::Path;

use super::check::{optimised, prepare};
use super::file::{FileError, FiledPlan, complete};
use super::tickets::tickets;
use crate::app::release::design_text;
use crate::domain::{BodyHash, Standing, TicketBody, TicketId};
use crate::manifest::Manifest;
use crate::ports::{Board, NewQuestion, Vcs};

/// What a finish did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finished {
    /// The epic's last record has its mark, or the epic has no record: nothing was posted.
    Nothing,
    /// The record was finished.
    Filed(Box<FiledPlan>),
}

impl Finished {
    /// The line `plan file` prints for it.
    pub fn summary(&self) -> String {
        match self {
            Self::Nothing => {
                "nothing to finish: the epic's last plan record has its filing mark".to_owned()
            }
            Self::Filed(filed) => filed.summary(),
        }
    }
}

/// Finishes the last plan record of `epic` that has no filing mark.
///
/// # Errors
///
/// A [`FileError`]; a refusal comes before the board is written to, a changed design after its
/// question alone.
pub fn finish(
    root: &Path,
    manifest: &Manifest,
    board: &dyn Board,
    vcs: &dyn Vcs,
    epic: TicketId,
) -> Result<Finished, FileError> {
    let epic_now = board.epic(epic).map_err(|error| match error {
        crate::ports::PortError::NotFound(_) => {
            FileError::Check(super::check::CheckError::NotAnEpic(epic))
        }
        other => FileError::Board(other),
    })?;
    let Some(last) = epic_now.records.last().filter(|last| last.mark.is_none()) else {
        return Ok(Finished::Nothing);
    };
    let record = last.record.clone();

    let version = epic_now.release().unwrap_or_default().to_owned();
    let design = epic_now
        .release()
        .and_then(|version| design_text(root, manifest, version))
        .ok_or(FileError::NoDesign(version))?;
    let now = BodyHash::of(&TicketBody::from(design.clone()));
    if now != record.design {
        let text = format!(
            "The design changed since plan record {} of epic {epic} was posted: the record has design sha256:{}, the design now has sha256:{}. Rule whether the half-filed plan is filed as it was written (re-sign the design as it was), or re-planned.",
            record.k, record.design, now
        );
        let question = NewQuestion {
            on: epic,
            text,
            door: false,
        };
        let asked = board
            .questions()
            .map_err(FileError::Board)?
            .iter()
            .any(|ticket| question.is_asked_by(ticket));
        if !asked {
            board.open_question(&question).map_err(FileError::Board)?;
        }
        return Err(FileError::DesignChanged {
            epic,
            record: record.k,
            recorded: record.design.to_string(),
            now: now.to_string(),
        });
    }

    let prepared = prepare(
        root,
        manifest,
        board,
        vcs,
        epic,
        record.plan.as_bytes(),
        Some(record.k),
    )?;
    let (plan_made, measures) = optimised(&prepared.input)?;
    let made = tickets(&prepared.parsed, &plan_made, &prepared.shapes(&design))
        .map_err(FileError::Tickets)?;

    let mut open = Vec::new();
    for ticket in &record.dropped {
        if board.standing(*ticket).map_err(FileError::Board)? == Standing::Open {
            open.push(*ticket);
        }
    }
    let done = complete(
        board,
        epic,
        &prepared.epic,
        &made,
        &plan_made,
        &open,
        record.k,
    )?;
    Ok(Finished::Filed(Box::new(FiledPlan::of(
        record.k,
        done,
        record.dropped,
        prepared.parsed.input.nodes.len(),
        &plan_made,
        measures,
    ))))
}

#[cfg(test)]
mod tests;
