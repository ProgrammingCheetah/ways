//! Epics (D5): each open epic on one row, a square per ticket in the hub's colours with a symbol
//! beside each colour, its tickets done of all, and its stage. `j`/`k` pick an epic, `←`/`→` a
//! ticket; Enter reads the epic in Detail.

use ratatui::text::{Line, Span};

use super::{Context, Cursor, DetailView, Effect, Hint, Pane, Title, View, hint, step};
use crate::app::chair::{ChairState, EpicView, Square};
use crate::cli::chair::keys::Key;
use crate::cli::chair::layout::PaneId;
use crate::cli::chair::text::pad;
use crate::domain::screen::Meaning;

/// The Epics pane.
pub struct Epics;

/// The columns of an epic's row at a pane's width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Columns {
    /// The width of the widest `#id`.
    id: usize,
    /// The cells for the title.
    name: usize,
    /// The cells for the squares.
    squares: usize,
    /// Whether the stage shows.
    stage: bool,
}

/// The cells of the counts column: `10/14`.
const COUNT: usize = 5;
/// The cells of the stage column, its two spaces included.
const STAGE: usize = 12;

impl Columns {
    fn of(state: &ChairState, width: usize) -> Self {
        let id = state
            .epics
            .iter()
            .map(|epic| epic.number.to_string().len())
            .max()
            .unwrap_or(4);
        let most = state
            .epics
            .iter()
            .map(|epic| epic.squares.len())
            .max()
            .unwrap_or(0);
        // The marker and a space, the id and a space, a space after the title and one after the
        // squares.
        let fixed = 2 + id + 1 + 1 + 1 + COUNT;
        let room = width.saturating_sub(fixed);
        let squares = most.min(room.saturating_sub(8));
        let left = room - squares;
        let stage = left >= 8 + STAGE;
        let name = if stage { (left - STAGE).min(30) } else { left };
        Self {
            id,
            name,
            squares,
            stage,
        }
    }

    /// The column within the row where the squares begin.
    fn squares_at(self) -> usize {
        2 + self.id + 1 + self.name + 1
    }
}

/// The selected epic of `state`.
pub fn selected<'a>(state: &'a ChairState, cursor: &Cursor) -> Option<&'a EpicView> {
    state.epics.get(cursor.epic)
}

/// The selected ticket of `state`.
pub fn selected_square<'a>(state: &'a ChairState, cursor: &Cursor) -> Option<&'a Square> {
    selected(state, cursor).and_then(|epic| epic.squares.get(cursor.ticket))
}

fn row(view: &View<'_>, epic: &EpicView, at: usize, columns: Columns) -> Line<'static> {
    let chosen = at == view.cursor.epic;
    let mut spans = vec![
        if chosen {
            view.paint(Meaning::Verb, "▸")
        } else {
            View::plain(" ")
        },
        View::plain(" "),
        view.paint(
            Meaning::Noun,
            format!("{:<width$}", epic.number.to_string(), width = columns.id),
        ),
        View::plain(" "),
        View::plain(pad(&epic.title, columns.name)),
        View::plain(" "),
    ];
    let shown = if epic.squares.len() > columns.squares {
        columns.squares.saturating_sub(1)
    } else {
        epic.squares.len()
    };
    for (index, square) in epic.squares.iter().take(shown).enumerate() {
        let mut symbol = view.square(square.state);
        if chosen && view.focused && index == view.cursor.ticket {
            symbol = Span::styled(symbol.content, symbol.style.patch(view.theme.reverse()));
        }
        spans.push(symbol);
    }
    if shown < epic.squares.len() {
        spans.push(view.paint(Meaning::Dim, "…"));
    }
    let drawn = shown + usize::from(shown < epic.squares.len());
    spans.push(View::plain(
        " ".repeat(columns.squares.saturating_sub(drawn) + 1),
    ));
    spans.push(view.paint(
        Meaning::Dim,
        format!("{:>COUNT$}", format!("{}/{}", epic.done, epic.total)),
    ));
    if columns.stage {
        spans.push(view.paint(Meaning::Dim, format!("  {}", epic.stage_text())));
    }
    Line::from(spans)
}

/// The legend under the epics, short when the pane is narrow.
fn legend(view: &View<'_>, width: u16) -> Line<'static> {
    let dim = |text: &str| view.paint(Meaning::Dim, text.to_owned());
    if width >= 51 {
        Line::from(vec![
            View::plain("  "),
            view.paint(Meaning::Done, "■"),
            dim(" done  "),
            view.paint(Meaning::Verb, view.spinner()),
            dim(" running  □ waiting  "),
            view.paint(Meaning::Waiting, "?"),
            dim(" yours  "),
            view.paint(Meaning::Failed, "✗"),
            dim(" problem"),
        ])
    } else {
        Line::from(vec![
            View::plain("  "),
            view.paint(Meaning::Done, "■"),
            dim("done "),
            view.paint(Meaning::Verb, view.spinner()),
            dim("run □wait "),
            view.paint(Meaning::Waiting, "?"),
            dim("you "),
            view.paint(Meaning::Failed, "✗"),
            dim("fix"),
        ])
    }
}

/// The version every open epic is of, when they share one.
fn release(state: &ChairState) -> Option<String> {
    let first = state.epics.first()?.release.as_ref()?;
    state
        .epics
        .iter()
        .all(|epic| epic.release.as_ref() == Some(first))
        .then(|| first.to_string())
}

impl Pane for Epics {
    fn title(&self, view: &View<'_>) -> Title {
        let text = view.state.and_then(release).map_or_else(
            || "Epics".to_owned(),
            |version| format!("Epics of {version}"),
        );
        Title {
            text,
            after: Vec::new(),
            right: vec![view.paint(Meaning::Dim, "squares: one per ticket")],
        }
    }

    fn body(&self, view: &View<'_>, width: u16, rows: u16) -> Vec<Line<'static>> {
        let Some(state) = view.state else {
            return vec![view.reading()];
        };
        if state.epics.is_empty() {
            return vec![
                Line::from(view.paint(
                    Meaning::Dim,
                    format!("  {}: no epic is open", state.project),
                )),
                Line::from(vec![
                    view.paint(Meaning::Dim, "  start a version: "),
                    view.paint(Meaning::Program, "zaf"),
                    view.paint(Meaning::Verb, " begin"),
                ]),
            ];
        }
        let columns = Columns::of(state, usize::from(width));
        let legend_rows = usize::from(rows >= 7);
        let room = usize::from(rows).saturating_sub(legend_rows);
        let first = super::first_shown(view.cursor.epic, room);
        let mut lines: Vec<Line<'static>> = state
            .epics
            .iter()
            .enumerate()
            .skip(first)
            .take(room)
            .map(|(at, epic)| row(view, epic, at, columns))
            .collect();
        if legend_rows == 1 {
            lines.resize(room, Line::default());
            lines.push(legend(view, width));
        }
        lines
    }

    fn key(&self, key: Key, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let Some(state) = context.state else {
            return Effect::Ignored;
        };
        if let Some(next) = step(key, cursor.epic, state.epics.len()) {
            if next != cursor.epic {
                cursor.ticket = 0;
            }
            cursor.epic = next;
            cursor.scroll = 0;
            if cursor.detail != DetailView::Graph {
                cursor.detail = DetailView::Epic;
            }
            return Effect::Done;
        }
        let squares = selected(state, cursor).map_or(0, |epic| epic.squares.len());
        match key {
            Key::Right => {
                cursor.ticket = (cursor.ticket + 1).min(squares.saturating_sub(1));
                Effect::Done
            }
            Key::Left => {
                cursor.ticket = cursor.ticket.saturating_sub(1);
                Effect::Done
            }
            Key::Enter if selected(state, cursor).is_some() => {
                cursor.detail = DetailView::Read;
                cursor.scroll = 0;
                Effect::Focus(PaneId::Detail)
            }
            Key::Char('s') => {
                selected(state, cursor).map_or(Effect::Done, |epic| Effect::Sign(epic.number))
            }
            _ => Effect::Ignored,
        }
    }

    fn click(&self, row: u16, column: u16, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let Some(state) = context.state else {
            return Effect::Done;
        };
        let rows = usize::from(context.rows.saturating_sub(1));
        let legend_rows = usize::from(rows >= 7);
        let first = super::first_shown(cursor.epic, rows.saturating_sub(legend_rows));
        let at = first + usize::from(row.saturating_sub(1));
        if row == 0 || at >= state.epics.len() {
            return Effect::Done;
        }
        if at != cursor.epic {
            cursor.ticket = 0;
        }
        cursor.epic = at;
        cursor.detail = DetailView::Epic;
        let columns = Columns::of(state, usize::from(context.width));
        let squares = state.epics[at].squares.len();
        if let Some(index) = usize::from(column).checked_sub(columns.squares_at())
            && index < squares.min(columns.squares)
        {
            cursor.ticket = index;
        }
        Effect::Done
    }

    fn hints(&self, _view: &View<'_>) -> Vec<Hint> {
        vec![
            hint("j/k", "epic"),
            hint("←→", "ticket"),
            hint("Enter", "read"),
            hint("s", "sign"),
            hint("d", "graph"),
            hint("l", "live"),
            hint("w", "why"),
            hint("q", "quit"),
        ]
    }
}
