//! Live (F7, D9): a ticket followed as it goes. `l` on a ticket follows it; with none chosen,
//! Live follows the first that runs. It shows what runs (its phase, how long, its last tool call),
//! the hub's reports on the ticket (a few general words each, as the hub records them) and the
//! tail of its phase run's log, both read on the tab's gatherer thread and read again with each
//! read of the project. A `claude -p` phase writes its log only at its end, so until then the pane
//! says "working".

use std::time::SystemTime;

use ratatui::text::Line;

use super::{Context, Cursor, Effect, Hint, Pane, Title, View, hint};
use crate::app::chair::look::{RunTail, TicketRead};
use crate::app::chair::{ChairState, RunningLine, SquareState};
use crate::cli::chair::keys::Key;
use crate::cli::chair::panes::now::age;
use crate::cli::chair::text::{fit, width};
use crate::domain::TicketId;
use crate::domain::screen::Meaning;

/// The Live pane.
pub struct Live;

/// The run Live follows: the ticket `l` chose while it runs, else the first that runs.
pub fn followed<'a>(state: &'a ChairState, cursor: &Cursor) -> Option<&'a RunningLine> {
    let running = &state.now.running;
    cursor
        .live
        .and_then(|ticket| running.iter().find(|line| line.ticket == Some(ticket)))
        .or_else(|| running.first())
}

/// The ticket Live follows: the one `l` chose, else the first instance's that runs, else the first
/// ticket whose phase runs.
pub fn followed_ticket(state: &ChairState, cursor: &Cursor) -> Option<TicketId> {
    cursor
        .live
        .or_else(|| state.now.running.iter().find_map(|line| line.ticket))
        .or_else(|| {
            state
                .epics
                .iter()
                .flat_map(|epic| &epic.squares)
                .find(|square| square.state == SquareState::Running)
                .map(|square| square.ticket)
        })
}

/// Whether `ticket` runs now: an instance runs on it, or one of its phases does.
pub fn runs(state: &ChairState, ticket: TicketId) -> bool {
    state
        .now
        .running
        .iter()
        .any(|line| line.ticket == Some(ticket))
        || super::detail::why::square(state, ticket)
            .is_some_and(|square| square.state == SquareState::Running)
}

/// `then` as an age before `now`: `3m05s ago`; `just now` for a time not before it.
fn ago(now: SystemTime, then: SystemTime) -> String {
    now.duration_since(then).map_or_else(
        |_| "just now".to_owned(),
        |took| format!("{} ago", age(took.as_secs())),
    )
}

/// The line under the title: what runs, or how the last run ended.
fn about(state: &ChairState, ticket: TicketId, read: Option<&TicketRead>) -> Option<String> {
    if let Some(line) = state
        .now
        .running
        .iter()
        .find(|line| line.ticket == Some(ticket))
    {
        let mut about = Vec::new();
        about.extend(line.phase.clone());
        about.extend(line.run_seconds.map(age));
        about.extend(line.doing.clone());
        return Some(about.join(" · "));
    }
    let run = read.and_then(|read| read.run.as_ref())?;
    Some(if run.live {
        format!(
            "{} · {}",
            run.action,
            ago(state.gathered_at, run.started_at)
        )
    } else {
        let ended = run
            .exit
            .map_or_else(|| "ended".to_owned(), |code| format!("exit {code}"));
        format!(
            "last run: {}, {ended} · started {}",
            run.action,
            ago(state.gathered_at, run.started_at)
        )
    })
}

/// The run's log, as many of its last lines as `rows` holds, with "working" while it runs.
fn log_lines(view: &View<'_>, run: &RunTail, rows: usize, cells: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(
        view.paint(Meaning::Bold, fit(&format!(" {} log", run.action), cells)),
    )];
    let working = usize::from(run.live);
    let room = rows.saturating_sub(1 + working);
    let first = run.lines.len().saturating_sub(room);
    lines.extend(
        run.lines[first..]
            .iter()
            .map(|line| Line::from(View::plain(fit(&format!("   {line}"), cells)))),
    );
    if run.live {
        lines.push(Line::from(vec![
            View::plain("   "),
            view.paint(Meaning::Verb, view.spinner()),
            view.paint(Meaning::Dim, " working"),
        ]));
    }
    if run.lines.is_empty() && !run.live {
        lines.push(Line::from(
            view.paint(Meaning::Dim, fit("   its log holds nothing", cells)),
        ));
    }
    lines
}

impl Pane for Live {
    fn title(&self, view: &View<'_>) -> Title {
        let Some(state) = view.state else {
            return Title::new("Live");
        };
        match followed_ticket(state, view.cursor) {
            Some(ticket) => {
                let word = if runs(state, ticket) {
                    "running"
                } else {
                    super::detail::why::square(state, ticket)
                        .map_or("", |square| square.state.word())
                };
                Title {
                    text: "Live".to_owned(),
                    after: vec![view.paint(Meaning::Noun, ticket.to_string())],
                    right: vec![view.paint(Meaning::Dim, word)],
                }
            }
            None => Title::new("Live"),
        }
    }

    fn body(&self, view: &View<'_>, columns: u16, rows: u16) -> Vec<Line<'static>> {
        let Some(state) = view.state else {
            return vec![view.reading()];
        };
        let Some(ticket) = followed_ticket(state, view.cursor) else {
            return vec![Line::from(view.paint(Meaning::Dim, "  nothing running"))];
        };
        let cells = usize::from(columns);
        let read = view.looks.ticket(ticket);
        let title = followed(state, view.cursor)
            .filter(|line| line.ticket == Some(ticket))
            .and_then(|line| line.title.clone())
            .or_else(|| {
                super::detail::why::square(state, ticket).map(|square| square.title.clone())
            })
            .unwrap_or_default();
        let symbol = if runs(state, ticket) {
            view.paint(Meaning::Verb, view.spinner())
        } else {
            super::detail::why::square(state, ticket)
                .map_or_else(|| View::plain(" "), |square| view.square(square.state))
        };
        let mut lines = vec![Line::from(vec![
            View::plain(" "),
            symbol,
            View::plain(" "),
            View::plain(fit(&title, cells.saturating_sub(4))),
        ])];
        if let Some(about) = about(state, ticket, read) {
            lines.push(Line::from(
                view.paint(Meaning::Dim, fit(&format!("   {about}"), cells)),
            ));
        }
        let Some(read) = read else {
            lines.push(Line::from(vec![
                View::plain("   "),
                view.paint(Meaning::Verb, view.spinner()),
                view.paint(
                    Meaning::Dim,
                    fit(" reading its reports and log…", cells.saturating_sub(4)),
                ),
            ]));
            return lines;
        };
        let left = usize::from(rows).saturating_sub(lines.len());
        // The reports get a third of the room (their title and a row at least), more when the
        // log needs less; the log takes the rest.
        let wanted_log = read
            .run
            .as_ref()
            .map_or(1, |run| run.lines.len().max(1) + 1 + usize::from(run.live));
        let report_room = if read.reports.is_empty() {
            0
        } else {
            let share = (left / 3).max(2);
            (read.reports.len() + 1).min(share.max(left.saturating_sub(wanted_log)))
        };
        if report_room >= 2 {
            lines.push(Line::from(view.paint(Meaning::Bold, " reports")));
            let shown = (report_room - 1).min(read.reports.len());
            for report in &read.reports[read.reports.len() - shown..] {
                // How long ago, who, what: as much of what as the width leaves.
                let when = state
                    .gathered_at
                    .duration_since(report.at)
                    .map_or_else(|_| "now".to_owned(), |took| age(took.as_secs()));
                let worker = fit(&report.worker, cells.saturating_sub(4 + width(&when)));
                let used = 3 + width(&when) + 1 + width(&worker);
                lines.push(Line::from(vec![
                    view.paint(Meaning::Dim, format!("   {when} ")),
                    view.paint(Meaning::Noun, worker),
                    View::plain(fit(
                        &format!(" {}", report.what),
                        cells.saturating_sub(used),
                    )),
                ]));
            }
        }
        let left = usize::from(rows).saturating_sub(lines.len());
        match &read.run {
            Some(run) if left > 0 => lines.extend(log_lines(view, run, left, cells)),
            Some(_) => {}
            None => lines.push(Line::from(
                view.paint(Meaning::Dim, fit("   no phase of it has run", cells)),
            )),
        }
        lines
    }

    fn key(&self, _key: Key, _context: Context<'_>, _cursor: &mut Cursor) -> Effect {
        Effect::Ignored
    }

    fn hints(&self, _view: &View<'_>) -> Vec<Hint> {
        vec![
            hint("w", "why"),
            hint("Tab", "next pane"),
            hint("q", "quit"),
        ]
    }
}
