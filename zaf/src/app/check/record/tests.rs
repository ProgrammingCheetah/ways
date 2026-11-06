use super::*;
use crate::app::check::task::LineKind;
use crate::app::check::{CheckedFinding, Step, StepResult, TaskLine};
use crate::domain::{Finding, FindingSource, IssueNumber, Location};
use crate::ports::PrNumber;
use std::time::{Duration, SystemTime};

#[test]
fn a_task_check_prints_one_line_per_thing_and_the_verdict() {
    let check = TaskCheck {
        lines: vec![
            TaskLine {
                kind: LineKind::Gate,
                subject: "light".to_owned(),
                status: LineStatus::Pass,
            },
            TaskLine {
                kind: LineKind::Test,
                subject: "exports_all".to_owned(),
                status: LineStatus::Fail("1 failed".to_owned()),
            },
            TaskLine {
                kind: LineKind::Acceptance,
                subject: "an empty list exports".to_owned(),
                status: LineStatus::Manual,
            },
        ],
    };
    let record = CheckRecord::from_task(
        &Target::Task(IssueNumber::from(7)),
        &check,
        SystemTime::UNIX_EPOCH,
    );
    let expected = "pass      gate light\n\
                    fail      test exports_all: 1 failed\n\
                    manual    acceptance an empty list exports\n\
                    task #7: must-fix";
    assert_eq!(record.to_string(), expected);
    assert_eq!(record.verdict.code(), 1);
}

#[test]
fn a_report_round_trips_through_json() {
    let step: Step = "runner:local:critique".parse().expect("step");
    let finding = Finding {
        source: FindingSource::from("runner:local:critique"),
        severity: Severity::MustFix,
        message: "the empty list is dropped".to_owned(),
        location: Some(Location {
            path: "src/export.rs".to_owned(),
            line: Some(12),
        }),
    };
    let report = CheckReport {
        steps: vec![StepResult {
            step,
            status: StepStatus::Ran,
        }],
        findings: vec![CheckedFinding {
            finding,
            also_found_by: vec![FindingSource::from("command:lint")],
        }],
    };
    let target = Target::Pr {
        pr: PrNumber::from(3),
        task: None,
    };
    let record = CheckRecord::from_report(&target, &report, None, SystemTime::UNIX_EPOCH);
    let text = serde_json::to_string(&record).expect("serializes");
    let back: CheckRecord = serde_json::from_str(&text).expect("parses");
    assert_eq!(back, record);
    assert_eq!(
        record.lines[1].to_string(),
        "must      runner:local:critique src/export.rs:12 the empty list is dropped (also command:lint)"
    );
    assert_eq!(record.verdict, Verdict::MustFix);
}

fn empty_report() -> CheckReport {
    CheckReport {
        steps: Vec::new(),
        findings: Vec::new(),
    }
}

fn pr_target() -> Target {
    Target::Pr {
        pr: PrNumber::from(3),
        task: None,
    }
}

fn empty_task_check() -> TaskCheck {
    TaskCheck { lines: Vec::new() }
}

#[test]
fn record_stores_the_head() {
    let record = CheckRecord::from_report(
        &pr_target(),
        &empty_report(),
        Some("abc123".to_owned()),
        SystemTime::UNIX_EPOCH,
    );
    assert_eq!(record.head.as_deref(), Some("abc123"));
    let text = serde_json::to_string(&record).expect("serializes");
    assert!(text.contains(r#""head":"abc123""#), "{text}");
    let back: CheckRecord = serde_json::from_str(&text).expect("parses");
    assert_eq!(back.head.as_deref(), Some("abc123"));
}

#[test]
fn task_record_has_no_head() {
    let record = CheckRecord::from_task(
        &Target::Task(IssueNumber::from(7)),
        &empty_task_check(),
        SystemTime::UNIX_EPOCH,
    );
    assert_eq!(record.head, None);
    let text = serde_json::to_string(&record).expect("serializes");
    assert!(!text.contains("head"), "{text}");
    assert!(text.contains(r#""at""#), "{text}");
}

#[test]
fn record_stores_its_time() {
    // 2026-11-04T00:00:00Z plus 999 ms: the stored time drops the fraction.
    let at = SystemTime::UNIX_EPOCH + Duration::new(1_793_750_400, 999_000_000);
    let report = CheckRecord::from_report(&pr_target(), &empty_report(), None, at);
    assert_eq!(report.at.as_deref(), Some("2026-11-04T00:00:00Z"));
    let task = CheckRecord::from_task(&Target::Task(IssueNumber::from(7)), &empty_task_check(), at);
    assert_eq!(task.at.as_deref(), Some("2026-11-04T00:00:00Z"));
    let text = serde_json::to_string(&report).expect("serializes");
    assert!(text.contains(r#""at":"2026-11-04T00:00:00Z""#), "{text}");
}

#[test]
fn old_record_still_loads() {
    let old = r#"{"target":"pr #5","verdict":"clean","lines":[
        {"status":"ran","source":"ci","text":""}]}"#;
    let record: CheckRecord = serde_json::from_str(old).expect("an old record loads");
    assert_eq!(record.head, None);
    assert_eq!(record.at, None);
    assert_eq!(record.lines.len(), 1);
    assert_eq!(record.verdict, Verdict::Clean);
}
