//! The tick as a pure function: a board snapshot, the state, the guard and the
//! settings in, a launch plan out. Nothing here touches a port or a file.

use std::collections::{BTreeMap, BTreeSet};

use super::state::SpawnState;
use crate::app::guard::Level;
use crate::domain::{Instance, InstanceNumber, IssueNumber, RoleName};
use crate::manifest::Manifest;
use crate::ports::Ticket;

/// The role the tick launches.
pub const TEAM_ROLE: &str = "team";

/// The branches of the tick that are not built yet; each is skipped as `not built`.
pub const NOT_BUILT: [&str; 3] = ["planning", "challenger", "release"];

/// The field of a task body that names its blockers.
const BLOCKED_BY: &str = "Blocked by:";

/// How many tasks an instance takes when its role sets no batch.
const UNBATCHED: u32 = 1;

/// What the tick reads from the board.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Tasks labelled `ready`, in issue order.
    pub ready: Vec<Ticket>,
    /// Tasks with a live claim.
    pub claimed: BTreeSet<IssueNumber>,
    /// Blockers that are closed.
    pub closed: BTreeSet<IssueNumber>,
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
    pub tasks: Vec<IssueNumber>,
}

/// Why an epic launched nothing this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum EpicSkip {
    /// It has no task that is ready, unblocked and unclaimed.
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
    Blocked(Vec<IssueNumber>),
    /// Another instance holds a claim on it.
    Claimed,
    /// A running instance already carries it.
    Running(String),
    /// It belongs to no epic.
    NoEpic,
}

/// What one tick decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The slots.
    pub slots: SlotCount,
    /// The instances to launch, one per epic at most, in epic order.
    pub launches: Vec<PlannedLaunch>,
    /// The epics that launched nothing, and why.
    pub epics: Vec<(IssueNumber, EpicSkip)>,
    /// The ready tasks left out, and why.
    pub tasks: Vec<(IssueNumber, TaskSkip)>,
}

/// The issues a task body names on its `Blocked by:` line; `—` or `none` name none.
pub fn blockers(body: &str) -> Vec<IssueNumber> {
    body.lines()
        .filter_map(|line| line.trim().strip_prefix(BLOCKED_BY))
        .flat_map(|rest| {
            rest.split(|character: char| character.is_whitespace() || character == ',')
        })
        .filter_map(|word| word.parse::<IssueNumber>().ok())
        .collect()
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

/// Plans one tick of the team-instance branch.
pub fn plan(snapshot: &Snapshot, state: &SpawnState, level: Level, settings: Settings) -> Plan {
    let allowed = allowed_slots(settings.slots, level);
    let running = u32::try_from(state.instances().len()).unwrap_or(u32::MAX);
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
        .map(|launch| (launch.task, launch.instance.as_str()))
        .collect();

    let mut tasks = Vec::new();
    let mut epics: BTreeMap<IssueNumber, Vec<IssueNumber>> = BTreeMap::new();
    for ticket in &snapshot.ready {
        let Some(epic) = ticket.epic else {
            tasks.push((ticket.number, TaskSkip::NoEpic));
            continue;
        };
        let launchable = epics.entry(epic).or_default();
        let open: Vec<IssueNumber> = blockers(&ticket.body)
            .into_iter()
            .filter(|blocker| !snapshot.closed.contains(blocker))
            .collect();
        if snapshot.claimed.contains(&ticket.number) {
            tasks.push((ticket.number, TaskSkip::Claimed));
        } else if let Some(instance) = carried.get(&ticket.number.get()) {
            tasks.push((ticket.number, TaskSkip::Running((*instance).to_owned())));
        } else if !open.is_empty() {
            tasks.push((ticket.number, TaskSkip::Blocked(open)));
        } else {
            launchable.push(ticket.number);
        }
    }

    let mut free = slots.free;
    let mut next = state.next_instance;
    let mut launches = Vec::new();
    let mut skipped = Vec::new();
    for (epic, launchable) in epics {
        if launchable.is_empty() {
            skipped.push((epic, EpicSkip::NothingLaunchable));
        } else if level == Level::Red {
            skipped.push((epic, EpicSkip::GuardRed));
        } else if free == 0 {
            skipped.push((epic, EpicSkip::NoFreeSlot));
        } else {
            let batch = usize::try_from(settings.batch).unwrap_or(usize::MAX);
            launches.push(PlannedLaunch {
                instance: Instance {
                    role: RoleName::from(TEAM_ROLE),
                    epic,
                    number: InstanceNumber::from(next),
                },
                tasks: launchable.into_iter().take(batch).collect(),
            });
            next = next.saturating_add(1);
            free -= 1;
        }
    }
    Plan {
        slots,
        launches,
        epics: skipped,
        tasks,
    }
}

#[cfg(test)]
mod tests;
