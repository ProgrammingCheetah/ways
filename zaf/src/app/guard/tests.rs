use std::path::PathBuf;
use std::time::Duration;

use super::*;
use crate::app::report::{PriceTable, Query};

const THRESHOLDS: [u8; 3] = [70, 85, 95];

/// The report fixtures cost about $33 in all.
fn query() -> Query {
    Query {
        roots: vec![
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/app/report/fixtures/projects"),
        ],
        hours: 100_000.0,
        matching: String::new(),
        top: 25,
        prices: PriceTable::default(),
        now: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
    }
}

fn budgeted(weekly: &str) -> (tempfile::TempDir, GuardState) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(LOCAL_FILE),
        format!("[budget]\nweekly_usd = {weekly}\n"),
    )
    .expect("local.toml");
    let state = run(dir.path(), query(), &THRESHOLDS).expect("guard");
    (dir, state)
}

#[test]
fn no_budget_is_green_and_says_so() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = run(dir.path(), query(), &THRESHOLDS).expect("guard");
    assert_eq!(state.level, Level::Green);
    assert_eq!(state.weekly_usd, None);
    assert_eq!(state.percent, 0.0);
    assert_eq!(state.line(), "no budget set");
    let written = read(dir.path()).expect("readable").expect("written");
    assert_eq!(written, state);
}

#[test]
fn each_band_has_its_level_and_the_file_is_written() {
    // Spend is about $33.4; the weekly budget sets the percent.
    for (weekly, level) in [
        ("100", Level::Green), // 33%
        ("47", Level::Yellow), // 71%
        ("39", Level::Orange), // 86%
        ("34", Level::Red),    // 98%
        ("10", Level::Red),    // far over
    ] {
        let (dir, state) = budgeted(weekly);
        assert_eq!(state.level, level, "weekly {weekly}: {state:?}");
        let file = std::fs::read_to_string(dir.path().join(GUARD_FILE)).expect("guard.json");
        let json: serde_json::Value = serde_json::from_str(&file).expect("json");
        assert_eq!(json["level"], level.to_string());
        assert_eq!(json["weekly_usd"], weekly.parse::<f64>().expect("number"));
        assert!(json["percent"].is_number());
        assert!(json["spent_usd"].as_f64().expect("spent") > 33.0);
        assert_eq!(json["computed_at"], "2027-01-15T08:00:00Z");
    }
}

#[test]
fn the_level_changes_at_each_threshold_exactly() {
    assert_eq!(level_for(69.9, &THRESHOLDS), Level::Green);
    assert_eq!(level_for(70.0, &THRESHOLDS), Level::Yellow);
    assert_eq!(level_for(84.9, &THRESHOLDS), Level::Yellow);
    assert_eq!(level_for(85.0, &THRESHOLDS), Level::Orange);
    assert_eq!(level_for(94.9, &THRESHOLDS), Level::Orange);
    assert_eq!(level_for(95.0, &THRESHOLDS), Level::Red);
    assert_eq!(level_for(400.0, &THRESHOLDS), Level::Red);
    assert_eq!(
        level_for(90.0, &[50]),
        Level::Yellow,
        "fewer thresholds, fewer steps"
    );
}

#[test]
fn the_line_reads_level_percent_and_dollars() {
    let state = assess(Some(40.0), 30.0, &THRESHOLDS, SystemTime::UNIX_EPOCH);
    assert_eq!(state.line(), "yellow 75% ($30 of $40)");
}

#[test]
fn a_bad_budget_is_refused() {
    assert!(weekly_budget("[budget]\nweekly_usd = 0\n").is_err());
    assert!(weekly_budget("[budget]\nweekly_usd = \"lots\"\n").is_err());
    assert!(weekly_budget("[budget").is_err());
    assert_eq!(weekly_budget("").expect("empty"), None);
    assert_eq!(
        weekly_budget("[budget]\nweekly_usd = 250\n").expect("int"),
        Some(250.0)
    );
}

#[test]
fn a_price_override_in_local_toml_moves_the_spend() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(LOCAL_FILE),
        "[budget]\nweekly_usd = 100\n[report.prices]\n\"claude-sonnet-5-5\" = [0, 0, 0, 0]\n\"claude-haiku-5-5\" = [0, 0, 0, 0]\n",
    )
    .expect("local.toml");
    let state = run(dir.path(), query(), &THRESHOLDS).expect("guard");
    assert!(state.spent_usd < 16.0, "{state:?}");
}

#[test]
fn reading_a_state_that_was_never_written_is_none() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(read(dir.path()).expect("absent"), None);
    std::fs::write(dir.path().join(GUARD_FILE), "{").expect("garbage");
    assert!(read(dir.path()).is_err());
}
