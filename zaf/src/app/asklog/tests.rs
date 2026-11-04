use super::*;
use crate::app::ask::CheckResult;

fn line(task: u64, kind: &str, question: &str, verdict: &str, pages: &[&str]) -> LogLine {
    LogLine {
        time: 1_800_000_000,
        task,
        from: "programmer-1".to_owned(),
        kind: kind.to_owned(),
        question: question.to_owned(),
        verdict: verdict.to_owned(),
        pages: pages.iter().map(|&page| page.to_owned()).collect(),
        answer: String::new(),
        rung: Some("local".to_owned()),
        check: CheckResult::Passed,
        read: crate::app::ask::Read::default(),
    }
}

#[test]
fn the_summary_ranks_pages_finds_repeats_and_gaps() {
    let lines = [
        line(
            12,
            "rule",
            "Is an empty list exported?",
            "settled",
            &["1001"],
        ),
        line(
            13,
            "rule",
            "Is an  EMPTY list exported?",
            "settled",
            &["1001", "1002"],
        ),
        line(13, "term", "What is a shelf?", "not settled", &[]),
        line(13, "term", "What is a shelf?", "not settled", &[]),
        line(
            14,
            "fact",
            "Is an empty list exported?",
            "conflict",
            &["1002", "1003"],
        ),
        line(14, "where", "Where do exports go?", "settled", &["1003"]),
    ];

    let summary = summarize(&lines);

    assert_eq!(summary.asks, 6);
    let pages: Vec<(&str, usize)> = summary
        .top_pages
        .iter()
        .map(|count| (count.page.as_str(), count.citations))
        .collect();
    assert_eq!(pages, [("1001", 2), ("1002", 2), ("1003", 2)]);

    assert_eq!(
        summary.recurring,
        [
            Recurring {
                kind: "rule".to_owned(),
                question: "Is an empty list exported?".to_owned(),
                times: 2,
                tasks: vec![12, 13],
            },
            Recurring {
                kind: "term".to_owned(),
                question: "What is a shelf?".to_owned(),
                times: 2,
                tasks: vec![13],
            },
        ]
    );

    assert_eq!(
        summary.not_settled,
        [Gap {
            task: 13,
            kind: "term".to_owned(),
            question: "What is a shelf?".to_owned(),
        }]
    );
}

#[test]
fn failed_lines_are_counted_and_listed_with_their_reason() {
    let mut failed = line(49, "rule", "Is it so?", "failed", &[]);
    failed.answer = "no rung gave a verified answer: haiku: run exited 1".to_owned();
    failed.rung = None;
    failed.check = CheckResult::Failed(vec![failed.answer.clone()]);
    let lines = [
        line(49, "fact", "What is it?", "not settled", &[]),
        failed.clone(),
        line(50, "term", "What is a shelf?", "settled", &["1001"]),
    ];

    let summary = summarize(&lines);

    assert_eq!(summary.asks, 3, "a failed ask was asked");
    assert_eq!(
        summary.failed,
        [Failure {
            task: 49,
            kind: "rule".to_owned(),
            question: "Is it so?".to_owned(),
            reason: failed.answer,
        }]
    );
    assert_eq!(summary.not_settled.len(), 1, "failed is not not settled");
}

#[test]
fn check_failed_lines_are_listed_apart_from_failed_and_not_settled() {
    let mut unreadable = line(83, "rule", "Is it so?", "check failed", &[]);
    unreadable.answer = "q1: check failed — page 21594113 could not be read".to_owned();
    unreadable.rung = None;
    unreadable.check = CheckResult::Failed(vec!["page 21594113 could not be read".to_owned()]);
    let lines = [
        unreadable,
        line(83, "term", "What is a shelf?", "not settled", &[]),
    ];

    let summary = summarize(&lines);

    assert_eq!(
        summary.check_failed,
        [Failure {
            task: 83,
            kind: "rule".to_owned(),
            question: "Is it so?".to_owned(),
            reason: "page 21594113 could not be read".to_owned(),
        }]
    );
    assert!(summary.failed.is_empty(), "not a rung failure");
    assert_eq!(summary.not_settled.len(), 1, "not a gap either");
}

#[test]
fn the_most_cited_page_comes_first() {
    let lines = [
        line(1, "fact", "a?", "settled", &["2002"]),
        line(1, "fact", "b?", "settled", &["2001", "2002"]),
    ];
    let summary = summarize(&lines);
    assert_eq!(summary.top_pages[0].page, "2002");
    assert_eq!(summary.top_pages[0].citations, 2);
    assert!(summary.recurring.is_empty());
}

#[test]
fn a_missing_log_is_an_empty_summary_and_bad_lines_are_skipped() {
    let state = tempfile::tempdir().expect("temp dir");
    let path = state.path().join("asklog.jsonl");
    let empty = asklog(&path).expect("summary");
    assert_eq!(empty.asks, 0);

    let good = serde_json::to_string(&line(1, "fact", "a?", "not settled", &[])).expect("json");
    std::fs::write(&path, format!("{good}\nnot json\n{good}\n")).expect("log");
    let summary = asklog(&path).expect("summary");
    assert_eq!(summary.asks, 2);
    assert_eq!(summary.recurring.len(), 1);
    assert_eq!(summary.not_settled.len(), 1);
}
