//! The tick as a pure function: a board snapshot, the state, the guard and the
//! settings in, a launch plan out. Nothing here touches a port or a file.
//!
//! Each open epic gets at most one launch a tick, chosen by its stage (ruling 48): a planning
//! instance for a signed epic with no tasks, the challenger for a planned one, and a team
//! instance for a challenged one with tasks to build. An unsigned epic, a done one and one
//! with a question open on it launch nothing.

use std::collections::{BTreeMap, BTreeSet};
use std::time::SystemTime;

use super::blockers::{self, DeadBlocker, DeadReason, Graph, blockers};
use super::state::SpawnState;
use crate::app::guard::Level;
use crate::domain::{
    CHALLENGER_ROLE, Epic, EpicStage, Instance, InstanceNumber, RoleName, Ticket, TicketId,
};
use crate::manifest::Manifest;

/// The role that builds a challenged epic's tasks.
pub const TEAM_ROLE: &str = "team";

pub use crate::domain::PLANNING_ROLE;

/// The branches of the tick that are not built yet; each is skipped as `not built`.
pub const NOT_BUILT: [&str; 1] = ["release"];

/// How many tasks an instance takes when its role sets no batch.
const UNBATCHED: u32 = 1;

/// What the tick reads from the board.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Tasks labelled `ready`, in issue order.
    pub ready: Vec<Ticket>,
    /// Tasks with a live claim.
    pub claimed: BTreeSet<TicketId>,
    /// Blockers that are closed.
    pub closed: BTreeSet<TicketId>,
    /// The open tasks the blockers lead to that are not ready, with the blockers each names;
    /// the ready tasks' own bodies are read from `ready`.
    pub open: Graph,
    /// Blockers that can never be seen closed, and why.
    pub dead: BTreeMap<TicketId, DeadReason>,
    /// The ready tasks whose body does not follow the template, each with what is wrong
    /// (the field names, or `line <n>`).
    pub unparsed: BTreeMap<TicketId, Vec<String>>,
    /// The ready tasks whose type has a `[phases]` list: the phase steps run them, so no team
    /// launch takes them.
    pub phased: BTreeSet<TicketId>,
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
}

/// What the tick reads from the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// `[spawn] slots`.
    pub slots: u32,
    /// `[roles.team] batch`, or one.
    pub batch: u32,
}

impl From<&Manifest> for Settings {
    fn from(manifest: &Manifest) -> Self {
        let batch = manifest
            .roles
            .get(&RoleName::from(TEAM_ROLE))
            .and_then(|role| role.batch)
            .unwrap_or(UNBATCHED);
        Self {
            slots: manifest.spawn.slots,
            batch: batch.max(UNBATCHED),
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
    /// Its tasks, at most a batch, in issue order.
    pub tasks: Vec<TicketId>,
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
    /// The challenger ran on it and left no report: it runs once per epic.
    ChallengerEnded,
    /// Every task is finished.
    Done,
    /// It is challenged and has no task that is ready, unblocked and unclaimed.
    NothingLaunchable,
    /// Every slot the guard allows is taken.
    NoFreeSlot,
    /// The guard is red: the spawner launches nothing.
    GuardRed,
}

/// Why a ready task was left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskSkip {
    /// A `Blocked by:` issue is still open; the open ones are named.
    Blocked(Vec<TicketId>),
    /// It is on a blocker cycle, each task waiting for the next; the cycle, from its lowest task.
    InCycle(Vec<TicketId>),
    /// Another instance holds a claim on it.
    Claimed,
    /// A running instance already carries it.
    Running(String),
    /// It belongs to no open epic.
    NoEpic,
    /// Its body does not follow the template; the fields at fault, or `line <n>`.
    DoesNotParse(Vec<String>),
    /// Its type has a `[phases]` list: the phase steps run it, not a team.
    Phased,
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
    /// The ready tasks left out, and why.
    pub tasks: Vec<(TicketId, TaskSkip)>,
    /// Every blocker cycle among the open tasks reached, each from its lowest task.
    pub cycles: Vec<Vec<TicketId>>,
    /// Every dead blocker the open tasks reached name.
    pub dead: Vec<DeadBlocker>,
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

/// Which instance an epic's stage calls for, if any: the role and the tasks it takes.
fn next_launch(
    epic: &Epic,
    launchable: &[TicketId],
    snapshot: &Snapshot,
    running: &[Instance],
    batch: usize,
) -> Result<(&'static str, Vec<TicketId>), EpicSkip> {
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
                Ok((PLANNING_ROLE, Vec::new()))
            }
        }
        EpicStage::Planned if runs(CHALLENGER_ROLE) => Err(EpicSkip::ChallengerRuns),
        EpicStage::Planned if launched_since(CHALLENGER_ROLE, None) => {
            Err(EpicSkip::ChallengerEnded)
        }
        EpicStage::Planned => Ok((CHALLENGER_ROLE, Vec::new())),
        _ if launchable.is_empty() => Err(EpicSkip::NothingLaunchable),
        _ => Ok((TEAM_ROLE, launchable.iter().copied().take(batch).collect())),
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
/// `next_launch` gives planning or the challenger. A team launch is not one (it is #449's),
/// and an epic held by a question, a running instance or a launch already made is none.
pub fn epic_candidates(snapshot: &Snapshot, state: &SpawnState) -> Vec<EpicCandidate> {
    let running = state.running_instances();
    snapshot
        .epics
        .iter()
        .filter_map(|epic| {
            let (role, _) = next_launch(epic, &[], snapshot, &running, 1).ok()?;
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
    let carried: BTreeMap<u64, &str> = state
        .launches
        .iter()
        .filter_map(|launch| Some((launch.task?, launch.instance.as_str())))
        .collect();

    let mut graph = snapshot.open.clone();
    for ticket in &snapshot.ready {
        graph.insert(ticket.id, blockers(ticket.body.as_str()));
    }
    let cycles = blockers::cycles(&graph);
    let dead = blockers::dead(&graph, &snapshot.dead);

    let open_epics: BTreeMap<TicketId, &Epic> =
        snapshot.epics.iter().map(|epic| (epic.id, epic)).collect();
    let mut tasks = Vec::new();
    let mut epics: BTreeMap<TicketId, Vec<TicketId>> = BTreeMap::new();
    for ticket in &snapshot.ready {
        let Some(epic) = ticket.epic.filter(|epic| open_epics.contains_key(epic)) else {
            tasks.push((ticket.id, TaskSkip::NoEpic));
            continue;
        };
        let launchable = epics.entry(epic).or_default();
        let open: Vec<TicketId> = blockers(ticket.body.as_str())
            .into_iter()
            .filter(|blocker| !snapshot.closed.contains(blocker))
            .collect();
        if snapshot.phased.contains(&ticket.id) {
            tasks.push((ticket.id, TaskSkip::Phased));
        } else if snapshot.claimed.contains(&ticket.id) {
            tasks.push((ticket.id, TaskSkip::Claimed));
        } else if let Some(instance) = carried.get(&ticket.id.get()) {
            tasks.push((ticket.id, TaskSkip::Running((*instance).to_owned())));
        } else if let Some(fields) = snapshot.unparsed.get(&ticket.id) {
            tasks.push((ticket.id, TaskSkip::DoesNotParse(fields.clone())));
        } else if let Some(cycle) = cycles.iter().find(|cycle| cycle.contains(&ticket.id)) {
            tasks.push((ticket.id, TaskSkip::InCycle(cycle.clone())));
        } else if !open.is_empty() {
            tasks.push((ticket.id, TaskSkip::Blocked(open)));
        } else {
            launchable.push(ticket.id);
        }
    }

    let running = state.running_instances();
    let batch = usize::try_from(settings.batch).unwrap_or(usize::MAX);
    let mut free = slots.free;
    let mut next = state.next_instance;
    let mut launches = Vec::new();
    let mut skipped = Vec::new();
    for (id, epic) in open_epics {
        let launchable = epics.get(&id).map_or(&[][..], Vec::as_slice);
        match next_launch(epic, launchable, snapshot, &running, batch) {
            Err(skip) => skipped.push((id, skip)),
            Ok(_) if level == Level::Red => skipped.push((id, EpicSkip::GuardRed)),
            Ok(_) if free == 0 => skipped.push((id, EpicSkip::NoFreeSlot)),
            Ok((role, tasks)) => {
                launches.push(PlannedLaunch {
                    instance: Instance {
                        role: RoleName::from(role),
                        epic: id,
                        number: InstanceNumber::from(next),
                    },
                    tasks,
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
        tasks,
        cycles,
        dead,
    }
}

#[cfg(test)]
mod tests;
