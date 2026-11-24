//! `plan check` (D17 step 2, I9): a plan read, its names checked against the epic's release
//! vocabulary, optimised with production options, and measured before and after.
//!
//! Nothing is written: the board and the repository are only read. `plan file` of this epic
//! runs the same entry and then files.

use std::collections::BTreeMap;
use std::path::Path;

use super::input::{OlderEpics, fill};
use super::replan::{
    DropBarred, NodesFiled, check_drops, filed_ids, fixed_nodes, next_record, of_record,
};
use crate::app::release::{VocabularyLoadError, release_vocabulary, vocabulary_shapes};
use crate::domain::plan::measure::{baseline, measure};
use crate::domain::plan::names::Added;
use crate::domain::plan::optimise::optimise;
use crate::domain::plan::text::{ParsedPlan, PlanFaults, parse_plan_bytes};
use crate::domain::plan::{
    EpicKey, Interfaces, Lift, Measures, Name, NodeId, Optimised, Options, PlanInput, Refusal,
};
use crate::domain::{Epic, TicketId};
use crate::manifest::Manifest;
use crate::ports::{Board, PortError, Vcs};

/// What the optimiser does to a good plan: its measures before any pass and after all of them.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Checked {
    /// The plan as written, its waits derived and nothing rewritten.
    pub before: Measures,
    /// The plan as the optimiser leaves it.
    pub after: Measures,
}

/// Why a plan is not checked. Each is a refusal of the state, but a board or a version control
/// that could not answer.
#[derive(Debug, thiserror::Error)]
pub enum CheckError {
    /// The ticket is no open epic.
    #[error("{0} is not an open epic")]
    NotAnEpic(TicketId),
    /// The board could not answer.
    #[error(transparent)]
    Board(PortError),
    /// The epic's release vocabulary is not loaded.
    #[error(transparent)]
    Vocabulary(#[from] VocabularyLoadError),
    /// The plan needs a name the vocabulary gives an epic planned before 0.6.0.
    #[error(transparent)]
    OlderEpic(#[from] OlderEpics),
    /// The plan does not read: every fault, each at its section and line.
    #[error(transparent)]
    Unreadable(#[from] PlanFaults),
    /// A `Drop:` ticket that is not open, or has a phase run or a pull request.
    #[error(transparent)]
    Drop(#[from] DropBarred),
    /// An id the plan's keys would make that a ticket of the epic holds already.
    #[error(transparent)]
    NodeFiled(#[from] NodesFiled),
    /// The plan reads and the optimiser refuses it: every refusal.
    #[error("the plan is refused")]
    Refused(Vec<Refusal>),
}

/// The options the verbs run the optimiser with in production (I4).
pub const PRODUCTION: Options = Options {
    lift: Lift::All,
    interfaces: Interfaces::One,
};

/// A plan read and made ready for the optimiser: the epic it is of, the plan as written and the
/// input the optimiser takes.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// The epic the plan is of, as the board reads it now.
    pub epic: Epic,
    /// The plan as written: its sections and its `Drop:` list, which filing needs.
    pub parsed: ParsedPlan,
    /// The optimiser's input: the plan's nodes, the epic's filed tickets as fixed nodes when it
    /// has a plan record (D29), the release's vocabulary and the names the other epics share.
    pub input: PlanInput,
    /// The shape of each name with a signature that an earlier record of the epic added: an
    /// interface ticket of this plan may hold one.
    pub earlier_shapes: BTreeMap<Name, String>,
}

impl Prepared {
    /// The shape of each name with a signature the plan may lift, but its own new names:
    /// `design`'s vocabulary first, then what the epic's earlier records added.
    pub fn shapes(&self, design: &str) -> BTreeMap<Name, String> {
        let mut shapes = vocabulary_shapes(design);
        for (name, shape) in &self.earlier_shapes {
            shapes.entry(name.clone()).or_insert_with(|| shape.clone());
        }
        shapes
    }
}

/// The input the optimiser takes for `plan`, the text a planner wrote as a plan of `epic`.
///
/// The plan is read, every node and the plan itself made `epic`'s, the release's vocabulary set,
/// and the names it shares with the other epics of the release gathered ([`fill`]). `plan file`
/// takes the same input, through [`prepare`].
///
/// An epic that has a plan record is re-planned (D29): its filed tickets enter as fixed nodes, the
/// record number is the next one, and a `Drop:` ticket must be open with no phase run and no pull
/// request ([`replan`](super::replan)).
pub fn plan_input(
    root: &Path,
    manifest: &Manifest,
    board: &dyn Board,
    vcs: &dyn Vcs,
    epic: TicketId,
    plan: &[u8],
) -> Result<PlanInput, CheckError> {
    prepare(root, manifest, board, vcs, epic, plan, None).map(|prepared| prepared.input)
}

/// [`plan_input`] with the epic and the plan as written kept beside the input.
///
/// `finishing` is the number of the half-filed record being finished: its plan is checked as it was
/// written, so the tickets it filed already are its own and no fixed nodes, its number is the
/// record number, and its `Drop:` tickets, closed or not, are not checked again.
pub fn prepare(
    root: &Path,
    manifest: &Manifest,
    board: &dyn Board,
    vcs: &dyn Vcs,
    epic: TicketId,
    plan: &[u8],
    finishing: Option<u64>,
) -> Result<Prepared, CheckError> {
    let found = board.epic(epic).map_err(|error| match error {
        PortError::NotFound(_) => CheckError::NotAnEpic(epic),
        other => CheckError::Board(other),
    })?;
    let record = finishing.map_or_else(
        || next_record(&found),
        |k| u32::try_from(k).unwrap_or(u32::MAX),
    );
    // What the epic's earlier records added with `New names:` lasts (D6): the plan is checked
    // against the release's vocabulary and those, as `plan verify` checks the graph.
    let earlier = Added::by(
        found
            .records
            .iter()
            .map(|entry| &entry.record)
            .filter(|earlier| earlier.k < u64::from(record)),
    );
    let vocabulary = earlier.onto(release_vocabulary(root, manifest, vcs, &found)?);
    let parsed = parse_plan_bytes(plan)?;
    if finishing.is_none() {
        check_drops(root, &found, &parsed.drop)?;
    }
    let ids: Vec<_> = parsed.input.nodes.iter().map(|n| n.id.clone()).collect();
    let own = |node: &NodeId| finishing.is_some_and(|k| of_record(node, &ids, epic, k));
    let taken = filed_ids(&found, &ids, &parsed.drop, &own);
    if !taken.is_empty() {
        return Err(NodesFiled(taken).into());
    }
    let mut input = parsed.input.clone();
    let key = EpicKey::from(epic.get().to_string());
    input.epic = Some(key.clone());
    for node in &mut input.nodes {
        node.epic = Some(key.clone());
    }
    input.record = record;
    input
        .nodes
        .extend(fixed_nodes(&found, &key, &parsed.drop, &own));
    let epics = board.epics().map_err(CheckError::Board)?;
    fill(&mut input, &found, &epics, &vocabulary)?;
    input.vocabulary = Some(vocabulary);
    Ok(Prepared {
        epic: found,
        parsed,
        input,
        earlier_shapes: earlier.shapes,
    })
}

/// Checks `plan`, the text a planner wrote, as a plan of `epic`.
///
/// The vocabulary is the signed design's of the release `epic` names, loaded by
/// [`release_vocabulary`]. Every node and the plan itself belong to `epic`, so the interface node
/// the lifts land in is `I@<epic>.1`. A plan the optimiser refuses gives every refusal; one it
/// takes gives its measures.
pub fn check(
    root: &Path,
    manifest: &Manifest,
    board: &dyn Board,
    vcs: &dyn Vcs,
    epic: TicketId,
    plan: &[u8],
) -> Result<Checked, CheckError> {
    let input = plan_input(root, manifest, board, vcs, epic, plan)?;
    optimised(&input).map(|(_, checked)| checked)
}

/// Runs the optimiser on `input` with the production options: the plan it leaves and its measures
/// before and after. `plan file` files from the plan it leaves.
pub fn optimised(input: &PlanInput) -> Result<(Optimised, Checked), CheckError> {
    // The optimiser names every refusal; the baseline stops at the first, so it runs after.
    let optimised = optimise(input, PRODUCTION).map_err(CheckError::Refused)?;
    let before = baseline(input).map_err(|refusal| CheckError::Refused(vec![refusal]))?;
    let after = measure(&optimised, input.lanes);
    Ok((optimised, Checked { before, after }))
}

#[cfg(test)]
mod tests;
