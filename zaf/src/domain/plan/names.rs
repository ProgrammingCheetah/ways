//! The names of a plan: the lists the vocabulary gives, the check that every name is known, and
//! the reading of a plan filed with `Blocked by` as needs.
//!
//! [`normalise`] fixes a plan's lists the way the reference's `normalise` does (D5, D6, D7, D13,
//! D29, M1) and refuses only a `planner-list`. [`check_names`] is a separate step: it refuses
//! an `unknown-name`, and the refusal pass calls it after the needs-itself check, so a plan with
//! a duplicate id and an unknown name is refused for the duplicate first. Both name every
//! problem they find, not the first alone; the first is the reference's.

use std::collections::{BTreeMap, BTreeSet};

use super::record::PlanRecord;
use super::text::parse_plan;
use super::{
    Filed, FiledWith, Name, NewNames, Node, NodeId, PlanInput, Refusal, RefusalCode, Vocabulary,
};

/// The kinds of name a plan may add to the vocabulary (D6): areas, signature, no signature.
const ADDABLE: [&str; 3] = ["areas", "signature", "no_signature"];

/// What the plans of an epic's records added to the vocabulary with `New names:` (D6): the areas
/// and the names by kind, and the shape each name with a signature was given.
///
/// What a record adds lasts: a later plan of the epic uses it without declaring it again, and
/// `plan verify` holds the epic's graph to it. Both read it here, so they agree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Added {
    /// The areas and the names, by kind.
    pub names: NewNames,
    /// The shape of each name added with a signature; the first record to give one wins.
    pub shapes: BTreeMap<Name, String>,
}

impl Added {
    /// What `records` added, in the order given. A record whose plan does not read adds none.
    pub fn by<'a>(records: impl IntoIterator<Item = &'a PlanRecord>) -> Self {
        let mut added = Self::default();
        for record in records {
            let Ok(parsed) = parse_plan(&record.plan) else {
                continue;
            };
            if let Some(new) = parsed.input.new_names {
                added.names.areas.extend(new.areas);
                added.names.signature.extend(new.signature);
                added.names.no_signature.extend(new.no_signature);
            }
            for (name, shape) in parsed.signatures {
                added.shapes.entry(name).or_insert(shape);
            }
        }
        added
    }

    /// `vocabulary` with what the records added: the vocabulary a later plan of the epic is
    /// checked against.
    pub fn onto(&self, mut vocabulary: Vocabulary) -> Vocabulary {
        vocabulary.areas.extend(self.names.areas.iter().cloned());
        vocabulary
            .signature
            .extend(self.names.signature.iter().cloned());
        vocabulary
            .no_signature
            .extend(self.names.no_signature.iter().cloned());
        vocabulary
    }
}

/// Reads a plan as the optimiser wants it, or says every `planner-list` it holds.
///
/// - The defaults are limit 2, lanes 4 and record 1 (the plan type's own defaults); `shared` is
///   a set, and every behaviour's lists are sets, so each is sorted and without repeats.
/// - With a vocabulary, `no_signature` is the vocabulary's plus the plan's new ones and
///   `existing` the vocabulary's. A plan that writes either list itself, or adds a kind of name
///   other than areas, signature and no signature, is refused `planner-list`.
/// - A done filed ticket leaves the plan and what it provides becomes `existing` (also kept in
///   `done_names`); an open one stays, marked `filed`.
/// - The nodes come out sorted by natural id.
///
/// Whether a name is known is [`check_names`]'s to say, not this function's.
pub fn normalise(plan: &PlanInput) -> Result<PlanInput, Vec<Refusal>> {
    let mut plan = plan.clone();
    if let Some(vocabulary) = &plan.vocabulary {
        let refusals = planner_lists(&plan);
        if !refusals.is_empty() {
            return Err(refusals);
        }
        let new = plan.new_names.clone().unwrap_or_default();
        plan.no_signature = Some(
            vocabulary
                .no_signature
                .union(&new.no_signature)
                .cloned()
                .collect(),
        );
        plan.existing = Some(vocabulary.existing.clone());
    }
    let no_signature = plan.no_signature.take().unwrap_or_default();
    let existing = plan.existing.take().unwrap_or_default();

    let (done, kept): (Vec<Node>, Vec<Node>) = plan
        .nodes
        .into_iter()
        .partition(|node| node.filed == Some(Filed::Done));
    plan.done_names = done
        .iter()
        .flat_map(|node| &node.behaviours)
        .flat_map(|behaviour| behaviour.provides.iter().cloned())
        .collect();
    plan.existing = Some(existing.union(&plan.done_names).cloned().collect());
    plan.no_signature = Some(no_signature);
    plan.nodes = kept;

    read_blocked_by(&mut plan);
    plan.nodes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(plan)
}

/// Every list a vocabulary's plan may not write: `no_signature` and `existing` of its own, and
/// a kind of name other than the three it may add.
fn planner_lists(plan: &PlanInput) -> Vec<Refusal> {
    let mut refusals = Vec::new();
    for (written, list) in [
        (plan.no_signature.is_some(), "no_signature"),
        (plan.existing.is_some(), "existing"),
    ] {
        if written {
            refusals.push(Refusal::new(
                RefusalCode::PlannerList,
                format!("{list} comes from the vocabulary"),
            ));
        }
    }
    if let Some(new) = &plan.new_names {
        for kind in new.other.keys().filter(|k| !ADDABLE.contains(&k.as_str())) {
            refusals.push(Refusal::new(
                RefusalCode::PlannerList,
                format!("a plan may not add {kind} names"),
            ));
        }
    }
    refusals
}

/// Reads each `Blocked by` line of a plan filed with them as a need (D4), as the reference's
/// `read_blocked_by` does. A plan not marked `filed_with: blocked_by` is left as it is.
///
/// - Each task provides `#<id>` on its first behaviour.
/// - Each line needs `#<m>` on that behaviour: a line naming an epic listed in `epics` needs each
///   of the epic's tasks, an epic with none stays a need of itself, and a task never needs itself.
/// - A `#` name no task of the plan provides is another epic's task (D13): it goes into `outside`
///   mapped to itself, and is never optimised.
/// - Every `#` name made is added to `no_signature`, so no wait is lifted.
///
/// The reading is done: `filed_with`, `epics` and each task's `blocked_by` are taken out.
pub fn read_blocked_by(plan: &mut PlanInput) {
    if plan.filed_with.take() != Some(FiledWith::BlockedBy) {
        return;
    }
    let members = std::mem::take(&mut plan.epics);
    let own: BTreeSet<NodeId> = plan.nodes.iter().map(|node| node.id.clone()).collect();
    let mut made: BTreeSet<Name> = BTreeSet::new();
    for node in &mut plan.nodes {
        let mut needed: BTreeSet<Name> = BTreeSet::new();
        for blocker in std::mem::take(&mut node.blocked_by) {
            let tasks = members.get(&blocker).filter(|tasks| !tasks.is_empty());
            let tasks = match tasks {
                Some(tasks) => tasks.clone(),
                None => vec![NodeId::from(&blocker)],
            };
            needed.extend(tasks.iter().filter(|task| **task != node.id).map(hash_name));
        }
        let provided = hash_name(&node.id);
        if let Some(first) = node.behaviours.first_mut() {
            first.provides.insert(provided.clone());
            first.needs.extend(needed.iter().cloned());
        }
        for name in &needed {
            if !own.contains(&NodeId::from(&name.as_str()[1..])) {
                plan.outside
                    .entry(name.clone())
                    .or_insert_with(|| NodeId::from(name.as_str()));
            }
        }
        made.extend(needed);
        made.insert(provided);
    }
    plan.no_signature
        .get_or_insert_with(BTreeSet::new)
        .extend(made);
}

/// `#<id>`: the name a task provides in a plan filed with `Blocked by`.
fn hash_name(id: &NodeId) -> Name {
    Name::from(format!("#{id}"))
}

/// Refuses every name that is neither the vocabulary's nor the plan's (D7), on a normalised
/// plan. An area is known only from the vocabulary or the plan's new areas; a provided or needed
/// name from the vocabulary, the plan's new names, `outside`, `existing`, or what an open filed
/// ticket provides. A plan with no vocabulary has nothing to check against. A filed ticket's own
/// names were checked when it was filed, so they are not.
///
/// Each refusal is `unknown-name`, naming the node and the name: `<node> writes <area>` or
/// `<node> provides|needs <name>`.
pub fn check_names(plan: &PlanInput) -> Result<(), Vec<Refusal>> {
    let Some(vocabulary) = &plan.vocabulary else {
        return Ok(());
    };
    let new = plan.new_names.clone().unwrap_or_default();
    let areas: BTreeSet<_> = vocabulary.areas.union(&new.areas).collect();
    let mut known: BTreeSet<&Name> = plan.outside.keys().collect();
    known.extend(&new.signature);
    known.extend(&new.no_signature);
    known.extend(plan.existing.iter().flatten());
    known.extend(&vocabulary.signature);
    known.extend(&vocabulary.no_signature);
    known.extend(&vocabulary.existing);
    let filed = |node: &Node| node.filed == Some(Filed::Open);
    for node in plan.nodes.iter().filter(|n| filed(n)) {
        known.extend(node.behaviours.iter().flat_map(|b| &b.provides));
    }

    let mut refusals: Vec<Refusal> = Vec::new();
    let mut add = |node: &Node, verb: &str, name: &dyn std::fmt::Display| {
        let refusal = Refusal::new(
            RefusalCode::UnknownName,
            format!("{} {verb} {name}", node.id),
        );
        if !refusals.contains(&refusal) {
            refusals.push(refusal);
        }
    };
    for node in plan.nodes.iter().filter(|n| !filed(n)) {
        for behaviour in &node.behaviours {
            for area in behaviour.writes.iter().filter(|a| !areas.contains(a)) {
                add(node, "writes", area);
            }
            for name in behaviour.provides.iter().filter(|n| !known.contains(n)) {
                add(node, "provides", name);
            }
            for name in behaviour.needs.iter().filter(|n| !known.contains(n)) {
                add(node, "needs", name);
            }
        }
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

#[cfg(test)]
mod blocked_by;
#[cfg(test)]
mod tests;
