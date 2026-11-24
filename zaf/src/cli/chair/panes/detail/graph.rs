//! Detail's graph (F6, D23): `d` draws the selected epic's graph as its tickets read now, a group
//! of rows for each layer. Each ticket shows its symbol (□ waiting or ready, ■ done, ? waiting on
//! the Engineer, ✗ a problem, the spinner running), its node, its ticket, its title and every wait
//! the graph holds on it, so no edge is hidden; the nodes of the critical path are marked ★. The
//! header gives the chain, the width of each layer, the critical path and the shape check. `v`
//! flips to the graph as the plan records wrote it (D24): its nodes with no states, headed "as
//! planned" with its own ★ path; an epic filed before 0.6.0 has the one view, headed "planned
//! before 0.6.0", and `v` does nothing there. The graph and the check are read by [`crate::app::chair::look::read_graph`]; [`rows`] and
//! [`header`] are the pure part, [`page`] draws them.

use std::collections::BTreeSet;

use ratatui::text::Line;

use super::Page;
use crate::app::chair::look::GraphRead;
use crate::app::chair::{ChairState, EpicView, SquareState};
use crate::app::plan::show::{PlanView, ViewKind, ViewNode};
use crate::cli::chair::panes::epics::selected;
use crate::cli::chair::panes::{Cursor, View};
use crate::cli::chair::text::{fit, width};
use crate::domain::TicketId;
use crate::domain::plan::NodeId;
use crate::domain::plan::read::WaitTarget;
use crate::domain::plan::shape::ShapeFailure;
use crate::domain::screen::Meaning;

/// One ticket of the drawn graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    /// Its node.
    pub node: NodeId,
    /// The ticket that holds it.
    pub ticket: Option<TicketId>,
    /// Its state, for its symbol; none when the epic holds no square for it.
    pub state: Option<SquareState>,
    /// Its title.
    pub title: String,
    /// Whether it is on the critical path.
    pub critical: bool,
    /// What it waits on: the nodes it comes after, then the tasks of other epics, each once.
    pub waits: Vec<String>,
}

/// The rows of each layer of `view`, in the view's own order; `epic` gives the squares' states.
pub fn rows(view: &PlanView, epic: Option<&EpicView>) -> Vec<Vec<GraphRow>> {
    let path: BTreeSet<&NodeId> = view.measures.critical_path.iter().collect();
    view.layers
        .iter()
        .map(|layer| {
            layer
                .iter()
                .filter_map(|id| view.nodes.iter().find(|node| node.id == *id))
                .map(|node| GraphRow {
                    node: node.id.clone(),
                    ticket: node.ticket,
                    state: state_of(node, epic),
                    title: node.title.clone(),
                    critical: path.contains(&node.id),
                    waits: waits_of(view, node),
                })
                .collect()
        })
        .collect()
}

/// The state of the square `node`'s ticket has in `epic`.
fn state_of(node: &ViewNode, epic: Option<&EpicView>) -> Option<SquareState> {
    let ticket = node.ticket?;
    epic?
        .squares
        .iter()
        .find(|square| square.ticket == ticket)
        .map(|square| square.state)
}

/// Every wait on `node`: the node it comes after for each order, and the other epic's task or
/// epic of each kept wait; none twice.
fn waits_of(view: &PlanView, node: &ViewNode) -> Vec<String> {
    let mut waits: Vec<String> = Vec::new();
    let orders = view
        .orders
        .iter()
        .filter(|order| order.to == node.id)
        .map(|order| order.from.to_string());
    let kept = view
        .kept
        .iter()
        .filter(|kept| kept.to == node.id)
        .map(|kept| match &kept.on {
            WaitTarget::Ticket { ticket, .. } => ticket.to_string(),
            WaitTarget::Epic(epic) => format!("epic {epic}"),
        });
    for wait in orders.chain(kept) {
        if !waits.contains(&wait) {
            waits.push(wait);
        }
    }
    waits
}

/// The header: `chain 3 · width 2 3 1 · critical 5: I@1004.1 → C → D · shape: pass`; the shape is
/// `fail` with the count when the graph breaks a rule, and left out for a graph of the older
/// template, which has none to hold to.
pub fn header(view: &PlanView, shape: &Result<Vec<ShapeFailure>, String>) -> String {
    let measures = &view.measures;
    let widths: Vec<String> = measures.width.iter().map(ToString::to_string).collect();
    let path: Vec<&str> = measures.critical_path.iter().map(NodeId::as_str).collect();
    let mut head = format!(
        "chain {} · width {} · critical {}: {}",
        measures.chain,
        widths.join(" "),
        measures.critical,
        path.join(" → ")
    );
    match (view.kind, shape) {
        (ViewKind::Older | ViewKind::Before, _) => {}
        (_, Ok(failures)) if failures.is_empty() => head.push_str(" · shape: pass"),
        (_, Ok(failures)) => head.push_str(&format!(" · shape: fail {}", failures.len())),
        (_, Err(_)) => head.push_str(" · shape: not checked"),
    }
    head
}

/// The title: `Graph of #224`.
pub fn title(state: &ChairState, cursor: &Cursor) -> String {
    selected(state, cursor).map_or_else(
        || "Graph".to_owned(),
        |epic| format!("Graph of {}", epic.number),
    )
}

/// The selected epic's graph, a group of rows for each layer.
pub fn page(page: &mut Page<'_, '_>, state: &ChairState, cursor: &Cursor) {
    let Some(epic) = selected(state, cursor) else {
        page.text(Some(Meaning::Dim), "No epic is open.");
        return;
    };
    let view = *page.view();
    page.text(Some(Meaning::Bold), &epic.title);
    match view.looks.graph(epic.number) {
        None => page.line(Line::from(vec![
            view.paint(Meaning::Verb, view.spinner()),
            view.paint(
                Meaning::Dim,
                format!(" reading the graph of epic {}…", epic.number),
            ),
        ])),
        Some(read) => drawn(page, epic, read, cursor.before),
    }
    page.blank();
    page.text(
        Some(Meaning::Dim),
        "★ critical path · ← waits on · v before/after · w why · d closes",
    );
}

/// The graph `read` of `epic`, as planned when `before` asks for it and the epic has one: its
/// header, the shape's failures, and its layers. The graph as planned holds no ticket's state.
fn drawn(page: &mut Page<'_, '_>, epic: &EpicView, read: &GraphRead, before: bool) {
    let view = *page.view();
    let cells = page.width();
    let after = match &read.view {
        Ok(graph) => graph,
        Err(why) => {
            page.text(Some(Meaning::Waiting), why);
            return;
        }
    };
    // An epic filed before 0.6.0 has the one view, and `v` does nothing there.
    let planned = before && after.kind != ViewKind::Older;
    let graph = if planned {
        match &read.before {
            Ok(graph) => graph,
            Err(why) => {
                page.text(Some(Meaning::Waiting), why);
                return;
            }
        }
    } else {
        after
    };
    match graph.kind {
        ViewKind::Older => page.text(Some(Meaning::Dim), "planned before 0.6.0"),
        ViewKind::Before => page.text(Some(Meaning::Dim), "as planned"),
        ViewKind::After => {}
    }
    page.text(Some(Meaning::Plain), &header(graph, &read.shape));
    match &read.shape {
        _ if planned => {}
        Ok(failures) => {
            for failure in failures {
                page.text(Some(Meaning::Failed), &format!("✗ {failure}"));
            }
        }
        Err(why) if graph.kind != ViewKind::Older => {
            page.text(
                Some(Meaning::Dim),
                &format!("the shape was not checked: {why}"),
            );
        }
        Err(_) => {}
    }
    let squares = if planned { None } else { Some(epic) };
    for (at, layer) in rows(graph, squares).iter().enumerate() {
        page.blank();
        page.text(Some(Meaning::Dim), &format!("layer {}", at + 1));
        for row in layer {
            let node = row.node.to_string();
            let ticket = row.ticket.map(|ticket| ticket.to_string());
            let waits = row.waits.join(" ");
            let waits_cells = if waits.is_empty() {
                0
            } else {
                3 + width(&waits)
            };
            // The ★ and its space, the square and its space, the node and its space, the ticket.
            let used =
                2 + 2 + width(&node) + 1 + ticket.as_ref().map_or(0, |ticket| width(ticket) + 1);
            let title_cells = cells.saturating_sub(used + waits_cells);
            let mark = if row.critical {
                view.paint(Meaning::Waiting, "★ ".to_owned())
            } else {
                View::plain("  ")
            };
            let square = match row.state {
                Some(state) => view.square(state),
                None => view.paint(Meaning::Dim, "·".to_owned()),
            };
            let mut spans = vec![
                mark,
                square,
                View::plain(" "),
                view.paint(Meaning::Noun, node),
                View::plain(" "),
            ];
            if let Some(ticket) = ticket {
                spans.push(view.paint(Meaning::Dim, ticket));
                spans.push(View::plain(" "));
            }
            spans.push(View::plain(fit(&row.title, title_cells)));
            if !row.waits.is_empty() {
                spans.push(view.paint(Meaning::Dim, " ←".to_owned()));
                for wait in &row.waits {
                    spans.push(View::plain(" "));
                    spans.push(view.paint(Meaning::Dim, wait.clone()));
                }
            }
            page.line(Line::from(spans));
        }
    }
}
