use super::*;
use crate::app::check::task::LineKind;
use crate::app::check::{CheckedFinding, Step, StepResult, TaskLine};
use crate::domain::{Finding, FindingSource, IssueNumber, Location};
use crate::ports::PrNumber;

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
    let record = CheckRecord::from_task(&Target::Task(IssueNumber::from(7)), &check);
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
    let record = CheckRecord::from_report(&target, &report);
    let text = serde_json::to_string(&record).expect("serializes");
    let back: CheckRecord = serde_json::from_str(&text).expect("parses");
    assert_eq!(back, record);
    assert_eq!(
        record.lines[1].to_string(),
        "must      runner:local:critique src/export.rs:12 the empty list is dropped (also command:lint)"
    );
    assert_eq!(record.verdict, Verdict::MustFix);
}
