use super::*;
use crate::app::check::step::tests::{ForgePorts, context, forge_with_pr};
use crate::domain::Location;
use crate::ports::{RuleId, ScanningAlert};
use std::time::Duration;

fn alert(rule: &str, path: &str, line: u32) -> ScanningAlert {
    ScanningAlert {
        rule: RuleId::from(rule),
        location: Some(Location {
            path: path.to_owned(),
            line: Some(line),
        }),
    }
}

#[test]
fn each_alert_is_a_must_fix_titled_with_its_rule() {
    let (forge, pr) = forge_with_pr();
    let alerts = vec![
        alert("js/sql-injection", "src/export.rs", 40),
        alert("js/path-injection", "src/read.rs", 9),
    ];
    forge.set_scanning_alerts(pr, alerts).expect("alerts");
    let source = FindingSource::from("codeql");
    let found = run(
        &source,
        &ForgePorts(forge),
        &context(pr, Duration::from_secs(1)),
    )
    .expect("step");
    let expected = vec![
        Finding {
            source: source.clone(),
            severity: Severity::MustFix,
            message: "js/sql-injection".to_owned(),
            location: Some(Location {
                path: "src/export.rs".to_owned(),
                line: Some(40),
            }),
        },
        Finding {
            source,
            severity: Severity::MustFix,
            message: "js/path-injection".to_owned(),
            location: Some(Location {
                path: "src/read.rs".to_owned(),
                line: Some(9),
            }),
        },
    ];
    assert_eq!(found, expected);
}

#[test]
fn no_alerts_is_no_findings() {
    let (forge, pr) = forge_with_pr();
    let source = FindingSource::from("codeql");
    let ports = ForgePorts(forge);
    let found = run(&source, &ports, &context(pr, Duration::from_secs(1)));
    assert_eq!(found, Ok(Vec::new()));
}
