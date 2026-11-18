//! What the steps' own tests build their lane from: a ticket's facts from its list and what is
//! known of it, gathered as the gather would hand them, and sorted by the tick's own gate.

use std::collections::BTreeSet;
use std::time::SystemTime;

use super::{Sorted, sort_lane};
use crate::app::actions::ListedPhase;
use crate::app::schedule::gather::{Beside, Gathered, phases_of};
use crate::domain::schedule::{Placement, PullFacts, RunRecord, TicketFacts};
use crate::domain::{TaskSize, TicketId};

/// The facts of `task` with the list `list`, of size `size`, ready since `ready_since`, its pull
/// request and its runs as given; no epic, no question, no dependency.
pub(crate) fn facts(
    task: u64,
    list: &[ListedPhase],
    size: TaskSize,
    ready_since: SystemTime,
    pull: Option<PullFacts>,
    runs: Vec<RunRecord>,
) -> TicketFacts {
    TicketFacts {
        ticket: TicketId::from(task),
        epic: None,
        open_questions: BTreeSet::new(),
        dependencies: Vec::new(),
        pull,
        runs,
        phases: phases_of(list),
        size,
        priority: None,
        ruled: Vec::new(),
        ready_since: Some(ready_since),
        merging: Default::default(),
        no_check_ruled: None,
    }
}

/// The tickets of `tickets`, each with its list, gathered as [`crate::app::schedule::gather`]
/// hands them, each under epic #1 with no question on it.
pub(crate) fn gathered(tickets: Vec<(TicketFacts, Vec<ListedPhase>)>) -> Gathered {
    let mut gathered = Gathered::default();
    for (facts, list) in tickets {
        gathered.beside.insert(
            facts.ticket,
            Beside {
                list,
                epic: TicketId::from(1),
                on_epic: BTreeSet::new(),
            },
        );
        gathered.tickets.push(facts);
    }
    gathered.tickets.sort_by_key(|facts| facts.ticket);
    gathered
}

/// The lane of `gathered` at `now`, sorted by the tick's gate ([`sort_lane`]).
pub(crate) fn sorted(gathered: &Gathered, now: SystemTime) -> Sorted {
    sort_lane(gathered, &Placement::of(&gathered.tickets, now), now)
}
