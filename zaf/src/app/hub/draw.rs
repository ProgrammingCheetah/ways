//! Draws a [`Snapshot`] as a frame of text. Pure: it reads no state and no clock.

use std::time::Duration;

use super::{EpicRow, GroupRow, ModelRow, Snapshot, TicketRow};
use crate::app::actions::Phase;

/// The columns the label before a bar takes, so the bars line up.
const LABEL_COLUMNS: usize = 28;
/// The cells of a bar when the line has room.
const BAR_CELLS: usize = 30;
/// Two spaces, the brackets, two spaces: the line's fixed width beside the bar's cells.
const BAR_FIXED: usize = 2 + 2 + 2;
/// The fewest cells a bar shrinks to before the line is cut instead.
const MIN_BAR_CELLS: usize = 4;
/// The steps of a ticket's bar: the seven Team Phases.
const PHASE_STEPS: usize = Phase::DECLARABLE.len();

/// Draws the frame: every epic, its tickets and its groups, no line wider than `width`
/// characters. Each line ends in a newline; a snapshot with no epic says so.
pub fn draw(snapshot: &Snapshot, width: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (index, epic) in snapshot.epics.iter().enumerate() {
        if index > 0 {
            lines.push(String::new());
        }
        epic_lines(epic, width, &mut lines);
    }
    if lines.is_empty() {
        lines.push("no open epics".to_owned());
    }
    let mut frame = String::new();
    for line in lines {
        frame.push_str(cut(&line, width).trim_end());
        frame.push('\n');
    }
    frame
}

fn epic_lines(epic: &EpicRow, width: usize, lines: &mut Vec<String>) {
    let tail = format!("{} of {}  ${:.2}", epic.done, epic.total, epic.dollars);
    let label = titled(
        &format!("epic #{}", epic.number),
        &epic.title,
        label_room(width, &tail),
    );
    lines.push(row(
        &label,
        LABEL_COLUMNS,
        (epic.done, epic.total),
        &tail,
        width,
    ));
    model_lines(&epic.models, "  ", width, lines);
    // Every ticket label of the epic is as wide as the longest, so the bars line up.
    let tickets = || {
        epic.tickets
            .iter()
            .chain(epic.groups.iter().flat_map(|group| &group.tickets))
    };
    let columns = tickets()
        .map(|ticket| ticket_label(ticket).chars().count())
        .max()
        .unwrap_or(0);
    lines.push(format!(
        "  {} · {} of {} slots free · {} ready · {}",
        epic.stage,
        epic.slots_free,
        epic.slots_total,
        epic.ready,
        questions(epic.questions)
    ));
    for ticket in &epic.tickets {
        lines.push(ticket_line(ticket, columns, width));
        model_lines(&ticket.models, "    ", width, lines);
    }
    for group in &epic.groups {
        group_lines(group, columns, width, lines);
    }
}

fn group_lines(group: &GroupRow, columns: usize, width: usize, lines: &mut Vec<String>) {
    lines.push(String::new());
    lines.push(row(
        &format!("group: {}", group.name),
        LABEL_COLUMNS,
        (group.done, group.total),
        &format!("{} of {}", group.done, group.total),
        width,
    ));
    for ticket in &group.tickets {
        lines.push(ticket_line(ticket, columns, width));
        model_lines(&ticket.models, "    ", width, lines);
    }
}

/// A ticket's label whole: its id, then its title.
fn ticket_label(ticket: &TicketRow) -> String {
    format!("  #{}  {}", ticket.number, ticket.title)
}

/// One ticket's line. Its label, the id then the title, is `columns` wide at most, and the title
/// is cut with an ellipsis when the line has no room for it.
fn ticket_line(ticket: &TicketRow, columns: usize, width: usize) -> String {
    let step = phase_step(ticket.phase);
    let since = ticket.since.map_or_else(|| "-".to_owned(), age);
    let tail = format!(
        "{}  {:<9}  {}  {}",
        ticket.who,
        ticket.phase.to_string(),
        ticket.word,
        since
    );
    let room = label_room(width, &tail);
    let label = titled(&format!("  #{}", ticket.number), &ticket.title, room);
    row(
        &label,
        columns.min(room.max(label.chars().count())),
        (u32::try_from(step).unwrap_or(0), PHASE_STEPS as u32),
        &tail,
        width,
    )
}

/// The columns a label may take on a line of `width` with `tail` after the bar, the bar at its
/// least.
fn label_room(width: usize, tail: &str) -> usize {
    width.saturating_sub(BAR_FIXED + MIN_BAR_CELLS + tail.chars().count())
}

/// `head`, two spaces and `title`, the title cut with an ellipsis to fit `room` columns in all.
/// A title with no room left is left out, and `head` is never cut.
fn titled(head: &str, title: &str, room: usize) -> String {
    let left = room.saturating_sub(head.chars().count() + 2);
    if title.is_empty() || left == 0 {
        return head.to_owned();
    }
    format!("{head}  {}", fit(title, left))
}

/// `text` whole when it fits `room` characters, else cut to `room` ending in an ellipsis.
fn fit(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_owned();
    }
    let kept: String = text.chars().take(room.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// The rows under a ticket or an epic, one per model that declared phases on it, each led by
/// `indent`: its role, a bar over the phases it declared and the phase it is in, `write ×2` from
/// the second time it enters it.
fn model_lines(models: &[ModelRow], indent: &str, width: usize, lines: &mut Vec<String>) {
    for model in models {
        let step = model
            .current
            .as_ref()
            .and_then(|current| model.phases.iter().position(|phase| phase == current))
            .map_or(0, |index| index + 1);
        let tail = match (&model.current, model.iteration) {
            (None, _) => "-".to_owned(),
            (Some(current), 0 | 1) => current.clone(),
            (Some(current), times) => format!("{current} ×{times}"),
        };
        lines.push(row(
            &format!("{indent}{}", model.role),
            LABEL_COLUMNS,
            (
                u32::try_from(step).unwrap_or(0),
                u32::try_from(model.phases.len()).unwrap_or(u32::MAX),
            ),
            &tail,
            width,
        ));
    }
}

/// The ticket's step among the seven phases; `Unclaimed` is before the first.
fn phase_step(phase: Phase) -> usize {
    Phase::DECLARABLE
        .iter()
        .position(|declared| *declared == phase)
        .map_or(0, |index| index + 1)
}

fn questions(count: u32) -> String {
    match count {
        0 => "no questions".to_owned(),
        1 => "1 question".to_owned(),
        count => format!("{count} questions"),
    }
}

/// One line: the label padded to `columns`, a bar of `done` of `of`, then the tail. The bar
/// shrinks to fit `width`.
fn row(label: &str, columns: usize, (done, of): (u32, u32), tail: &str, width: usize) -> String {
    let label = format!("{label:<columns$}");
    let fixed = label.chars().count() + BAR_FIXED + tail.chars().count();
    let cells = width.saturating_sub(fixed).clamp(MIN_BAR_CELLS, BAR_CELLS);
    format!("{label}  [{}]  {tail}", bar_of(done, of, cells))
}

/// A bar of `cells`, `done` of `of` of it filled.
fn bar_of(done: u32, of: u32, cells: usize) -> String {
    let filled = if of == 0 {
        0
    } else {
        (cells * done as usize / of as usize).min(cells)
    };
    format!("{}{}", "#".repeat(filled), "-".repeat(cells - filled))
}

/// Time since a report as one unit: `40s`, `2m`, `3h`, `2d`.
fn age(since: Duration) -> String {
    let seconds = since.as_secs();
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}

/// The first `width` characters of a line.
fn cut(line: &str, width: usize) -> String {
    line.chars().take(width).collect()
}
