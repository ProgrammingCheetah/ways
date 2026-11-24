//! The tick as a pure function: a board snapshot, the state, the guard and the
//! settings in, a launch plan out. Nothing here touches a port or a file.
//!
//! Each open epic gets at most one launch a tick, chosen by its stage (ruling 48): a planning
//! instance for a signed epic with no tasks and the challenger for a planned one. A challenged
//! epic launches nothing here: its tasks are built by their phases (docs/zaf-actions.md §5.1),
//! never by a team instance. An unsigned epic, a done one and one with a question open on it
//! launch nothing.
//!
//! An epic whose last plan record has no filing mark is finished from its record, once a tick
//! and only when no planning instance runs on it and no question is open on it ([`half_filed`],
//! [`finish_half_filed`], D9, M14). That step reads and writes the board through `plan file`'s
//! own finish; it comes before the launches are planned, which are pure.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::SystemTime;

use super::blockers::{self, DeadBlocker, DeadReason, Graph};
use super::state::SpawnState;
use crate::app::guard::Level;
use crate::app::plan::file::FileError;
use crate::app::plan::finish::{Finished, finish};
use crate::domain::plan::read::PlanGraph;
use crate::domain::plan::record::GraphHash;
use crate::domain::{
    CHALLENGER_ROLE, Epic, EpicStage, Instance, InstanceNumber, RoleName, Ticket, TicketId,
};
use crate::manifest::Manifest;
use crate::ports::{Board, NewQuestion, PortError, Vcs};

pub use crate::domain::PLANNING_ROLE;

/// The branches of the tick that are not built yet; each is skipped as `not built`.
pub const NOT_BUILT: [&str; 1] = ["release"];

/// What the tick reads from the board.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Tasks labelled `ready`, in issue order; the plan reads their blockers for cycles and
    /// dead ones.
    pub ready: Vec<Ticket>,
    /// The open tasks the blockers lead to that are not ready, with the blockers each names;
    /// the ready tasks' own bodies are read from `ready`.
    pub open: Graph,
    /// What each ready task waits on, read from the wait source (D19); a ready task with no
    /// entry here reads from its own `Blocked by:` line.
    pub ready_waits: Graph,
    /// Blockers that can never be seen closed, and why.
    pub dead: BTreeMap<TicketId, DeadReason>,
    /// The open epics, with everything their stage is derived from.
    pub epics: Vec<Epic>,
    /// The issues an open question is raised on.
    pub questioned: BTreeSet<TicketId>,
    /// Every launch the run folders record, running or ended.
    pub launched: Vec<PastLaunch>,
}

/// A launch as its run folder records it: the instance and when the tick reserved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PastLaunch {
    /// The instance.
    pub instance: Instance,
    /// When the tick reserved it.
    pub reserved_at: SystemTime,
    /// `LaunchRecord.graph`: the epic's graph hash at a challenger's launch (I14); `None` for
    /// any other role's launch and for a record written before the field.
    pub graph: Option<GraphHash>,
}

/// What the tick reads from the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// `[spawn] slots`.
    pub slots: u32,
    /// `Settings.merge_per_tick: u32`, from `[merge] per_tick`: the merges one tick may land,
    /// at least 1.
    pub merge_per_tick: u32,
}

impl From<&Manifest> for Settings {
    fn from(manifest: &Manifest) -> Self {
        Self {
            slots: manifest.spawn.slots,
            merge_per_tick: manifest.merge.per_tick,
        }
    }
}

/// The slots this tick: the manifest's, what the guard allows, and what is free.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotCount {
    /// `[spawn] slots`.
    pub total: u32,
    /// The guard level read.
    pub level: Level,
    /// What the guard allows of the total.
    pub allowed: u32,
    /// Instances still holding a slot.
    pub running: u32,
    /// What this tick may launch.
    pub free: u32,
}

/// One instance to launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedLaunch {
    /// The instance, numbered from the state's counter.
    pub instance: Instance,
}

/// Why an epic launched nothing this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum EpicSkip {
    /// The Engineer has not signed it, or edited it since: `zaf epic sign` is theirs to run.
    Unsigned,
    /// A question is open on it: nothing launches until the Engineer rules.
    Questioned,
    /// A planning instance runs on it.
    PlanningRuns,
    /// A planning instance ran on it since it was signed and filed no task.
    PlannedNothing,
    /// The challenger runs on it.
    ChallengerRuns,
    /// The challenger ran on it and left no report: it runs once per graph hash, or once ever
    /// for an epic with no plan record.
    ChallengerEnded,
    /// Every task is finished.
    Done,
    /// It is challenged: its tasks are built by their phases, so no instance launches on it.
    BuiltByPhases,
    /// Every slot the guard allows is taken.
    NoFreeSlot,
    /// The guard is red: the spawner launches nothing.
    GuardRed,
}

/// What one tick decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The slots.
    pub slots: SlotCount,
    /// The instances to launch, one per epic at most, in epic order.
    pub launches: Vec<PlannedLaunch>,
    /// The epics that launched nothing, and why.
    pub epics: Vec<(TicketId, EpicSkip)>,
    /// Every blocker cycle among the open tasks reached, each from its lowest task.
    pub cycles: Vec<Vec<TicketId>>,
    /// Every dead blocker the open tasks reached name.
    pub dead: Vec<DeadBlocker>,
    /// The task phases that run now. The plan decides on [`SlotCount`] alone, so this changes no
    /// launch; the printed slots line counts them, as the tick's budget does. [`plan`] leaves it
    /// `0` and the I/O step fills it in.
    pub phases_running: u32,
}

/// The slots the guard allows: yellow halves them (at least one), orange allows one, red none.
pub fn allowed_slots(total: u32, level: Level) -> u32 {
    match level {
        Level::Green => total,
        Level::Yellow => (total / 2).max(1),
        Level::Orange => 1,
        Level::Red => 0,
    }
}

/// Whether a challenger was already launched on the epic for its graph as it reads now (D22,
/// M14, Q13): an earlier challenger launch record holds the graph's current hash. An epic with
/// no plan record has no graph to hash, so any challenger launch ever answers it.
fn challenger_launched(epic: &Epic, snapshot: &Snapshot) -> bool {
    let mut launches = snapshot.launched.iter().filter(|past| {
        past.instance.epic == epic.id && past.instance.role.as_str() == CHALLENGER_ROLE
    });
    if epic.records.is_empty() {
        return launches.next().is_some();
    }
    let now = PlanGraph::read(epic, &snapshot.epics).hash();
    launches.any(|past| past.graph.as_ref() == Some(&now))
}

/// Which instance an epic's stage calls for, if any: planning or the challenger. A challenged
/// epic calls for none, its tasks being built by their phases.
fn next_launch(
    epic: &Epic,
    snapshot: &Snapshot,
    running: &[Instance],
) -> Result<&'static str, EpicSkip> {
    let runs = |role: &str| {
        running
            .iter()
            .any(|instance| instance.epic == epic.id && instance.role.as_str() == role)
    };
    let launched_since = |role: &str, since: Option<SystemTime>| {
        snapshot.launched.iter().any(|past| {
            past.instance.epic == epic.id
                && past.instance.role.as_str() == role
                && since.is_none_or(|since| past.reserved_at >= since)
        })
    };
    let stage = epic.stage();
    match stage {
        EpicStage::Unsigned => return Err(EpicSkip::Unsigned),
        EpicStage::Done => return Err(EpicSkip::Done),
        _ if snapshot.questioned.contains(&epic.id) => return Err(EpicSkip::Questioned),
        // A planning instance may still be filing: the epic waits for it whatever its stage.
        _ if runs(PLANNING_ROLE) => return Err(EpicSkip::PlanningRuns),
        _ => {}
    }
    match stage {
        EpicStage::Signed => {
            let signed_at = epic.signature.as_ref().map(|signature| signature.at);
            if launched_since(PLANNING_ROLE, signed_at) {
                Err(EpicSkip::PlannedNothing)
            } else {
                Ok(PLANNING_ROLE)
            }
        }
        EpicStage::Planned if runs(CHALLENGER_ROLE) => Err(EpicSkip::ChallengerRuns),
        EpicStage::Planned if challenger_launched(epic, snapshot) => Err(EpicSkip::ChallengerEnded),
        EpicStage::Planned => Ok(CHALLENGER_ROLE),
        _ => Err(EpicSkip::BuiltByPhases),
    }
}

/// An epic launch as an item of the one model order (#427): the planning instance for a signed
/// epic with no tasks, the challenger for a planned one, and how long that stage has waited.
/// It is charged XS, whatever the epic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicCandidate {
    /// The epic.
    pub epic: TicketId,
    /// The role to launch: [`PLANNING_ROLE`] or [`CHALLENGER_ROLE`].
    pub role: RoleName,
    /// When its stage began to wait, the item's weight: the signature of a signed epic, the
    /// planning instance's exit marker of a planned one (the ruling on #459). A planned epic
    /// with no such marker on it has waited at most since its signature, which stands in.
    pub since: SystemTime,
}

/// The epic launches the tick may start this tick, each as an [`EpicCandidate`]: the epics
/// `next_launch` gives planning or the challenger, and an epic held by a question, a running
/// instance or a launch already made is none.
pub fn epic_candidates(snapshot: &Snapshot, state: &SpawnState) -> Vec<EpicCandidate> {
    let running = state.running_instances();
    snapshot
        .epics
        .iter()
        .filter_map(|epic| {
            let role = next_launch(epic, snapshot, &running).ok()?;
            let signed_at = epic.signature.as_ref().map(|signature| signature.at)?;
            let since = match role {
                PLANNING_ROLE => signed_at,
                CHALLENGER_ROLE => epic.planned_at.unwrap_or(signed_at),
                _ => return None,
            };
            Some(EpicCandidate {
                epic: epic.id,
                role: RoleName::from(role),
                since,
            })
        })
        .collect()
}

/// The epics whose last plan record has no filing mark and that nothing holds back: no planning
/// instance of `running` runs on them and no question in `questioned` is open on them. An epic
/// whose finish was refused for a changed design keeps its question, so it is left alone while
/// that question is open.
pub fn half_filed(
    epics: &[Epic],
    questioned: &BTreeSet<TicketId>,
    running: &[Instance],
) -> Vec<TicketId> {
    epics
        .iter()
        .filter(|epic| epic.half_filed())
        .filter(|epic| !questioned.contains(&epic.id))
        .filter(|epic| {
            !running
                .iter()
                .any(|instance| instance.epic == epic.id && instance.role.as_str() == PLANNING_ROLE)
        })
        .map(|epic| epic.id)
        .collect()
}

/// What the tick's finish did for one half-filed epic.
#[derive(Debug)]
pub struct HalfFinished {
    /// The epic.
    pub epic: TicketId,
    /// What the finish did, or why it refused.
    pub outcome: Result<Finished, FileError>,
}

impl HalfFinished {
    /// The line the tick's block says of it.
    pub fn line(&self) -> String {
        match &self.outcome {
            Ok(finished) => format!("epic {}: {}", self.epic, finished.summary()),
            Err(error) => format!("epic {}: plan not finished: {error}", self.epic),
        }
    }
}

/// Finishes, once each, the plans [`half_filed`] names among the board's open epics, with the
/// finish of `plan file` (D9, M14). A refusal is the epic's own result and stops no other epic;
/// a board that cannot be read is the tick's error.
///
/// # Errors
///
/// A [`PortError`] when the epics or the questions cannot be read.
pub fn finish_half_filed(
    root: &Path,
    manifest: &Manifest,
    board: &dyn Board,
    vcs: &dyn Vcs,
    state: &SpawnState,
) -> Result<Vec<HalfFinished>, PortError> {
    let epics = board.epics()?;
    let questioned = board
        .questions()?
        .iter()
        .filter_map(|question| NewQuestion::raised_on(question.body.as_str()))
        .collect();
    Ok(half_filed(&epics, &questioned, &state.running_instances())
        .into_iter()
        .map(|epic| HalfFinished {
            epic,
            outcome: finish(root, manifest, board, vcs, epic),
        })
        .collect())
}

/// Plans one tick: at most one launch per open epic, by its stage.
pub fn plan(snapshot: &Snapshot, state: &SpawnState, level: Level, settings: Settings) -> Plan {
    let allowed = allowed_slots(settings.slots, level);
    let running = u32::try_from(state.running()).unwrap_or(u32::MAX);
    let slots = SlotCount {
        total: settings.slots,
        level,
        allowed,
        running,
        free: allowed.saturating_sub(running),
    };
    let mut graph = snapshot.open.clone();
    for ticket in &snapshot.ready {
        graph.insert(ticket.id, blockers::named_by(&snapshot.ready_waits, ticket));
    }
    let cycles = blockers::cycles(&graph);
    let dead = blockers::dead(&graph, &snapshot.dead);

    let open_epics: BTreeMap<TicketId, &Epic> =
        snapshot.epics.iter().map(|epic| (epic.id, epic)).collect();

    let running = state.running_instances();
    let mut free = slots.free;
    let mut next = state.next_instance;
    let mut launches = Vec::new();
    let mut skipped = Vec::new();
    for (id, epic) in open_epics {
        match next_launch(epic, snapshot, &running) {
            Err(skip) => skipped.push((id, skip)),
            Ok(_) if level == Level::Red => skipped.push((id, EpicSkip::GuardRed)),
            Ok(_) if free == 0 => skipped.push((id, EpicSkip::NoFreeSlot)),
            Ok(role) => {
                launches.push(PlannedLaunch {
                    instance: Instance {
                        role: RoleName::from(role),
                        epic: id,
                        number: InstanceNumber::from(next),
                    },
                });
                next = next.saturating_add(1);
                free -= 1;
            }
        }
    }
    Plan {
        slots,
        launches,
        epics: skipped,
        cycles,
        dead,
        phases_running: 0,
    }
}

#[cfg(test)]
mod tests;
