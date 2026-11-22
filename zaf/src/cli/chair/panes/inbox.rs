//! The Inbox (D5): everything that waits on the Engineer, most blocking first, as the hub's Next
//! for you list has it, and, with no release due, the offer to begin the next version (F16). Enter
//! answers a question, signs an epic (after its undo), runs a release phase due in the phase pane,
//! takes a broken project to Problems, and leaves ready work no scheduler ticks to the chair's `b`.

use ratatui::text::Line;

use super::{Context, Cursor, Effect, Hint, Pane, Title, Tone, View, first_shown, hint, step};
use crate::app::chair::{ChairState, InboxItem};
use crate::app::hub::next::NextKind;
use crate::cli::chair::keys::Key;
use crate::cli::chair::layout::PaneId;
use crate::cli::chair::text::{fit, width};
use crate::domain::screen::Meaning;

/// The Inbox pane.
pub struct Inbox;

/// What Enter on `item` does. Ready work no scheduler ticks is ignored here: the chair takes the
/// Enter as `b`, since starting the build needs the project's state.
pub fn open(item: &InboxItem) -> Effect {
    match item.kind {
        NextKind::Question | NextKind::Door | NextKind::Failed => item
            .number
            .filter(|_| item.answerable())
            .map_or(Effect::Done, Effect::Answer),
        NextKind::Sign => item.number.map_or(Effect::Done, Effect::Sign),
        // The offer to begin the next version runs nothing yet: the phase pane (F8) runs it.
        NextKind::Phase if item.begins.is_some() => Effect::Note(
            Tone::Plain,
            format!(
                "the phase pane runs {} once it comes (F8); nothing ran",
                item.command.as_deref().unwrap_or("zaf begin")
            ),
        ),
        NextKind::Phase => super::phase::version_of(item).map_or_else(
            || {
                Effect::Note(
                    Tone::Failed,
                    "the release this phase is due for is unknown".to_owned(),
                )
            },
            Effect::RunPhase,
        ),
        NextKind::Unticked => Effect::Ignored,
        NextKind::Broken => Effect::Focus(PaneId::Problems),
    }
}

/// The epic `item` asks the Engineer to sign, for a sign item.
pub fn signs(item: &InboxItem) -> Option<crate::domain::TicketId> {
    item.number.filter(|_| item.kind == NextKind::Sign)
}

/// The selected item of `state`.
pub fn selected<'a>(state: &'a ChairState, cursor: &Cursor) -> Option<&'a InboxItem> {
    state.inbox.get(cursor.inbox)
}

fn row(view: &View<'_>, item: &InboxItem, selected: bool, width_left: usize) -> Line<'static> {
    let marker = view.marker(selected);
    let (symbol, head) = match item.kind {
        NextKind::Question | NextKind::Door | NextKind::Failed => (
            view.paint(
                Meaning::Waiting,
                if item.kind == NextKind::Door {
                    "!"
                } else {
                    "?"
                },
            ),
            item.number
                .map_or_else(String::new, |number| number.to_string()),
        ),
        NextKind::Sign => (
            view.paint(Meaning::Verb, "◆"),
            item.number
                .map_or_else(String::new, |number| format!("sign {number}")),
        ),
        NextKind::Phase | NextKind::Unticked => (view.paint(Meaning::Verb, "▶"), String::new()),
        NextKind::Broken => (view.paint(Meaning::Failed, "✗"), String::new()),
    };
    let blocks = format!("{:>2}", item.blocks);
    let text = match (&item.question, item.kind) {
        (Some(question), _) => question.headline().to_owned(),
        (None, NextKind::Sign) => item
            .title
            .split_once(": ")
            .map_or(item.title.as_str(), |(_, title)| title)
            .to_owned(),
        (None, _) => item.title.clone(),
    };
    let used = 2 + 2 + width(&head) + usize::from(!head.is_empty()) + blocks.len() + 1;
    let mut spans = vec![marker, View::plain(" "), symbol, View::plain(" ")];
    if !head.is_empty() {
        spans.push(view.paint(Meaning::Noun, head));
        spans.push(View::plain(" "));
    }
    spans.push(view.paint(Meaning::Dim, blocks));
    spans.push(View::plain(" "));
    spans.push(View::plain(fit(&text, width_left.saturating_sub(used))));
    Line::from(spans)
}

impl Pane for Inbox {
    fn title(&self, view: &View<'_>) -> Title {
        let count = view.state.map_or(0, ChairState::waiting);
        Title {
            text: "Waiting on you".to_owned(),
            after: vec![view.paint(Meaning::Waiting, count.to_string())],
            right: vec![view.paint(Meaning::Dim, "by work held")],
        }
    }

    fn body(&self, view: &View<'_>, width: u16, rows: u16) -> Vec<Line<'static>> {
        let Some(state) = view.state else {
            return vec![view.reading()];
        };
        if state.inbox.is_empty() {
            return vec![Line::from(vec![
                View::plain("  "),
                view.paint(Meaning::Done, "✓"),
                View::plain(" nothing waits on you"),
            ])];
        }
        let selected = view.cursor.inbox.min(state.inbox.len() - 1);
        let first = first_shown(selected, usize::from(rows));
        state
            .inbox
            .iter()
            .enumerate()
            .skip(first)
            .take(usize::from(rows))
            .map(|(at, item)| row(view, item, at == selected, usize::from(width)))
            .collect()
    }

    fn key(&self, key: Key, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let Some(state) = context.state else {
            return Effect::Ignored;
        };
        if let Some(next) = step(key, cursor.inbox, state.inbox.len()) {
            cursor.inbox = next;
            cursor.scroll = 0;
            return Effect::Done;
        }
        match key {
            Key::Enter => selected(state, cursor).map_or(Effect::Done, open),
            Key::Char('s') => selected(state, cursor)
                .and_then(signs)
                .map_or(Effect::Ignored, Effect::Sign),
            _ => Effect::Ignored,
        }
    }

    fn click(&self, row: u16, _column: u16, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let Some(state) = context.state else {
            return Effect::Done;
        };
        let rows = usize::from(context.rows.saturating_sub(1));
        let first = first_shown(cursor.inbox.min(state.inbox.len().saturating_sub(1)), rows);
        let at = first + usize::from(row.saturating_sub(1));
        if row > 0 && at < state.inbox.len() {
            cursor.inbox = at;
            cursor.scroll = 0;
        }
        Effect::Done
    }

    fn hints(&self, _view: &View<'_>) -> Vec<Hint> {
        vec![
            hint("j/k", "move"),
            hint("Enter", "answer"),
            hint("o", "start phase"),
            hint("Tab", "next pane"),
            hint("q", "quit"),
        ]
    }
}
