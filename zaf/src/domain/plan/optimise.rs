//! The optimiser `f` (D8, D9, D11, D13, D14, D29, M3, I4): normalise, refuse, then split, derive,
//! the cycle check, lift, settle and thin, each pass once and in the reference's order
//! (`optimize` in `optimize.py`), each in its own file so no two tasks edit one.
//!
//! [`optimise`] is the entry. It returns the nodes by natural id, the waits by the edge key and
//! the waits on other epics' tasks (`kept`), which no pass touches.

use std::collections::BTreeSet;

use super::graph::Dag;
use super::{
    KeptWait, Lift, Name, Node, NodeId, Optimised, Options, Order, PlanInput, Refusal, RefusalCode,
    names, refuse,
};

pub mod derive;
pub mod lift;
pub mod settle;
pub mod split;
pub mod thin;

/// The plan while the passes rewrite it: its nodes and lists, the waits derived so far and the
/// options the run has. Each pass takes it whole and changes what it owns.
#[derive(Debug, Clone)]
pub struct Working {
    /// The plan; its nodes are the working nodes.
    pub plan: PlanInput,
    /// The options of the run.
    pub options: Options,
    /// The waits between the nodes.
    pub orders: BTreeSet<Order>,
}

/// Rewrites a plan without changing what it builds, or refuses it with everything wrong. The
/// same plan gives the same result, whatever order its nodes are listed in.
///
/// The steps are the reference's: the plan is normalised and refused (a break is named, never
/// repaired), split into one behaviour a node, its waits derived, the waits checked once more
/// for a cycle (a split plan can hold one the whole plan did not show), then lifted (with
/// `lift: All`), settled and thinned. `kept` is each need on a name in `outside` as a wait from
/// the node `outside` maps it to, found on the nodes as they come out.
pub fn optimise(plan: &PlanInput, options: Options) -> Result<Optimised, Vec<Refusal>> {
    let plan = names::normalise(plan)?;
    refuse::refuse(&plan)?;
    let mut working = Working {
        plan,
        options,
        orders: BTreeSet::new(),
    };
    split::split(&mut working);
    if let Some(id) = held_twice(&working.plan.nodes) {
        return Err(vec![Refusal::new(
            RefusalCode::DuplicateId,
            format!("{id}: a part of a split node takes the id another node holds"),
        )]);
    }
    derive::derive(&mut working);
    if let Some(cycle) = cycle_of(&working) {
        let ids: Vec<&str> = cycle.iter().map(NodeId::as_str).collect();
        return Err(vec![Refusal::new(RefusalCode::Cycle, ids.join(" "))]);
    }
    if options.lift == Lift::All {
        lift::lift(&mut working);
    }
    settle::settle(&mut working).map_err(|refusal| vec![refusal])?;
    thin::thin(&mut working);
    let kept = kept(&working.plan);
    let mut nodes = working.plan.nodes;
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Optimised {
        nodes,
        orders: working.orders,
        kept,
    })
}

/// The first id two of the nodes hold, in id order. The refusal pass names one in the plan as
/// written; a split can make another, beside a filed ticket that holds a part's id.
fn held_twice(nodes: &[Node]) -> Option<&NodeId> {
    let mut seen = BTreeSet::new();
    let mut twice = BTreeSet::new();
    for node in nodes {
        if !seen.insert(&node.id) {
            twice.insert(&node.id);
        }
    }
    twice.into_iter().next()
}

/// The ids of a cycle among the waits derived so far, when there is one.
fn cycle_of(working: &Working) -> Option<Vec<NodeId>> {
    let ids: Vec<NodeId> = working.plan.nodes.iter().map(|n| n.id.clone()).collect();
    let edges: Vec<(NodeId, NodeId)> = working
        .orders
        .iter()
        .map(|order| (order.from.clone(), order.to.clone()))
        .collect();
    Dag::new(&ids, &edges).cycle()
}

/// D13: each need on a name another epic provides, as a wait from the node `outside` maps the
/// name to. The set sorts by the edge key, and nothing optimises it.
fn kept(plan: &PlanInput) -> BTreeSet<KeptWait> {
    let mut out = BTreeSet::new();
    for node in &plan.nodes {
        let needs = node
            .behaviours
            .iter()
            .flat_map(|behaviour| &behaviour.needs);
        for name in needs {
            let name: &Name = name;
            if let Some(from) = plan.outside.get(name) {
                out.insert(KeptWait {
                    from: from.clone(),
                    to: node.id.clone(),
                    needs: name.clone(),
                });
            }
        }
    }
    out
}
