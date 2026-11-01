//! `zaf guard`: the budget level the spawner reads.
//!
//! The Engineer sets a weekly budget in dollars under `[budget]` in
//! `.zaf/local.toml`; the week's estimated cost comes from [`crate::app::report`];
//! the percent between them is mapped against the manifest's `[spawn] guard`
//! thresholds to a [`Level`]. The result is written to `.zaf/guard.json` for the
//! spawner and `zaf status` to read.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::report::{self, ReportError};

/// The state file's name under the state folder.
pub const GUARD_FILE: &str = "guard.json";

/// The local, machine-private overrides under the state folder.
pub const LOCAL_FILE: &str = "local.toml";

/// How tight the budget is.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    strum::Display,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Level {
    /// Under the first threshold: run as planned.
    Green,
    /// First threshold passed: Sonnet-only, smaller batches.
    Yellow,
    /// Second threshold passed: one slot.
    Orange,
    /// Third threshold passed: the spawner stops.
    Red,
}

/// What `.zaf/guard.json` holds.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GuardState {
    /// The level.
    pub level: Level,
    /// Spend as a percent of the weekly budget; 0 without a budget.
    pub percent: f64,
    /// The week's estimated cost in dollars.
    pub spent_usd: f64,
    /// The weekly budget; none when unset.
    pub weekly_usd: Option<f64>,
    /// When it was computed, RFC 3339.
    pub computed_at: String,
}

impl GuardState {
    /// The one line `zaf guard` and `zaf status` print.
    pub fn line(&self) -> String {
        match self.weekly_usd {
            None => "no budget set".to_owned(),
            Some(weekly) => format!(
                "{} {:.0}% (${:.0} of ${:.0})",
                self.level, self.percent, self.spent_usd, weekly
            ),
        }
    }
}

/// Why the guard could not be computed or stored.
#[derive(Debug, thiserror::Error)]
pub enum GuardError {
    /// `.zaf/local.toml` could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// `.zaf/local.toml` is not valid TOML or `[budget]` is malformed.
    #[error("bad .zaf/local.toml: {0}")]
    Local(String),
    /// The price table in `local.toml` is malformed.
    #[error(transparent)]
    Prices(#[from] ReportError),
    /// `.zaf/guard.json` could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// The file.
        path: PathBuf,
        /// Why.
        source: std::io::Error,
    },
    /// `.zaf/guard.json` is there but unreadable as a guard state.
    #[error("bad {path}: {reason}")]
    State {
        /// The file.
        path: PathBuf,
        /// Why.
        reason: String,
    },
}

/// The text of `<state dir>/local.toml`; empty when the file is absent.
pub fn read_local(state_dir: &Path) -> Result<String, GuardError> {
    let path = state_dir.join(LOCAL_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(GuardError::Read { path, source }),
    }
}

/// The weekly budget in dollars from the text of a `local.toml`; none when unset.
pub fn weekly_budget(local_toml: &str) -> Result<Option<f64>, GuardError> {
    let document: toml::Table = local_toml
        .parse()
        .map_err(|error: toml::de::Error| GuardError::Local(error.to_string()))?;
    let Some(value) = document
        .get("budget")
        .and_then(toml::Value::as_table)
        .and_then(|budget| budget.get("weekly_usd"))
    else {
        return Ok(None);
    };
    let dollars = value
        .as_float()
        .or_else(|| value.as_integer().map(|whole| whole as f64))
        .filter(|dollars| *dollars > 0.0)
        .ok_or_else(|| GuardError::Local("[budget] weekly_usd must be a positive number".into()))?;
    Ok(Some(dollars))
}

/// The level for a percent of the budget: one step for each threshold reached.
pub fn level_for(percent: f64, thresholds: &[u8]) -> Level {
    let reached = thresholds
        .iter()
        .filter(|threshold| percent >= f64::from(**threshold))
        .count();
    match reached {
        0 => Level::Green,
        1 => Level::Yellow,
        2 => Level::Orange,
        _ => Level::Red,
    }
}

/// The guard state for a week's spend; green without a budget.
pub fn assess(
    weekly_usd: Option<f64>,
    spent_usd: f64,
    thresholds: &[u8],
    now: SystemTime,
) -> GuardState {
    let percent = weekly_usd.map_or(0.0, |weekly| 100.0 * spent_usd / weekly);
    GuardState {
        level: level_for(percent, thresholds),
        percent,
        spent_usd,
        weekly_usd,
        computed_at: humantime::format_rfc3339_seconds(now).to_string(),
    }
}

/// Reads the budget and the week's transcripts, assesses and writes `guard.json`.
///
/// `query` selects the transcripts (its prices are replaced by the ones in
/// `local.toml`); `thresholds` are the manifest's `[spawn] guard`.
pub fn run(
    state_dir: &Path,
    mut query: report::Query,
    thresholds: &[u8],
) -> Result<GuardState, GuardError> {
    let local = read_local(state_dir)?;
    let weekly = weekly_budget(&local)?;
    query.prices = report::PriceTable::from_local_toml(&local)?;
    let spent = report::report(&query).est_usd;
    let state = assess(weekly, spent, thresholds, query.now);
    write(state_dir, &state)?;
    Ok(state)
}

/// Writes `<state dir>/guard.json`, creating the folder.
pub fn write(state_dir: &Path, state: &GuardState) -> Result<(), GuardError> {
    let path = state_dir.join(GUARD_FILE);
    let text = serde_json::to_string_pretty(state).unwrap_or_default();
    std::fs::create_dir_all(state_dir)
        .and_then(|()| std::fs::write(&path, text))
        .map_err(|source| GuardError::Write { path, source })
}

/// Reads `<state dir>/guard.json`; none when it was never written.
pub fn read(state_dir: &Path) -> Result<Option<GuardState>, GuardError> {
    let path = state_dir.join(GUARD_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(GuardError::Read { path, source }),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| GuardError::State {
            path,
            reason: error.to_string(),
        })
}

#[cfg(test)]
mod tests;
