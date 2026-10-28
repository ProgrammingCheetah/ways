//! `zaf report`: tokens and estimated dollars per model, the subagent growth
//! profile and the per-run table, read from Claude Code transcripts.
//!
//! This is `tools/burn.py` ported: the same sums, the same grouping and the same
//! text, so the baseline's numbers reproduce. Transcripts are `*.jsonl` files
//! under a config dir's `projects/` folder; [`transcript_roots`] names the
//! folders to read, [`report`] reads them.

mod prices;
mod transcript;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub use prices::{PriceTable, Rates};
pub use transcript::{Run, parse_run};

/// Why a report could not be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReportError {
    /// `[report.prices]` in `local.toml` is malformed.
    #[error("bad price table in .zaf/local.toml: {0}")]
    Prices(String),
}

/// What to read and how to cut it.
#[derive(Debug, Clone)]
pub struct Query {
    /// Folders searched recursively for `*.jsonl`.
    pub roots: Vec<PathBuf>,
    /// Only transcripts modified within this many hours of `now`.
    pub hours: f64,
    /// Only runs whose path or first prompt contains this; empty keeps all.
    pub matching: String,
    /// How many rows of the per-run table.
    pub top: usize,
    /// What a token costs.
    pub prices: PriceTable,
    /// The moment the window ends.
    pub now: SystemTime,
}

/// The `projects/` folders to read: one per `<state dir>/profiles/*/`, plus the
/// default config dir's when one is given (the `--all` case).
pub fn transcript_roots(state_dir: &Path, default_config_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut profiles: Vec<PathBuf> = std::fs::read_dir(state_dir.join("profiles"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    profiles.sort();
    let mut roots: Vec<PathBuf> = profiles.iter().map(|dir| dir.join("projects")).collect();
    roots.extend(default_config_dir.map(|dir| dir.join("projects")));
    roots
}

/// One model's totals, over the runs whose main model it is.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ModelTotals {
    /// The model.
    pub model: String,
    /// Turns.
    pub turns: u64,
    /// Fresh input tokens.
    pub input: u64,
    /// Cache-write tokens.
    pub cache_write: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Output tokens.
    pub output: u64,
    /// Estimated dollars.
    pub est_usd: f64,
}

/// How subagent contexts grow: the part fixed at turn one against the part that grows.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Growth {
    /// Subagent runs of three turns or more.
    pub runs: usize,
    /// Median turns.
    pub median_turns: f64,
    /// Median context at turn one.
    pub median_context_first: f64,
    /// Median context at the last turn.
    pub median_context_last: f64,
    /// Percent of all context reads that is the turn-one context repeated.
    pub fixed_percent: f64,
    /// The rest.
    pub growth_percent: f64,
}

/// A row of the per-run table.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RunRow {
    /// `sub` for a subagent, `main` otherwise.
    pub kind: &'static str,
    /// The run's model.
    pub model: String,
    /// Estimated dollars.
    pub est_usd: f64,
    /// Turns.
    pub turns: u64,
    /// Context at turn one.
    pub context_first: u64,
    /// Context at the last turn.
    pub context_last: u64,
    /// Output tokens.
    pub output: u64,
    /// The first prompt.
    pub first_prompt: String,
    /// The transcript.
    pub path: String,
}

/// The report: totals, per-model sums, the growth profile and the dearest runs.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Report {
    /// Runs read.
    pub runs: usize,
    /// Turns over all runs.
    pub turns: u64,
    /// Estimated dollars over all runs.
    pub est_usd: f64,
    /// Per model, dearest first.
    pub models: Vec<ModelTotals>,
    /// Subagent growth; none without a subagent run of three turns or more.
    pub growth: Option<Growth>,
    /// The dearest runs, dearest first.
    pub table: Vec<RunRow>,
}

/// Reads every transcript the query selects and sums them.
pub fn report(query: &Query) -> Report {
    let cut = window_start(query.now, query.hours);
    let mut files = Vec::new();
    for root in &query.roots {
        collect_jsonl(root, &mut files);
    }
    files.sort();
    files.dedup();
    let runs: Vec<Run> = files
        .iter()
        .filter(|path| {
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| modified >= cut)
        })
        .filter_map(|path| parse_run(path, &query.prices))
        .filter(|run| {
            query.matching.is_empty()
                || run.path.to_string_lossy().contains(&query.matching)
                || run.first_prompt.contains(&query.matching)
        })
        .collect();
    summarize(&runs, query.top)
}

fn window_start(now: SystemTime, hours: f64) -> SystemTime {
    let span = Duration::try_from_secs_f64(hours * 3600.0).unwrap_or(if hours > 0.0 {
        Duration::MAX
    } else {
        Duration::ZERO
    });
    now.checked_sub(span).unwrap_or(SystemTime::UNIX_EPOCH)
}

fn collect_jsonl(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() {
            collect_jsonl(&path, out);
        } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
            out.push(path);
        }
    }
}

fn summarize(runs: &[Run], top: usize) -> Report {
    let mut models: Vec<ModelTotals> = Vec::new();
    for run in runs {
        let index = models
            .iter()
            .position(|totals| totals.model == run.model)
            .unwrap_or_else(|| {
                models.push(ModelTotals {
                    model: run.model.clone(),
                    turns: 0,
                    input: 0,
                    cache_write: 0,
                    cache_read: 0,
                    output: 0,
                    est_usd: 0.0,
                });
                models.len() - 1
            });
        let totals = &mut models[index];
        totals.turns += run.turns;
        totals.input += run.input;
        totals.cache_write += run.cache_write;
        totals.cache_read += run.cache_read;
        totals.output += run.output;
        totals.est_usd += run.cost;
    }
    models.sort_by(|a, b| b.est_usd.total_cmp(&a.est_usd));
    let mut dearest: Vec<&Run> = runs.iter().collect();
    dearest.sort_by(|a, b| b.cost.total_cmp(&a.cost));
    Report {
        runs: runs.len(),
        turns: runs.iter().map(|run| run.turns).sum(),
        est_usd: runs.iter().map(|run| run.cost).sum(),
        models,
        growth: growth_of(runs),
        table: dearest.into_iter().take(top).map(row_of).collect(),
    }
}

fn row_of(run: &Run) -> RunRow {
    RunRow {
        kind: if run.is_subagent() { "sub" } else { "main" },
        model: run.model.clone(),
        est_usd: run.cost,
        turns: run.turns,
        context_first: run.context.first().copied().unwrap_or(0),
        context_last: run.context.last().copied().unwrap_or(0),
        output: run.output,
        first_prompt: run.first_prompt.clone(),
        path: run.path.to_string_lossy().into_owned(),
    }
}

fn growth_of(runs: &[Run]) -> Option<Growth> {
    let subagents: Vec<&Run> = runs
        .iter()
        .filter(|run| run.is_subagent() && run.context.len() >= 3)
        .collect();
    if subagents.is_empty() {
        return None;
    }
    let first = |run: &&Run| run.context[0] as f64;
    let last = |run: &&Run| run.context[run.context.len() - 1] as f64;
    let fixed: f64 = subagents
        .iter()
        .map(|run| run.context[0] as f64 * run.turns as f64)
        .sum();
    let reads: f64 = subagents
        .iter()
        .map(|run| run.context.iter().sum::<u64>() as f64)
        .sum();
    let reads = if reads == 0.0 { 1.0 } else { reads };
    Some(Growth {
        runs: subagents.len(),
        median_turns: median(subagents.iter().map(|run| run.turns as f64).collect()),
        median_context_first: median(subagents.iter().map(first).collect()),
        median_context_last: median(subagents.iter().map(last).collect()),
        fixed_percent: 100.0 * fixed / reads,
        growth_percent: 100.0 - 100.0 * fixed / reads,
    })
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        f64::midpoint(values[middle - 1], values[middle])
    }
}

impl Report {
    /// The report as `tools/burn.py` prints it; `no runs` when nothing was read.
    pub fn render_text(&self) -> String {
        use std::fmt::Write as _;
        if self.runs == 0 {
            return "no runs\n".to_owned();
        }
        let mut out = String::new();
        let mut line = |text: String| {
            out.push_str(&text);
            out.push('\n');
        };
        line(format!(
            "runs {}  turns {}  est ${:.0}",
            self.runs, self.turns, self.est_usd
        ));
        line(format!(
            "{:<18} {:>6} {:>12} {:>12} {:>8} {:>7}",
            "model", "turns", "cache write", "cache read", "output", "est $"
        ));
        for m in &self.models {
            line(format!(
                "{:<18} {:>6} {:>11.1}M {:>11.1}M {:>7.2}M {:>7.0}",
                m.model,
                m.turns,
                m.cache_write as f64 / 1e6,
                m.cache_read as f64 / 1e6,
                m.output as f64 / 1e6,
                m.est_usd
            ));
        }
        if let Some(g) = &self.growth {
            line(String::new());
            line(format!(
                "subagent runs {}: median {:.0} turns, context {:.0}k at turn 1 -> {:.0}k at the end; fixed part {:.0}% of context reads, growth {:.0}%",
                g.runs,
                g.median_turns,
                g.median_context_first / 1e3,
                g.median_context_last / 1e3,
                g.fixed_percent,
                g.growth_percent
            ));
        }
        line(String::new());
        line(format!(
            "{:<10} {:>6} {:>5} {:>6} {:>6} {:>6}  run",
            "model", "est $", "turns", "ctx1", "ctxN", "out"
        ));
        for r in &self.table {
            let model: String = r.model.chars().skip(7).take(10).collect();
            let prompt: String = r.first_prompt.chars().take(70).collect();
            let mut row = String::new();
            let _ = write!(
                row,
                "{:<10} {:>6.1} {:>5} {:>5.0}k {:>5.0}k {:>5.0}k  {}: {}",
                model,
                r.est_usd,
                r.turns,
                r.context_first as f64 / 1e3,
                r.context_last as f64 / 1e3,
                r.output as f64 / 1e3,
                r.kind,
                prompt
            );
            line(row);
        }
        out
    }

    /// The report as JSON for the spawner.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

#[cfg(test)]
mod tests;
