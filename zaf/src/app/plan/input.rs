//! The names a plan shares with the other epics of its release (D13, Q10, Q19): what fills
//! [`PlanInput::outside`] and [`PlanInput::shared`] before the optimiser runs.
//!
//! Pure over the epics the board lists: nothing is read or written here. `plan check` and
//! `plan file` of an epic both go through [`fill`].
//!
//! - `outside` maps a name the plan needs, and does not provide, to the node of another epic that
//!   provides it; a name the plan provides that another epic provides too is mapped as well, so
//!   the optimiser refuses it as two providers.
//! - `shared` lists the names with a signature that the vocabulary gives this epic, and those a
//!   filed ticket of another epic waits on this epic for, so the epic lifts them into its
//!   interface ticket even when none of its own tasks needs them.
//! - `owed` lists the names the vocabulary gives this epic that a filed ticket of another epic
//!   waits on it for, signature or not: the plan must provide each, or it is `unprovided`.
//!
//! The provider a name maps to is, in order: a filed task of another epic of the release that
//! holds the name (an interface ticket whose signatures hold it, else the ticket that provides it,
//! which a wait names by its number); the epic the vocabulary gives the name to, as `I@<epic>.1`,
//! the node a wait on that epic's first plan resolves through (it reads as `epic #<n>`) while the
//! epic has filed no ticket that holds the name; and, for a name written `#<n>`, that filed task.

use std::collections::BTreeSet;
use std::fmt;

use crate::domain::plan::read::holder;
use crate::domain::plan::{Name, NodeId, PlanInput, Vocabulary, WaitOn};
use crate::domain::{Epic, Standing, TaskBody, TicketId};

/// A name the vocabulary gives an epic planned before 0.6.0, which a plan needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OlderEpicName {
    /// The name.
    pub name: Name,
    /// The epic the vocabulary gives it to.
    pub epic: TicketId,
}

impl fmt::Display for OlderEpicName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} is given to epic {}, planned before 0.6.0: need that epic's task as `#<n>`",
            self.name, self.epic
        )
    }
}

/// Every name a plan needs that the vocabulary gives an epic planned before 0.6.0, such an epic
/// being one with tasks and no plan record. An old epic's tickets hold no name, so no wait on it
/// could ever be resolved: the way on is a wait on one of its tasks by number.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
pub struct OlderEpics(pub Vec<OlderEpicName>);

/// Fills `input`'s `outside`, `shared` and `owed` for `epic`.
///
/// `all` is every open epic of the board; the release's others are those of `epic`'s release but
/// `epic` itself. `vocabulary` is the release's. Names already in `outside`, `shared` and `owed`
/// stay.
///
/// # Errors
///
/// [`OlderEpics`] for each name the plan needs that the vocabulary gives an epic planned before
/// 0.6.0; `input` is left as it was.
pub fn fill(
    input: &mut PlanInput,
    epic: &Epic,
    all: &[Epic],
    vocabulary: &Vocabulary,
) -> Result<(), OlderEpics> {
    let mut others: Vec<&Epic> = all
        .iter()
        .filter(|other| other.id != epic.id && other.release() == epic.release())
        .collect();
    others.sort_by_key(|other| other.id);

    let behaviours = || input.nodes.iter().flat_map(|node| &node.behaviours);
    let provided: BTreeSet<Name> = behaviours().flat_map(|b| b.provides.clone()).collect();
    let needed: BTreeSet<Name> = behaviours()
        .flat_map(|b| b.needs.clone())
        .filter(|name| !provided.contains(name))
        .collect();

    let mut outside = Vec::new();
    let mut older = Vec::new();
    let names = needed
        .iter()
        .map(|name| (name, true))
        .chain(provided.iter().map(|name| (name, false)));
    for (name, needs_it) in names {
        if vocabulary.existing.contains(name) {
            continue;
        }
        match provider(name, needs_it, &others, all, epic, vocabulary) {
            Provider::None => {}
            Provider::Node(node) => outside.push((name.clone(), node)),
            Provider::Older(owner) => older.push(OlderEpicName {
                name: name.clone(),
                epic: owner,
            }),
        }
    }
    if !older.is_empty() {
        return Err(OlderEpics(older));
    }
    for (name, node) in outside {
        input.outside.entry(name).or_insert(node);
    }
    let waited = waited(epic, &others);
    input.shared.extend(shared(epic, &waited, vocabulary));
    input.owed.extend(
        waited
            .into_iter()
            .filter(|name| vocabulary.epic.get(name) == Some(&epic.id)),
    );
    Ok(())
}

/// What a name resolves to among the other epics.
enum Provider {
    /// No other epic gives it.
    None,
    /// The node of another epic that does.
    Node(NodeId),
    /// An epic planned before 0.6.0, which a need on the name is refused for.
    Older(TicketId),
}

fn provider(
    name: &Name,
    needs_it: bool,
    others: &[&Epic],
    all: &[Epic],
    epic: &Epic,
    vocabulary: &Vocabulary,
) -> Provider {
    if let Some(number) = name.task() {
        let filed = needs_it
            && all
                .iter()
                .filter(|other| other.id != epic.id)
                .flat_map(|other| &other.tasks)
                .any(|task| task.number == number);
        return if filed {
            Provider::Node(NodeId::from(name.as_str()))
        } else {
            Provider::None
        };
    }
    for other in others {
        let Some((task, node, interface)) = holder(other, name) else {
            continue;
        };
        return Provider::Node(if interface {
            node
        } else {
            NodeId::from(format!("#{}", task.number.get()))
        });
    }
    let Some(owner) = vocabulary.epic.get(name) else {
        return Provider::None;
    };
    let Some(owner) = others.iter().find(|other| other.id == *owner) else {
        return Provider::None;
    };
    if needs_it && !owner.tasks.is_empty() && owner.records.is_empty() {
        return Provider::Older(owner.id);
    }
    Provider::Node(NodeId::from(format!("I@{}.1", owner.id.get())))
}

/// The names an open filed ticket of another epic waits on `epic` for.
fn waited(epic: &Epic, others: &[&Epic]) -> BTreeSet<Name> {
    others
        .iter()
        .flat_map(|other| &other.tasks)
        .filter(|task| task.standing == Standing::Open)
        .filter_map(|task| match TaskBody::parse(task.body.as_str()) {
            Ok(TaskBody::Planned(ticket)) => Some(ticket.waits),
            _ => None,
        })
        .flatten()
        .filter(|wait| wait.on == WaitOn::Epic(epic.id))
        .map(|wait| wait.name)
        .collect()
}

/// The names with a signature that other epics need from `epic`: those the vocabulary gives it,
/// and those an open filed ticket of another epic waits on it for (`waited`).
fn shared(epic: &Epic, waited: &BTreeSet<Name>, vocabulary: &Vocabulary) -> BTreeSet<Name> {
    vocabulary
        .epic
        .iter()
        .filter(|(_, owner)| **owner == epic.id)
        .map(|(name, _)| name)
        .chain(waited)
        .filter(|name| vocabulary.signature.contains(*name))
        .cloned()
        .collect()
}
