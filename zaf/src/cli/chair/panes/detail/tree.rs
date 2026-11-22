//! Detail's dependency tree (F6, D8): `d` draws the selected epic's tickets as a tree, each under
//! the first ticket inside the epic it is blocked by, with its symbol (□ waiting or ready, ■ done,
//! ? waiting on the Engineer, ✗ a problem, the spinner running) and what the core says it waits on
//! (tickets, its epic, questions, checks). A ticket on a blocker cycle, which no root reaches, is
//! drawn as a root of its own. [`layout`] is the pure part; [`page`] draws it.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::text::Line;

use super::Page;
use crate::app::chair::{ChairState, EpicView, Square, SquareState};
use crate::app::hub::state::{WaitKind, WaitState};
use crate::cli::chair::panes::epics::selected;
use crate::cli::chair::panes::{Cursor, View};
use crate::cli::chair::text::{fit, width};
use crate::domain::TicketId;
use crate::domain::screen::Meaning;

/// One row of the drawn tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    /// What is drawn before the ticket: the lines of the rows above and its branch, `│  ├─`.
    pub prefix: String,
    /// The ticket.
    pub ticket: TicketId,
    /// Its state, for its symbol.
    pub state: SquareState,
    /// Its title.
    pub title: String,
    /// What it waits on, in short.
    pub waits: Vec<Short>,
}

/// One thing a ticket waits on, in short: `#230`, `?#285`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Short {
    /// How it reads.
    pub text: String,
    /// Whether it waits on the Engineer: a question.
    pub yours: bool,
}

/// What `wait` reads as in the tree.
pub fn short(wait: &WaitState) -> Short {
    let number = wait
        .number
        .map_or_else(String::new, |number| format!("#{number}"));
    let (text, yours) = match wait.on {
        WaitKind::Ticket => (number, false),
        WaitKind::Question => (format!("?{number}"), true),
        WaitKind::Epic => ("its epic".to_owned(), false),
        WaitKind::Check => ("checks".to_owned(), false),
        WaitKind::Merge => (format!("merge {number}"), false),
        WaitKind::Base => ("base checks".to_owned(), false),
    };
    Short { text, yours }
}

/// What `square` waits on in short: the core's waits, else, for a ticket the core does not place
/// that waits, the blockers of its epic that are not done.
fn waits_of(square: &Square, epic: &EpicView) -> Vec<Short> {
    if !square.waits.is_empty() {
        return square.waits.iter().map(short).collect();
    }
    if square.state != SquareState::Waiting {
        return Vec::new();
    }
    square
        .blocked_by
        .iter()
        .filter(|blocker| {
            epic.squares
                .iter()
                .find(|other| other.ticket == **blocker)
                .is_none_or(|other| other.state != SquareState::Done)
        })
        .map(|blocker| Short {
            text: blocker.to_string(),
            yours: false,
        })
        .collect()
}

/// `epic`'s tickets as a tree: each under the first ticket of the epic its `Blocked by:` line
/// names, in the epic's order; the ones no root reaches (a cycle) as roots of their own.
pub fn layout(epic: &EpicView) -> Vec<TreeRow> {
    let inside: BTreeSet<TicketId> = epic.squares.iter().map(|square| square.ticket).collect();
    let parent = |square: &Square| {
        square
            .blocked_by
            .iter()
            .copied()
            .find(|blocker| *blocker != square.ticket && inside.contains(blocker))
    };
    let mut children: BTreeMap<Option<TicketId>, Vec<&Square>> = BTreeMap::new();
    for square in &epic.squares {
        children.entry(parent(square)).or_default().push(square);
    }
    let mut rows = Vec::new();
    let mut drawn = BTreeSet::new();
    let roots = children.get(&None).cloned().unwrap_or_default();
    branch(epic, &roots, "", &children, &mut drawn, &mut rows);
    // A ticket on a cycle hangs under a ticket no root reaches: it starts a tree of its own.
    for square in &epic.squares {
        if !drawn.contains(&square.ticket) {
            branch(epic, &[square], "", &children, &mut drawn, &mut rows);
        }
    }
    rows
}

/// Draws `kids` under `prefix`, each followed by its own children.
fn branch(
    epic: &EpicView,
    kids: &[&Square],
    prefix: &str,
    children: &BTreeMap<Option<TicketId>, Vec<&Square>>,
    drawn: &mut BTreeSet<TicketId>,
    rows: &mut Vec<TreeRow>,
) {
    let kids: Vec<&Square> = kids
        .iter()
        .copied()
        .filter(|square| !drawn.contains(&square.ticket))
        .collect();
    for (at, square) in kids.iter().enumerate() {
        if !drawn.insert(square.ticket) {
            continue;
        }
        let last = at + 1 == kids.len();
        let (arm, below) = if last {
            ("└─", "   ")
        } else {
            ("├─", "│  ")
        };
        rows.push(TreeRow {
            prefix: format!("{prefix}{arm}"),
            ticket: square.ticket,
            state: square.state,
            title: square.title.clone(),
            waits: waits_of(square, epic),
        });
        let grandchildren = children
            .get(&Some(square.ticket))
            .cloned()
            .unwrap_or_default();
        branch(
            epic,
            &grandchildren,
            &format!("{prefix}{below}"),
            children,
            drawn,
            rows,
        );
    }
}

/// The tree's title: `Tree of #224`.
pub fn title(state: &ChairState, cursor: &Cursor) -> String {
    selected(state, cursor).map_or_else(
        || "Tree".to_owned(),
        |epic| format!("Tree of {}", epic.number),
    )
}

/// The selected epic's tickets as a tree.
pub fn page(page: &mut Page<'_, '_>, state: &ChairState, cursor: &Cursor) {
    let Some(epic) = selected(state, cursor) else {
        page.text(Some(Meaning::Dim), "No epic is open.");
        return;
    };
    let view = *page.view();
    let cells = page.width();
    page.text(Some(Meaning::Bold), &epic.title);
    if epic.squares.is_empty() {
        page.text(Some(Meaning::Dim), "It has no tickets yet.");
    }
    for row in layout(epic) {
        let id = row.ticket.to_string();
        let waits: Vec<String> = row.waits.iter().map(|wait| wait.text.clone()).collect();
        let waits_cells = if waits.is_empty() {
            0
        } else {
            3 + width(&waits.join(" "))
        };
        let used = width(&row.prefix) + 2 + width(&id) + 1;
        let title_cells = cells.saturating_sub(used + waits_cells);
        let mut spans = vec![
            view.paint(Meaning::Dim, row.prefix.clone()),
            view.square(row.state),
            View::plain(" "),
            view.paint(Meaning::Noun, id),
            View::plain(" "),
            View::plain(fit(&row.title, title_cells)),
        ];
        if !row.waits.is_empty() {
            spans.push(view.paint(Meaning::Dim, " ←"));
            for wait in &row.waits {
                let meaning = if wait.yours {
                    Meaning::Waiting
                } else {
                    Meaning::Dim
                };
                spans.push(View::plain(" "));
                spans.push(view.paint(meaning, wait.text.clone()));
            }
        }
        page.line(Line::from(spans));
    }
    page.blank();
    page.text(
        Some(Meaning::Dim),
        "← what each waits on, ?# your question · w why · d closes the tree",
    );
}
