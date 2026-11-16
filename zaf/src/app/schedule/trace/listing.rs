//! What `zaf spawn trace [<ticket>] [--since <when>]` prints of the trace (docs/zaf-actions.md
//! §5.1): the moves of every line, oldest first, one each with the local time of its tick:
//!
//! ```text
//! 17:15 #7 lane(implement) -> running(implement)
//! ```
//!
//! When the moves shown cross a day, each day opens with its date on a line of its own
//! (`2026-11-13`). With `--more`, what each tick did follows its moves, one line each, indented
//! under them. Pure: the lines, the time and the UTC offset are handed in.

use std::time::{Duration, SystemTime};

use serde_json::{Value, json};

use super::{Move, Recorded};
use crate::domain::TicketId;

/// How far a line the tick did is indented under its moves: past the time.
const INDENT: &str = "      ";

/// Which moves to show: one ticket's, or every ticket's, from a time on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Filter {
    /// Only this ticket's moves; every ticket's when none.
    pub ticket: Option<TicketId>,
    /// Only the moves of ticks that began at or after this time; every one when none.
    pub since: Option<SystemTime>,
}

/// `text` as a `--since`, read at `now`: an RFC 3339 time (`2026-11-12T17:00:00Z`; with no zone it
/// is UTC, and a space may stand for the `T`), or a span back from `now` (`2h`, `3d`, `90min`).
pub fn since(text: &str, now: SystemTime) -> Result<SystemTime, String> {
    let text = text.trim();
    if let Ok(at) = humantime::parse_rfc3339_weak(text) {
        return Ok(at);
    }
    let span = humantime::parse_duration(text).map_err(|_| {
        format!(
            "`{text}` is neither a time such as 2026-11-12T17:00:00Z nor a span such as 2h or 3d"
        )
    })?;
    Ok(now.checked_sub(span).unwrap_or(SystemTime::UNIX_EPOCH))
}

/// One tick's moves the filter keeps, with the tick.
fn shown<'a>(recorded: &'a [Recorded], filter: &Filter) -> Vec<(&'a Recorded, Vec<&'a Move>)> {
    let mut ticks: Vec<&Recorded> = recorded
        .iter()
        .filter(|line| filter.since.is_none_or(|since| line.at >= since))
        .collect();
    // Two ticks that overlapped may have written out of order; the listing is by when each began.
    ticks.sort_by_key(|line| line.at);
    ticks
        .into_iter()
        .filter_map(|line| {
            let moves: Vec<&Move> = line
                .moved
                .iter()
                .filter(|moved| filter.ticket.is_none_or(|ticket| moved.ticket == ticket))
                .collect();
            (!moves.is_empty()).then_some((line, moves))
        })
        .collect()
}

/// The local date and time of day of `at`, `utc_offset` seconds from UTC: `2026-11-12` and
/// `17:15`.
fn local(at: SystemTime, utc_offset: i64) -> (String, String) {
    let shift = Duration::from_secs(utc_offset.unsigned_abs());
    let shifted = if utc_offset >= 0 {
        at.checked_add(shift)
    } else {
        at.checked_sub(shift)
    }
    .unwrap_or(at);
    let text = humantime::format_rfc3339_seconds(shifted).to_string();
    (
        text.get(..10).unwrap_or_default().to_owned(),
        text.get(11..16).unwrap_or_default().to_owned(),
    )
}

/// The moves the filter keeps, as text (see the module above); no line when none is kept.
pub fn lines(recorded: &[Recorded], filter: &Filter, more: bool, utc_offset: i64) -> Vec<String> {
    let shown = shown(recorded, filter);
    let mut days: Vec<String> = shown
        .iter()
        .map(|(line, _)| local(line.at, utc_offset).0)
        .collect();
    days.dedup();
    let dated = days.len() > 1;
    let mut lines = Vec::new();
    let mut day = None;
    for (line, moves) in shown {
        let (date, clock) = local(line.at, utc_offset);
        if dated && day.as_ref() != Some(&date) {
            lines.push(date.clone());
            day = Some(date);
        }
        lines.extend(
            moves
                .iter()
                .map(|moved| format!("{clock} {}", moved.line())),
        );
        if more {
            lines.extend(
                line.did
                    .lines()
                    .into_iter()
                    .map(|did| format!("{INDENT}{did}")),
            );
        }
    }
    lines
}

/// The moves the filter keeps, as `--json` prints them: `{"moves": [{"at": "<time>", "ticket": 7,
/// "from": "lane(implement)", "to": "running(implement)"}]}`, oldest first, with `from` null for a
/// new ticket and `to` null for one gone; with `more`, each carries its tick's `did`.
pub fn to_json(recorded: &[Recorded], filter: &Filter, more: bool) -> Value {
    let moves: Vec<Value> = shown(recorded, filter)
        .into_iter()
        .flat_map(|(line, moves)| {
            moves.into_iter().map(move |moved| {
                let mut entry = json!({
                    "at": humantime::format_rfc3339_seconds(line.at).to_string(),
                    "ticket": moved.ticket,
                    "from": moved.from,
                    "to": moved.to,
                });
                if more {
                    entry["did"] = serde_json::to_value(&line.did).unwrap_or(Value::Null);
                }
                entry
            })
        })
        .collect();
    json!({ "moves": moves })
}

#[cfg(test)]
mod tests;
