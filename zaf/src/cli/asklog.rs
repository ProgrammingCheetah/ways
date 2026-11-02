//! `zaf asklog`: the pages cited most, the questions that keep returning, the gaps.

use super::Cli;
use super::exit::{BACKEND, OK, fail_composition};

use crate::app::ask::ASK_LOG;
use crate::app::asklog::{AskLogSummary, asklog};
use crate::composition::Composition;

/// How many pages the terse view lists; `--more` lists them all.
const TOP_PAGES: usize = 10;

/// Prints the ask log's summary.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return fail_composition(&path, &error),
    };
    let summary = match asklog(&composition.state_dir().join(ASK_LOG)) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return BACKEND;
        }
    };
    if cli.json {
        println!("{}", serde_json::json!(summary));
        return OK;
    }
    print!("{}", render(&summary, cli.more));
    OK
}

/// The terse view of the summary; `more` lists every cited page.
fn render(summary: &AskLogSummary, more: bool) -> String {
    use std::fmt::Write as _;

    if summary.asks == 0 {
        return "The ask log is empty.\n".to_owned();
    }
    let mut out = String::new();
    let shown = if more { usize::MAX } else { TOP_PAGES };
    let _ = writeln!(out, "{} questions asked", summary.asks);
    out.push_str("Top pages:\n");
    for count in summary.top_pages.iter().take(shown) {
        let _ = writeln!(out, "  {} cited {}", count.page, count.citations);
    }
    out.push_str("Asked more than once:\n");
    for recurring in &summary.recurring {
        let tasks: Vec<String> = recurring
            .tasks
            .iter()
            .map(|task| format!("#{task}"))
            .collect();
        let _ = writeln!(
            out,
            "  {}x {}: {} ({})",
            recurring.times,
            recurring.kind,
            recurring.question,
            tasks.join(", ")
        );
    }
    out.push_str("Not settled:\n");
    for gap in &summary.not_settled {
        let _ = writeln!(out, "  #{} {}: {}", gap.task, gap.kind, gap.question);
    }
    out.push_str("Failed:\n");
    for failure in &summary.failed {
        let _ = writeln!(
            out,
            "  #{} {}: {} ({})",
            failure.task, failure.kind, failure.question, failure.reason
        );
    }
    out.push_str("Check failed:\n");
    for failure in &summary.check_failed {
        let _ = writeln!(
            out,
            "  #{} {}: {} ({})",
            failure.task, failure.kind, failure.question, failure.reason
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::asklog::Failure;

    #[test]
    fn the_failed_heading_lists_failed_lines() {
        let summary = AskLogSummary {
            asks: 2,
            top_pages: Vec::new(),
            recurring: Vec::new(),
            not_settled: Vec::new(),
            failed: vec![Failure {
                task: 49,
                kind: "rule".to_owned(),
                question: "Is it so?".to_owned(),
                reason: "no rung gave a verified answer: haiku: run exited 1".to_owned(),
            }],
            check_failed: Vec::new(),
        };

        let text = render(&summary, false);

        let (_, failed) = text.split_once("Failed:\n").expect("a Failed heading");
        assert_eq!(
            failed,
            "  #49 rule: Is it so? (no rung gave a verified answer: haiku: run exited 1)\nCheck failed:\n"
        );
    }

    #[test]
    fn the_check_failed_heading_lists_check_failed_lines_apart_from_failed() {
        let summary = AskLogSummary {
            asks: 1,
            top_pages: Vec::new(),
            recurring: Vec::new(),
            not_settled: Vec::new(),
            failed: Vec::new(),
            check_failed: vec![Failure {
                task: 83,
                kind: "rule".to_owned(),
                question: "Is it so?".to_owned(),
                reason: "page 21594113 could not be read".to_owned(),
            }],
        };

        let text = render(&summary, false);

        let (before, check_failed) = text
            .split_once("Check failed:\n")
            .expect("a Check failed heading");
        assert_eq!(
            check_failed,
            "  #83 rule: Is it so? (page 21594113 could not be read)\n"
        );
        let (_, failed) = before.split_once("Failed:\n").expect("a Failed heading");
        assert_eq!(failed, "", "no rung failure listed");
    }
}
