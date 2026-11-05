//! `zaf report --epics`: one row per epic, what it cost set beside what it finished.
//!
//! The rows come from the ledger, so an epic whose transcripts have expired still prints. A
//! run belongs to the epic of the task its row names, as the board has it; a run that names
//! no task, or only tasks the board knows no epic for, belongs to the epic its row holds. A
//! subagent already holds its parent session's epic in the ledger. Tasks done are read from
//! the board. Dollars are computed here from the price table, never stored, so a changed
//! price moves every row alike. Each row also holds the median dollars per done task by size
//! (S, M): a run that names tasks splits evenly across them, any other run across the epic's
//! done tasks, and a task with no readable size is in the epic's total but in no median. Each
//! row also compares its median per size with the last earlier epic's: lower or not, on at
//! least [`MIN_TASKS`] done tasks of that size on both sides, and `too few` otherwise, never a
//! verdict. The spend outside any epic is not a row: it is one line after the rows, so that
//! spend stays visible.

use std::collections::{BTreeMap, HashMap};

use super::ledger::LedgerRow;
use super::{PriceTable, median};
use crate::domain::{IssueNumber, TaskLabel, TaskSize, TaskTemplate};
use crate::ports::{Board, PortError};

/// What the text and the JSON say about every dollar figure.
pub const ESTIMATE_NOTE: &str = "estimates: dollars are API list prices applied to the ledger's tokens at reading, a proxy for weight against the weekly limit; compare rows, do not bill from them";

/// The done tasks of one size that each side of a comparison needs before it gives a verdict.
pub const MIN_TASKS: usize = 5;

/// The role of a run whose prompt gave none.
const NO_ROLE: &str = "unattributed";

/// Tokens by kind.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Tokens {
    /// Fresh input tokens.
    pub input: u64,
    /// Cache-write tokens.
    pub cache_write: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Output tokens.
    pub output: u64,
}

/// One role's part of an epic's estimated dollars.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RoleShare {
    /// The role, `unattributed` for a run whose prompt gave none.
    pub role: String,
    /// Estimated dollars.
    pub est_usd: f64,
    /// Percent of the epic's estimated dollars.
    pub share_percent: f64,
}

/// One row of `zaf report --epics`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EpicRow {
    /// The epic's issue number.
    pub epic: u64,
    /// Tasks of this epic the board has `done` among those the ledger saw work on.
    pub tasks_done: usize,
    /// Estimated dollars over everything the epic cost.
    pub est_usd: f64,
    /// Estimated dollars per task done; none with no task done.
    pub usd_per_task: Option<f64>,
    /// Tokens by kind.
    pub tokens: Tokens,
    /// Turns over every run.
    pub turns: u64,
    /// Turns per task done; none with no task done.
    pub turns_per_task: Option<f64>,
    /// Median context at turn one over the epic's subagent runs; none without one.
    pub context_first: Option<f64>,
    /// Median context at the last turn over the epic's subagent runs; none without one.
    pub context_last: Option<f64>,
    /// Each role's share of the estimated dollars, largest first.
    pub roles: Vec<RoleShare>,
    /// Median estimated dollars per done task, by size.
    pub median_usd: SizeMedians,
    /// How many done tasks each median is over.
    pub done_by_size: SizeCounts,
    /// The median per size against the last earlier epic's; none for the first epic.
    pub versus_last: SizeVerdicts,
}

/// How many done tasks of each size an epic has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct SizeCounts {
    /// Done tasks of size S.
    pub s: usize,
    /// Done tasks of size M.
    pub m: usize,
}

/// Whether an epic's median beat the last earlier epic's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Lower, on at least [`MIN_TASKS`] tasks of the size on both sides.
    Lower,
    /// Equal or higher, on at least [`MIN_TASKS`] tasks of the size on both sides.
    NotLower,
    /// Fewer than [`MIN_TASKS`] tasks of the size on either side: no verdict.
    TooFew,
}

impl Verdict {
    /// The word the text prints.
    pub fn word(self) -> &'static str {
        match self {
            Self::Lower => "lower",
            Self::NotLower => "not lower",
            Self::TooFew => "too few",
        }
    }
}

/// The verdict for each size, against the last earlier epic.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct SizeVerdicts {
    /// Size S; none when there is no earlier epic.
    pub s: Option<Verdict>,
    /// Size M; none when there is no earlier epic.
    pub m: Option<Verdict>,
}

/// One size's task count and median, now against the last earlier epic's.
fn verdict(now: (usize, Option<f64>), before: (usize, Option<f64>)) -> Verdict {
    match (now, before) {
        ((count, Some(now)), (earlier, Some(before)))
            if count >= MIN_TASKS && earlier >= MIN_TASKS =>
        {
            if now < before {
                Verdict::Lower
            } else {
                Verdict::NotLower
            }
        }
        _ => Verdict::TooFew,
    }
}

/// `row` against `last`, the nearest earlier epic's row.
fn compare(row: &EpicRow, last: &EpicRow) -> SizeVerdicts {
    SizeVerdicts {
        s: Some(verdict(
            (row.done_by_size.s, row.median_usd.s),
            (last.done_by_size.s, last.median_usd.s),
        )),
        m: Some(verdict(
            (row.done_by_size.m, row.median_usd.m),
            (last.done_by_size.m, last.median_usd.m),
        )),
    }
}

/// The median estimated dollars per done task, for each size.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize)]
pub struct SizeMedians {
    /// Over the done tasks of size S; none without one.
    pub s: Option<f64>,
    /// Over the done tasks of size M; none without one.
    pub m: Option<f64>,
}

/// What the board holds on a task: its epic, whether it is done and its size when the body
/// reads.
#[derive(Debug, Clone, Copy)]
struct Found {
    epic: IssueNumber,
    done: bool,
    size: Option<TaskSize>,
}

/// The done tasks of one epic that the ledger saw work on, each with its size when it reads.
type DoneTasks = [(u64, Option<TaskSize>)];

/// The epic of every task the rows name, as the board has it, and whether it is done.
struct Tasks<'a> {
    board: &'a dyn Board,
    seen: HashMap<u64, Option<Found>>,
}

impl<'a> Tasks<'a> {
    fn new(board: &'a dyn Board) -> Self {
        Self {
            board,
            seen: HashMap::new(),
        }
    }

    /// The task's epic, state and size; none for a task the board does not have or that has
    /// no epic. A body that does not read gives no size. Any other board failure is an error.
    fn look_up(&mut self, task: u64) -> Result<Option<Found>, PortError> {
        if let Some(known) = self.seen.get(&task) {
            return Ok(*known);
        }
        let found = match self.board.task(IssueNumber::from(task)) {
            Ok(ticket) => ticket.epic.map(|epic| Found {
                epic,
                done: ticket.label == Some(TaskLabel::Done),
                size: ticket
                    .body
                    .parse::<TaskTemplate>()
                    .ok()
                    .map(|template| template.size),
            }),
            Err(PortError::NotFound(_)) => None,
            Err(error) => return Err(error),
        };
        self.seen.insert(task, found);
        Ok(found)
    }
}

/// What ran outside any epic: the runs no task and no row put in one.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Outside {
    /// Runs.
    pub runs: usize,
    /// Turns over every run.
    pub turns: u64,
    /// Estimated dollars.
    pub est_usd: f64,
    /// Tokens by kind.
    pub tokens: Tokens,
}

/// The per-epic rows and the one line for what ran outside any epic.
#[derive(Debug, Clone, PartialEq)]
pub struct EpicView {
    /// One row per epic, in epic order.
    pub rows: Vec<EpicRow>,
    /// What ran outside any epic; none when every run belongs to an epic.
    pub outside: Option<Outside>,
}

/// One row per epic that has a run in `rows`, in epic order.
///
/// A board failure other than a missing issue is an error: a row with a guessed task count
/// would look like a measurement.
pub fn epic_rows(
    rows: &[LedgerRow],
    board: &dyn Board,
    prices: &PriceTable,
) -> Result<Vec<EpicRow>, PortError> {
    epic_view(rows, board, prices).map(|view| view.rows)
}

/// The rows of [`epic_rows`] and what ran outside any epic, from one pass over the board.
pub fn epic_view(
    rows: &[LedgerRow],
    board: &dyn Board,
    prices: &PriceTable,
) -> Result<EpicView, PortError> {
    let mut tasks = Tasks::new(board);
    let mut by_epic: BTreeMap<u64, Vec<&LedgerRow>> = BTreeMap::new();
    let mut loose: Vec<&LedgerRow> = Vec::new();
    for row in rows {
        let mut epic = None;
        for task in &row.tasks {
            if let Some(found) = tasks.look_up(*task)? {
                epic = Some(found.epic.get());
                break;
            }
        }
        match epic.or(row.epic) {
            Some(epic) => by_epic.entry(epic).or_default().push(row),
            None => loose.push(row),
        }
    }
    let mut done: BTreeMap<u64, Vec<(u64, Option<TaskSize>)>> = BTreeMap::new();
    let mut named: Vec<u64> = rows.iter().flat_map(|row| row.tasks.clone()).collect();
    named.sort_unstable();
    named.dedup();
    for task in named {
        if let Some(found) = tasks.look_up(task)?
            && found.done
        {
            done.entry(found.epic.get())
                .or_default()
                .push((task, found.size));
        }
    }
    let mut rows: Vec<EpicRow> = by_epic
        .into_iter()
        .map(|(epic, runs)| {
            row_of(
                epic,
                &runs,
                done.get(&epic).map_or(&[], Vec::as_slice),
                prices,
            )
        })
        .collect();
    for index in 1..rows.len() {
        let versus = compare(&rows[index], &rows[index - 1]);
        rows[index].versus_last = versus;
    }
    Ok(EpicView {
        rows,
        outside: outside_of(&loose, prices),
    })
}

fn outside_of(runs: &[&LedgerRow], prices: &PriceTable) -> Option<Outside> {
    if runs.is_empty() {
        return None;
    }
    let mut outside = Outside {
        runs: runs.len(),
        turns: 0,
        est_usd: 0.0,
        tokens: Tokens::default(),
    };
    for run in runs {
        outside.turns += run.turns;
        outside.est_usd += cost_of(run, prices);
        outside.tokens.input += run.input;
        outside.tokens.cache_write += run.cache_write;
        outside.tokens.cache_read += run.cache_read;
        outside.tokens.output += run.output;
    }
    Some(outside)
}

fn cost_of(row: &LedgerRow, prices: &PriceTable) -> f64 {
    prices
        .rates(&row.model)
        .cost(row.input, row.cache_write, row.cache_read, row.output)
}

/// The median dollars per done task, by size.
///
/// A run whose row names tasks splits its dollars evenly across them; any other run splits
/// evenly across the epic's done tasks. A task not done takes its share out of every median,
/// and a task whose size does not read is in no median, though its dollars stay in the
/// epic's total.
fn size_medians(runs: &[&LedgerRow], done: &DoneTasks, prices: &PriceTable) -> SizeMedians {
    let mut spent: BTreeMap<u64, f64> = done.iter().map(|(task, _)| (*task, 0.0)).collect();
    for run in runs {
        let cost = cost_of(run, prices);
        let mut named = run.tasks.clone();
        named.sort_unstable();
        named.dedup();
        if named.is_empty() {
            let share = cost / done.len().max(1) as f64;
            spent.values_mut().for_each(|total| *total += share);
        } else {
            let share = cost / named.len() as f64;
            for task in named {
                if let Some(total) = spent.get_mut(&task) {
                    *total += share;
                }
            }
        }
    }
    let of_size = |wanted: TaskSize| {
        let costs: Vec<f64> = done
            .iter()
            .filter(|(_, size)| *size == Some(wanted))
            .map(|(task, _)| spent[task])
            .collect();
        (!costs.is_empty()).then(|| median(costs))
    };
    SizeMedians {
        s: of_size(TaskSize::S),
        m: of_size(TaskSize::M),
    }
}

fn count_of(done: &DoneTasks, wanted: TaskSize) -> usize {
    done.iter()
        .filter(|(_, size)| *size == Some(wanted))
        .count()
}

fn row_of(epic: u64, runs: &[&LedgerRow], done: &DoneTasks, prices: &PriceTable) -> EpicRow {
    let tasks_done = done.len();
    let mut tokens = Tokens::default();
    let mut by_role: BTreeMap<String, f64> = BTreeMap::new();
    let mut est_usd = 0.0;
    let mut turns = 0;
    for run in runs {
        tokens.input += run.input;
        tokens.cache_write += run.cache_write;
        tokens.cache_read += run.cache_read;
        tokens.output += run.output;
        turns += run.turns;
        let cost = cost_of(run, prices);
        est_usd += cost;
        let role = run.role.clone().unwrap_or_else(|| NO_ROLE.to_owned());
        *by_role.entry(role).or_default() += cost;
    }
    let mut roles: Vec<RoleShare> = by_role
        .into_iter()
        .map(|(role, cost)| RoleShare {
            role,
            est_usd: cost,
            share_percent: if est_usd > 0.0 {
                100.0 * cost / est_usd
            } else {
                0.0
            },
        })
        .collect();
    roles.sort_by(|a, b| b.est_usd.total_cmp(&a.est_usd));
    let subagents: Vec<&&LedgerRow> = runs.iter().filter(|run| run.parent_run.is_some()).collect();
    let median_of = |value: fn(&LedgerRow) -> u64| {
        (!subagents.is_empty())
            .then(|| median(subagents.iter().map(|run| value(run) as f64).collect()))
    };
    let per_task = |total: f64| (tasks_done > 0).then(|| total / tasks_done as f64);
    EpicRow {
        epic,
        tasks_done,
        est_usd,
        usd_per_task: per_task(est_usd),
        tokens,
        turns,
        turns_per_task: per_task(turns as f64),
        context_first: median_of(|run| run.context_first),
        context_last: median_of(|run| run.context_last),
        roles,
        median_usd: size_medians(runs, done, prices),
        done_by_size: SizeCounts {
            s: count_of(done, TaskSize::S),
            m: count_of(done, TaskSize::M),
        },
        versus_last: SizeVerdicts::default(),
    }
}

/// The rows as text: the note that the dollars are estimates, then one line per epic.
pub fn render_text(rows: &[EpicRow]) -> String {
    let mut out = format!("{ESTIMATE_NOTE}\n");
    if rows.is_empty() {
        out.push_str("no epic runs in the ledger\n");
        return out;
    }
    let none = || "-".to_owned();
    let thousands = |value: Option<f64>| value.map_or_else(none, |v| format!("{:.0}k", v / 1e3));
    out.push_str(&format!(
        "{:<6} {:>5} {:>8} {:>7} {:>7} {:>7} {:>9} {:>9} {:>10} {:>6} {:>6} {:>8} {:>8} {:>9} {:>8}  roles\n",
        "epic",
        "done",
        "est $",
        "$/task",
        "med S $",
        "med M $",
        "S vs last",
        "M vs last",
        "turns/task",
        "ctx1",
        "ctxN",
        "input",
        "cache wr",
        "cache rd",
        "output"
    ));
    for row in rows {
        let roles: Vec<String> = row
            .roles
            .iter()
            .map(|role| format!("{} {:.0}%", role.role, role.share_percent))
            .collect();
        out.push_str(&format!(
            "{:<6} {:>5} {:>8.2} {:>7} {:>7} {:>7} {:>9} {:>9} {:>10} {:>6} {:>6} {:>7.2}M {:>7.1}M {:>8.1}M {:>7.2}M  {}\n",
            format!("#{}", row.epic),
            row.tasks_done,
            row.est_usd,
            row.usd_per_task.map_or_else(none, |v| format!("{v:.2}")),
            row.median_usd.s.map_or_else(none, |v| format!("{v:.2}")),
            row.median_usd.m.map_or_else(none, |v| format!("{v:.2}")),
            row.versus_last.s.map_or_else(none, |v| v.word().to_owned()),
            row.versus_last.m.map_or_else(none, |v| v.word().to_owned()),
            row.turns_per_task.map_or_else(none, |v| format!("{v:.0}")),
            thousands(row.context_first),
            thousands(row.context_last),
            row.tokens.input as f64 / 1e6,
            row.tokens.cache_write as f64 / 1e6,
            row.tokens.cache_read as f64 / 1e6,
            row.tokens.output as f64 / 1e6,
            roles.join(", ")
        ));
    }
    out
}

/// The rows as JSON, with the note that the dollars are estimates.
pub fn to_json(rows: &[EpicRow]) -> String {
    json_of(rows, None)
}

fn json_of(rows: &[EpicRow], outside: Option<&Outside>) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "note": ESTIMATE_NOTE,
        "epics": rows,
        "outside_any_epic": outside,
    }))
    .unwrap_or_else(|_| "{}".to_owned())
}

impl EpicView {
    /// The rows as text, then one line for what ran outside any epic, when anything did.
    pub fn render_text(&self) -> String {
        let mut out = render_text(&self.rows);
        if let Some(outside) = &self.outside {
            out.push_str(&format!(
                "outside any epic: {} runs, {} turns, est ${:.2}\n",
                outside.runs, outside.turns, outside.est_usd
            ));
        }
        out
    }

    /// The rows as JSON, with `outside_any_epic` holding the line's figures, or null.
    pub fn to_json(&self) -> String {
        json_of(&self.rows, self.outside.as_ref())
    }
}

#[cfg(test)]
mod tests;
