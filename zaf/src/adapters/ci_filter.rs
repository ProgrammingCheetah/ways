//! Reading a workflow file's triggers that reach a pull request.
//!
//! The one place the YAML reader is named (a test in `tests/architecture.rs` holds it there):
//! the repository's workflow files are untrusted text, so they are parsed here and handed to the
//! domain as a [`WorkflowFilter`], which says only what [`crate::domain::checks_expected`] needs.
//! Three triggers reach a pull request: `pull_request`, `pull_request_target` and a `push` to the
//! slice's branch. Whatever this reader is not sure to read right comes out as
//! [`PullRequestFilter::Unsure`], which expects a check.

use serde_norway::{Mapping, Value};

use crate::domain::{PullRequestFilter, RepoPath, WorkflowFilter};

/// The `pull_request` keys that do not narrow which changes run the workflow.
const NEUTRAL_KEYS: [&str; 1] = ["types"];

/// The filter of the workflow file at `path` whose text is `text`.
pub fn read_workflow(path: &str, text: &str) -> WorkflowFilter {
    WorkflowFilter {
        path: RepoPath::from(path),
        filter: read_filter(text),
    }
}

/// Whether the file `name` of a workflows folder is one a forge runs: `.yml` or `.yaml`.
pub fn is_workflow_name(name: &str) -> bool {
    name.ends_with(".yml") || name.ends_with(".yaml")
}

/// Whether `name` is plain enough to address a file by: letters, digits, `.`, `-` and `_`. A
/// name the base put outside that is not fetched, and is read as unreadable.
pub fn is_plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// The workflow at `path` as a file that could not be read: not sure, so a check is expected.
pub fn unreadable(path: &str) -> WorkflowFilter {
    WorkflowFilter {
        path: RepoPath::from(path),
        filter: PullRequestFilter::Unsure,
    }
}

fn read_filter(text: &str) -> PullRequestFilter {
    let Ok(Value::Mapping(root)) = serde_norway::from_str::<Value>(text) else {
        return PullRequestFilter::Unsure;
    };
    // A YAML 1.1 reader may turn the key `on` into a boolean.
    let triggers = root
        .iter()
        .find(|(key, _)| key.as_str() == Some("on") || **key == Value::Bool(true))
        .map(|(_, value)| value);
    match triggers {
        Some(Value::String(name)) => by_name(name),
        Some(Value::Sequence(names)) => {
            let mut parts = Vec::new();
            for name in names {
                match name.as_str() {
                    Some(name) => parts.push(by_name(name)),
                    None => return PullRequestFilter::Unsure,
                }
            }
            combine(parts)
        }
        Some(Value::Mapping(events)) => {
            let mut parts = Vec::new();
            for (key, value) in events {
                parts.push(match key.as_str() {
                    Some("pull_request" | "pull_request_target") => from_event(value, from_filters),
                    Some("push") => from_event(value, from_push),
                    _ => continue,
                });
            }
            combine(parts)
        }
        _ => PullRequestFilter::Unsure,
    }
}

/// A trigger named with no filter: the events that reach a pull request run on any change.
fn by_name(name: &str) -> PullRequestFilter {
    match name {
        "pull_request" | "pull_request_target" | "push" => PullRequestFilter::AnyChange,
        _ => PullRequestFilter::NoTrigger,
    }
}

/// An event's value: empty means no filter, a mapping is read by `filters`, anything else is
/// not sure.
fn from_event(value: &Value, filters: fn(&Mapping) -> PullRequestFilter) -> PullRequestFilter {
    match value {
        Value::Null => PullRequestFilter::AnyChange,
        Value::Mapping(map) => filters(map),
        _ => PullRequestFilter::Unsure,
    }
}

/// What the triggers read together ask: one that lets every change through, or is not sure,
/// settles it (both expect a check); the narrowing ones stand side by side.
fn combine(parts: Vec<PullRequestFilter>) -> PullRequestFilter {
    let mut narrowing = Vec::new();
    let mut unsure = false;
    for part in parts {
        match part {
            PullRequestFilter::NoTrigger => {}
            PullRequestFilter::AnyChange => return PullRequestFilter::AnyChange,
            PullRequestFilter::Unsure => unsure = true,
            other => narrowing.push(other),
        }
    }
    if unsure {
        return PullRequestFilter::Unsure;
    }
    match narrowing.len() {
        0 => PullRequestFilter::NoTrigger,
        1 => narrowing.remove(0),
        _ => PullRequestFilter::Either(narrowing),
    }
}

/// The patterns of a list of strings; none when it is empty or holds anything else.
fn patterns_of(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::Sequence(items) if !items.is_empty() => items
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect(),
        _ => None,
    }
}

/// The `push` mapping: `branches` narrows to the slice's branch, and `tags` beside it does not
/// matter to a branch push; `tags` alone never runs on one. `paths` or `paths-ignore` beside
/// `branches` only narrow further, so the read stays the branches (no check that will come is
/// dropped); with no `branches` they are not sure. Any other key (`branches-ignore`, a key not
/// known) is not sure.
fn from_push(filters: &Mapping) -> PullRequestFilter {
    let mut branches = None;
    let mut tags = false;
    let mut paths = false;
    for (key, value) in filters {
        let Some(patterns) = patterns_of(value) else {
            return PullRequestFilter::Unsure;
        };
        match key.as_str() {
            Some("branches") => branches = Some(patterns),
            Some("tags") => tags = true,
            Some("paths" | "paths-ignore") => paths = true,
            _ => return PullRequestFilter::Unsure,
        }
    }
    match branches {
        Some(patterns) => PullRequestFilter::Branches(patterns),
        None if paths => PullRequestFilter::Unsure,
        None if tags => PullRequestFilter::NoTrigger,
        None => PullRequestFilter::AnyChange,
    }
}

/// The `pull_request` or `pull_request_target` mapping: a `paths` list alone is read; any other
/// narrowing key (`paths-ignore`, a branch filter, a key not known) is not sure.
fn from_filters(filters: &Mapping) -> PullRequestFilter {
    let mut paths = None;
    for (key, value) in filters {
        match key.as_str() {
            Some("paths") => match value {
                Value::Sequence(items) if !items.is_empty() => {
                    let mut patterns = Vec::new();
                    for item in items {
                        match item.as_str() {
                            Some(pattern) => patterns.push(pattern.to_owned()),
                            None => return PullRequestFilter::Unsure,
                        }
                    }
                    paths = Some(patterns);
                }
                _ => return PullRequestFilter::Unsure,
            },
            Some(name) if NEUTRAL_KEYS.contains(&name) => {}
            _ => return PullRequestFilter::Unsure,
        }
    }
    paths.map_or(PullRequestFilter::AnyChange, PullRequestFilter::Paths)
}

#[cfg(test)]
mod tests;
