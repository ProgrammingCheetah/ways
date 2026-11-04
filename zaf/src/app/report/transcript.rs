//! The transcript parser: one Claude Code `*.jsonl` file in, one [`Run`] out.
//!
//! A transcript is one JSON object per line. Only two kinds of line matter: the
//! first real user message names the run, and every assistant message with a
//! `usage` block is one turn. The same assistant message is written to the file
//! more than once, so a turn is counted once per message id. A line that is not
//! JSON is skipped, never fatal.
//!
//! The whole first prompt is read once, for the run's [`Attribution`], and dropped:
//! a [`Run`] keeps only its first 120 characters.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::prices::PriceTable;

/// How long a run's name may be.
const FIRST_PROMPT_CHARS: usize = 120;

/// The role of a prompt carrying an ask's `for: #<task>` line.
const KEEPER_ROLE: &str = "keeper";
/// The role of a prompt opening with `Epic #<n>`.
const PLANNING_ROLE: &str = "planning";
/// The role of a subagent whose own prompt names no other.
const PROGRAMMER_ROLE: &str = "programmer";

/// Who a run worked for, read from its whole first prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attribution {
    /// The run's role, when the prompt gives one.
    pub role: Option<String>,
    /// The epic the prompt names; a subagent takes its parent's in the ledger.
    pub epic: Option<u64>,
    /// The tasks the prompt names.
    pub tasks: Vec<u64>,
}

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
    /// The role, epic and tasks its first prompt gives.
    pub attribution: Attribution,
    /// The first line's `timestamp`, as written.
    pub first_time: Option<String>,
    /// The last line's `timestamp`, as written.
    pub last_time: Option<String>,
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
    let mut whole_prompt: Option<String> = None;
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
        attribution: Attribution::default(),
        first_time: None,
        last_time: None,
    };
    for line in text.lines() {
        let Ok(object) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(time) = object.get("timestamp").and_then(Value::as_str) {
            if run.first_time.is_none() {
                run.first_time = Some(time.to_owned());
            }
            run.last_time = Some(time.to_owned());
        }
        match object.get("type").and_then(Value::as_str) {
            Some("user") if whole_prompt.is_none() => {
                whole_prompt = prompt_of(&object);
            }
            Some("assistant") => take_turn(&object, prices, &mut seen, &mut models, &mut run),
            _ => {}
        }
    }
    if run.turns == 0 {
        return None;
    }
    let whole_prompt = whole_prompt.unwrap_or_default();
    run.first_prompt = collapse_whitespace(&whole_prompt)
        .chars()
        .take(FIRST_PROMPT_CHARS)
        .collect();
    run.attribution = attribute(&whole_prompt, run.is_subagent());
    run.model = dominant(&models);
    Some(run)
}

/// What a whole first prompt says of its run. An ask's `for:` line comes first: a keeper's
/// prompt also carries its asker's instance on the `from:` line, and any page text after it.
fn attribute(prompt: &str, subagent: bool) -> Attribution {
    if let Some((task, epic)) = keeper_ask(prompt) {
        return Attribution {
            role: Some(KEEPER_ROLE.to_owned()),
            epic,
            tasks: vec![task],
        };
    }
    let default_role = subagent.then(|| PROGRAMMER_ROLE.to_owned());
    let tasks = tasks_named(prompt);
    if let Some((role, epic)) = instance_named(prompt) {
        return Attribution {
            role: Some(role),
            epic: Some(epic),
            tasks,
        };
    }
    if let Some(epic) = opening_epic(prompt) {
        return Attribution {
            role: default_role.or_else(|| Some(PLANNING_ROLE.to_owned())),
            epic: Some(epic),
            tasks,
        };
    }
    Attribution {
        role: default_role,
        epic: None,
        tasks,
    }
}

/// The first `for: #<task>` line, with the epic of an instance on the `from:` line above it.
fn keeper_ask(prompt: &str) -> Option<(u64, Option<u64>)> {
    let mut previous = "";
    for line in prompt.lines() {
        let task = line
            .trim()
            .strip_prefix("for:")
            .map(str::trim)
            .filter(|rest| !rest.contains(char::is_whitespace))
            .and_then(issue_number);
        if let Some(task) = task {
            let epic = previous
                .trim()
                .strip_prefix("from:")
                .and_then(instance_named)
                .map(|(_, epic)| epic);
            return Some((task, epic));
        }
        previous = line;
    }
    None
}

/// The first `<role> instance <n> on #<epic>` in `text`.
fn instance_named(text: &str) -> Option<(String, u64)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words.windows(5).find_map(|window| {
        let [role, "instance", number, "on", epic] = window else {
            return None;
        };
        if !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let role = role.trim_start_matches(|character: char| !character.is_alphanumeric());
        let role_is_a_name = !role.is_empty()
            && role
                .chars()
                .all(|character| character.is_alphanumeric() || "-_".contains(character));
        let epic = issue_number(epic)?;
        role_is_a_name.then(|| (role.to_owned(), epic))
    })
}

/// The `<n>` of a prompt opening with `Epic #<n>`.
fn opening_epic(prompt: &str) -> Option<u64> {
    let rest = prompt.trim_start().strip_prefix("Epic")?;
    let word = rest.split_whitespace().next()?;
    rest.starts_with(char::is_whitespace)
        .then(|| issue_number(word))
        .flatten()
}

/// Every task named by `task #<n>` or `Tasks: #<a> #<b>`, in any case, each once.
fn tasks_named(prompt: &str) -> Vec<u64> {
    let words: Vec<&str> = prompt.split_whitespace().collect();
    let mut tasks = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let word = word
            .trim_start_matches(|character: char| !character.is_alphanumeric())
            .trim_end_matches(':')
            .to_ascii_lowercase();
        if word != "task" && word != "tasks" {
            continue;
        }
        let numbers = words[index + 1..]
            .iter()
            .map_while(|next| issue_number(next));
        for task in numbers {
            if !tasks.contains(&task) {
                tasks.push(task);
            }
        }
    }
    tasks
}

/// The issue number a word opens with: `#12`, `#12,`, `#12's`; not `#12a`, `#0` or `12`.
fn issue_number(word: &str) -> Option<u64> {
    let rest = word.strip_prefix('#')?;
    let end = rest
        .find(|character: char| !character.is_ascii_digit())
        .unwrap_or(rest.len());
    let (digits, after) = rest.split_at(end);
    if after.starts_with(char::is_alphanumeric) {
        return None;
    }
    digits.parse().ok().filter(|number| *number > 0)
}

/// The whole text of a user line, if that line is a real prompt.
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
    Some(text)
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
