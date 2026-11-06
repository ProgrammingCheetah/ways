use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use super::*;
use crate::app::actions::{Caller, MINUTE, Nesting, RunOutcome, RunResult, run_action};
use crate::fakes::FakeBoard;
use crate::ports::Ticket;

const TASK: u64 = 200;
const EPIC: u64 = 103;
const TITLE: &str = "do: a target gets its slice in the environment";

/// A project and a zaf home in one temp folder.
struct Place {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Place {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = fs::canonicalize(dir.path()).expect("real path");
        let (root, home) = (base.join("project"), base.join("home"));
        for folder in [root.join(".zaf/actions"), home.clone()] {
            fs::create_dir_all(folder).expect("mkdir");
        }
        Self {
            _dir: dir,
            root,
            home,
        }
    }

    /// An action that writes its environment, one `NAME=value` per line, to `env` in the root and
    /// leaves `ran` behind.
    fn dumper(&self, name: &str, metadata: &str) {
        let file = self.root.join(".zaf/actions").join(name);
        let text = format!(
            "#!/bin/sh\n# zaf description: d\n# zaf 1: failed\n{metadata}env > \"$ZAF_ROOT/env\"\n: > \"$ZAF_ROOT/ran\"\n"
        );
        fs::write(&file, text).expect("write action");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    /// Records `pr` as the task's pull request file, as `slice-pr` does.
    fn record_pr(&self, task: u64, pr: &str) {
        let slices = self.root.join(".zaf/slices");
        fs::create_dir_all(&slices).expect("mkdir");
        fs::write(slices.join(format!("{task}.pr")), pr).expect("record");
    }

    fn ran(&self) -> bool {
        self.root.join("ran").exists()
    }

    /// The environment the body saw.
    fn env(&self) -> BTreeMap<String, String> {
        fs::read_to_string(self.root.join("env"))
            .expect("the body ran")
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect()
    }

    fn request<'a>(
        &'a self,
        name: &'a str,
        args: &'a [OsString],
        slice: Option<SliceContext<'a>>,
    ) -> RunRequest<'a> {
        RunRequest {
            name,
            args,
            cwd: &self.root,
            project_root: Some(&self.root),
            home: &self.home,
            timeout_minutes: 1,
            minute: MINUTE,
            nesting: Nesting::default(),
            caller: Caller::Engineer,
            only: None,
            slice,
            env: BTreeMap::new(),
        }
    }

    fn run(&self, name: &str, args: &[&str], slice: Option<SliceContext<'_>>) -> RunOutcome {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        run_action(&self.request(name, &args, slice))
    }
}

fn board() -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(Ticket {
        number: IssueNumber::from(TASK),
        epic: Some(IssueNumber::from(EPIC)),
        title: TITLE.to_owned(),
        body: String::new(),
        label: None,
    });
    board
}

fn context<'a>(
    board: &'a dyn Board,
    branches: &'a BranchesSection,
    version: Option<&'a str>,
) -> SliceContext<'a> {
    SliceContext {
        board: Ok(board),
        forge: Err("no forge in this test".to_owned()),
        vcs: Err("no version control in this test".to_owned()),
        branches,
        version,
    }
}

fn failing<'a>(branches: &'a BranchesSection, version: Option<&'a str>) -> SliceContext<'a> {
    SliceContext {
        board: Err("the board is down".to_owned()),
        forge: Err("no forge in this test".to_owned()),
        vcs: Err("no version control in this test".to_owned()),
        branches,
        version,
    }
}

/// The reason of a body that never ran.
fn assert_not_run(outcome: &RunOutcome) -> &str {
    assert_eq!(outcome.result, RunResult::NotRun, "{outcome:?}");
    assert_eq!(outcome.exit, 125, "{outcome:?}");
    assert!(outcome.log.is_none(), "{outcome:?}");
    outcome.meaning.as_deref().expect("a reason")
}

/// The variables as they stand on a command, a removed one as `None`.
fn on_command(command: &Command) -> BTreeMap<String, Option<String>> {
    command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect()
}

/// A command as the caller's environment leaves it: all four variables set to something.
fn caller_command() -> Command {
    let mut command = Command::new("true");
    for var in [SLICE_VAR, PR_VAR, TASK_TITLE_VAR, BASE_VAR] {
        command.env(var, "from the caller");
    }
    command
}

#[test]
fn target_task_gets_its_slice_env() {
    let place = Place::new();
    place.dumper("slice-thing", "# zaf target: task\n");
    place.record_pr(TASK, "17");
    let (board, branches) = (board(), BranchesSection::default());

    let outcome = place.run(
        "slice-thing",
        &["200"],
        Some(context(&board, &branches, Some("0.2.0"))),
    );
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");

    let env = place.env();
    assert_eq!(env[SLICE_VAR], "feature/0.2.0-103-200");
    assert_eq!(env[PR_VAR], "17");
    assert_eq!(env[TASK_TITLE_VAR], TITLE);
    assert_eq!(env[BASE_VAR], "feature/0.2.0");
}

#[test]
fn target_task_without_a_record_has_no_pr() {
    let place = Place::new();
    place.dumper("slice-thing", "# zaf target: task\n");
    let (board, branches) = (board(), BranchesSection::default());

    let outcome = place.run(
        "slice-thing",
        &["#200"],
        Some(context(&board, &branches, Some("0.2.0"))),
    );
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    let env = place.env();
    assert_eq!(env[SLICE_VAR], "feature/0.2.0-103-200");
    assert!(!env.contains_key(PR_VAR), "{env:?}");

    // A record that is no pull request number is no record.
    place.record_pr(TASK, "seventeen");
    let outcome = place.run(
        "slice-thing",
        &["200"],
        Some(context(&board, &branches, Some("0.2.0"))),
    );
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    assert!(!place.env().contains_key(PR_VAR));
}

#[test]
fn caller_cannot_set_the_slice_env() {
    let place = Place::new();
    let (board, branches) = (board(), BranchesSection::default());
    let args = [OsString::from("200")];
    let request = place.request("x", &args, Some(context(&board, &branches, Some("0.2.0"))));

    // No record: the caller's pull request is removed, not left standing.
    let mut command = caller_command();
    SliceEnv::for_run(&request, true)
        .expect("resolves")
        .apply(&mut command);
    let vars = on_command(&command);
    assert_eq!(vars[SLICE_VAR].as_deref(), Some("feature/0.2.0-103-200"));
    assert_eq!(vars[TASK_TITLE_VAR].as_deref(), Some(TITLE));
    assert_eq!(vars[BASE_VAR].as_deref(), Some("feature/0.2.0"));
    assert_eq!(vars[PR_VAR], None);

    // With a record, its number replaces the caller's.
    place.record_pr(TASK, "9");
    let mut command = caller_command();
    SliceEnv::for_run(&request, true)
        .expect("resolves")
        .apply(&mut command);
    assert_eq!(on_command(&command)[PR_VAR].as_deref(), Some("9"));
}

#[test]
fn board_failure_is_not_run() {
    let place = Place::new();
    place.dumper("slice-thing", "# zaf target: task\n");
    let branches = BranchesSection::default();

    // A board that cannot be built or reached.
    let outcome = place.run(
        "slice-thing",
        &["200"],
        Some(failing(&branches, Some("0.2.0"))),
    );
    let reason = assert_not_run(&outcome);
    assert!(reason.contains("the board is down"), "{reason}");

    // A board that has no such ticket.
    let empty = FakeBoard::default();
    let outcome = place.run(
        "slice-thing",
        &["200"],
        Some(context(&empty, &branches, Some("0.2.0"))),
    );
    let reason = assert_not_run(&outcome);
    assert!(reason.contains("#200"), "{reason}");

    // Nothing started.
    assert!(!place.ran());
}

#[test]
fn target_task_that_cannot_name_its_task_or_slice_is_not_run() {
    let place = Place::new();
    place.dumper("slice-thing", "# zaf target: task\n");
    let (board, branches) = (board(), BranchesSection::default());

    let outcome = place.run(
        "slice-thing",
        &[],
        Some(context(&board, &branches, Some("0.2.0"))),
    );
    assert!(assert_not_run(&outcome).contains("first argument"));
    let outcome = place.run(
        "slice-thing",
        &["soon"],
        Some(context(&board, &branches, Some("0.2.0"))),
    );
    assert!(assert_not_run(&outcome).contains("first argument"));

    // No release version for a pattern that names it: the slice cannot be named.
    let outcome = place.run(
        "slice-thing",
        &["200"],
        Some(context(&board, &branches, None)),
    );
    assert!(assert_not_run(&outcome).contains("#200"));

    // No project to ask.
    let outcome = place.run("slice-thing", &["200"], None);
    assert!(assert_not_run(&outcome).contains("no project"));
    assert!(!place.ran());
}

#[test]
fn targetless_run_gets_no_slice_env() {
    let place = Place::new();
    place.dumper("plain", "");
    // The board is not asked: a run with no target does not need it.
    let branches = BranchesSection::default();
    let outcome = place.run("plain", &["200"], Some(failing(&branches, Some("0.2.0"))));
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    let env = place.env();
    for var in TASK_VARS {
        assert!(!env.contains_key(var), "{var} in {env:?}");
    }
    assert_eq!(env[BASE_VAR], "feature/0.2.0");

    // What the caller handed on is removed from the command, not passed through.
    let request = place.request("plain", &[], Some(failing(&branches, Some("0.2.0"))));
    let mut command = caller_command();
    SliceEnv::for_run(&request, false)
        .expect("no board needed")
        .apply(&mut command);
    let vars = on_command(&command);
    for var in TASK_VARS {
        assert_eq!(vars[var], None, "{var}");
    }
    assert_eq!(vars[BASE_VAR].as_deref(), Some("feature/0.2.0"));
}

#[test]
fn base_unset_without_a_version() {
    let place = Place::new();
    place.dumper("plain", "");
    let (board, branches) = (board(), BranchesSection::default());

    // The default pattern names `{version}`: filled when there is one, absent when there is not.
    let outcome = place.run(
        "plain",
        &[],
        Some(context(&board, &branches, Some("0.2.0"))),
    );
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    assert_eq!(place.env()[BASE_VAR], "feature/0.2.0");

    let outcome = place.run("plain", &[], Some(context(&board, &branches, None)));
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    assert!(!place.env().contains_key(BASE_VAR));

    // Outside a project there is no pattern.
    let outcome = place.run("plain", &[], None);
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    assert!(!place.env().contains_key(BASE_VAR));

    // A caller's value does not stand in for one zaf cannot fill.
    let request = place.request("plain", &[], Some(context(&board, &branches, None)));
    let mut command = caller_command();
    SliceEnv::for_run(&request, false)
        .expect("no board needed")
        .apply(&mut command);
    assert_eq!(on_command(&command)[BASE_VAR], None);
}

#[test]
fn base_follows_the_pattern() {
    let board = board();
    // A pattern that needs a ticket cannot be filled by a run that names none.
    let ticketed = BranchesSection {
        integration: "release/{epic}".into(),
        ..BranchesSection::default()
    };
    assert_eq!(base_of(Some(&context(&board, &ticketed, Some("1")))), None);
    // One that names no version needs none.
    let fixed = BranchesSection {
        integration: "main".into(),
        ..BranchesSection::default()
    };
    assert_eq!(
        base_of(Some(&context(&board, &fixed, None))).as_deref(),
        Some("main")
    );
}
