//! An epic's minutes of work and its minutes on questions, from its start, its end and the
//! intervals its questions were open.
//!
//! [`EpicTimes::minutes`] is a pure calculation: no board, no files. Each question interval is
//! clipped to the window from start to end, the clipped intervals are united so that time two
//! questions share counts once, and the work is the window less that union. A question still
//! open counts until the end.
//!
//! [`epic_times`] gathers the three facts the calculation reads: the start from the phase run
//! folders, the end from the board's close times and the question intervals from the board's
//! questions. An epic whose facts are not all there is left out, with the reason.
//!
//! [`times_view`] is `zaf report --times`: the epics built before graph planning (a 0.5.x release,
//! no plan record) against the epics planned as a graph (a plan record), each row's average and
//! median minutes of work per planned task, whether the graph row is lower, and every epic left
//! out with its reason.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, SystemTime};

use super::epics::Verdict;
use super::median;
use crate::app::schedule::record::read_runs;
use crate::domain::plan::NodeKind;
use crate::domain::plan::text::parse_plan;
use crate::domain::screen::{self, Line, Meaning, Text, clean};
use crate::domain::{Epic, EpicRecord, Standing, TaskBody, TicketId};
use crate::ports::{Board, PortError};

/// When one question was open: from `opened` to `closed`, or still open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Question {
    /// When the question was raised.
    pub opened: SystemTime,
    /// When it was answered, `None` while it is open.
    pub closed: Option<SystemTime>,
}

/// An epic's window and the questions that were open in it: `EpicTimes { start, end, questions }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicTimes {
    /// When the epic's work began.
    pub start: SystemTime,
    /// When the epic's work ended.
    pub end: SystemTime,
    /// Every question the epic's tasks raised.
    pub questions: Vec<Question>,
}

/// What gathering an epic's times found: its times, or the reason it is left out of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Timed {
    /// The epic's start, end and question intervals.
    EpicTimes(EpicTimes),
    /// The epic is left out, for this reason.
    LeftOut(String),
}

/// The reason for an epic none of whose tasks has a phase run folder.
pub const NO_PHASE_RECORD: &str = "no phase record";

/// `epic_times(board, phases_dir, epic) -> Result<Timed, PortError>`: the epic's start, end and
/// question intervals.
///
/// - The start is the earliest started time of any phase run folder in `phases_dir` whose `task`
///   file names one of the epic's tasks.
/// - The end is the latest close time of the epic's tasks on the board.
/// - The questions are every question on the epic and on each of its tasks: each one's opened
///   time ([`Board::opened_at`]), and its standing time ([`Board::standing_at`]) when closed.
///
/// An epic with no phase run folder for any task, with a task still open, with no task close
/// time, or with a question the board gives no opened time (or, closed, no close time) is
/// [`Timed::LeftOut`] with its reason. A board or a folder that cannot be read is an error.
pub fn epic_times(board: &dyn Board, phases_dir: &Path, epic: &Epic) -> Result<Timed, PortError> {
    let mut start: Option<SystemTime> = None;
    for task in &epic.tasks {
        let runs = read_runs(phases_dir, task.number, &|_| false).map_err(PortError::Backend)?;
        // Oldest start first.
        if let Some(first) = runs.first() {
            start = Some(start.map_or(first.started_at, |at| at.min(first.started_at)));
        }
    }
    let Some(start) = start else {
        return Ok(Timed::LeftOut(NO_PHASE_RECORD.to_owned()));
    };
    if let Some(open) = epic
        .tasks
        .iter()
        .find(|task| task.standing == Standing::Open)
    {
        return Ok(Timed::LeftOut(format!("{} is still open", open.number)));
    }
    let Some(end) = epic.tasks.iter().filter_map(|task| task.closed_at).max() else {
        return Ok(Timed::LeftOut("no task close time".to_owned()));
    };
    let mut questions = Vec::new();
    let raised_on = std::iter::once(epic.id).chain(epic.tasks.iter().map(|task| task.number));
    for on in raised_on {
        for (ticket, _) in board.questions_on(on)? {
            match interval(board, ticket.id)? {
                Ok(found) => questions.push(found),
                Err(reason) => return Ok(Timed::LeftOut(reason)),
            }
        }
    }
    Ok(Timed::EpicTimes(EpicTimes {
        start,
        end,
        questions,
    }))
}

/// One question's interval, or the reason the board's answer cannot make one.
fn interval(board: &dyn Board, number: TicketId) -> Result<Result<Question, String>, PortError> {
    let Some(opened) = board.opened_at(number)? else {
        return Ok(Err(format!("question {number} has no opened time")));
    };
    let standing = board.standing_at(number)?;
    let closed = match (standing.standing, standing.closed_at) {
        (Standing::Open, _) => None,
        (_, Some(at)) => Some(at),
        (_, None) => return Ok(Err(format!("question {number} has no close time"))),
    };
    Ok(Ok(Question { opened, closed }))
}

/// Where an epic's time went, in minutes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Minutes {
    /// The window less the time on questions.
    pub work: f64,
    /// The union of the question intervals inside the window.
    pub questions: f64,
}

impl EpicTimes {
    /// `minutes() -> Minutes { work, questions }`: the minutes of work and the minutes on
    /// questions, overlapping question time counted once.
    pub fn minutes(&self) -> Minutes {
        let window = self.end.duration_since(self.start).unwrap_or_default();
        let mut spans: Vec<(SystemTime, SystemTime)> = self
            .questions
            .iter()
            .filter_map(|question| {
                let from = question.opened.max(self.start);
                let to = question.closed.unwrap_or(self.end).min(self.end);
                (from < to).then_some((from, to))
            })
            .collect();
        spans.sort();
        let mut asked = Duration::ZERO;
        let mut current: Option<(SystemTime, SystemTime)> = None;
        for (from, to) in spans {
            match current {
                Some((begin, finish)) if from <= finish => current = Some((begin, finish.max(to))),
                Some((begin, finish)) => {
                    asked += finish.duration_since(begin).unwrap_or_default();
                    current = Some((from, to));
                }
                None => current = Some((from, to)),
            }
        }
        if let Some((begin, finish)) = current {
            asked += finish.duration_since(begin).unwrap_or_default();
        }
        Minutes {
            work: minutes(window.saturating_sub(asked)),
            questions: minutes(asked),
        }
    }
}

/// The minutes of work per planned task of an epic with `times`; none with no task planned.
pub fn minutes_per_planned(epic: &Epic, times: &EpicTimes) -> Option<f64> {
    let planned = planned_count(epic, &epic.records);
    (planned > 0).then(|| times.minutes().work / planned as f64)
}

/// `planned_count(epic, records) -> usize`: how many tasks the epic was planned to have.
///
/// For an epic planned as a graph, it is the `Task:` sections of every record's plan as written,
/// less the tasks the dropped tickets held ([`tasks_held`]), each ticket taken off once however
/// many records name it. For an epic with no record it is its tasks that count (see
/// `EpicTask::counts`): open or done, not closed as not planned. It reads; it files and closes
/// nothing.
pub fn planned_count(epic: &Epic, records: &[EpicRecord]) -> usize {
    if records.is_empty() {
        return epic.tasks.iter().filter(|task| task.counts()).count();
    }
    let written: usize = records
        .iter()
        .map(|entry| task_sections(&entry.record.plan))
        .sum();
    let dropped: BTreeSet<TicketId> = records
        .iter()
        .flat_map(|entry| entry.record.dropped.iter().copied())
        .collect();
    let taken_off: usize = dropped.iter().map(|ticket| tasks_held(epic, *ticket)).sum();
    written.saturating_sub(taken_off)
}

/// How many written tasks a dropped ticket held, so the count stays in written tasks: one for each
/// section of a ticket filed from a plan (two for a merged `A+B`), none for an interface ticket or
/// a ticket of the older template, which no plan's `Task:` section stands for. A ticket the epic
/// does not hold, or whose body does not read, counts as the one task it most likely was.
fn tasks_held(epic: &Epic, ticket: TicketId) -> usize {
    let body = epic
        .tasks
        .iter()
        .find(|task| task.number == ticket)
        .map(|task| TaskBody::parse(task.body.as_str()));
    match body {
        Some(Ok(TaskBody::Planned(planned))) => match planned.kind {
            NodeKind::Interface => 0,
            NodeKind::Body => planned.sections.len(),
        },
        Some(Ok(TaskBody::Template(_))) => 0,
        Some(Err(_)) | None => 1,
    }
}

/// The `Task:` sections of a plan as written: what the plan's parse reads, and for a plan that
/// no longer parses the lines that open a section.
fn task_sections(plan: &str) -> usize {
    match parse_plan(plan) {
        Ok(parsed) => parsed.sections.len(),
        Err(_) => plan
            .lines()
            .filter(|line| line.starts_with("Task:"))
            .count(),
    }
}

/// The reason for an epic of release 0.6.0 that no plan record planned.
pub const OLD_WAY: &str = "0.6.0's own, built the old way";

/// The reason for an epic whose body names no release.
pub const NO_RELEASE: &str = "no release";

/// The reason for an epic of a release other than 0.5.x and 0.6.0 that no plan record planned.
pub const OTHER_RELEASE: &str = "a release other than 0.5.x and 0.6.0, built the old way";

/// The reason for an epic planned to have no task, which has no minutes per planned task.
pub const NONE_PLANNED: &str = "no task planned";

/// One row of the comparison: the epics sorted into it and their minutes of work per planned task.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TimesRow {
    /// How many epics are in the row.
    pub count: usize,
    /// The mean over them; none when the row has no epic.
    pub average: Option<f64>,
    /// The median over them; none when the row has no epic.
    pub median: Option<f64>,
}

impl TimesRow {
    fn of(values: Vec<f64>) -> Self {
        Self {
            count: values.len(),
            average: (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64),
            median: (!values.is_empty()).then(|| median(values)),
        }
    }
}

/// An epic the comparison leaves out, and why.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LeftOut {
    /// The epic's issue number.
    pub epic: u64,
    /// Why it is not in either row.
    pub reason: String,
}

/// `zaf report --times`: the 0.5.x row, the graph row, whether the graph row is lower and what
/// was left out.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct TimesView {
    /// The epics of a 0.5 release with no plan record.
    pub baseline: TimesRow,
    /// The epics with a plan record.
    pub graph: TimesRow,
    /// Whether the graph row's average is lower; none when either row has no epic.
    pub lower_average: Option<Verdict>,
    /// Whether the graph row's median is lower; none when either row has no epic.
    pub lower_median: Option<Verdict>,
    /// Every epic in neither row, in epic order.
    pub left_out: Vec<LeftOut>,
}

/// Whether `version` is a 0.5 release: `0.5` or `0.5.<anything>`.
fn is_baseline(version: &str) -> bool {
    let mut parts = version.split('.');
    parts.next() == Some("0") && parts.next() == Some("5")
}

/// Which row an epic is in, or why it is in none, before its times are read.
enum Place {
    Baseline,
    Graph,
    Out(&'static str),
}

fn sorted(epic: &Epic) -> Place {
    if !epic.records.is_empty() {
        return Place::Graph;
    }
    match epic.release() {
        Some(version) if is_baseline(version) => Place::Baseline,
        Some("0.6.0") => Place::Out(OLD_WAY),
        Some(_) => Place::Out(OTHER_RELEASE),
        None => Place::Out(NO_RELEASE),
    }
}

/// `times_view(board, phases_dir) -> Result<TimesView, PortError>`: every open epic on the board
/// sorted into the 0.5.x row (its `Release:` line names a 0.5 version and it has no plan record),
/// the graph row (it has a plan record) or the left-out list.
///
/// A 0.6.0 epic with no plan record, one of another release and one of no release are left out
/// before their times are read; any other epic is left out with the reason [`epic_times`] gives
/// (no phase record, a task still open, ...) or [`NONE_PLANNED`]. A board or a folder that cannot
/// be read is an error.
pub fn times_view(board: &dyn Board, phases_dir: &Path) -> Result<TimesView, PortError> {
    let (mut baseline, mut graph) = (Vec::new(), Vec::new());
    let mut left_out = Vec::new();
    for epic in board.epics()? {
        let row = match sorted(&epic) {
            Place::Out(reason) => {
                left_out.push(LeftOut {
                    epic: epic.id.get(),
                    reason: reason.to_owned(),
                });
                continue;
            }
            Place::Baseline => &mut baseline,
            Place::Graph => &mut graph,
        };
        let reason = match epic_times(board, phases_dir, &epic)? {
            Timed::LeftOut(reason) => reason,
            Timed::EpicTimes(times) => match minutes_per_planned(&epic, &times) {
                Some(value) => {
                    row.push(value);
                    continue;
                }
                None => NONE_PLANNED.to_owned(),
            },
        };
        left_out.push(LeftOut {
            epic: epic.id.get(),
            reason,
        });
    }
    let baseline = TimesRow::of(baseline);
    let graph = TimesRow::of(graph);
    let versus = |graph: Option<f64>, baseline: Option<f64>| match (graph, baseline) {
        (Some(graph), Some(baseline)) if graph < baseline => Some(Verdict::Lower),
        (Some(_), Some(_)) => Some(Verdict::NotLower),
        _ => None,
    };
    Ok(TimesView {
        lower_average: versus(graph.average, baseline.average),
        lower_median: versus(graph.median, baseline.median),
        baseline,
        graph,
        left_out,
    })
}

impl TimesView {
    /// The two rows, the verdicts, then the left-out list, one epic and its reason a line.
    pub fn lines(&self) -> Vec<Line> {
        let plain = |text: String| vec![(Meaning::Plain, Text::from(text))];
        let none = || "-".to_owned();
        let figure = |value: Option<f64>| value.map_or_else(none, |v| format!("{v:.1}"));
        let mut lines = vec![
            plain("minutes of work per planned task".to_owned()),
            vec![(
                Meaning::Bold,
                Text::from(format!(
                    "{:<8} {:>5} {:>8} {:>8}",
                    "row", "epics", "average", "median"
                )),
            )],
        ];
        for (name, row) in [("0.5.x", &self.baseline), ("graph", &self.graph)] {
            lines.push(plain(format!(
                "{:<8} {:>5} {:>8} {:>8}",
                name,
                row.count,
                figure(row.average),
                figure(row.median)
            )));
        }
        lines.push(plain(match (self.lower_average, self.lower_median) {
            (Some(average), Some(median)) => format!(
                "graph against 0.5.x: average {}, median {}",
                average.word(),
                median.word()
            ),
            _ => "graph against 0.5.x: no verdict, a row has no epic".to_owned(),
        }));
        if !self.left_out.is_empty() {
            lines.push(plain("left out:".to_owned()));
        }
        for out in &self.left_out {
            lines.push(plain(format!("#{} {}", out.epic, clean(&out.reason))));
        }
        lines
    }

    /// The view as text with no colour.
    pub fn render_text(&self) -> String {
        screen::plain(&self.lines())
    }

    /// The view as JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

fn minutes(span: Duration) -> f64 {
    span.as_secs_f64() / 60.0
}

#[cfg(test)]
mod tests;
