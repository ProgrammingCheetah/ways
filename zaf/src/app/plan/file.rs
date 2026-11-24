//! `plan file` (D17 steps 1 to 7, I9): a checked plan filed on the board as one run.
//!
//! In order: the epic's last record must have its mark, which is asked before the plan is read, so
//! a half-filed epic is told to finish whatever the new plan holds; the plan is checked as `plan check` checks
//! it and its tickets are made, so a refused plan has posted nothing; the plan record is posted
//! (I8); each ticket is filed in natural id order, a node already filed under the epic being
//! skipped, so a second run files each once; each `Drop:` ticket is closed as not planned (D29);
//! the epic is read back and the filing mark posted with the graph's hash.
//!
//! A run that stops after the record leaves a record with no mark: a half-filed plan, which the
//! next run refuses to build on ([`FileError::Unmarked`]) and finishing from the record is the
//! finish task's.
//!
//! On an epic that has a record the plan is a re-plan (D29): the epic's filed tickets are fixed
//! nodes of the input ([`super::replan`]), so only the plan's new nodes are filed and no filed body
//! is touched.

use std::collections::BTreeSet;
use std::path::Path;

use super::check::{CheckError, Checked, optimised, prepare};
use super::tickets::{TicketError, TicketToFile, tickets};
use crate::app::release::design_text;
use crate::domain::plan::read::PlanGraph;
use crate::domain::plan::record::{FilingMark, PlanRecord};
use crate::domain::plan::{NodeId, Optimised};
use crate::domain::{
    BodyHash, Epic, GroupName, NewTask, TaskBody, TicketBody, TicketId, TicketState, TicketTitle,
    TicketType,
};
use crate::manifest::Manifest;
use crate::ports::{Board, PortError, Vcs};

/// The most characters a plan record holds, the comment as it renders.
///
/// A forge's comment holds 65,536 characters; the limit leaves room for what the board adds.
pub const MAX_RECORD_CHARS: usize = 60_000;

/// Why a plan is not filed. Every variant but [`FileError::Board`] and a check's own board or
/// version-control failure is a refusal of the state, and each refusal came before anything was
/// posted.
#[derive(Debug, thiserror::Error)]
pub enum FileError {
    /// The plan is not checked: the check's own refusal.
    #[error(transparent)]
    Check(#[from] CheckError),
    /// A node makes no ticket: every reason, each naming its node.
    #[error("the plan makes no ticket for {} node(s)", .0.len())]
    Tickets(Vec<TicketError>),
    /// The epic's last plan record has no filing mark.
    #[error(
        "epic {epic}'s plan record {record} has no filing mark: finish it first with `zaf plan file {}` and empty stdin",
        .epic.get()
    )]
    Unmarked {
        /// The epic.
        epic: TicketId,
        /// The record without a mark.
        record: u64,
    },
    /// The design changed since the unmarked record was posted: a question was raised on the epic.
    #[error(
        "the design changed since plan record {record} of epic {epic}: it had sha256:{recorded}, it has sha256:{now}; a question is raised on the epic and nothing is filed"
    )]
    DesignChanged {
        /// The epic.
        epic: TicketId,
        /// The record without a mark.
        record: u64,
        /// The design hash the record holds.
        recorded: String,
        /// The design hash now.
        now: String,
    },
    /// The record would not fit in a comment.
    #[error(
        "the plan record would be {chars} characters, past the {limit} a comment holds: split the plan in two"
    )]
    TooLong {
        /// How long the record renders.
        chars: usize,
        /// The most it may be.
        limit: usize,
    },
    /// A `Drop:` ticket that is no task of the epic.
    #[error("Drop: {} {} no task of epic {epic}: drop only its own tickets", list(.tickets), if .tickets.len() == 1 { "is" } else { "are" })]
    NotItsTask {
        /// The epic.
        epic: TicketId,
        /// The tickets named.
        tickets: Vec<TicketId>,
    },
    /// The release's design cannot be read to hash it.
    #[error("the design of release {0} cannot be read: sign it again, then file the plan")]
    NoDesign(String),
    /// The board could not answer or refused, after the check passed. A record posted before it
    /// stays, unmarked.
    #[error(transparent)]
    Board(PortError),
}

fn list(tickets: &[TicketId]) -> String {
    let words: Vec<String> = tickets.iter().map(ToString::to_string).collect();
    words.join(" ")
}

/// What a filing did, for the summary line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiledPlan {
    /// The number of the record posted.
    pub record: u64,
    /// Each ticket filed now, by node, in the order filed.
    pub filed: Vec<(NodeId, TicketId)>,
    /// How many nodes were already filed under the epic and so not filed again.
    pub skipped: usize,
    /// The tickets closed as not planned.
    pub dropped: Vec<TicketId>,
    /// How many tasks the plan wrote.
    pub planned: usize,
    /// How many interface tickets the plan has.
    pub interfaces: usize,
    /// How many nodes hold several tasks merged.
    pub merges: usize,
    /// The plan's measures before and after the optimiser.
    pub measures: Checked,
}

impl FiledPlan {
    /// The one line `plan file` prints: `filed 7 tickets for 9 planned tasks (1 interface, 1
    /// merge): chain 5 → 3, critical 8 → 5`.
    pub fn summary(&self) -> String {
        let count = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        let mut parts = Vec::new();
        if self.interfaces > 0 {
            parts.push(count(self.interfaces, "interface"));
        }
        if self.merges > 0 {
            parts.push(count(self.merges, "merge"));
        }
        let shape = if parts.is_empty() {
            String::new()
        } else {
            format!(" ({})", parts.join(", "))
        };
        let skipped = if self.skipped > 0 {
            format!(", {} already filed", self.skipped)
        } else {
            String::new()
        };
        format!(
            "filed {} for {}{shape}: chain {} → {}, critical {} → {}{skipped}",
            count(self.filed.len(), "ticket"),
            count(self.planned, "planned task"),
            self.measures.before.chain,
            self.measures.after.chain,
            self.measures.before.critical,
            self.measures.after.critical,
        )
    }
}

/// Files `plan`, the text a planner wrote, as a plan of `epic`.
///
/// # Errors
///
/// A [`FileError`]; every refusal comes before the board is written to.
pub fn file(
    root: &Path,
    manifest: &Manifest,
    board: &dyn Board,
    vcs: &dyn Vcs,
    epic: TicketId,
    plan: &[u8],
) -> Result<FiledPlan, FileError> {
    // A half-filed epic is finished first, whatever the new plan says: its check would read the
    // tickets and the drops of the record in flight as the board's settled state. A ticket that is
    // no epic is the check's to refuse.
    if let Ok(standing) = board.epic(epic)
        && let Some(last) = standing.records.last()
        && last.mark.is_none()
    {
        return Err(FileError::Unmarked {
            epic,
            record: last.record.k,
        });
    }
    let prepared = prepare(root, manifest, board, vcs, epic, plan, None)?;
    let epic_now = &prepared.epic;
    let (plan_made, measures) = optimised(&prepared.input)?;
    let vocabulary_text = epic_now
        .release()
        .and_then(|version| design_text(root, manifest, version))
        .ok_or_else(|| FileError::NoDesign(epic_now.release().unwrap_or_default().to_owned()))?;
    let shapes = prepared.shapes(&vocabulary_text);
    let made = tickets(&prepared.parsed, &plan_made, &shapes).map_err(FileError::Tickets)?;

    let own: BTreeSet<TicketId> = epic_now.tasks.iter().map(|task| task.number).collect();
    let strangers: Vec<TicketId> = prepared
        .parsed
        .drop
        .iter()
        .copied()
        .filter(|ticket| !own.contains(ticket))
        .collect();
    if !strangers.is_empty() {
        return Err(FileError::NotItsTask {
            epic,
            tickets: strangers,
        });
    }

    let record = PlanRecord {
        k: epic_now.records.last().map_or(1, |last| last.record.k + 1),
        design: BodyHash::of(&TicketBody::from(vocabulary_text)),
        dropped: prepared.parsed.drop.clone(),
        plan: String::from_utf8_lossy(plan).into_owned(),
    };
    let chars = record.to_string().chars().count();
    if chars > MAX_RECORD_CHARS {
        return Err(FileError::TooLong {
            chars,
            limit: MAX_RECORD_CHARS,
        });
    }

    board
        .comment(epic, &record.to_string())
        .map_err(FileError::Board)?;
    let done = complete(
        board,
        epic,
        epic_now,
        &made,
        &plan_made,
        &record.dropped,
        record.k,
    )?;

    Ok(FiledPlan::of(
        record.k,
        done,
        record.dropped,
        prepared.parsed.input.nodes.len(),
        &plan_made,
        measures,
    ))
}

/// What [`complete`] did: the tickets filed, by node, and how many nodes were already filed.
pub(super) type Done = (Vec<(NodeId, TicketId)>, usize);

/// Steps 4 to 6 of D17, which a finish repeats: files each node of `made` the epic does not hold,
/// closes `dropped`, reads the epic back and posts the mark of record `k`.
pub(super) fn complete(
    board: &dyn Board,
    epic: TicketId,
    epic_now: &Epic,
    made: &[TicketToFile],
    plan_made: &Optimised,
    dropped: &[TicketId],
    k: u64,
) -> Result<Done, FileError> {
    let already = filed_nodes(epic_now);
    let mut filed = Vec::new();
    let mut skipped = 0;
    for ticket in made {
        if already.contains(&ticket.node) {
            skipped += 1;
            continue;
        }
        let number = board
            .file_task(&new_task(epic, ticket, plan_made))
            .map_err(FileError::Board)?;
        filed.push((ticket.node.clone(), number));
    }
    for ticket in dropped {
        board.drop_task(*ticket).map_err(FileError::Board)?;
    }
    let graph = read_back(board, epic)?;
    board
        .comment(
            epic,
            &FilingMark {
                k,
                graph: graph.hash(),
            }
            .to_string(),
        )
        .map_err(FileError::Board)?;
    Ok((filed, skipped))
}

impl FiledPlan {
    /// What a filing of record `k` did, from what [`complete`] gave back.
    pub(super) fn of(
        k: u64,
        (filed, skipped): Done,
        dropped: Vec<TicketId>,
        planned: usize,
        plan_made: &Optimised,
        measures: Checked,
    ) -> Self {
        Self {
            record: k,
            filed,
            skipped,
            dropped,
            planned,
            interfaces: measures.after.interfaces,
            merges: plan_made
                .nodes
                .iter()
                .filter(|node| node.id.as_str().contains('+'))
                .count(),
            measures,
        }
    }
}

/// The nodes the epic's tickets already hold, closed ones too.
fn filed_nodes(epic: &Epic) -> BTreeSet<NodeId> {
    epic.tasks
        .iter()
        .filter_map(|task| match TaskBody::parse(task.body.as_str()) {
            Ok(TaskBody::Planned(ticket)) => Some(ticket.node),
            _ => None,
        })
        .collect()
}

/// The graph of `epic` as the board reads it now.
fn read_back(board: &dyn Board, epic: TicketId) -> Result<PlanGraph, FileError> {
    let epics = board.epics().map_err(FileError::Board)?;
    let found = epics
        .iter()
        .find(|candidate| candidate.id == epic)
        .ok_or_else(|| {
            FileError::Board(PortError::NotFound(format!("{epic} is not an open epic")))
        })?;
    Ok(PlanGraph::read(found, &epics))
}

/// The task to file for `ticket`: blocked when the plan has it wait on anything, ready when not.
fn new_task(epic: TicketId, ticket: &TicketToFile, plan: &Optimised) -> NewTask {
    let waits = plan.orders.iter().any(|order| order.to == ticket.node)
        || plan.kept.iter().any(|kept| kept.to == ticket.node);
    NewTask {
        epic,
        title: TicketTitle::from(ticket.title.as_str()),
        body: TicketBody::from(ticket.body.to_string()),
        state: if waits {
            TicketState::Blocked
        } else {
            TicketState::Ready
        },
        group: ticket.group.as_deref().map(GroupName::from),
        kind: ticket
            .kind
            .as_deref()
            .map_or_else(TicketType::task, TicketType::from),
    }
}

#[cfg(test)]
mod tests;
