use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::json;

use super::*;

/// The real keeper role text: a keeper's first prompt opens with it.
const KEEPER_ROLE_TEXT: &str = include_str!("../../../../assets/keeper.md");

const USER_TIME: &str = "2026-11-04T10:00:00.000Z";
const TURN_TIME: &str = "2026-11-04T10:00:05.000Z";

/// A temp dir holding a ledger and one Claude `projects/` folder.
struct Scene {
    _temp: tempfile::TempDir,
    db: PathBuf,
    projects: PathBuf,
}

impl Scene {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let db = temp.path().join("home").join(LEDGER_FILE);
        let projects = temp.path().join("projects");
        fs::create_dir_all(&projects).expect("projects");
        Self {
            _temp: temp,
            db,
            projects,
        }
    }

    fn sources(&self) -> Vec<Source> {
        vec![Source {
            root: self.projects.clone(),
            project: Some("ways".to_owned()),
        }]
    }

    fn write(&self, path: &str, content: &str) -> PathBuf {
        let file = self.projects.join(path);
        fs::create_dir_all(file.parent().expect("parent")).expect("folder");
        fs::write(&file, content).expect("transcript");
        file
    }

    fn ingest(&self) -> usize {
        Ledger::open(&self.db)
            .expect("open")
            .ingest(&self.sources())
            .expect("ingest")
    }

    fn rows(&self) -> Vec<LedgerRow> {
        Ledger::open(&self.db).expect("open").rows().expect("rows")
    }

    fn row(&self, run_id: &str) -> LedgerRow {
        self.rows()
            .into_iter()
            .find(|row| row.run_id == run_id)
            .unwrap_or_else(|| panic!("no row for {run_id}"))
    }

    /// What one prompt gives, in a main session.
    fn attributed(&self, prompt: &str) -> LedgerRow {
        self.write("-demo/one.jsonl", &transcript(prompt));
        self.ingest();
        self.row("one")
    }
}

fn user_line(prompt: &str) -> String {
    json!({
        "type": "user",
        "timestamp": USER_TIME,
        "message": {"role": "user", "content": prompt},
    })
    .to_string()
}

fn turn_line(id: &str, input: u64, write: u64, read: u64, output: u64) -> String {
    json!({
        "type": "assistant",
        "timestamp": TURN_TIME,
        "message": {
            "id": id,
            "model": "claude-opus-5-5",
            "usage": {
                "input_tokens": input,
                "cache_creation_input_tokens": write,
                "cache_read_input_tokens": read,
                "output_tokens": output,
            },
        },
    })
    .to_string()
}

fn transcript(prompt: &str) -> String {
    format!("{}\n{}\n", user_line(prompt), turn_line("m1", 1, 2, 3, 4))
}

fn append(file: &Path, line: &str) {
    let mut text = fs::read_to_string(file).expect("read");
    text.push_str(line);
    text.push('\n');
    fs::write(file, text).expect("append");
}

#[test]
fn ledger_is_created_with_schema_1() {
    let scene = Scene::new();
    assert!(!scene.db.exists());
    let ledger = Ledger::open(&scene.db).expect("open");
    assert!(ledger.rows().expect("rows").is_empty());
    drop(ledger);
    assert!(scene.db.is_file(), "the file and its folder are created");
    let connection = Connection::open(&scene.db).expect("connect");
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("version");
    assert_eq!(version, 1);
    assert_eq!(SCHEMA_VERSION, 1);
    let tables: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'runs'",
            [],
            |row| row.get(0),
        )
        .expect("tables");
    assert_eq!(tables, 1);
    Ledger::open(&scene.db).expect("an existing ledger opens again");
}

#[test]
fn a_row_holds_the_whole_run() {
    let scene = Scene::new();
    let lines = [
        user_line("You are team instance 15 on #103, the team. Tasks: #146 #147."),
        turn_line("m1", 10, 20, 30, 40),
        turn_line("m1", 10, 20, 30, 40),
        turn_line("m2", 1, 2, 300, 4),
    ];
    let file = scene.write("-demo/session.jsonl", &(lines.join("\n") + "\n"));
    assert_eq!(scene.ingest(), 1);
    let row = scene.row("session");
    let (size, modified_ns) = stamp_of(&file).expect("stamp");
    assert_eq!(
        row,
        LedgerRow {
            run_id: "session".to_owned(),
            parent_run: None,
            project: "ways".to_owned(),
            role: Some("team".to_owned()),
            epic: Some(103),
            tasks: vec![146, 147],
            model: "claude-opus-5-5".to_owned(),
            first_time: Some(USER_TIME.to_owned()),
            last_time: Some(TURN_TIME.to_owned()),
            turns: 2,
            input: 11,
            cache_write: 22,
            cache_read: 330,
            output: 44,
            context_first: 60,
            context_last: 303,
            first_prompt: "You are team instance 15 on #103, the team. Tasks: #146 #147."
                .to_owned(),
            path: file.to_string_lossy().into_owned(),
            size,
            modified_ns,
        }
    );
}

#[test]
fn a_source_without_a_project_names_the_claude_folder() {
    let temp = tempfile::tempdir().expect("temp dir");
    let state_dir = temp.path().join("state");
    let profile = state_dir.join("profiles/build");
    let config = temp.path().join("config");
    fs::create_dir_all(&profile).expect("profile");
    fs::create_dir_all(&config).expect("config");
    let sources = sources(&state_dir, "ways", Some(&config));
    assert_eq!(
        sources,
        vec![
            Source {
                root: profile.join("projects"),
                project: Some("ways".to_owned()),
            },
            Source {
                root: config.join("projects"),
                project: None,
            },
        ]
    );
    let file = config.join("projects/-home-zuri-other/s.jsonl");
    fs::create_dir_all(file.parent().expect("parent")).expect("folder");
    fs::write(&file, transcript("hello")).expect("transcript");
    let mut ledger = Ledger::open(&temp.path().join(LEDGER_FILE)).expect("open");
    assert_eq!(ledger.ingest(&sources).expect("ingest"), 1);
    assert_eq!(ledger.rows().expect("rows")[0].project, "-home-zuri-other");
}

#[test]
fn a_config_dir_that_is_a_profile_counts_once() {
    let temp = tempfile::tempdir().expect("temp dir");
    let state_dir = temp.path().join("state");
    let profile = state_dir.join("profiles/build");
    fs::create_dir_all(&profile).expect("profile");
    let sources = sources(&state_dir, "ways", Some(&profile));
    assert_eq!(
        sources,
        vec![Source {
            root: profile.join("projects"),
            project: Some("ways".to_owned()),
        }]
    );
}

#[test]
fn unchanged_transcript_changes_nothing() {
    let scene = Scene::new();
    scene.write("-demo/session.jsonl", &transcript("hello"));
    assert_eq!(scene.ingest(), 1);
    let before = scene.rows();
    assert_eq!(scene.ingest(), 0);
    assert_eq!(scene.rows(), before);
    // A file that was read again would overwrite this mark.
    let connection = Connection::open(&scene.db).expect("connect");
    connection
        .execute("UPDATE runs SET turns = 99", [])
        .expect("mark");
    drop(connection);
    let bytes = fs::read(&scene.db).expect("bytes");
    assert_eq!(scene.ingest(), 0);
    assert_eq!(scene.row("session").turns, 99);
    assert_eq!(fs::read(&scene.db).expect("bytes"), bytes);
}

#[test]
fn grown_transcript_updates_its_row() {
    let scene = Scene::new();
    let file = scene.write("-demo/session.jsonl", &transcript("hello"));
    assert_eq!(scene.ingest(), 1);
    let before = scene.row("session");
    assert_eq!(before.turns, 1);
    append(&file, &turn_line("m2", 5, 6, 700, 8));
    assert_eq!(scene.ingest(), 1);
    let after = scene.row("session");
    assert_eq!(scene.rows().len(), 1, "the same row, not a second one");
    assert_eq!(after.turns, 2);
    assert_eq!(after.output, 12);
    assert_eq!(after.context_first, before.context_first);
    assert_eq!(after.context_last, 711);
    assert!(after.size > before.size);
}

#[test]
fn deleted_transcript_keeps_its_row() {
    let scene = Scene::new();
    let file = scene.write("-demo/session.jsonl", &transcript("hello"));
    scene.ingest();
    let before = scene.rows();
    assert_eq!(before.len(), 1);
    fs::remove_file(&file).expect("delete");
    assert_eq!(scene.ingest(), 0);
    assert_eq!(scene.rows(), before);
}

#[test]
fn attribution_reads_the_instance_line() {
    let scene = Scene::new();
    let row = scene.attributed(
        "You are team instance 15 on #103, the team instance for epic #103 of project ways. \
         Tasks: #146 #147. Read /brief.md first and follow it exactly.",
    );
    assert_eq!(row.role.as_deref(), Some("team"));
    assert_eq!(row.epic, Some(103));
    assert_eq!(row.tasks, vec![146, 147]);

    let far = format!("{} planning instance 2 on #7.", "x ".repeat(200));
    let row = scene.attributed(&far);
    assert_eq!(row.role.as_deref(), Some("planning"));
    assert_eq!(row.epic, Some(7), "past the 120 characters too");

    let row = scene.attributed("Finish task #12 and Task #13, then task #12 again");
    assert_eq!(row.tasks, vec![12, 13], "`task #<n>` names a task, once");
    assert_eq!(row.role, None);
    assert_eq!(row.epic, None, "anything else has no epic");

    let row = scene.attributed("team instance x on #7, and a task #0 and task 5");
    assert_eq!((row.role, row.epic), (None, None));
    assert!(row.tasks.is_empty());
}

#[test]
fn attribution_opening_epic_is_planning() {
    let scene = Scene::new();
    let row = scene.attributed("  Epic #103: plan the release");
    assert_eq!(row.role.as_deref(), Some("planning"));
    assert_eq!(row.epic, Some(103));

    let row = scene.attributed("Epic #103. Task #5 first.");
    assert_eq!(row.tasks, vec![5]);

    for not_opening in ["Plan Epic #103", "Epical #103", "epic #103", "Epic 103"] {
        let row = scene.attributed(not_opening);
        assert_eq!((row.role, row.epic), (None, None), "{not_opening}");
    }
}

#[test]
fn attribution_keeper_for_line_past_the_first_chars() {
    let ask = |asker: &str| {
        format!(
            "{KEEPER_ROLE_TEXT}\n\n## The ask\n\nfrom: {asker}\nfor: #146\ncontext: a ledger\n\
             questions:\n  q1: rule  | Is a run's project its folder?\n\n\
             ## Pages\n\nA page naming team instance 9 on #7 and task #8.\n"
        )
    };
    let prompt = ask("programmer instance 3 on #103");
    assert!(prompt.find("for: #146").expect("the line") > 120);
    let row = scene_keeper(&prompt);
    assert_eq!(row.role.as_deref(), Some("keeper"));
    assert_eq!(row.tasks, vec![146]);
    assert_eq!(row.epic, Some(103), "the asker's epic, never a page's");
    assert!(!row.first_prompt.contains("for:"));

    let row = scene_keeper(&ask("local"));
    assert_eq!(row.role.as_deref(), Some("keeper"));
    assert_eq!(row.epic, None);

    let row =
        scene_keeper("The for: #146 is mid-line, and this one is not a task:\nfor: #146 and on");
    assert_eq!(row.role, None, "only a line that is nothing else");
}

fn scene_keeper(prompt: &str) -> LedgerRow {
    Scene::new().attributed(prompt)
}

#[test]
fn subagent_takes_its_parents_epic() {
    let scene = Scene::new();
    let parent = scene.write(
        "-demo/session.jsonl",
        &transcript("You are team instance 15 on #103, the team."),
    );
    scene.write(
        "-demo/session/subagents/agent-a.jsonl",
        &transcript("Task #146 of project ways, epic #103."),
    );
    let reviewer = scene.write(
        "-demo/session/subagents/agent-b.jsonl",
        &transcript("You are reviewer instance 2 on #55."),
    );
    assert_eq!(scene.ingest(), 3);
    let programmer = scene.row("session/agent-a");
    assert_eq!(programmer.parent_run.as_deref(), Some("session"));
    assert_eq!(programmer.role.as_deref(), Some("programmer"));
    assert_eq!(programmer.epic, Some(103));
    assert_eq!(programmer.tasks, vec![146]);
    let named = scene.row("session/agent-b");
    assert_eq!(named.role.as_deref(), Some("reviewer"), "its own role");
    assert_eq!(named.epic, Some(103), "its parent's epic over its own");

    // The parent's transcript is gone: its row still gives the epic to a grown subagent.
    fs::remove_file(&parent).expect("delete");
    append(&reviewer, &turn_line("m2", 1, 1, 1, 1));
    assert_eq!(scene.ingest(), 1);
    assert_eq!(scene.row("session/agent-b").epic, Some(103));
    assert_eq!(scene.row("session/agent-b").turns, 2);
}

#[test]
fn a_subagent_read_before_its_parent_takes_the_epic_later() {
    let scene = Scene::new();
    scene.write(
        "-demo/late/subagents/agent-a.jsonl",
        &transcript("Build the thing."),
    );
    scene.ingest();
    let alone = scene.row("late/agent-a");
    assert_eq!(alone.role.as_deref(), Some("programmer"));
    assert_eq!(alone.epic, None);
    scene.write("-demo/late.jsonl", &transcript("Epic #9: plan it"));
    assert_eq!(scene.ingest(), 1);
    assert_eq!(scene.row("late/agent-a").epic, Some(9));
}

#[test]
fn no_dollars_are_stored() {
    let scene = Scene::new();
    let secret = "SECRET-PAST-THE-NAME";
    let prompt = format!("{} {secret}", "word ".repeat(60));
    scene.write("-demo/session.jsonl", &transcript(&prompt));
    assert_eq!(scene.ingest(), 1);
    let row = scene.row("session");
    assert_eq!(row.first_prompt.chars().count(), 120);

    let connection = Connection::open(&scene.db).expect("connect");
    let mut statement = connection
        .prepare("SELECT name FROM pragma_table_info('runs')")
        .expect("columns");
    let columns: Vec<String> = statement
        .query_map([], |column| column.get(0))
        .expect("columns")
        .collect::<Result<_, _>>()
        .expect("columns");
    assert!(columns.len() >= 16);
    for column in &columns {
        for money in ["cost", "usd", "dollar", "price", "est"] {
            assert!(!column.contains(money), "{column} holds {money}");
        }
    }

    let file = fs::read(&scene.db).expect("bytes");
    let held = String::from_utf8_lossy(&file);
    assert!(held.contains("word word"), "the file is plain text");
    assert!(!held.contains(secret), "no prompt past the 120 characters");
}
