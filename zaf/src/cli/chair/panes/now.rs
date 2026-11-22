//! Now (D5): what runs, how much is ready and waiting, the slots, the guard and the machine's
//! budget (F14), the releases due, and whether a scheduler keeps the build going, with the keys
//! that start and stop it (F9, F15).

use ratatui::text::Line;

use super::{Context, Cursor, Effect, Hint, Pane, Title, View, hint, step};
use crate::app::chair::{NowView, RunningLine, WatchView};
use crate::cli::chair::keys::Key;
use crate::cli::chair::text::{fit, width};
use crate::domain::screen::Meaning;

/// The Now pane.
pub struct Now;

/// `seconds` as a run's age: `45s`, `3m05s`, `2h04m`.
pub fn age(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m{:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h{:02}m", seconds / 3600, seconds / 60 % 60),
    }
}

fn running(view: &View<'_>, line: &RunningLine, selected: bool, cells: usize) -> Line<'static> {
    let id = line
        .ticket
        .map_or_else(|| "#?".to_owned(), |ticket| ticket.to_string());
    let after = line.run_seconds.map(age).unwrap_or_default();
    let title = line.title.clone().unwrap_or_else(|| line.instance.clone());
    let used = 4 + width(&id) + 1 + after.len() + 1;
    Line::from(vec![
        view.marker(selected),
        View::plain(" "),
        view.paint(Meaning::Verb, view.spinner()),
        View::plain(" "),
        view.paint(Meaning::Noun, id),
        View::plain(" "),
        View::plain(fit(&title, cells.saturating_sub(used))),
        View::plain(" "),
        view.paint(Meaning::Dim, after),
    ])
}

/// Whether the project's scheduler runs, and the key that starts or stops it when the row has
/// room for it: the key bar shows both keys whatever the row's width.
fn scheduler(view: &View<'_>, watch: Option<&WatchView>, cells: usize) -> Line<'static> {
    let (mut spans, (key, what)) = match watch {
        Some(watch) if watch.running => (
            vec![
                View::plain("  "),
                view.paint(Meaning::Dim, "scheduler builds "),
                view.paint(
                    Meaning::Noun,
                    watch
                        .version
                        .as_ref()
                        .map_or_else(|| "?".to_owned(), ToString::to_string),
                ),
            ],
            ("B", " stops it"),
        ),
        Some(_) => (
            vec![
                View::plain("  "),
                view.paint(Meaning::Dim, "no scheduler runs"),
            ],
            ("b", " starts one"),
        ),
        None => {
            return Line::from(view.paint(Meaning::Dim, "  the scheduler could not be read"));
        }
    };
    let used: usize = spans.iter().map(|span| width(&span.content)).sum();
    if used + width(" · ") + width(key) + width(what) <= cells {
        spans.extend([
            view.paint(Meaning::Dim, " · "),
            view.paint(Meaning::Verb, key),
            view.paint(Meaning::Dim, what),
        ]);
    }
    Line::from(spans)
}

/// The fewest cells a cut limit is still worth showing in.
const SOME_WORDS: usize = 10;

/// The limits on one line, as many as the width holds, in this order: the slots busy, the guard,
/// the machine's units in use. The guard is in the waiting colour once it holds the spawner back.
pub fn limits(view: &View<'_>, now: &NowView, cells: usize) -> Line<'static> {
    let mut parts: Vec<(Meaning, String)> = Vec::new();
    if let Some(slots) = now.slots {
        let meaning = if slots.in_use >= slots.total {
            Meaning::Waiting
        } else {
            Meaning::Dim
        };
        parts.push((meaning, format!("slots {}/{}", slots.in_use, slots.total)));
    }
    match &now.guard {
        Some(guard) => parts.push((
            if guard.tight {
                Meaning::Waiting
            } else {
                Meaning::Dim
            },
            format!("guard {}", guard.line),
        )),
        None => parts.push((Meaning::Dim, "no guard yet".to_owned())),
    }
    if let Some(budget) = now.budget {
        let used = format!("{:.1}", budget.in_use.as_f64());
        parts.push((
            Meaning::Dim,
            budget.units.map_or_else(
                || format!("{used} units, no cap"),
                |units| format!("{used}/{units} units"),
            ),
        ));
    }
    let mut spans = vec![View::plain("  ")];
    let mut used = 2;
    for (at, (meaning, text)) in parts.into_iter().enumerate() {
        let gap = if at == 0 { "" } else { " · " };
        let left = cells.saturating_sub(used + width(gap));
        // A part that does not fit is cut when a few words of it still do, and ends the line.
        let cut = width(&text) > left;
        if cut && left < SOME_WORDS && at > 0 {
            break;
        }
        if at > 0 {
            spans.push(view.paint(Meaning::Dim, gap));
        }
        spans.push(view.paint(meaning, fit(&text, left)));
        if cut {
            break;
        }
        used += width(gap) + width(&text);
    }
    Line::from(spans)
}

impl Pane for Now {
    fn title(&self, view: &View<'_>) -> Title {
        let right = view.state.map_or_else(Vec::new, |state| {
            vec![view.paint(Meaning::Dim, format!("{} running", state.now.running.len()))]
        });
        Title {
            text: "Now".to_owned(),
            after: Vec::new(),
            right,
        }
    }

    fn body(&self, view: &View<'_>, width: u16, rows: u16) -> Vec<Line<'static>> {
        let Some(state) = view.state else {
            return vec![view.reading()];
        };
        let now = &state.now;
        let cells = usize::from(width);
        let mut lines: Vec<Line<'static>> = Vec::new();
        // The counts, the releases due, the scheduler and the limits take a row each below what
        // runs.
        let below = 3 + now.due.len();
        let room = usize::from(rows).saturating_sub(below).max(1);
        let selected = view.cursor.running.min(now.running.len().saturating_sub(1));
        lines.extend(
            now.running
                .iter()
                .enumerate()
                .take(room)
                .map(|(at, line)| running(view, line, at == selected, cells)),
        );
        if now.running.is_empty() {
            lines.push(Line::from(view.paint(Meaning::Dim, "  nothing running")));
        }
        lines.push(Line::from(vec![
            View::plain("  "),
            view.paint(Meaning::Done, format!("ready {}", now.ready)),
            View::plain("  "),
            view.paint(Meaning::Dim, format!("waiting {}", now.waiting)),
        ]));
        for due in &now.due {
            lines.push(Line::from(vec![
                View::plain("  "),
                view.paint(Meaning::Verb, "▶"),
                View::plain(" "),
                view.paint(Meaning::Noun, due.version.to_string()),
                View::plain(format!(" {} due", due.phase)),
            ]));
        }
        lines.push(scheduler(view, now.watch.as_ref(), cells));
        // The limits come last: the scheduler and its keys keep their row when a short pane
        // holds one row too few.
        lines.push(limits(view, now, cells));
        lines
    }

    fn key(&self, key: Key, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let Some(state) = context.state else {
            return Effect::Ignored;
        };
        if let Some(next) = step(key, cursor.running, state.now.running.len()) {
            cursor.running = next;
            return Effect::Done;
        }
        Effect::Ignored
    }

    fn click(&self, row: u16, _column: u16, context: Context<'_>, cursor: &mut Cursor) -> Effect {
        let Some(state) = context.state else {
            return Effect::Done;
        };
        let at = usize::from(row.saturating_sub(1));
        if row > 0 && at < state.now.running.len() {
            cursor.running = at;
        }
        Effect::Done
    }

    fn hints(&self, _view: &View<'_>) -> Vec<Hint> {
        vec![
            hint("j/k", "move"),
            hint("l", "live"),
            hint("w", "why"),
            hint("x", "stop it"),
            hint("b/B", "build"),
            hint("t", "tick"),
            hint("q", "quit"),
        ]
    }
}
