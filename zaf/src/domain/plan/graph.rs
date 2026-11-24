//! The directed acyclic graph the passes read: the only file under `domain/plan` that may name a
//! graph crate (`tests/architecture.rs` fences them).
//!
//! The graph is a petgraph `Graph` built afresh from a list of nodes and a list of waits, both
//! sorted by natural id first, and nothing is ever removed from it (removal reorders indices). So
//! the same graph gives the same answers however its lists were given (D16). Only two functions
//! of rustworkx-core are used, `lexicographical_topological_sort` and `layers`; its
//! `longest_path` is never used, because its ties change between runs.

use std::collections::{BTreeMap, BTreeSet};

use petgraph::Directed;
use petgraph::adj::UnweightedList;
use petgraph::algo::{is_cyclic_directed, kosaraju_scc, toposort, tred};
use petgraph::graph::{Graph, NodeIndex};
use petgraph::visit::IntoNeighbors;
use rustworkx_core::dag_algo;

use super::NodeId;

/// A wait between two nodes, by index.
type Wait = (NodeIndex, NodeIndex);

/// The nodes of a plan and the waits between them (I12).
///
/// `cycle` answers whether the waits go round. `order`, `layers`, `reduction` and `closure` read
/// a graph with no cycle: a caller asks `cycle` first (the refusal pass does), and each of these
/// panics on a graph that has one rather than answer with part of it.
#[derive(Debug, Clone, Default)]
pub struct Dag {
    graph: Graph<NodeId, (), Directed>,
}

impl PartialEq for Dag {
    fn eq(&self, other: &Self) -> bool {
        self.nodes().eq(other.nodes()) && self.waits() == other.waits()
    }
}

impl Eq for Dag {}

impl Dag {
    /// A graph of these nodes and these edges, each edge from the node that comes first.
    ///
    /// Both lists are sorted by natural id, and a node or an edge given twice is kept once. An
    /// edge with an end that is not among the nodes is left out: the passes refuse such a plan
    /// before they build the graph.
    pub fn new(nodes: &[NodeId], edges: &[(NodeId, NodeId)]) -> Self {
        let nodes: BTreeSet<&NodeId> = nodes.iter().collect();
        let edges: BTreeSet<&(NodeId, NodeId)> = edges.iter().collect();
        let mut graph = Graph::with_capacity(nodes.len(), edges.len());
        let mut index = BTreeMap::new();
        for id in nodes {
            index.insert(id, graph.add_node(id.clone()));
        }
        for (from, to) in edges {
            if let (Some(&from), Some(&to)) = (index.get(from), index.get(to)) {
                graph.add_edge(from, to, ());
            }
        }
        Self { graph }
    }

    /// The node ids in natural order, which is the order of their indices.
    fn nodes(&self) -> impl Iterator<Item = &NodeId> {
        self.graph.node_weights()
    }

    /// Every wait, sorted: the edges in the order the graph holds them are sorted by `new`
    /// already, but the order is stated here and not left to that.
    fn waits(&self) -> BTreeSet<(&NodeId, &NodeId)> {
        self.graph
            .edge_indices()
            .filter_map(|edge| self.graph.edge_endpoints(edge))
            .map(|(from, to)| (&self.graph[from], &self.graph[to]))
            .collect()
    }

    fn id(&self, index: NodeIndex) -> NodeId {
        self.graph[index].clone()
    }

    /// The nodes of the waits that go round, sorted, or `None` for a graph with no cycle. When
    /// several groups of nodes go round, the group whose first node comes first is named.
    pub fn cycle(&self) -> Option<Vec<NodeId>> {
        if !is_cyclic_directed(&self.graph) {
            return None;
        }
        let mut groups: Vec<Vec<NodeId>> = kosaraju_scc(&self.graph)
            .into_iter()
            .filter(|group| {
                group.len() > 1
                    || group
                        .first()
                        .is_some_and(|&node| self.graph.contains_edge(node, node))
            })
            .map(|group| {
                let mut ids: Vec<NodeId> = group.into_iter().map(|node| self.id(node)).collect();
                ids.sort();
                ids
            })
            .collect();
        groups.sort();
        groups.into_iter().next()
    }

    /// A topological order of the nodes, as indices.
    fn toposorted(&self) -> Vec<NodeIndex> {
        toposort(&self.graph, None)
            .unwrap_or_else(|_| panic!("the waits of a plan go round: ask `cycle` first"))
    }

    /// Every wait of the reduction and of the closure, as pairs of node indices.
    fn reduced(&self) -> (Vec<Wait>, Vec<Wait>) {
        let order = self.toposorted();
        let (list, _) = tred::dag_to_toposorted_adjacency_list(&self.graph, &order);
        let (reduction, closure) = tred::dag_transitive_reduction_closure(&list);
        let pairs = |lists: &UnweightedList<u32>| {
            let mut out = Vec::new();
            for (at, &from) in order.iter().enumerate() {
                let at = u32::try_from(at).expect("a plan has fewer than 2^32 nodes");
                for to in lists.neighbors(at) {
                    out.push((from, order[to as usize]));
                }
            }
            out
        };
        (pairs(&reduction), pairs(&closure))
    }

    /// The waits that stay when every wait another path already gives is left out, sorted.
    pub fn reduction(&self) -> BTreeSet<(NodeId, NodeId)> {
        let (reduction, _) = self.reduced();
        reduction
            .into_iter()
            .map(|(from, to)| (self.id(from), self.id(to)))
            .collect()
    }

    /// Every node with the nodes that come after it through any chain of waits.
    pub fn closure(&self) -> BTreeMap<NodeId, BTreeSet<NodeId>> {
        let (_, closure) = self.reduced();
        let mut out: BTreeMap<NodeId, BTreeSet<NodeId>> = self
            .nodes()
            .map(|id| (id.clone(), BTreeSet::new()))
            .collect();
        for (from, to) in closure {
            out.entry(self.id(from)).or_default().insert(self.id(to));
        }
        out
    }

    /// One order of the nodes in which every wait comes first: of the nodes free to go next, the
    /// one with the lowest natural id goes first.
    pub fn order(&self) -> Vec<NodeId> {
        let order = dag_algo::lexicographical_topological_sort(
            &self.graph,
            |node| Ok::<_, std::convert::Infallible>(self.graph[node].clone()),
            false,
            None,
        )
        .unwrap_or_else(|_| panic!("the waits of a plan go round: ask `cycle` first"));
        // A cycle shortens the answer without saying so.
        assert_eq!(
            order.len(),
            self.graph.node_count(),
            "the waits of a plan go round: ask `cycle` first"
        );
        order.into_iter().map(|node| self.id(node)).collect()
    }

    /// The nodes in layers: the first holds those that wait for none, each next one those whose
    /// last wait is on the layer before. Each layer is sorted.
    pub fn layers(&self) -> Vec<Vec<NodeId>> {
        self.toposorted();
        let sources: Vec<NodeIndex> = self
            .graph
            .node_indices()
            .filter(|&node| {
                self.graph
                    .neighbors_directed(node, petgraph::Direction::Incoming)
                    .next()
                    .is_none()
            })
            .collect();
        dag_algo::layers(&self.graph, sources)
            .map(|layer| {
                let layer = layer
                    .unwrap_or_else(|_| panic!("the waits of a plan go round: ask `cycle` first"));
                let mut ids: Vec<NodeId> = layer.into_iter().map(|node| self.id(node)).collect();
                ids.sort();
                ids
            })
            .filter(|layer| !layer.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests;
