//! The scheduler's wait source (D19, I6): what a ticket waits on, read from the graph.
//!
//! [`Waits::read`] reads each open epic of the release with [`PlanGraph::read`], the release's
//! open epics at hand, and gives per ticket its waits: the graph's edges into it (needs,
//! implements, turns) and its kept waits, `Waits on: #<n>` and `Waits on: epic #<n>`. The latter
//! is a wait on a ticket once the providing epic is planned, and stays a wait on the epic until
//! then. A ticket of the older template reads from its `Blocked by` lines, as needs, by the
//! graph read's own older reading (D4, Q11).
//!
//! Nothing is stored (D18): every call reads the board again. The reader is app-level; no
//! domain file imports it. [`Waits::graph`] gives the waits on tickets in the shape of the
//! `Blocked by:` walk ([`Graph`]), so the cycle walk takes it unchanged.

use std::collections::{BTreeMap, BTreeSet};

use super::Graph;
use crate::domain::plan::read::{PlanGraph, WaitTarget};
use crate::domain::plan::{Name, NodeId};
use crate::domain::{Epic, Standing, TicketId};
use crate::ports::{Board, PortError};

/// What a ticket waits on.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Wait {
    /// A ticket: the one that provides what the waiter needs.
    Ticket(TicketId),
    /// An epic that has filed no ticket holding the name yet.
    Epic {
        /// The epic.
        epic: TicketId,
        /// The name it is to provide.
        name: Name,
    },
}

impl Wait {
    /// The issue waited on: the ticket, or the epic.
    pub fn on(&self) -> TicketId {
        match self {
            Self::Ticket(ticket) => *ticket,
            Self::Epic { epic, .. } => *epic,
        }
    }
}

/// Each ticket of the release's open epics with what it waits on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Waits {
    /// Every ticket the epics hold, closed ones included, with its waits sorted and without
    /// repeats.
    by_ticket: BTreeMap<TicketId, Vec<Wait>>,
    /// The tickets that are open.
    open: BTreeSet<TicketId>,
    /// The name each wait on a ticket is for, by the waiter and the ticket waited on: the
    /// first in name order when the waiter needs several from it.
    names: BTreeMap<(TicketId, TicketId), Name>,
}

impl Waits {
    /// The waits of the tickets of `release_epics`, each epic read with [`PlanGraph::read`]
    /// among the release's open epics. An epic of `release_epics` that is not open is left out.
    ///
    /// Interface: `Waits::read(board, release_epics) -> Result<Waits, PortError>`.
    ///
    /// # Errors
    /// [`PortError`] when the board cannot list its epics.
    pub fn read(board: &dyn Board, release_epics: &[TicketId]) -> Result<Self, PortError> {
        Ok(Self::from_epics(&board.epics()?, release_epics))
    }

    /// The same as [`Waits::read`] over epics the caller already holds, so a caller that has
    /// read the board's epics does not read them again.
    pub fn from_epics(epics: &[Epic], release_epics: &[TicketId]) -> Self {
        let release: Vec<Epic> = epics
            .iter()
            .filter(|epic| release_epics.contains(&epic.id))
            .cloned()
            .collect();
        Self::of_epics(&release)
    }

    /// The waits of the tickets of `release`, the open epics already listed: what [`Waits::read`]
    /// gives, for a caller that holds the epics and reads the board no more.
    pub fn of_epics(release: &[Epic]) -> Self {
        let mut waits = Self::default();
        for epic in release {
            waits.add(&PlanGraph::read(epic, release));
        }
        waits
    }

    /// Whether the epics hold `ticket`: one they do not hold has no waits to read.
    pub fn holds(&self, ticket: TicketId) -> bool {
        self.by_ticket.contains_key(&ticket)
    }

    /// What `ticket` waits on; none for a ticket the epics do not hold.
    ///
    /// Interface: `Waits::of(TicketId) -> &[Wait]`.
    pub fn of(&self, ticket: TicketId) -> &[Wait] {
        self.by_ticket.get(&ticket).map_or(&[], Vec::as_slice)
    }

    /// The name `ticket` waits on the ticket `on` for (a `turn:<area>` for a turn); none when
    /// it does not wait on it.
    ///
    /// Interface: `Waits::name(TicketId, TicketId) -> Option<&Name>`; read by the scheduler's
    /// gather, so a wait that can never end is asked about by its name.
    pub fn name(&self, ticket: TicketId, on: TicketId) -> Option<&Name> {
        self.names.get(&(ticket, on))
    }

    /// The waits on tickets of each open ticket, as the `Blocked by:` walk holds them: a wait on
    /// an epic is left out, a ticket has an entry when it waits on none.
    ///
    /// Interface: `Waits::graph() -> Graph`.
    pub fn graph(&self) -> Graph {
        self.open
            .iter()
            .map(|ticket| {
                let named = self
                    .of(*ticket)
                    .iter()
                    .filter_map(|wait| match wait {
                        Wait::Ticket(on) => Some(*on),
                        Wait::Epic { .. } => None,
                    })
                    .collect();
                (*ticket, named)
            })
            .collect()
    }

    /// Takes in one epic's graph.
    fn add(&mut self, graph: &PlanGraph) {
        let mut holders: BTreeMap<&NodeId, Vec<(TicketId, Standing)>> = BTreeMap::new();
        for node in &graph.nodes {
            if let Some(filed) = &node.filed {
                holders
                    .entry(&node.id)
                    .or_default()
                    .push((filed.ticket, filed.standing));
                self.by_ticket.entry(filed.ticket).or_default();
                if filed.standing == Standing::Open {
                    self.open.insert(filed.ticket);
                }
            }
        }
        let tickets = |id: &NodeId| holders.get(id).cloned().unwrap_or_default();
        for order in &graph.orders {
            // A ticket closed as not planned gave the name once: when another holds the node,
            // that one is waited on.
            let mut from = tickets(&order.from);
            if from
                .iter()
                .any(|(_, standing)| *standing != Standing::ClosedNotPlanned)
            {
                from.retain(|(_, standing)| *standing != Standing::ClosedNotPlanned);
            }
            for (waiter, _) in tickets(&order.to) {
                for (provider, _) in &from {
                    self.insert(waiter, Wait::Ticket(*provider), &order.needs);
                }
            }
        }
        for kept in &graph.kept {
            let wait = match &kept.on {
                WaitTarget::Ticket { ticket, .. } => Wait::Ticket(*ticket),
                WaitTarget::Epic(epic) => Wait::Epic {
                    epic: *epic,
                    name: kept.name.clone(),
                },
            };
            for (waiter, _) in tickets(&kept.to) {
                self.insert(waiter, wait.clone(), &kept.name);
            }
        }
    }

    /// Adds `wait` for `name` to `ticket`, keeping the list sorted and without repeats. A ticket
    /// never waits on itself.
    fn insert(&mut self, ticket: TicketId, wait: Wait, name: &Name) {
        if wait == Wait::Ticket(ticket) {
            return;
        }
        if let Wait::Ticket(on) = wait {
            let kept = self
                .names
                .entry((ticket, on))
                .or_insert_with(|| name.clone());
            if name < kept {
                *kept = name.clone();
            }
        }
        let waits = self.by_ticket.entry(ticket).or_default();
        if let Err(at) = waits.binary_search(&wait) {
            waits.insert(at, wait);
        }
    }
}

#[cfg(test)]
mod tests;
