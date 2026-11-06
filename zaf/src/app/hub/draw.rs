//! Draws a [`Snapshot`] as a frame of text. Pure: it reads no state and no clock.

use std::time::Duration;

use super::{EpicRow, GroupRow, Snapshot, TicketRow};
use crate::app::actions::Phase;

/// The columns the label before a bar takes, so the bars line up.
const LABEL_COLUMNS: usize = 28;
/// The cells of a bar when the line has room.
const BAR_CELLS: usize = 30;
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
    lines.push(row(
        &format!("epic #{}", epic.number),
        (epic.done, epic.total),
        &tail,
        width,
    ));
    lines.push(format!(
        "  {} of {} slots free · {} ready · {}",
        epic.slots_free,
        epic.slots_total,
        epic.ready,
        questions(epic.questions)
    ));
    for ticket in &epic.tickets {
        lines.push(ticket_line(ticket, width));
        model_lines(ticket, width, lines);
    }
    for group in &epic.groups {
        group_lines(group, width, lines);
    }
}

fn group_lines(group: &GroupRow, width: usize, lines: &mut Vec<String>) {
    lines.push(String::new());
    lines.push(row(
        &format!("group: {}", group.name),
        (group.done, group.total),
        &format!("{} of {}", group.done, group.total),
        width,
    ));
    for ticket in &group.tickets {
        lines.push(ticket_line(ticket, width));
        model_lines(ticket, width, lines);
    }
}

fn ticket_line(ticket: &TicketRow, width: usize) -> String {
    let step = phase_step(ticket.phase);
    let since = ticket.since.map_or_else(|| "-".to_owned(), age);
    let tail = format!(
        "{:<9}  {}  {}",
        ticket.phase.to_string(),
        ticket.word,
        since
    );
    row(
        &format!("  #{}  {}", ticket.number, ticket.who),
        (u32::try_from(step).unwrap_or(0), PHASE_STEPS as u32),
        &tail,
        width,
    )
}

/// The rows under a ticket, one per model that declared phases: its role, a bar over the phases
/// it declared and the phase it is in, `write ×2` from the second time it enters it.
fn model_lines(ticket: &TicketRow, width: usize, lines: &mut Vec<String>) {
    for model in &ticket.models {
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
            &format!("    {}", model.role),
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

/// One line: the label, a bar of `done` of `of`, then the tail. The bar shrinks to fit `width`.
fn row(label: &str, (done, of): (u32, u32), tail: &str, width: usize) -> String {
    let label = format!("{label:<LABEL_COLUMNS$}");
    // Two spaces, the brackets, two spaces: the line's fixed width beside the bar's cells.
    let fixed = label.chars().count() + 2 + 2 + 2 + tail.chars().count();
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
