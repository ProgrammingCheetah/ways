//! The transcript parser: one Claude Code `*.jsonl` file in, one [`Run`] out.
//!
//! A transcript is one JSON object per line. Only two kinds of line matter: the
//! first real user message names the run, and every assistant message with a
//! `usage` block is one turn. The same assistant message is written to the file
//! more than once, so a turn is counted once per message id. A line that is not
//! JSON is skipped, never fatal.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::prices::PriceTable;

/// How long a run's name may be.
const FIRST_PROMPT_CHARS: usize = 120;

/// One transcript, summed.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// The transcript file.
    pub path: PathBuf,
    /// The first user message that is not a command echo, whitespace collapsed.
    pub first_prompt: String,
    /// The model most of its turns ran on.
    pub model: String,
    /// The context read at each turn: fresh input plus cache writes plus cache reads.
    pub context: Vec<u64>,
    /// Turns, once per message id.
    pub turns: u64,
    /// Fresh input tokens.
    pub input: u64,
    /// Cache-write tokens.
    pub cache_write: u64,
    /// Cache-read tokens.
    pub cache_read: u64,
    /// Output tokens.
    pub output: u64,
    /// Estimated dollars, each message at its own model's rates.
    pub cost: f64,
}

impl Run {
    /// Whether the transcript is a subagent's, which Claude Code files under `subagents/`.
    pub fn is_subagent(&self) -> bool {
        self.path.to_string_lossy().contains("/subagents/")
    }
}

/// Reads one transcript; none when it cannot be read or holds no turn.
pub fn parse_run(path: &Path, prices: &PriceTable) -> Option<Run> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut seen = std::collections::HashSet::new();
    let mut first_prompt: Option<String> = None;
    let mut models: Vec<(String, u64)> = Vec::new();
    let mut run = Run {
        path: path.to_owned(),
        first_prompt: String::new(),
        model: String::new(),
        context: Vec::new(),
        turns: 0,
        input: 0,
        cache_write: 0,
        cache_read: 0,
        output: 0,
        cost: 0.0,
    };
    for line in text.lines() {
        let Ok(object) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match object.get("type").and_then(Value::as_str) {
            Some("user") if first_prompt.is_none() => {
                first_prompt = prompt_of(&object);
            }
            Some("assistant") => take_turn(&object, prices, &mut seen, &mut models, &mut run),
            _ => {}
        }
    }
    if run.turns == 0 {
        return None;
    }
    run.first_prompt = first_prompt.unwrap_or_default();
    run.model = dominant(&models);
    Some(run)
}

/// The run's name from a user line, if that line is a real prompt.
fn prompt_of(object: &Value) -> Option<String> {
    let content = object.get("message")?.get("content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part.is_object())
            .map(|part| part.get("text").and_then(Value::as_str).unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" "),
        _ => return None,
    };
    if text.trim().is_empty() || text.starts_with('<') {
        return None;
    }
    Some(
        collapse_whitespace(&text)
            .chars()
            .take(FIRST_PROMPT_CHARS)
            .collect(),
    )
}

/// Every run of whitespace becomes one space; the ends keep theirs.
fn collapse_whitespace(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_space = false;
    for character in text.chars() {
        if character.is_whitespace() {
            if !in_space {
                out.push(' ');
            }
            in_space = true;
        } else {
            out.push(character);
            in_space = false;
        }
    }
    out
}

fn take_turn(
    object: &Value,
    prices: &PriceTable,
    seen: &mut std::collections::HashSet<String>,
    models: &mut Vec<(String, u64)>,
    run: &mut Run,
) {
    let Some(message) = object.get("message") else {
        return;
    };
    let usage = message.get("usage").and_then(Value::as_object);
    let model = message
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.is_empty());
    let (Some(usage), Some(model)) = (usage.filter(|usage| !usage.is_empty()), model) else {
        return;
    };
    if let Some(id) = message.get("id").and_then(Value::as_str)
        && !seen.insert(id.to_owned())
    {
        return;
    }
    match models.iter_mut().find(|(name, _)| name == model) {
        Some((_, count)) => *count += 1,
        None => models.push((model.to_owned(), 1)),
    }
    let count = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let input = count("input_tokens");
    let cache_write = count("cache_creation_input_tokens");
    let cache_read = count("cache_read_input_tokens");
    let output = count("output_tokens");
    run.turns += 1;
    run.input += input;
    run.cache_write += cache_write;
    run.cache_read += cache_read;
    run.output += output;
    run.cost += prices
        .rates(model)
        .cost(input, cache_write, cache_read, output);
    run.context.push(input + cache_write + cache_read);
}

/// The model with the most turns; the first seen wins a tie.
fn dominant(models: &[(String, u64)]) -> String {
    let mut best: Option<&(String, u64)> = None;
    for entry in models {
        if best.is_none_or(|current| entry.1 > current.1) {
            best = Some(entry);
        }
    }
    best.map(|(name, _)| name.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests;
