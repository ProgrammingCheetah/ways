use clap::Parser;

use super::*;

const MANIFEST: &str =
    "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\nrepo = \"owner/demo\"\n";

fn cli() -> Cli {
    Cli::try_parse_from(["zaf", "guard"]).expect("parses")
}

fn project() -> (tempfile::TempDir, Composition) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("ZAF.toml"), MANIFEST).expect("manifest");
    let composition = Composition::load(dir.path()).expect("loads");
    (dir, composition)
}

#[test]
fn without_a_budget_it_is_green_and_writes_the_file() {
    let (dir, composition) = project();
    assert_eq!(compute(&cli(), &composition, None), OK);
    let file = std::fs::read_to_string(dir.path().join(".zaf/guard.json")).expect("guard.json");
    let json: serde_json::Value = serde_json::from_str(&file).expect("json");
    assert_eq!(json["level"], "green");
    assert!(json["weekly_usd"].is_null());
}

#[test]
fn a_bad_budget_exits_backend() {
    let (dir, composition) = project();
    std::fs::create_dir_all(dir.path().join(".zaf")).expect("state dir");
    std::fs::write(
        dir.path().join(".zaf/local.toml"),
        "[budget]\nweekly_usd = -1\n",
    )
    .expect("local.toml");
    assert_eq!(compute(&cli(), &composition, None), BACKEND);
}

#[test]
fn the_manifest_thresholds_are_the_ones_used() {
    let (dir, _) = project();
    std::fs::write(
        dir.path().join("ZAF.toml"),
        format!("{MANIFEST}[spawn]\nguard = [1, 2, 3]\n"),
    )
    .expect("manifest");
    let composition = Composition::load(dir.path()).expect("loads");
    std::fs::create_dir_all(dir.path().join(".zaf/profiles/p/projects")).expect("profile");
    let transcript = r#"{"type":"assistant","message":{"id":"a","model":"claude-opus-5-5","usage":{"output_tokens":1000000}}}"#;
    std::fs::write(
        dir.path().join(".zaf/profiles/p/projects/t.jsonl"),
        transcript,
    )
    .expect("transcript");
    std::fs::write(
        dir.path().join(".zaf/local.toml"),
        "[budget]\nweekly_usd = 100\n",
    )
    .expect("local.toml");
    assert_eq!(compute(&cli(), &composition, None), OK);
    let file = std::fs::read_to_string(dir.path().join(".zaf/guard.json")).expect("guard.json");
    let json: serde_json::Value = serde_json::from_str(&file).expect("json");
    assert_eq!(
        json["level"], "red",
        "a $20 spend of $100 passes thresholds 1, 2 and 3"
    );
    assert_eq!(json["percent"], 20.0);
}
