//! The ledger through the binary: `zaf report` and `zaf spawn --once` fill it.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{Project, outcome, write};
use rusqlite::Connection;
use zaf::app::report::ledger::{LEDGER_FILE, Ledger, LedgerRow};
use zaf::app::report::{self, PriceTable, Query};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n";

/// A tick needs the role it launches and the runner that role runs on.
const SPAWN_MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n";

const SESSION: &str = include_str!("../src/app/report/fixtures/projects/-demo/session-a.jsonl");
const SUBAGENT: &str =
    include_str!("../src/app/report/fixtures/projects/-demo/session-a/subagents/agent-b.jsonl");
const EXPECTED: &str = include_str!("../src/app/report/fixtures/burn.expected.txt");

const TURN: &str = r#"{"type":"assistant","timestamp":"2026-11-04T10:00:05.000Z","message":{"id":"m1","model":"claude-opus-5-5","usage":{"input_tokens":1,"cache_creation_input_tokens":2,"cache_read_input_tokens":3,"output_tokens":4}}}"#;

/// A project whose default config dir is an empty folder of its own, never the machine's.
fn project(manifest: &str) -> (Project, PathBuf) {
    let mut project = Project::new(manifest);
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    (project, config)
}

/// A one-turn transcript with this first prompt.
fn transcript(prompt: &str) -> String {
    let user = serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": prompt},
    });
    format!("{user}\n{TURN}\n")
}

fn write_profile_session(project: &Project, name: &str, prompt: &str) {
    write(
        &project.root,
        &format!(".zaf/profiles/build/projects/-demo/{name}.jsonl"),
        &transcript(prompt),
    );
}

fn rows(db: &Path) -> Vec<LedgerRow> {
    Ledger::open(db).expect("open").rows().expect("rows")
}

fn run_ids(db: &Path) -> Vec<String> {
    rows(db).into_iter().map(|row| row.run_id).collect()
}

#[test]
fn zaf_home_moves_the_ledger() {
    let (mut project, config) = project(MANIFEST);
    write_profile_session(&project, "profiled", "Epic #103: plan it");
    write(
        &config,
        "projects/-home-other/defaulted.jsonl",
        &transcript("hello"),
    );
    let moved = project.temp.path().join("moved/home");
    project.env("ZAF_HOME", moved.to_str().expect("utf-8"));

    let (code, _, stderr) = outcome(&project.zaf(&["report"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(!project.home.join(LEDGER_FILE).exists());
    let db = moved.join(LEDGER_FILE);
    let rows = rows(&db);
    assert_eq!(rows.len(), 2, "the profiles and the default config dir");
    assert_eq!(rows[0].run_id, "defaulted");
    assert_eq!(rows[0].project, "-home-other");
    assert_eq!(rows[1].run_id, "profiled");
    assert_eq!(rows[1].project, "example");
    assert_eq!(rows[1].role.as_deref(), Some("planning"));
    assert_eq!(rows[1].epic, Some(103));
}

#[test]
fn every_form_of_report_ingests_everything() {
    let forms: [&[&str]; 4] = [
        &["report", "--all"],
        &["--json", "report"],
        &["report", "--hours", "0", "--match", "nothing", "--top", "1"],
        &["report", "--match", "nothing"],
    ];
    for form in forms {
        let (project, config) = project(MANIFEST);
        write_profile_session(&project, "profiled", "hello");
        write(
            &config,
            "projects/-other/defaulted.jsonl",
            &transcript("hi"),
        );
        let (code, _, stderr) = outcome(&project.zaf(form));
        assert_eq!(code, 0, "{form:?}: {stderr}");
        assert_eq!(
            run_ids(&project.home.join(LEDGER_FILE)),
            ["defaulted", "profiled"],
            "{form:?}"
        );
    }
}

#[test]
fn newer_schema_exits_2() {
    let (project, _config) = project(MANIFEST);
    let db = project.home.join(LEDGER_FILE);
    let connection = Connection::open(&db).expect("connect");
    connection
        .pragma_update(None, "user_version", 2)
        .expect("version");
    drop(connection);
    let (code, stdout, stderr) = outcome(&project.zaf(&["report"]));
    assert_eq!(code, 2);
    assert!(stderr.contains(db.to_str().expect("utf-8")), "{stderr}");
    assert!(stderr.contains("schema version 2"), "{stderr}");
    assert_eq!(stdout, "");
}

#[test]
fn a_ledger_that_cannot_be_opened_exits_2_naming_the_file() {
    let (project, _config) = project(MANIFEST);
    let db = project.home.join(LEDGER_FILE);

    fs::write(&db, "not a database, and long enough to be read as one").expect("write");
    let (code, _, stderr) = outcome(&project.zaf(&["report"]));
    assert_eq!(code, 2);
    assert!(stderr.contains(db.to_str().expect("utf-8")), "{stderr}");

    fs::remove_file(&db).expect("remove");
    fs::create_dir(&db).expect("directory");
    let (code, _, stderr) = outcome(&project.zaf(&["report"]));
    assert_eq!(code, 2);
    assert!(stderr.contains(db.to_str().expect("utf-8")), "{stderr}");
}

#[test]
fn concurrent_ingest_both_succeed() {
    let (project, _config) = project(MANIFEST);
    for number in 0..30 {
        write_profile_session(&project, &format!("session-{number:02}"), "hello");
    }
    let results: Vec<(i32, String, String)> = std::thread::scope(|scope| {
        let runs: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| outcome(&project.zaf(&["report"]))))
            .collect();
        runs.into_iter()
            .map(|run| run.join().expect("thread"))
            .collect()
    });
    for (code, _, stderr) in &results {
        assert_eq!(*code, 0, "{stderr}");
        assert!(!stderr.contains("ledger"), "{stderr}");
    }
    assert_eq!(rows(&project.home.join(LEDGER_FILE)).len(), 30);
}

#[test]
fn spawn_tick_ingests() {
    let (mut project, _config) = project(SPAWN_MANIFEST);
    project.on("*", "[]");
    write_profile_session(&project, "profiled", "hello");
    let (code, stdout, stderr) = outcome(&project.zaf(&["spawn", "--once", "--version", "0.2.0"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
    assert_eq!(run_ids(&project.home.join(LEDGER_FILE)), ["profiled"]);
}

#[test]
fn a_dry_run_writes_no_ledger() {
    let (mut project, _config) = project(SPAWN_MANIFEST);
    project.on("*", "[]");
    write_profile_session(&project, "profiled", "hello");
    let args = ["spawn", "--once", "--version", "0.2.0", "--dry-run"];
    let (code, stdout, stderr) = outcome(&project.zaf(&args));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!project.home.join(LEDGER_FILE).exists());
}

#[test]
fn spawn_ledger_failure_only_warns() {
    let args = ["spawn", "--once", "--version", "0.2.0"];
    let (mut healthy, _config) = project(SPAWN_MANIFEST);
    healthy.on("*", "[]");
    let (healthy_code, healthy_stdout, _) = outcome(&healthy.zaf(&args));

    let (mut broken, _config) = project(SPAWN_MANIFEST);
    broken.on("*", "[]");
    let db = broken.home.join(LEDGER_FILE);
    fs::create_dir(&db).expect("a directory where the file belongs");
    let (code, stdout, stderr) = outcome(&broken.zaf(&args));
    assert_eq!(code, healthy_code);
    assert_eq!(code, 0);
    assert_eq!(stdout, healthy_stdout);
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains(db.to_str().expect("utf-8")), "{stderr}");
}

#[test]
fn report_output_is_unchanged() {
    let (project, config) = project(MANIFEST);
    write(
        &project.root,
        ".zaf/profiles/build/projects/-demo/session-a.jsonl",
        SESSION,
    );
    write(
        &project.root,
        ".zaf/profiles/build/projects/-demo/session-a/subagents/agent-b.jsonl",
        SUBAGENT,
    );
    // In the ledger, and not in the report without `--all`.
    write(
        &config,
        "projects/-other/defaulted.jsonl",
        &transcript("hello"),
    );
    let hours = ["report", "--hours", "100000"];

    // The first run also reports the homes it made, as it did before the ledger.
    let (code, stdout, stderr) = outcome(&project.zaf(&hours));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, EXPECTED);
    assert!(!stderr.contains("ledger"), "{stderr}");
    assert_eq!(
        outcome(&project.zaf(&hours)),
        (0, EXPECTED.to_owned(), String::new())
    );
    assert_eq!(rows(&project.home.join(LEDGER_FILE)).len(), 3);

    // The binary finds its root from its working directory, which comes back with symlinks
    // resolved; macOS's temp dir sits behind one (`/var` is `/private/var`).
    let root = project.root.canonicalize().expect("root");
    let query = Query {
        roots: vec![root.join(".zaf/profiles/build/projects")],
        hours: 100_000.0,
        matching: String::new(),
        top: 25,
        prices: PriceTable::default(),
        now: std::time::SystemTime::now(),
    };
    let (code, stdout, stderr) = outcome(&project.zaf(&["--json", "report", "--hours", "100000"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, format!("{}\n", report::report(&query).to_json()));

    let (code, stdout, _) = outcome(&project.zaf(&["report", "--match", "absent"]));
    assert_eq!(code, 0);
    assert_eq!(stdout, "no runs\n");

    let (code, stdout, _) = outcome(&project.zaf(&["report", "--all", "--hours", "100000"]));
    assert_eq!(code, 0);
    assert!(stdout.starts_with("runs 3 "), "{stdout}");
}
