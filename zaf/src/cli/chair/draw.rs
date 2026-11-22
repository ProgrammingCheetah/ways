//! Draws the chair into a frame: the tab bar and its rule, the panes and their borders, the status
//! line, the key bar and the palette, as the mock lays them out. Only reads the [`Chair`];
//! ratatui's diffing sends the terminal only the cells that changed.

use std::time::{Instant, SystemTime};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::layout::PaneId;
use super::model::{Chair, MIN_COLUMNS, MIN_ROWS, Mode, Status};
use super::palette;
use super::panes::{Tone, first_shown, pane};
use super::text::{fit, width};
use crate::app::chair::ChairState;
use crate::cli::steps::FRAMES;
use crate::domain::screen::Meaning;

/// The palette's width.
const PALETTE_WIDTH: u16 = 40;

/// Draws `chair` into `frame` at `now` (`wall` on the clock, for the state's age).
pub fn draw(chair: &Chair, frame: &mut Frame<'_>, now: Instant, wall: SystemTime) {
    let area = frame.area();
    draw_into(chair, frame.buffer_mut(), area, now, wall);
}

/// Draws `chair` into `buffer` over `area`.
pub fn draw_into(chair: &Chair, buffer: &mut Buffer, area: Rect, now: Instant, wall: SystemTime) {
    if !chair.fits() {
        let words = format!(
            " zaf needs at least {MIN_COLUMNS}×{MIN_ROWS}; this terminal is {}×{}.",
            area.width, area.height
        );
        buffer.set_string(
            area.x,
            area.y,
            fit(&words, usize::from(area.width)),
            Style::new(),
        );
        return;
    }
    let state = chair.shown();
    tab_bar(chair, buffer, area, state.as_ref());
    let dim = chair.theme.style(Meaning::Dim);
    let panes_area = chair.panes_area();
    for (x, y, glyph) in chair.layout.borders(panes_area) {
        if let Some(cell) = buffer.cell_mut((x, y)) {
            cell.set_char(glyph).set_style(dim);
        }
    }
    for (id, rect) in chair.layout.panes(panes_area) {
        draw_pane(chair, buffer, id, rect, state.as_ref());
    }
    status_line(chair, buffer, area, now, wall);
    key_bar(chair, buffer, area);
    if let Mode::Palette(selected) = chair.mode {
        palette(chair, buffer, area, selected);
    }
}

/// Writes `spans` at `(x, y)`, cut at `width` cells.
fn put(buffer: &mut Buffer, x: u16, y: u16, spans: Vec<Span<'static>>, width: u16) {
    buffer.set_line(x, y, &Line::from(spans), width);
}

/// The cells `spans` take.
fn cells(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| width(&span.content)).sum()
}

/// `left`, and `right` flush with the end of `width` when both fit with a space between.
fn joined(left: Vec<Span<'static>>, right: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let (used, more) = (cells(&left), cells(&right));
    if right.is_empty() || used + 1 + more > width {
        return left;
    }
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(width - used - more)));
    spans.extend(right);
    spans
}

fn tab_bar(chair: &Chair, buffer: &mut Buffer, area: Rect, state: Option<&ChairState>) {
    let theme = chair.theme;
    let mut left = vec![
        Span::raw(" "),
        Span::styled("zaf", theme.style(Meaning::Program)),
        Span::raw("  "),
    ];
    for (index, tab) in chair.tabs.iter().enumerate() {
        let style = if index == chair.active {
            theme.current()
        } else {
            theme.style(Meaning::Dim)
        };
        left.push(Span::styled(tab.name.to_string(), style));
        let badge = chair.badge(index);
        if badge > 0 {
            left.push(Span::raw(" "));
            left.push(Span::styled(
                format!("?{badge}"),
                theme.style(Meaning::Waiting),
            ));
        }
        left.push(Span::raw("   "));
    }
    if let Some(pane) = chair.layout.maximised() {
        left.push(Span::styled(
            format!(" max: {} ", pane.name()),
            theme.reverse(),
        ));
    }
    let spinner = FRAMES[chair.frame % FRAMES.len()];
    let right = match state.and_then(|state| state.now.watch.as_ref()) {
        Some(watch) if watch.running => vec![
            Span::styled(
                watch
                    .version
                    .as_ref()
                    .map_or_else(|| "?".to_owned(), ToString::to_string),
                theme.style(Meaning::Noun),
            ),
            Span::raw(" "),
            Span::styled(spinner, theme.style(Meaning::Verb)),
            Span::raw(" building "),
            Span::styled("·", theme.style(Meaning::Dim)),
            Span::raw(format!(
                " {} running ",
                state.map_or(0, |state| state.now.running.len())
            )),
        ],
        _ if state.is_none() => vec![Span::styled("reading… ", theme.style(Meaning::Dim))],
        _ => vec![Span::styled("nothing building ", theme.style(Meaning::Dim))],
    };
    put(
        buffer,
        area.x,
        area.y,
        joined(left, right, usize::from(area.width)),
        area.width,
    );
}

fn draw_pane(
    chair: &Chair,
    buffer: &mut Buffer,
    id: PaneId,
    rect: Rect,
    state: Option<&ChairState>,
) {
    let view = chair.view(state, id);
    let theme = chair.theme;
    let pane = pane(id);
    let title = pane.title(&view);
    let mut left = if view.focused {
        vec![
            Span::styled("▌ ", theme.style(Meaning::Verb)),
            Span::styled(title.text, theme.style(Meaning::Verb)),
        ]
    } else {
        vec![Span::raw("  "), Span::styled(title.text, theme.bold())]
    };
    if !title.after.is_empty() {
        left.push(Span::raw(" "));
        left.extend(title.after);
    }
    let mut right = title.right;
    if !right.is_empty() {
        right.push(Span::raw(" "));
    }
    put(
        buffer,
        rect.x,
        rect.y,
        joined(left, right, usize::from(rect.width)),
        rect.width,
    );
    let rows = rect.height.saturating_sub(1);
    let body = Rect {
        y: rect.y + 1,
        height: rows,
        ..rect
    };
    if pane.draw_body(&view, body, buffer) {
        return;
    }
    for (offset, line) in pane
        .body(&view, rect.width, rows)
        .into_iter()
        .take(usize::from(rows))
        .enumerate()
    {
        let y = rect.y + 1 + u16::try_from(offset).unwrap_or(0);
        buffer.set_line(rect.x, y, &line, rect.width);
    }
}

/// `spans` cut to `width` cells, the last one kept ended with `…` when any was cut.
fn cut(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    if cells(&spans) <= width {
        return spans;
    }
    let mut kept = Vec::new();
    let mut used = 0;
    for span in spans {
        let size = super::text::width(&span.content);
        if used + size < width {
            used += size;
            kept.push(span);
            continue;
        }
        let content = fit(&span.content, width - used);
        kept.push(Span::styled(content, span.style));
        break;
    }
    kept
}

fn status_line(chair: &Chair, buffer: &mut Buffer, area: Rect, now: Instant, wall: SystemTime) {
    let theme = chair.theme;
    let y = area.bottom().saturating_sub(2);
    let paint = |meaning: Meaning, text: &str| Span::styled(text.to_owned(), theme.style(meaning));
    let tab = chair.tab();
    // The age of what is shown, and the spinner while a read runs: both when both are true.
    let mut right = Vec::new();
    if tab.reading.is_some() {
        right.push(paint(Meaning::Verb, FRAMES[chair.frame % FRAMES.len()]));
        right.push(Span::raw(" "));
    }
    match &tab.state {
        Some(state) => {
            let seconds = wall
                .duration_since(state.gathered_at)
                .map_or(0, |age| age.as_secs());
            right.push(paint(Meaning::Dim, &format!("read {seconds}s ago ")));
        }
        None if tab.reading.is_some() => right.push(paint(Meaning::Dim, "reading ")),
        None => {}
    }
    // The state's age always shows; what is said gives way.
    let room = usize::from(area.width).saturating_sub(cells(&right) + 1);
    let mut left = vec![Span::raw(" ")];
    match chair.status(now) {
        Status::Pending {
            words,
            left: seconds,
            more,
        } => {
            left.extend([
                paint(Meaning::Done, "✓"),
                Span::raw(format!(" {words} ")),
                paint(Meaning::Dim, "·"),
                Span::raw(" "),
                paint(Meaning::Verb, "u"),
                Span::raw(" to undo "),
                paint(Meaning::Dim, &format!("({seconds}s)")),
            ]);
            if more > 0 {
                left.push(paint(Meaning::Dim, &format!(" · {more} more wait")));
            }
        }
        Status::Note(note) => {
            let symbol = match note.tone {
                Tone::Done => Some(paint(Meaning::Done, "✓ ")),
                Tone::Failed => Some(paint(Meaning::Failed, "✗ ")),
                Tone::Undone => Some(Span::raw("↶ ")),
                Tone::Plain => None,
            };
            left.extend(symbol);
            left.push(if note.tone == Tone::Plain {
                paint(Meaning::Dim, &note.words)
            } else {
                Span::raw(note.words.clone())
            });
        }
        Status::Ask(ask) => left.extend(super::build::spans(ask, theme)),
        Status::Phase(question) => left.extend(super::panes::phase::spans(&question, theme)),
        Status::Idle => {
            let mut reminders = vec![
                vec![paint(Meaning::Dim, "Closing zaf never stops the build")],
                vec![paint(Meaning::Verb, ":"), paint(Meaning::Dim, " actions")],
                vec![paint(Meaning::Verb, "^w"), paint(Meaning::Dim, " windows")],
            ];
            if chair.tabs.len() > 1 {
                reminders.push(vec![
                    paint(Meaning::Verb, &format!("1-{}", chair.tabs.len().min(9))),
                    paint(Meaning::Dim, " projects"),
                ]);
            }
            for (index, reminder) in reminders.into_iter().enumerate() {
                let mut next = left.clone();
                if index > 0 {
                    next.push(paint(Meaning::Dim, " · "));
                }
                next.extend(reminder);
                if cells(&next) > room {
                    break;
                }
                left = next;
            }
        }
    }
    put(
        buffer,
        area.x,
        y,
        joined(cut(left, room), right, usize::from(area.width)),
        area.width,
    );
}

fn key_bar(chair: &Chair, buffer: &mut Buffer, area: Rect) {
    let theme = chair.theme;
    let mut spans = vec![Span::raw(" ")];
    for hint in chair.hints() {
        spans.push(Span::styled(hint.key, theme.style(Meaning::Verb)));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(hint.what, theme.style(Meaning::Dim)));
        spans.push(Span::raw("  "));
    }
    put(
        buffer,
        area.x,
        area.bottom().saturating_sub(1),
        spans,
        area.width,
    );
}

/// A border row of the palette, [`PALETTE_WIDTH`] wide: `left`, a dash, `label`, a rule, `aside`,
/// a dash, `right`.
fn rule(
    corners: (char, char),
    label: Option<Span<'static>>,
    aside: Option<Span<'static>>,
    style: Style,
) -> Vec<Span<'static>> {
    let words = label
        .iter()
        .chain(aside.iter())
        .map(|span| width(&span.content));
    let fill = usize::from(PALETTE_WIDTH).saturating_sub(4 + words.sum::<usize>());
    let mut spans = vec![Span::styled(format!("{}─", corners.0), style)];
    spans.extend(label);
    spans.push(Span::styled("─".repeat(fill), style));
    spans.extend(aside);
    spans.push(Span::styled(format!("─{}", corners.1), style));
    spans
}

/// The palette: a box over the panes listing every action, scrolled so the selected one shows,
/// with how many more are above or below on its borders.
fn palette(chair: &Chair, buffer: &mut Buffer, area: Rect, selected: usize) {
    let theme = chair.theme;
    let verb = theme.style(Meaning::Verb);
    let dim = theme.style(Meaning::Dim);
    let x = area.x + area.width.saturating_sub(PALETTE_WIDTH) / 2;
    let y = area.y + 4;
    let count = palette::len();
    // The entries reach down to the row above the bottom border, which sits above the status line.
    let room = usize::from(area.bottom().saturating_sub(4).saturating_sub(y)).max(1);
    let shown = count.min(room);
    let first = first_shown(selected, shown);
    let more = |arrow: &str, many: usize| {
        (many > 0).then(|| Span::styled(format!(" {arrow} {many} more "), dim))
    };
    let heading = Some(Span::styled(" actions ", theme.bold()));
    put(
        buffer,
        x,
        y,
        rule(('┌', '┐'), heading, more("↑", first), verb),
        PALETTE_WIDTH,
    );
    for (row, (index, entry)) in palette::entries()
        .enumerate()
        .skip(first)
        .take(shown)
        .enumerate()
    {
        let text = format!(" {:<28} {:>7} ", fit(entry.name, 28), entry.key);
        let style = if index == selected {
            theme.reverse()
        } else {
            Style::new()
        };
        put(
            buffer,
            x,
            y + 1 + u16::try_from(row).unwrap_or(0),
            vec![
                Span::styled("│", verb),
                Span::styled(text, style),
                Span::styled("│", verb),
            ],
            PALETTE_WIDTH,
        );
    }
    put(
        buffer,
        x,
        y + 1 + u16::try_from(shown).unwrap_or(0),
        rule(('└', '┘'), None, more("↓", count - first - shown), verb),
        PALETTE_WIDTH,
    );
}

#[cfg(test)]
mod tests;
