//! `zaf asklog`: the pages cited most, the questions that keep returning, the gaps.

use super::Cli;
use super::exit::{BACKEND, OK, fail_composition};

use crate::app::ask::ASK_LOG;
use crate::app::asklog::asklog;
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
    if summary.asks == 0 {
        println!("The ask log is empty.");
        return OK;
    }
    println!("{} questions asked", summary.asks);
    let shown = if cli.more { usize::MAX } else { TOP_PAGES };
    println!("Top pages:");
    for count in summary.top_pages.iter().take(shown) {
        println!("  {} cited {}", count.page, count.citations);
    }
    println!("Asked more than once:");
    for recurring in &summary.recurring {
        let tasks: Vec<String> = recurring
            .tasks
            .iter()
            .map(|task| format!("#{task}"))
            .collect();
        println!(
            "  {}x {}: {} ({})",
            recurring.times,
            recurring.kind,
            recurring.question,
            tasks.join(", ")
        );
    }
    println!("Not settled:");
    for gap in &summary.not_settled {
        println!("  #{} {}: {}", gap.task, gap.kind, gap.question);
    }
    OK
}
