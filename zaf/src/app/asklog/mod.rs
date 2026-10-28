//! `zaf asklog`: the ask log as the design's own analytics.
//!
//! The pages cited most, the questions asked more than once (a question that
//! keeps returning was not settled), and the `not settled` answers (the gaps).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use super::ask::{AskFailure, LogLine, read_log};

/// A page and how many answers cited it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PageCount {
    /// The page.
    pub page: String,
    /// How many logged answers cite it.
    pub citations: usize,
}

/// A question asked more than once, by kind and text, whatever the task.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Recurring {
    /// Its kind.
    pub kind: String,
    /// The question as first asked.
    pub question: String,
    /// How many times it was asked.
    pub times: usize,
    /// The tasks it was asked for, ascending.
    pub tasks: Vec<u64>,
}

/// A question the design did not answer.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Gap {
    /// The task it was asked for.
    pub task: u64,
    /// Its kind.
    pub kind: String,
    /// The question.
    pub question: String,
}

/// The log, summed up.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AskLogSummary {
    /// How many lines the log holds.
    pub asks: usize,
    /// Cited pages, most cited first, ties in page order.
    pub top_pages: Vec<PageCount>,
    /// Questions asked more than once, most asked first, ties in first-asked order.
    pub recurring: Vec<Recurring>,
    /// `not settled` answers, one per task and question, in first-asked order.
    pub not_settled: Vec<Gap>,
}

/// Reads the log at `path` and sums it up; a missing log is an empty summary.
pub fn asklog(path: &Path) -> Result<AskLogSummary, AskFailure> {
    Ok(summarize(&read_log(path)?))
}

/// Sums up log lines.
pub fn summarize(lines: &[LogLine]) -> AskLogSummary {
    let mut citations: BTreeMap<&str, usize> = BTreeMap::new();
    for page in lines.iter().flat_map(|line| &line.pages) {
        *citations.entry(page.as_str()).or_default() += 1;
    }
    let mut top_pages: Vec<PageCount> = citations
        .into_iter()
        .map(|(page, citations)| PageCount {
            page: page.to_owned(),
            citations,
        })
        .collect();
    top_pages.sort_by_key(|count| Reverse(count.citations));

    let mut asked: Vec<(String, Recurring)> = Vec::new();
    for line in lines {
        let key = format!("{}|{}", line.kind, normalize(&line.question));
        match asked.iter_mut().find(|(known, _)| *known == key) {
            Some((_, recurring)) => {
                recurring.times += 1;
                if !recurring.tasks.contains(&line.task) {
                    recurring.tasks.push(line.task);
                    recurring.tasks.sort_unstable();
                }
            }
            None => asked.push((
                key,
                Recurring {
                    kind: line.kind.clone(),
                    question: line.question.clone(),
                    times: 1,
                    tasks: vec![line.task],
                },
            )),
        }
    }
    let mut recurring: Vec<Recurring> = asked
        .into_iter()
        .map(|(_, recurring)| recurring)
        .filter(|recurring| recurring.times > 1)
        .collect();
    recurring.sort_by_key(|recurring| Reverse(recurring.times));

    let mut seen = BTreeSet::new();
    let not_settled = lines
        .iter()
        .filter(|line| line.verdict == "not settled")
        .filter(|line| seen.insert((line.task, line.kind.clone(), normalize(&line.question))))
        .map(|line| Gap {
            task: line.task,
            kind: line.kind.clone(),
            question: line.question.clone(),
        })
        .collect();

    AskLogSummary {
        asks: lines.len(),
        top_pages,
        recurring,
        not_settled,
    }
}

/// Case and whitespace aside, two questions with the same words are the same question.
fn normalize(question: &str) -> String {
    question
        .split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
