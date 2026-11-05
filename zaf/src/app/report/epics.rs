//! `zaf report --epics`: one row per epic, what it cost set beside what it finished.
//!
//! The rows come from the ledger, so an epic whose transcripts have expired still prints. A
//! run belongs to the epic of the task its row names, as the board has it; a run that names
//! no task, or only tasks the board knows no epic for, belongs to the epic its row holds. A
//! subagent already holds its parent session's epic in the ledger. Tasks done are read from
//! the board. Dollars are computed here from the price table, never stored, so a changed
//! price moves every row alike. The spend outside any epic is not a row: it is one line after
//! the rows, so that spend stays visible.

use std::collections::{BTreeMap, HashMap};

use super::ledger::LedgerRow;
use super::{PriceTable, median};
use crate::domain::{IssueNumber, TaskLabel};
use crate::ports::{Board, PortError};

/// What the text and the JSON say about every dollar figure.
pub const ESTIMATE_NOTE: &str = "estimates: dollars are API list prices applied to the ledger's tokens at reading, a proxy for weight against the weekly limit; compare rows, do not bill from them";

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
}

/// The epic of every task the rows name, as the board has it, and whether it is done.
struct Tasks<'a> {
    board: &'a dyn Board,
    seen: HashMap<u64, Option<(IssueNumber, bool)>>,
}

impl<'a> Tasks<'a> {
    fn new(board: &'a dyn Board) -> Self {
        Self {
            board,
            seen: HashMap::new(),
        }
    }

    /// The task's epic and whether it is done; none for a task the board does not have or
    /// that has no epic. Any other board failure is an error.
    fn look_up(&mut self, task: u64) -> Result<Option<(IssueNumber, bool)>, PortError> {
        if let Some(known) = self.seen.get(&task) {
            return Ok(*known);
        }
        let found = match self.board.task(IssueNumber::from(task)) {
            Ok(ticket) => ticket
                .epic
                .map(|epic| (epic, ticket.label == Some(TaskLabel::Done))),
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
            if let Some((found, _)) = tasks.look_up(*task)? {
                epic = Some(found.get());
                break;
            }
        }
        match epic.or(row.epic) {
            Some(epic) => by_epic.entry(epic).or_default().push(row),
            None => loose.push(row),
        }
    }
    let mut done: BTreeMap<u64, usize> = BTreeMap::new();
    let mut named: Vec<u64> = rows.iter().flat_map(|row| row.tasks.clone()).collect();
    named.sort_unstable();
    named.dedup();
    for task in named {
        if let Some((epic, true)) = tasks.look_up(task)? {
            *done.entry(epic.get()).or_default() += 1;
        }
    }
    let rows = by_epic
        .into_iter()
        .map(|(epic, runs)| {
            row_of(
                epic,
                &runs,
                done.get(&epic).copied().unwrap_or_default(),
                prices,
            )
        })
        .collect();
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

fn row_of(epic: u64, runs: &[&LedgerRow], tasks_done: usize, prices: &PriceTable) -> EpicRow {
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
        "{:<6} {:>5} {:>8} {:>7} {:>10} {:>6} {:>6} {:>8} {:>8} {:>9} {:>8}  roles\n",
        "epic",
        "done",
        "est $",
        "$/task",
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
            "{:<6} {:>5} {:>8.2} {:>7} {:>10} {:>6} {:>6} {:>7.2}M {:>7.1}M {:>8.1}M {:>7.2}M  {}\n",
            format!("#{}", row.epic),
            row.tasks_done,
            row.est_usd,
            row.usd_per_task.map_or_else(none, |v| format!("{v:.2}")),
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
