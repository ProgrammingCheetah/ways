//! Everything wrong with a plan that is seen before it is rewritten: a duplicate id, a node with
//! no behaviour, a bad size, a behaviour over the limit, a name with two providers, a need nobody
//! provides, a name another epic waits on this one for and the plan leaves out (D13), a cycle.
//! Nothing is repaired.
//!
//! The pass reads a normalised plan (`names::normalise`) and names every problem, not the first
//! alone. The order is the reference's (`check` and `layers` in `optimize.py`), so the first
//! refusal is the code the reference raises: the checks of each node in turn (`duplicate-id`,
//! `no-behaviour`, `bad-id`, then for each behaviour `no-behaviour` on a blank text, `bad-size`,
//! `too-large`, `needs-itself`), the vocabulary's `unknown-name`, `two-providers`, `unprovided`,
//! `fixed-provider`, and last `cycle`.

use std::collections::{BTreeMap, BTreeSet};

use super::graph::Dag;
use super::names::check_names;
use super::optimise::Working;
use super::optimise::derive::derive;
use super::{Filed, Name, Node, NodeId, NodeKind, Options, PlanInput, Refusal, RefusalCode};

/// Refuses a broken plan with every thing wrong with it, the reference's first first. A plan that
/// is whole gives `Ok`.
pub fn refuse(plan: &PlanInput) -> Result<(), Vec<Refusal>> {
    let mut refusals = Refusals::default();
    each_node(plan, &mut refusals);
    if let Err(unknown) = check_names(plan) {
        for refusal in unknown {
            refusals.add(refusal);
        }
    }
    let provider = providers(plan, &mut refusals);
    unprovided(plan, &provider, &mut refusals);
    fixed_providers(plan, &provider, &mut refusals);
    cycle(plan, &mut refusals);
    if refusals.0.is_empty() {
        Ok(())
    } else {
        Err(refusals.0)
    }
}

/// The refusals so far, in the order they were found and each once.
#[derive(Default)]
struct Refusals(Vec<Refusal>);

impl Refusals {
    fn add(&mut self, refusal: Refusal) {
        if !self.0.contains(&refusal) {
            self.0.push(refusal);
        }
    }

    fn push(&mut self, code: RefusalCode, detail: impl std::fmt::Display) {
        self.add(Refusal::new(code, detail));
    }
}

/// The checks that read one node alone, in the reference's order for each node in turn.
fn each_node(plan: &PlanInput, refusals: &mut Refusals) {
    let mut seen: BTreeSet<&NodeId> = BTreeSet::new();
    for node in &plan.nodes {
        if !seen.insert(&node.id) {
            refusals.push(RefusalCode::DuplicateId, &node.id);
        }
        if node.behaviours.is_empty() {
            refusals.push(RefusalCode::NoBehaviour, &node.id);
            continue;
        }
        // D9: a merged node holds one behaviour per part of its id.
        if node.id.as_str().contains('+')
            && node.id.as_str().split('+').count() != node.behaviours.len()
        {
            refusals.push(
                RefusalCode::BadId,
                format!(
                    "{} holds {} behaviours for {} parts",
                    node.id,
                    node.behaviours.len(),
                    node.id.as_str().split('+').count()
                ),
            );
        }
        for behaviour in &node.behaviours {
            if behaviour.text.trim().is_empty() {
                refusals.push(RefusalCode::NoBehaviour, &node.id);
            }
            match behaviour.size.weight().filter(|weight| weight.0 >= 1) {
                None => refusals.push(RefusalCode::BadSize, &node.id),
                Some(weight) => {
                    if node.kind == NodeKind::Body && weight > plan.limit {
                        refusals.push(
                            RefusalCode::TooLarge,
                            format!(
                                "{} has a behaviour of size {weight}, over {}",
                                node.id, plan.limit
                            ),
                        );
                    }
                }
            }
            if let Some(name) = behaviour.provides.intersection(&behaviour.needs).next() {
                refusals.push(
                    RefusalCode::NeedsItself,
                    format!("{} needs {name}, which it provides", node.id),
                );
            }
        }
    }
}

/// The node that provides each name, the first to give it. A name given again, one another epic
/// provides, or one a done ticket provided is `two-providers`.
fn providers<'a>(plan: &'a PlanInput, refusals: &mut Refusals) -> BTreeMap<&'a Name, &'a NodeId> {
    let mut provider: BTreeMap<&Name, &NodeId> = BTreeMap::new();
    for node in &plan.nodes {
        for name in node.behaviours.iter().flat_map(|b| &b.provides) {
            if let Some(first) = provider.get(name) {
                let detail = if **first == node.id {
                    format!("{} provides {name} twice", node.id)
                } else {
                    format!("{} provides {name}, which {first} provides too", node.id)
                };
                refusals.push(RefusalCode::TwoProviders, detail);
            } else if let Some(other) = plan.outside.get(name) {
                refusals.push(
                    RefusalCode::TwoProviders,
                    format!(
                        "{} provides {name}, which {other} of another epic provides",
                        node.id
                    ),
                );
            } else if plan.done_names.contains(name) {
                refusals.push(
                    RefusalCode::TwoProviders,
                    format!("{} provides {name}, which a done ticket provided", node.id),
                );
            }
            provider.entry(name).or_insert(&node.id);
        }
    }
    provider
}

/// A need nobody provides, the code does not hold and no other epic gives; then each name another
/// epic's filed ticket waits on this epic for (`owed`, D13) that no node provides and no done
/// ticket provided.
fn unprovided(plan: &PlanInput, provider: &BTreeMap<&Name, &NodeId>, refusals: &mut Refusals) {
    let existing = plan.existing.as_ref();
    for node in &plan.nodes {
        for name in needs(node) {
            if !provider.contains_key(name)
                && !existing.is_some_and(|list| list.contains(name))
                && !plan.outside.contains_key(name)
            {
                refusals.push(RefusalCode::Unprovided, format!("{} needs {name}", node.id));
            }
        }
    }
    for name in &plan.owed {
        if !provider.contains_key(name) && !plan.done_names.contains(name) {
            refusals.push(
                RefusalCode::Unprovided,
                format!("a ticket of another epic waits on this epic for {name}"),
            );
        }
    }
}

/// D29: a new task may not wait on an open filed task for a name with a signature that no
/// interface holds: the filed ticket cannot be changed to implement one.
fn fixed_providers(plan: &PlanInput, provider: &BTreeMap<&Name, &NodeId>, refusals: &mut Refusals) {
    let by_id: BTreeMap<&NodeId, &Node> = plan.nodes.iter().map(|node| (&node.id, node)).collect();
    let fixed = |node: &Node| node.filed == Some(Filed::Open);
    for node in plan.nodes.iter().filter(|node| !fixed(node)) {
        for name in needs(node) {
            let owner = provider.get(name).and_then(|id| by_id.get(*id));
            let no_signature = plan
                .no_signature
                .as_ref()
                .is_some_and(|list| list.contains(name));
            if let Some(owner) = owner.filter(|o| fixed(o) && o.kind == NodeKind::Body)
                && !no_signature
            {
                refusals.push(
                    RefusalCode::FixedProvider,
                    format!("{} needs {name} from {}", node.id, owner.id),
                );
            }
        }
    }
}

/// Waits that go round, on the waits `derive` gives the plan as written. The nodes of the group
/// whose first node comes first are named.
fn cycle(plan: &PlanInput, refusals: &mut Refusals) {
    let mut working = Working {
        plan: plan.clone(),
        options: Options::default(),
        orders: BTreeSet::new(),
    };
    derive(&mut working);
    let ids: Vec<NodeId> = plan.nodes.iter().map(|node| node.id.clone()).collect();
    let waits: Vec<(NodeId, NodeId)> = working
        .orders
        .into_iter()
        .map(|order| (order.from, order.to))
        .collect();
    if let Some(group) = Dag::new(&ids, &waits).cycle() {
        let names: Vec<&str> = group.iter().map(NodeId::as_str).collect();
        refusals.push(RefusalCode::Cycle, names.join(" "));
    }
}

/// Every name a node needs, sorted.
fn needs(node: &Node) -> BTreeSet<&Name> {
    node.behaviours.iter().flat_map(|b| &b.needs).collect()
}

#[cfg(test)]
mod tests;
