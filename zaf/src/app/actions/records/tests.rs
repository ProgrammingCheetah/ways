use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use super::*;
use crate::app::actions::{Caller, MINUTE, Nesting, run_action};

const TASK: u64 = 140;

/// A project and a zaf home in one temp folder, with a directory a body can name.
struct Place {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    workspace: PathBuf,
}

impl Place {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(dir.path()).expect("real path");
        let (root, home, workspace) = (
            base.join("project"),
            base.join("home"),
            base.join("a workspace"),
        );
        for folder in [root.join(".zaf/actions"), home.clone(), workspace.clone()] {
            fs::create_dir_all(folder).expect("mkdir");
        }
        Self {
            _dir: dir,
            root,
            home,
            workspace,
        }
    }

    /// An action of `name` whose body is `body`.
    fn action(&self, name: &str, body: &str) {
        let file = self.root.join(".zaf/actions").join(name);
        let text = format!("#!/bin/sh\n# zaf description: d\n# zaf 1: failed\n{body}\n");
        fs::write(&file, text).expect("write action");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    /// Runs `name` in the project with `args`.
    fn run(&self, name: &str, args: &[&str]) -> RunOutcome {
        self.run_at(Some(&self.root), name, args)
    }

    fn run_at(&self, project_root: Option<&Path>, name: &str, args: &[&str]) -> RunOutcome {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        run_action(&RunRequest {
            name,
            args: &args,
            cwd: &self.root,
            project_root,
            home: &self.home,
            timeout_minutes: 1,
            minute: MINUTE,
            nesting: Nesting::default(),
            caller: Caller::Engineer,
            only: None,
            slice: None,
        })
    }

    fn slices(&self) -> PathBuf {
        self.root.join(".zaf/slices")
    }

    fn records(&self) -> SliceRecords {
        SliceRecords::new(&self.root)
    }

    /// The names in the records' folder, sorted; none when it does not exist.
    fn written(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.slices()) else {
            return Vec::new();
        };
        let mut names: Vec<String> = entries
            .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
            .collect();
        names.sort();
        names
    }
}

fn task() -> IssueNumber {
    IssueNumber::from(TASK)
}

/// A body that writes `value` as its last line, single-quoted for the shell.
fn hands_back(value: &str) -> String {
    format!("echo working\nprintf '%s\\n' '{value}'")
}

/// `outcome` is `FAIL`, exit 2, its headline naming the log.
fn assert_refused(outcome: &RunOutcome) {
    assert_eq!(outcome.result, RunResult::Fail, "{outcome:?}");
    assert_eq!(outcome.exit, 2, "{outcome:?}");
    let log = outcome.log.as_deref().expect("the body ran, so a log");
    let headline = outcome.headline().expect("a FAIL prints");
    assert!(
        headline.contains(&log.display().to_string()),
        "{headline} names no log"
    );
}

#[test]
fn slice_open_writes_the_workspace_record() {
    let place = Place::new();
    let path = place.workspace.display().to_string();
    place.action(SLICE_OPEN, &hands_back(&path));

    let outcome = place.run(SLICE_OPEN, &["140"]);

    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    assert_eq!(outcome.exit, 0);
    assert_eq!(
        fs::read_to_string(place.slices().join("140")).expect("the record"),
        path
    );
    assert_eq!(place.written(), ["140"], "no temp file left behind");
    assert_eq!(
        place.records().workspace(task()).as_ref(),
        Some(&place.workspace)
    );
    assert_eq!(place.records().pr(task()), None);
}

#[test]
fn bad_workspace_value_exits_2() {
    let place = Place::new();
    let file = place.root.join("a-file");
    fs::write(&file, "").expect("write");
    let missing = place.root.join("not-there");
    for value in [
        missing.display().to_string(),
        file.display().to_string(),
        // Exists from where the body runs, and names some other place from where it is read.
        ".".to_owned(),
        ".zaf".to_owned(),
    ] {
        place.action(SLICE_OPEN, &hands_back(&value));
        let outcome = place.run(SLICE_OPEN, &["140"]);
        assert_refused(&outcome);
        assert!(
            outcome.meaning.as_deref().unwrap_or("").contains(&value),
            "{outcome:?}"
        );
        assert_eq!(place.written(), Vec::<String>::new(), "{value}");
    }
}

#[test]
fn slice_pr_writes_the_pr_record() {
    let place = Place::new();
    place.action(SLICE_PR, &hands_back("271"));

    let outcome = place.run(SLICE_PR, &["140"]);

    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    assert_eq!(
        fs::read_to_string(place.slices().join("140.pr")).expect("the record"),
        "271"
    );
    assert_eq!(place.written(), ["140.pr"], "no temp file left behind");
    assert_eq!(place.records().pr(task()), Some(PrNumber::from(271)));
    assert_eq!(place.records().workspace(task()), None);
}

#[test]
fn bad_pr_value_exits_2() {
    let place = Place::new();
    for value in [
        "twelve",
        "#12",
        "-1",
        "+1",
        "1.5",
        "12 ",
        " 12",
        "12 34",
        "0x10",
        "0",
        "99999999999999999999999",
        "١٢",
    ] {
        place.action(SLICE_PR, &hands_back(value));
        assert_refused(&place.run(SLICE_PR, &["140"]));
        assert_eq!(place.written(), Vec::<String>::new(), "{value}");
    }
}

#[test]
fn a_bad_value_leaves_the_last_record_as_it_was() {
    let place = Place::new();
    place.action(SLICE_PR, &hands_back("271"));
    assert_eq!(place.run(SLICE_PR, &["140"]).exit, 0);

    place.action(SLICE_PR, &hands_back("nope"));
    assert_refused(&place.run(SLICE_PR, &["140"]));
    assert_eq!(place.records().pr(task()), Some(PrNumber::from(271)));

    place.action(SLICE_PR, &hands_back("272"));
    assert_eq!(place.run(SLICE_PR, &["140"]).exit, 0);
    assert_eq!(place.records().pr(task()), Some(PrNumber::from(272)));
}

#[test]
fn a_body_that_fails_writes_nothing_and_keeps_its_code() {
    let place = Place::new();
    let path = place.workspace.display().to_string();
    place.action(SLICE_OPEN, &format!("{}\nexit 1", hands_back(&path)));

    let outcome = place.run(SLICE_OPEN, &["140"]);

    assert_eq!(outcome.exit, 1, "{outcome:?}");
    assert_eq!(outcome.meaning.as_deref(), Some("failed"));
    assert_eq!(place.written(), Vec::<String>::new());
}

#[test]
fn no_value_at_all_exits_2() {
    let place = Place::new();
    place.action(SLICE_OPEN, "true");
    assert_refused(&place.run(SLICE_OPEN, &["140"]));
    assert_eq!(place.written(), Vec::<String>::new());
}

#[test]
fn another_name_keeps_nothing() {
    let place = Place::new();
    place.action("slice-opener", &hands_back("not checked"));
    let outcome = place.run("slice-opener", &["140"]);
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    assert_eq!(place.written(), Vec::<String>::new());
}

#[test]
fn a_task_that_is_no_number_writes_nowhere() {
    let place = Place::new();
    let path = place.workspace.display().to_string();
    place.action(SLICE_OPEN, &hands_back(&path));
    place.action(SLICE_PR, &hands_back("271"));

    let args: [&[&str]; 4] = [&[], &["../../escaped"], &["/tmp/escaped"], &["140/.."]];
    for args in args {
        for name in [SLICE_OPEN, SLICE_PR] {
            assert_refused(&place.run(name, args));
        }
    }

    assert_eq!(place.written(), Vec::<String>::new());
    assert!(!place.root.join("escaped").exists());
    assert!(!place.root.join("escaped.pr").exists());
}

#[test]
fn a_task_written_with_its_hash_is_the_same_record() {
    let place = Place::new();
    place.action(SLICE_PR, &hands_back("271"));
    assert_eq!(place.run(SLICE_PR, &["#140"]).exit, 0);
    assert_eq!(place.written(), ["140.pr"]);
}

#[test]
fn outside_a_project_nothing_is_kept() {
    let place = Place::new();
    let actions = place.home.join("actions");
    fs::create_dir_all(&actions).expect("mkdir");
    let file = actions.join(SLICE_PR);
    let body = hands_back("271");
    fs::write(
        &file,
        format!("#!/bin/sh\n# zaf description: d\n# zaf 1: failed\n{body}\n"),
    )
    .expect("write");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("chmod");

    assert_refused(&place.run_at(None, SLICE_PR, &["140"]));
    assert_eq!(place.written(), Vec::<String>::new());
}

#[test]
fn a_link_at_the_record_is_replaced_not_written_through() {
    let place = Place::new();
    let elsewhere = place.root.join("elsewhere");
    fs::write(&elsewhere, "untouched").expect("write");
    fs::create_dir_all(place.slices()).expect("mkdir");
    symlink(&elsewhere, place.slices().join("140.pr")).expect("link");
    place.action(SLICE_PR, &hands_back("271"));

    assert_eq!(place.run(SLICE_PR, &["140"]).exit, 0);

    assert_eq!(fs::read_to_string(&elsewhere).expect("read"), "untouched");
    let record = place.slices().join("140.pr");
    assert!(
        fs::symlink_metadata(&record)
            .expect("record")
            .file_type()
            .is_file()
    );
    assert_eq!(fs::read_to_string(record).expect("read"), "271");
}

#[test]
fn a_refused_value_is_quoted_and_cut_short() {
    let place = Place::new();
    let long = "x".repeat(SHOWN_CHARS * 2);
    place.action(SLICE_PR, &format!("printf '\\033[2J{long}\\n'"));

    let outcome = place.run(SLICE_PR, &["140"]);

    assert_refused(&outcome);
    let meaning = outcome.meaning.expect("a meaning");
    assert!(!meaning.contains('\u{1b}'), "{meaning}");
    assert!(meaning.len() < SHOWN_CHARS + 100, "{meaning}");
}

#[test]
fn hand_back_is_the_last_non_empty_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("log");
    for (text, value) in [
        ("", None),
        ("\n\n", None),
        ("  \n\t\n", None),
        ("one", Some("one")),
        ("one\n", Some("one")),
        ("one\r\n", Some("one")),
        ("one\ntwo\n", Some("two")),
        ("one\ntwo\n\n  \n", Some("two")),
        ("one\n  two \n", Some("  two ")),
    ] {
        fs::write(&log, text).expect("write");
        assert_eq!(hand_back(&log).expect("read").as_deref(), value, "{text:?}");
    }
    assert!(hand_back(&dir.path().join("none")).is_err());
}

#[test]
fn records_that_do_not_hold_read_as_none() {
    let place = Place::new();
    let records = place.records();
    assert_eq!(records.workspace(task()), None);
    assert_eq!(records.pr(task()), None);

    fs::create_dir_all(place.slices()).expect("mkdir");
    fs::write(place.slices().join("140"), "relative/path").expect("write");
    fs::write(place.slices().join("140.pr"), "abc").expect("write");
    assert_eq!(records.workspace(task()), None);
    assert_eq!(records.pr(task()), None);

    // A workspace that has gone is no workspace.
    let gone = place.root.join("gone");
    fs::write(place.slices().join("140"), gone.display().to_string()).expect("write");
    assert_eq!(records.workspace(task()), None);
}

const SLICE_MERGE: &str = "slice-merge";
const SLICE_DROP: &str = "slice-drop";

impl Place {
    fn runs(&self) -> PathBuf {
        self.root.join(".zaf/runs")
    }

    /// The records of `task` and of its pull request `pr`, and a check record each.
    fn leave_records(&self, task: u64, pr: u64) {
        fs::create_dir_all(self.slices()).expect("mkdir");
        fs::create_dir_all(self.runs()).expect("mkdir");
        let workspace = self.workspace.display().to_string();
        let files = [
            (self.slices().join(task.to_string()), workspace),
            (self.slices().join(format!("{task}.pr")), pr.to_string()),
            (
                self.runs().join(format!("check-task-{task}.json")),
                "{}".to_owned(),
            ),
            (
                self.runs().join(format!("check-pr-{pr}.json")),
                "{}".to_owned(),
            ),
        ];
        for (file, text) in files {
            fs::write(file, text).expect("write");
        }
    }

    /// The names in the slices and runs folders, sorted.
    fn left(&self) -> Vec<String> {
        let mut names = self.written();
        if let Ok(entries) = fs::read_dir(self.runs()) {
            names.extend(
                entries.map(|entry| entry.expect("entry").file_name().to_string_lossy().into()),
            );
        }
        names.sort();
        names
    }
}

#[test]
fn forget_removes_the_check_records() {
    let place = Place::new();
    place.leave_records(140, 271);
    place.leave_records(141, 272);

    place.records().forget(task()).expect("forget");

    assert_eq!(
        place.left(),
        ["141", "141.pr", "check-pr-272.json", "check-task-141.json"]
    );
    assert_eq!(place.records().workspace(task()), None);
    assert_eq!(place.records().pr(task()), None);
    // Forgetting what is not there is done.
    place.records().forget(task()).expect("forget again");
    SliceRecords::new(&place.root.join("nowhere"))
        .forget(task())
        .expect("no folders at all");
}

#[test]
fn forget_without_a_pr_record_removes_the_task_check_record() {
    let place = Place::new();
    place.leave_records(140, 271);
    fs::remove_file(place.slices().join("140.pr")).expect("remove");

    place.records().forget(task()).expect("forget");

    // The pull request is unknown, so only its record can be named by the task.
    assert_eq!(place.left(), ["check-pr-271.json"]);
}

#[test]
fn forget_replaces_a_link_rather_than_following_it() {
    let place = Place::new();
    place.leave_records(140, 271);
    let elsewhere = place.root.join("elsewhere");
    fs::write(&elsewhere, "untouched").expect("write");
    let check = place.runs().join("check-task-140.json");
    fs::remove_file(&check).expect("remove");
    symlink(&elsewhere, &check).expect("link");

    place.records().forget(task()).expect("forget");

    assert_eq!(fs::read_to_string(&elsewhere).expect("read"), "untouched");
    assert!(fs::symlink_metadata(&check).is_err());
}

#[test]
fn merge_and_drop_forget_the_records() {
    for name in [SLICE_MERGE, SLICE_DROP] {
        let place = Place::new();
        place.leave_records(140, 271);
        place.leave_records(141, 272);
        place.action(name, "echo done");

        let outcome = place.run(name, &["140"]);

        assert_eq!(outcome.result, RunResult::Ok, "{name}: {outcome:?}");
        assert_eq!(
            place.left(),
            ["141", "141.pr", "check-pr-272.json", "check-task-141.json"],
            "{name}"
        );
    }
}

#[test]
fn a_task_written_with_its_hash_is_forgotten_too() {
    let place = Place::new();
    place.leave_records(140, 271);
    place.action(SLICE_DROP, "true");
    assert_eq!(place.run(SLICE_DROP, &["#140"]).exit, 0);
    assert_eq!(place.left(), Vec::<String>::new());
}

#[test]
fn failed_run_keeps_the_records() {
    let place = Place::new();
    place.leave_records(140, 271);
    let all = place.left();
    assert_eq!(all.len(), 4);

    for name in [SLICE_MERGE, SLICE_DROP] {
        // The body fails.
        place.action(name, "exit 1");
        let outcome = place.run(name, &["140"]);
        assert_eq!(outcome.exit, 1, "{name}: {outcome:?}");
        assert_eq!(place.left(), all, "{name}");

        // The body is refused before it starts.
        let file = place.root.join(".zaf/actions").join(name);
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("chmod");
        let outcome = place.run(name, &["140"]);
        assert_eq!(outcome.result, RunResult::NotRun, "{name}: {outcome:?}");
        assert_eq!(place.left(), all, "{name}");

        // The body exits 0 but names no task: it is a failure and keeps everything.
        place.action(name, "true");
        for args in [&[][..], &["../../escaped"][..], &["140/.."][..]] {
            assert_refused(&place.run(name, args));
            assert_eq!(place.left(), all, "{name} {args:?}");
        }
    }
}

#[test]
fn another_name_forgets_nothing() {
    let place = Place::new();
    place.leave_records(140, 271);
    let all = place.left();
    place.action("slice-merger", "true");
    assert_eq!(place.run("slice-merger", &["140"]).exit, 0);
    assert_eq!(place.left(), all);
}
