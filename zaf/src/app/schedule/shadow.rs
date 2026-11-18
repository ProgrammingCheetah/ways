//! The core's placement shown beside the old plan, deciding nothing: the `placement` key of
//! `zaf spawn --once --dry-run --json` (docs/zaf-actions.md §5.2), and the `tickets` of each line
//! of the tick's trace ([`super::trace`]).
//!
//! [`placement`] gathers every open ticket with a phase list ([`super::gather::gather`]) and
//! places each at `now` ([`places`]), in ticket order:
//!
//! ```json
//! {"tickets": [
//!    {"ticket": 7, "place": "running", "phase": "implement", "head": null, "started_at": "<time>"},
//!    {"ticket": 8, "place": "waiting", "on": [{"epic": 2}, {"question": 30}, {"ticket": 25},
//!                                            {"check": {"ticket": 8, "head": "<commit>"}}]},
//!    {"ticket": 12, "place": "waiting", "on": [{"merge": 11}, {"base": {"head": "<commit>"}}]},
//!    {"ticket": 9, "place": "lane", "next": "review", "since": "<time>", "weight": 3600},
//!    {"ticket": 10, "place": "done"}],
//!  "unreadable": [{"ticket": 11, "reason": "<why>"}]}
//! ```
//!
//! Times are RFC 3339 in UTC, to the second; a weight is the age it counts as, in seconds. A read
//! every ticket needs that fails gives `{"error": "<why>"}` instead; it fails nothing else.

use std::time::SystemTime;

use serde_json::{Value, json};

use super::gather::{Gathered, Unreadable, gather};
use crate::app::spawn::phase::Ports;
use crate::domain::TicketId;
use crate::domain::schedule::{
    Place, TicketFacts, WaitingOn, Weight, current_phase, place, weight,
};

/// One ticket in its place, with its weight when it is in the lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The ticket.
    pub ticket: TicketId,
    /// Where it sits.
    pub place: Place,
    /// Its weight in the lane; none in any other place.
    pub weight: Option<Weight>,
}

/// The placement of every open ticket with a phase list at `now`, as the dry run's JSON carries
/// it (see the module above).
pub fn placement(ports: &Ports<'_>, now: SystemTime) -> Value {
    match gather(ports) {
        Ok(gathered) => to_json(&gathered, now),
        Err(error) => json!({ "error": error.to_string() }),
    }
}

/// Every ticket of `gathered` placed at `now` ([`place`]), in the gather's order, which is ticket
/// order; one in the lane carries its weight at `now`.
pub fn places(gathered: &Gathered, now: SystemTime) -> Vec<Placed> {
    gathered
        .tickets
        .iter()
        .map(|facts| {
            let place = place(facts, now);
            let weight = match &place {
                Place::InLane { since, .. } => Some(weight(*since, facts.priority, now)),
                Place::Running(_) | Place::Waiting(_) | Place::Done => None,
            };
            Placed {
                ticket: facts.ticket,
                place,
                weight,
            }
        })
        .collect()
}

/// The name of the phase `facts`'s ticket is in, as the core reads it: the first whose end fact
/// does not hold at the pull request's head; none once every phase has ended.
pub fn current_phase_name(facts: &TicketFacts) -> Option<String> {
    current_phase(facts).map(|current| current.phase.name.to_string())
}

/// `gathered` placed at `now`, as JSON.
pub fn to_json(gathered: &Gathered, now: SystemTime) -> Value {
    let tickets: Vec<Value> = places(gathered, now).iter().map(placed_json).collect();
    let unreadable: Vec<Value> = gathered.unreadable.iter().map(unreadable_json).collect();
    json!({ "tickets": tickets, "unreadable": unreadable })
}

/// One placed ticket as JSON: its number, then its place's fields (see the module above).
pub fn placed_json(placed: &Placed) -> Value {
    let mut entry = place_json(&placed.place);
    entry["ticket"] = json!(placed.ticket.get());
    if let Some(weight) = placed.weight {
        entry["weight"] = json!(weight.age().as_secs());
    }
    entry
}

/// A ticket that could not be read, as JSON: its number and why.
pub fn unreadable_json(unreadable: &Unreadable) -> Value {
    json!({ "ticket": unreadable.ticket.get(), "reason": unreadable.reason })
}

/// One place, as JSON, without its ticket.
fn place_json(place: &Place) -> Value {
    match place {
        Place::Running(run) => json!({
            "place": "running",
            "phase": run.phase.as_str(),
            "head": run.head.as_ref().map(|head| head.as_str()),
            "started_at": time(run.started_at),
        }),
        Place::Waiting(waits) => json!({
            "place": "waiting",
            "on": waits.iter().map(waiting_json).collect::<Vec<_>>(),
        }),
        Place::InLane { next, since } => json!({
            "place": "lane",
            "next": next.as_str(),
            "since": time(*since),
        }),
        Place::Done => json!({ "place": "done" }),
    }
}

/// One thing a ticket waits on, as JSON.
fn waiting_json(waiting_on: &WaitingOn) -> Value {
    match waiting_on {
        WaitingOn::Epic(epic) => json!({ "epic": epic.get() }),
        WaitingOn::Question(question) => json!({ "question": question.get() }),
        WaitingOn::Ticket(ticket) => json!({ "ticket": ticket.get() }),
        WaitingOn::Check(ticket, head) => {
            json!({ "check": { "ticket": ticket.get(), "head": head.as_str() } })
        }
        WaitingOn::Merge(ticket) => json!({ "merge": ticket.get() }),
        WaitingOn::Base(head) => json!({ "base": { "head": head.as_str() } }),
    }
}

/// A time as the JSON carries it: RFC 3339 in UTC, to the second.
pub fn time(at: SystemTime) -> String {
    humantime::format_rfc3339_seconds(at).to_string()
}

#[cfg(test)]
mod tests;
