//! What the hub reads of the process table: which launches still run and so which slots are
//! free, and which claims belong to an instance that has ended. It reads them the way
//! `zaf status` does ([`spawn::derive`], [`spawn::orphaned`]) and reads them again on every
//! redraw, so a process that dies frees its slot at the next frame. It touches the run folders
//! and the process table only, never the network.

use std::collections::BTreeSet;
use std::path::Path;

use super::{HubError, Snapshot};
use crate::app::spawn;
use crate::domain::TicketId;
use crate::ports::{ClaimRecord, Holder};

/// The slots and the claims as the process table has them now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Liveness {
    /// The spawner's slots with no instance in them.
    pub slots_free: u32,
    /// The tasks whose claim is held by an instance that has ended.
    pub orphaned: BTreeSet<TicketId>,
}

impl Liveness {
    /// Reads the run folders against `alive`: one slot is taken per live instance, and a
    /// claim is orphaned by the rule `zaf status` uses.
    pub fn read(
        state_dir: &Path,
        slots_total: u32,
        claims: &[ClaimRecord],
        alive: &dyn Fn(&Holder) -> bool,
    ) -> Result<Self, HubError> {
        let failed = |error: spawn::SpawnError| HubError::Spawn(error.to_string());
        let (state, runs) = spawn::derive(state_dir, alive).map_err(failed)?;
        let held = spawn::held(state_dir, alive).map_err(failed)?;
        let ended = spawn::ended_phases(state_dir, alive).map_err(failed)?;
        let orphaned = spawn::orphaned(claims, &runs, &ended, alive, &held)
            .into_iter()
            .map(|claim| claim.task)
            .collect();
        Ok(Self {
            slots_free: slots_total
                .saturating_sub(u32::try_from(state.running()).unwrap_or(u32::MAX)),
            orphaned,
        })
    }
}

impl Snapshot {
    /// Sets every epic's free slots from `live` and drops the ticket rows of orphaned claims.
    pub fn apply(&mut self, live: &Liveness) {
        for epic in &mut self.epics {
            epic.slots_free = live.slots_free.min(epic.slots_total);
            let groups = epic.groups.iter_mut().map(|group| &mut group.tickets);
            for tickets in std::iter::once(&mut epic.tickets).chain(groups) {
                tickets.retain(|ticket| !live.orphaned.contains(&TicketId::from(ticket.number)));
            }
        }
    }
}
