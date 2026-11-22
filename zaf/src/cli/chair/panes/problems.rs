//! Problems (D5): what keeps the project from running, the claims of instances that ended and the
//! phases that got nowhere, most work held first. Enter runs a problem's fix when the hub named a
//! zaf verb for it (R3), in the tab's project, answers a failed phase's question, and says of an
//! orphaned claim that the next tick frees it.

use ratatui::text::Line;

use super::{Context, Cursor, Effect, Hint, Pane, Title, Tone, View, first_shown, hint, step};
use crate::app::chair::{ChairAction, ChairState, Fix, ProblemItem, TroubleKind};
use crate::cli::chair::keys::Key;
use crate::cli::chair::text::fit;
use crate::domain::screen::Meaning;

/// The Problems pane.
pub struct Problems;

/// What Enter on `problem` of `state` does.
pub fn open(state: &ChairState, problem: &ProblemItem) -> Effect {
    match problem.kind {
        TroubleKind::Orphaned => Effect::Note(
            Tone::Plain,
            format!(
                "{} is freed by the next tick: its instance has ended",
                problem
                    .ticket
                    .map_or_else(|| "the claim".to_owned(), |ticket| ticket.to_string())
            ),
        ),
        TroubleKind::FailedPhase => problem.question.map_or(Effect::Done, Effect::Answer),
        TroubleKind::Task(_) => Effect::Note(
            Tone::Plain,
            "fix it on the board: nothing here runs a fix for it".to_owned(),
        ),
        TroubleKind::Project(_) => match (problem.verb, problem.fix.as_deref()) {
            (Some(verb), fix) => {
                let command =
                    fix.map_or_else(|| format!("zaf {}", verb.args().join(" ")), str::to_owned);
                Effect::Write(ChairAction::Fix(Fix::new(
                    state.root.clone(),
                    verb,
                    &command,
                )))
            }
            (None, Some(fix)) => Effect::Note(Tone::Plain, format!("run it by hand: {fix}")),
            (None, None) => Effect::Note(Tone::Plain, "no zaf verb fixes it".to_owned()),
        },
    }
}

fn row(view: &View<'_>, problem: &ProblemItem, selected: bool, width: usize) -> Line<'static> {
    let mut spans = vec![
        view.marker(selected),
        View::plain(" "),
        view.paint(Meaning::Failed, "✗"),
        View::plain(" "),
    ];
    let mut used = 4;
    if let Some(ticket) = problem.ticket {
        let id = ticket.to_string();
        used += id.len() + 1;
        spans.push(view.paint(Meaning::Noun, id));
        spans.push(View::plain(" "));
    }
    match problem.kind {
        TroubleKind::Orphaned => {
            spans.push(view.paint(Meaning::Failed, "orphaned"));
            spans.push(view.paint(Meaning::Dim, format!(" holds {}", problem.holds)));
        }
        TroubleKind::FailedPhase | TroubleKind::Project(_) | TroubleKind::Task(_) => {
            spans.push(View::plain(fit(
                &problem.headline,
                width.saturating_sub(used),
            )));
        }
    }
    Line::from(spans)
}

impl Pane for Problems {
    fn title(&self, view: &View<'_>) -> Title {
        let count = view.state.map_or(0, |state| state.problems.len());
        let meaning = if count == 0 {
            Meaning::Done
        } else {
            Meaning::Failed
        };
        Title {
            text: "Problems".to_owned(),
            after: vec![view.paint(meaning, count.to_string())],
            right: Vec::new(),
        }
    }

    fn body(&self, view: &View<'_>, width: u16, rows: u16) -> Vec<Line<'static>> {
        let Some(state) = view.state else {
            return vec![view.reading()];
        };
        if state.problems.is_empty() {
            return vec![Line::from(vec![
                View::plain("  "),
                view.paint(Meaning::Done, "✓"),
                View::plain(" nothing is wrong"),
            ])];
        }
        let selected = view.cursor.problem.min(state.problems.len() - 1);
        let first = first_shown(selected, usize::from(rows));
        state
            .problems
            .iter()
            .enumerate()
            .skip(first)
            .take(usize::from(rows))
            .map(|(at, problem)| row(view, problem, at == selected, usize::from(width)))
            .collect()
    }

    fn key(&self, key: Key, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let Some(state) = context.state else {
            return Effect::Ignored;
        };
        if let Some(next) = step(key, cursor.problem, state.problems.len()) {
            cursor.problem = next;
            cursor.scroll = 0;
            return Effect::Done;
        }
        match key {
            Key::Enter => state
                .problems
                .get(cursor.problem)
                .map_or(Effect::Done, |problem| open(state, problem)),
            _ => Effect::Ignored,
        }
    }

    fn click(&self, row: u16, _column: u16, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let Some(state) = context.state else {
            return Effect::Done;
        };
        let rows = usize::from(context.rows.saturating_sub(1));
        let first = first_shown(
            cursor.problem.min(state.problems.len().saturating_sub(1)),
            rows,
        );
        let at = first + usize::from(row.saturating_sub(1));
        if row > 0 && at < state.problems.len() {
            cursor.problem = at;
            cursor.scroll = 0;
        }
        Effect::Done
    }

    fn hints(&self, _view: &View<'_>) -> Vec<Hint> {
        vec![
            hint("j/k", "move"),
            hint("Enter", "fix"),
            hint("Tab", "next pane"),
            hint("q", "quit"),
        ]
    }
}
