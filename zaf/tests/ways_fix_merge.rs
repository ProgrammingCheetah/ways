//! A merge whose base conflicts starts `fix`, and the fix round can bring the base in (slice 8;
//! docs/zaf-actions.md §5.1, §7). Through the binary, on a throwaway jj repository with a bare
//! remote, the pack's own actions, a fake `gh` whose pull request head is what the remote holds
//! and whose checks pass once a test says so, and a fake programmer.
//!
//! `merge` exits 1 for a conflict only, so the scheduler starts `fix`; the fix brief's `Why this
//! run` names the paths; `sync-base` leaves the conflicted merge in the workspace with the work
//! that was there, and pushes nothing; once the fix publishes the merge, the ticket waits for the
//! checks at the new head, is reviewed there and merged. A base that moved on and merges cleanly
//! is merged with no new round, and a merge the forge refused is not a failure to fix. A ruling on a question a review raised
//! reaches the fix once, in its brief, and a fix that finds nothing to change lets the review
//! apply again.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{Project, REPO, jj, outcome, run, write};

const BASE: &str = "feature/v1";
const SLICE: &str = "feature/v1-1-2";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"

[vcs]
kind = "jj"

[runners.claude]
command = "true"

[phases]
Task = ["implement", "ci", "review", "merge"]
"#;

/// The pack's actions the phases run, and the steps they call.
const ACTIONS: [&str; 19] = [
    "workspace-open",
    "workspace-drop",
    "commit",
    "push",
    "pr-open",
    "merge-base",
    "pr-merge",
    "slice-open",
    "slice-pr",
    "slice-merge",
    "slice-drop",
    "sync-base",
    "implement",
    "ci",
    "ci-wait",
    "ci-rerun",
    "review",
    "merge",
    "fix",
];

/// How long a launched phase and the tick it asks for get to end.
const PATIENCE: Duration = Duration::from_secs(120);

/// Task 2 of epic 1, a Task in the task template, claimed by the implement instance.
fn ticket() -> serde_json::Value {
    serde_json::json!({
        "number": 2,
        "title": "Export the list",
        "body": "Goal: Members export their reading list as CSV.\n\
                 Owned paths: src/export/**\n\
                 Interfaces: none\n\
                 Design pages: none\n\
                 Acceptance:\n  - the export writes one row per book\n\
                 Tests: export_writes_rows\n\
                 Security trigger: no\n\
                 Model: sonnet\n\
                 Size: S\n\
                 Blocked by: —\n",
        "labels": [{"name": "task"}, {"name": "claimed"}],
        "parent": {"number": 1},
        "state": "OPEN",
        "comments": [{"body": "claimed by implement instance 1 on #1 at 2027-01-15T08:00:00Z"}],
        "issueType": {"name": "Task", "id": "IT_1"},
    })
}

/// An action as the ways repository ships it, byte for byte the pack's (`pack_actions.rs`).
fn action_text(name: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions")
        .join(name);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The scene: the project and where the fakes keep what they were told.
struct Scene {
    project: Project,
}

impl Scene {
    /// A project whose root is a jj repository with the base pushed to a bare remote, the pack's
    /// actions, the built `zaf`, a fake `claude` and a `gh` in front of the fake one, which answers
    /// the pull request's head with what the remote's slice branch holds and its checks as
    /// passed once [`Scene::green`] marked that head.
    fn new() -> Self {
        let mut project = Project::new(MANIFEST);
        let temp = project.temp.path().to_owned();
        let root = project.root.clone();
        run(&temp, "git", &["init", "-q", "--bare", "remote.git"]);
        jj(&temp, &["git", "init", "repo"]);
        jj(
            &root,
            &["config", "set", "--repo", "user.name", "Test Author"],
        );
        jj(
            &root,
            &[
                "config",
                "set",
                "--repo",
                "user.email",
                "author@example.com",
            ],
        );
        let remote = temp.join("remote.git");
        jj(
            &root,
            &["git", "remote", "add", "origin", &remote.to_string_lossy()],
        );
        write(&root, ".gitignore", ".zaf/*\n");
        write(&root, "notes.txt", "one\ntwo\n");
        jj(&root, &["commit", "-m", "Start the project"]);
        jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
        jj(&root, &["git", "push", "--bookmark", BASE]);

        let ticket = ticket().to_string();
        project
            .on(
                "issue list --label task --state open *",
                &format!("[{ticket}]"),
            )
            .on("issue list --label claimed *", "[{\"number\":2}]")
            .on("issue view 2 *", &ticket)
            .on("issue list --label epic *", "[]")
            .on("issue list --label question *", "[]")
            .on("issue list --label task --state all *", "[]")
            .on(&format!("pr list --head {SLICE} *"), "")
            .on(
                "pr create *",
                &format!("https://github.com/{REPO}/pull/7\n"),
            )
            .on(
                "pr view 7 --json state,createdAt,mergedAt*",
                r#"{"state":"OPEN","createdAt":"2027-01-15T09:00:00Z","mergedAt":null}"#,
            )
            .on("pr view 7 --json state --jq *", "OPEN\n")
            .on("pr view 7 --json state *", r#"{"state":"OPEN"}"#)
            .on("api *pulls/7/files*", r#"[[{"filename":"notes.txt"}]]"#)
            .on("api *", "[[]]")
            .on("pr merge *", "")
            .on("issue edit *", "")
            .on("issue close *", "")
            .on("issue comment *", "")
            .on("*", "[]")
            .env("ZAF_VERSION", "v1");

        // The `gh` in front: the head and the checks are the remote's, the rest the fake's.
        let front = temp.join("front-bin");
        let script = format!(
            "#!/bin/sh\n\
             remote='{remote}'\n\
             head=$(git -C \"$remote\" rev-parse --verify -q 'refs/heads/{SLICE}')\n\
             case \"$*\" in\n\
             \"pr view 7 --json headRefOid --jq .headRefOid\"*)\n\
             printf '%s\\n' \"$head\"; exit 0 ;;\n\
             \"pr view 7 --json state,createdAt,mergedAt,mergeable,headRefOid\"*)\n\
             printf '{{\"state\":\"OPEN\",\"createdAt\":\"2027-01-15T09:00:00Z\",\"mergedAt\":null,\"headRefOid\":\"%s\"}}\\n' \"$head\"; exit 0 ;;\n\
             \"pr checks 7 --json bucket,name,link\"*)\n\
             if [ -n \"$head\" ] && [ -f \"$FAKE_GH_DIR/green-$head\" ]; then\n\
             printf 'pass\\tci\\t\\n'\n\
             else\n\
             printf 'pending\\tci\\t\\n'\n\
             fi\n\
             exit 0 ;;\n\
             \"pr checks 7 --json name,bucket,completedAt\"*)\n\
             if [ -n \"$head\" ] && [ -f \"$FAKE_GH_DIR/green-$head\" ]; then\n\
             printf '[{{\"name\":\"ci\",\"bucket\":\"pass\",\"completedAt\":\"2027-01-15T09:30:00Z\"}}]\\n'\n\
             else\n\
             printf '[{{\"name\":\"ci\",\"bucket\":\"pending\",\"completedAt\":null}}]\\n'\n\
             fi\n\
             exit 0 ;;\n\
             esac\n\
             exec '{fixtures}/gh' \"$@\"\n",
            remote = remote.display(),
            fixtures = common::fixtures().display(),
        );
        write(&front, "gh", &script);
        fs::set_permissions(front.join("gh"), fs::Permissions::from_mode(0o755)).expect("mode");

        // The fake programmer: it keeps each prompt, runs the commands the test left in `do`, one
        // per line, from the project root, and writes `resolved` over `notes.txt` when the test
        // left that file.
        let folder = temp.join("programmer");
        fs::create_dir_all(&folder).expect("the programmer's folder");
        let record = folder.display();
        let claude = format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$2\" >> '{record}/prompts'\n\
             workspace=${{2##*Workspace: }}\n\
             workspace=${{workspace%%. Findings:*}}\n\
             workspace=${{workspace%.}}\n\
             if [ -f '{record}/do' ]; then\n\
             while IFS= read -r line; do sh -c \"$line\" >> '{record}/did' 2>&1; \
             echo \"exit $?\" >> '{record}/did'; done < '{record}/do'\n\
             fi\n\
             if [ -f '{record}/resolved' ]; then cp '{record}/resolved' \"$workspace/notes.txt\"; fi\n"
        );
        let bin = temp.join("programmer-bin");
        write(&bin, "claude", &claude);
        fs::set_permissions(bin.join("claude"), fs::Permissions::from_mode(0o755)).expect("mode");
        project.env(
            "PATH",
            &common::path_with_zaf(&[&bin, &front, &common::fixtures()]),
        );

        // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
        let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
        assert_eq!(code, 0, "{stdout}{stderr}");
        let actions = root.join(".zaf/actions");
        for name in ACTIONS {
            write(&actions, name, &action_text(name));
            fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750))
                .expect("mode");
        }
        // A tick refuses profiles that are not current.
        let config = temp.join("claude-config");
        fs::create_dir_all(&config).expect("config");
        project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
        let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
        assert_eq!(code, 0, "{stdout}{stderr}");
        Self { project }
    }

    /// A scene whose task 2 has its slice open with `notes.txt` set to `content` and published
    /// as pull request 7, the head the remote holds: implement is done.
    fn published(content: &str) -> Self {
        let scene = Self::new();
        let (code, text) = scene.action(&["slice-open", "2"]);
        assert_eq!(code, 0, "{text}");
        write(&scene.workspace(), "notes.txt", content);
        let (code, text) = scene.action(&["slice-pr", "2"]);
        assert_eq!(code, 0, "{text}");
        assert_eq!(scene.head(), scene.remote_slice());
        scene
    }

    /// `zaf do --more <args>` from the project root: the exit code and the whole log.
    fn action(&self, args: &[&str]) -> (i32, String) {
        let mut command = vec!["do", "--more"];
        command.extend_from_slice(args);
        let (code, stdout, stderr) = outcome(&self.project.zaf(&command));
        (code, format!("{stdout}{stderr}"))
    }

    /// [`Scene::action`] as the fix round's programmer runs it: under the fix phase's instance.
    fn as_fix(&self, args: &[&str]) -> (i32, String) {
        let mut command = vec!["do", "--more"];
        command.extend_from_slice(args);
        let output = self
            .project
            .command_in(&self.project.root, &command)
            .env("ZAF_INSTANCE", "fix instance 9 on #1")
            .output()
            .expect("run zaf");
        let (code, stdout, stderr) = outcome(&output);
        (code, format!("{stdout}{stderr}"))
    }

    /// The slice's workspace.
    fn workspace(&self) -> PathBuf {
        self.project
            .root
            .canonicalize()
            .expect("canonical root")
            .join(".zaf/workspaces")
            .join(SLICE)
    }

    /// The workspace's latest commit, the one a push publishes.
    fn head(&self) -> String {
        jj(
            &self.workspace(),
            &["log", "--no-graph", "-r", "@-", "-T", "commit_id"],
        )
    }

    /// What the remote holds under the slice's branch: the pull request's head.
    fn remote_slice(&self) -> String {
        run(
            &self.project.temp.path().join("remote.git"),
            "git",
            &["rev-parse", SLICE],
        )
        .trim()
        .to_owned()
    }

    /// Marks the checks at the pull request's current head as passed.
    fn green(&self) {
        let scene = self.project.temp.path().join("gh-scene");
        write(&scene, &format!("green-{}", self.remote_slice()), "");
    }

    /// Pushes a commit that sets `path` to `content` onto the base from a second clone, as another
    /// merged pull request would.
    fn advance_base_elsewhere(&self, path: &str, content: &str) {
        let temp = self.project.temp.path();
        let other = temp.join("other");
        if !other.exists() {
            let remote = temp.join("remote.git");
            run(
                temp,
                "git",
                &["clone", "-q", &remote.to_string_lossy(), "other"],
            );
            run(&other, "git", &["checkout", "-q", BASE]);
        }
        run(&other, "git", &["pull", "-q", "origin", BASE]);
        write(&other, path, content);
        run(&other, "git", &["add", path]);
        run(
            &other,
            "git",
            &[
                "-c",
                "user.name=Other",
                "-c",
                "user.email=other@example.com",
                "commit",
                "-qm",
                "Another pull request",
            ],
        );
        run(&other, "git", &["push", "-q", "origin", BASE]);
    }

    /// `zaf spawn phase 2 <action>`, then waits until every process it started has ended, the
    /// tick its launcher asks for included.
    fn spawn_phase(&self, action: &str) {
        let (code, stdout, stderr) = outcome(&self.project.zaf(&["spawn", "phase", "2", action]));
        assert_eq!(code, 0, "{stdout}{stderr}");
        self.await_quiet();
    }

    /// One tick, `zaf spawn --once`, then waits until what it launched has ended.
    fn tick(&self) -> String {
        let (code, stdout, stderr) =
            outcome(&self.project.zaf(&["spawn", "--once", "--version", "v1"]));
        assert_eq!(code, 0, "{stdout}{stderr}");
        self.await_quiet();
        stdout
    }

    /// Waits until no process works in the project: none runs in it or names it.
    fn await_quiet(&self) {
        let root = self.project.root.canonicalize().expect("canonical root");
        let root_text = root.to_string_lossy().into_owned();
        let deadline = Instant::now() + PATIENCE;
        loop {
            let busy = working_in(&root, &root_text);
            if busy.is_empty() {
                return;
            }
            assert!(Instant::now() < deadline, "still running: {busy:?}");
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// The run folders of task 2, oldest first, each with the action it ran.
    fn runs(&self) -> Vec<(String, PathBuf)> {
        let Ok(entries) = fs::read_dir(self.project.root.join(".zaf/phases")) else {
            return Vec::new();
        };
        let mut runs: Vec<(String, PathBuf)> = entries
            .map(|entry| entry.expect("entry").path())
            .filter(|dir| fs::read_to_string(dir.join("task")).is_ok_and(|task| task.trim() == "2"))
            .map(|dir| {
                let action = fs::read_to_string(dir.join("action")).expect("action");
                (action.trim().to_owned(), dir)
            })
            .collect();
        runs.sort_by_key(|(_, dir)| {
            fs::metadata(dir.join("task"))
                .and_then(|meta| meta.modified())
                .expect("started")
        });
        runs
    }

    /// The run folders of `action`, oldest first.
    fn runs_of(&self, action: &str) -> Vec<PathBuf> {
        self.runs()
            .into_iter()
            .filter(|(name, _)| name == action)
            .map(|(_, dir)| dir)
            .collect()
    }

    /// Where the dry run places task 2: its `place` and its `next`, or what it waits on.
    fn placement(&self) -> serde_json::Value {
        let (code, stdout, stderr) = outcome(&self.project.zaf(&[
            "--json",
            "spawn",
            "--once",
            "--dry-run",
            "--version",
            "v1",
        ]));
        assert_eq!(code, 0, "{stdout}{stderr}");
        let json: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON document");
        let tickets = json["placement"]["tickets"].as_array().expect("tickets");
        tickets
            .iter()
            .find(|ticket| ticket["ticket"] == 2)
            .unwrap_or_else(|| panic!("#2 is not placed: {json}"))
            .clone()
    }

    /// `zaf brief 2 --phase fix` as the fix round runs it, and the pack it wrote.
    fn fix_brief(&self) -> String {
        let output = self
            .project
            .command_in(&self.project.root, &["brief", "2", "--phase", "fix"])
            .env("ZAF_INSTANCE", "fix instance 9 on #1")
            .output()
            .expect("run zaf");
        let (code, stdout, stderr) = outcome(&output);
        assert_eq!(code, 0, "{stdout}{stderr}");
        let pack = stdout.lines().next().expect("the pack's path");
        fs::read_to_string(pack).expect("the pack")
    }

    /// The fake programmer's folder.
    fn programmer(&self) -> PathBuf {
        self.project.temp.path().join("programmer")
    }

    /// Writes a phase run folder by hand, as the launcher leaves one that ended: `action` started
    /// at the current head and left it, with `exit`, raising `questions`.
    fn ran(&self, action: &str, exit: i32, questions: &[u64]) -> PathBuf {
        let head = self.remote_slice();
        let dir = self
            .project
            .root
            .join(".zaf/phases")
            .join(format!("{action}-by-hand-{}", self.runs().len()));
        write(&dir, "action", action);
        write(&dir, "head", &head);
        write(&dir, "size", "S");
        write(&dir, "instance", &format!("{action} instance 50 on #1"));
        if !questions.is_empty() {
            let lines: Vec<String> = questions.iter().map(ToString::to_string).collect();
            write(&dir, "questions", &format!("{}\n", lines.join("\n")));
        }
        write(&dir, "task", "2");
        write(&dir, "head-end", &head);
        write(&dir, "output", "");
        // A run ends after it starts: the exit is written a moment later.
        std::thread::sleep(Duration::from_millis(20));
        write(&dir, "exit", &format!("{exit}\n"));
        std::thread::sleep(Duration::from_millis(20));
        dir
    }
}

fn read(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join(name)).unwrap_or_else(|error| panic!("{name}: {error}"))
}

/// A scene whose merge stopped on a conflict: the slice and the base both changed `notes.txt`'s
/// second line, the review was clean, and the tick it asked for ran the merge.
fn conflicted() -> Scene {
    let scene = Scene::published("one\nslice\n");
    scene.advance_base_elsewhere("notes.txt", "one\nbase\n");
    scene.green();
    scene.spawn_phase("review");
    scene
}

#[test]
fn a_merge_refused_for_a_conflict_starts_fix() {
    let mut scene = conflicted();
    scene.project.on_first(
        "issue create *",
        "https://github.com/example/tracker/issues/60\n",
    );
    let head = scene.remote_slice();

    // The tick after the review ran the merge inline; it stopped on the conflict, exit 1, with
    // the paths last in what its run folder kept, and nothing moved.
    let merges = scene.runs_of("merge");
    assert_eq!(merges.len(), 1, "{:?}", scene.runs());
    assert_eq!(read(&merges[0], "exit"), "1\n");
    let output = read(&merges[0], "output");
    assert!(
        output.contains("the base conflicts with the slice in: notes.txt"),
        "{output}"
    );
    assert!(output.contains("zaf do sync-base 2"), "{output}");
    assert_eq!(scene.remote_slice(), head, "nothing was pushed");
    assert!(
        !scene
            .project
            .gh_calls()
            .iter()
            .any(|call| call.starts_with("pr merge")),
        "nothing was merged"
    );

    // The core puts the ticket in the lane at the fix: the merge failed on a conflict.
    let placed = scene.placement();
    assert_eq!(placed["place"], "lane", "{placed}");
    assert_eq!(placed["next"], "fix", "{placed}");

    // The merge itself is not run again; the fix starts through the one gate.
    let (code, _, stderr) = outcome(&scene.project.zaf(&["spawn", "phase", "2", "merge"]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("merge failed, fix is next"), "{stderr}");
    scene.spawn_phase("fix");
    let prompts = fs::read_to_string(scene.programmer().join("prompts")).expect("prompted");
    assert!(prompts.contains("Task #2, fix round"), "{prompts}");

    // This programmer changed nothing, so the head stayed and the merge is never run again: the
    // tick each fix's end asks for starts the fix again until it gets nowhere, then asks.
    assert_eq!(scene.runs_of("fix").len(), 3, "{:?}", scene.runs());
    assert_eq!(scene.runs_of("merge").len(), 1, "{:?}", scene.runs());
    assert!(
        scene
            .project
            .gh_calls()
            .iter()
            .any(|call| call.starts_with("issue create") && call.contains("fix got nowhere")),
        "{:?}",
        scene.project.gh_calls()
    );
}

#[test]
fn the_fix_brief_names_the_conflicting_paths() {
    let scene = conflicted();
    let head = scene.remote_slice();

    let pack = scene.fix_brief();

    // Straight after the ticket: the phase, the head, the code and its meaning, the paths.
    let why = pack
        .split_once("## Why this run\n")
        .map(|(_, rest)| rest.split("\n## ").next().unwrap_or(rest))
        .unwrap_or_else(|| panic!("no Why this run: {pack}"));
    eprintln!("## Why this run\n{why}");
    assert!(
        why.contains(&format!(
            "Phase `merge` failed at {head}: exit 1, conflicted:"
        )),
        "{why}"
    );
    assert!(
        why.contains("the base conflicts with the slice in: notes.txt"),
        "{why}"
    );
    assert!(why.contains("zaf do sync-base 2"), "{why}");
    assert!(
        pack.find("## Why this run") < pack.find("## Rulings"),
        "{pack}"
    );
    // A listed phase's pack is as it was: no Why this run.
    let (code, stdout, stderr) = outcome(&scene.project.zaf(&["brief", "2", "--phase", "merge"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let listed = fs::read_to_string(stdout.lines().next().expect("path")).expect("pack");
    assert!(!listed.contains("## Why this run"), "{listed}");
}

#[test]
fn sync_base_leaves_the_conflicts_in_the_workspace_for_the_programmer() {
    let scene = Scene::published("one\nslice\n");
    scene.advance_base_elsewhere("notes.txt", "one\nbase\n");
    let head = scene.remote_slice();
    let dir = scene.workspace();
    // Work the programmer has not committed yet.
    write(&dir, "draft.txt", "kept\n");

    let (code, text) = scene.as_fix(&["sync-base", "2"]);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("conflicts in:"), "{text}");
    assert!(text.contains("notes.txt"), "{text}");
    assert!(text.contains("nothing was pushed"), "{text}");
    // The conflicted merge is the working copy, with both sides in the file, and the draft rode
    // along; nothing reached the remote.
    let notes = fs::read_to_string(dir.join("notes.txt")).expect("notes");
    assert!(notes.contains("<<<<<<<"), "{notes}");
    assert!(notes.contains("slice") && notes.contains("base"), "{notes}");
    assert_eq!(
        fs::read_to_string(dir.join("draft.txt")).expect("the draft stays"),
        "kept\n"
    );
    assert_eq!(
        jj(&dir, &["log", "--no-graph", "-r", "@", "-T", "conflict"]),
        "true"
    );
    assert_eq!(
        jj(
            &dir,
            &["log", "--no-graph", "-r", "@", "-T", "parents.len()"]
        ),
        "2"
    );
    assert_eq!(scene.remote_slice(), head, "nothing was pushed");

    // Asked again before the conflicts are resolved, it refuses and changes nothing.
    let (code, text) = scene.as_fix(&["sync-base", "2"]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("unresolved conflicts"), "{text}");
    // Nor is a merge with conflict markers published.
    let (code, text) = scene.as_fix(&["slice-pr", "2"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("still has conflicts"), "{text}");
    assert_eq!(scene.remote_slice(), head, "nothing was pushed");
}

#[test]
fn sync_base_on_a_clean_base_merges_and_says_so() {
    let scene = Scene::published("one\nslice\n");
    scene.advance_base_elsewhere("merged.txt", "merged\n");
    let head = scene.remote_slice();
    let dir = scene.workspace();

    let (code, text) = scene.as_fix(&["sync-base", "2"]);

    assert_eq!(code, 0, "{text}");
    assert!(
        text.contains(&format!(
            "merged {BASE} into {SLICE} cleanly; nothing was pushed"
        )),
        "{text}"
    );
    // The merge is committed under an empty working copy, and the slice's bookmark follows it.
    assert!(dir.join("merged.txt").is_file());
    let merge = scene.head();
    assert_ne!(merge, head);
    assert_eq!(
        jj(
            &dir,
            &["log", "--no-graph", "-r", "@-", "-T", "parents.len()"]
        ),
        "2"
    );
    assert_eq!(scene.remote_slice(), head, "nothing was pushed");

    // Again: the base has nothing the slice lacks.
    let (code, text) = scene.as_fix(&["sync-base", "2"]);
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("up to date"), "{text}");
    assert_eq!(scene.head(), merge);
}

#[test]
fn after_the_fix_resolves_the_conflict_the_ticket_is_checked_reviewed_and_merged() {
    let scene = conflicted();
    let conflicted_head = scene.remote_slice();
    // The fix round's programmer brings the base in, then resolves the conflict; the fix action
    // publishes what it left.
    write(&scene.programmer(), "do", "zaf do sync-base 2\n");
    write(&scene.programmer(), "resolved", "one\nslice\nbase\n");

    scene.spawn_phase("fix");

    let did = fs::read_to_string(scene.programmer().join("did")).expect("the programmer ran");
    assert!(did.contains("conflicts in:"), "{did}");
    assert!(did.contains("exit 1"), "{did}");
    let fixes = scene.runs_of("fix");
    assert_eq!(
        read(&fixes[0], "exit"),
        "0\n",
        "{}",
        read(&fixes[0], "output")
    );
    // The merge is published: the pull request's head moved, and it holds both sides resolved.
    let merged_head = scene.remote_slice();
    assert_ne!(merged_head, conflicted_head);
    assert_eq!(read(&fixes[0], "head-end"), merged_head);
    let remote = scene.project.temp.path().join("remote.git");
    assert_eq!(
        run(&remote, "git", &["show", &format!("{SLICE}:notes.txt")]),
        "one\nslice\nbase\n"
    );
    assert_eq!(
        run(&remote, "git", &["rev-list", "--parents", "-n", "1", SLICE])
            .split_whitespace()
            .count(),
        3,
        "a merge commit"
    );

    // Checked: the ticket waits on the checks at the new head, and nothing runs meanwhile.
    let placed = scene.placement();
    assert_eq!(placed["place"], "waiting", "{placed}");
    assert!(placed.to_string().contains(&merged_head), "{placed}");

    // Green: the tick launches the review at the new head; the tick its end asks for merges.
    scene.green();
    scene.tick();

    let reviews = scene.runs_of("review");
    assert_eq!(reviews.len(), 2, "{:?}", scene.runs());
    assert_eq!(read(&reviews[1], "head"), merged_head);
    assert_eq!(read(&reviews[1], "exit"), "0\n");
    let merges = scene.runs_of("merge");
    assert_eq!(merges.len(), 2, "{:?}", scene.runs());
    assert_eq!(read(&merges[1], "head"), merged_head);
    assert_eq!(
        read(&merges[1], "exit"),
        "0\n",
        "{}",
        read(&merges[1], "output")
    );
    assert!(
        scene.project.gh_calls().contains(&format!(
            "pr merge 7 --merge --match-head-commit {merged_head} --repo {REPO}"
        )),
        "{:?}",
        scene.project.gh_calls()
    );
    assert!(!scene.workspace().exists(), "the workspace is dropped");
}

#[test]
fn a_base_that_moved_and_merges_cleanly_is_merged_without_a_new_round() {
    let scene = Scene::published("one\nslice\n");
    scene.advance_base_elsewhere("merged.txt", "merged\n");
    let head = scene.remote_slice();
    scene.green();

    // The review is clean; the merge it leads to finds the base merges cleanly and lands the
    // slice at the head that was checked and reviewed.
    scene.spawn_phase("review");

    let merges = scene.runs_of("merge");
    assert_eq!(merges.len(), 1, "{:?}", scene.runs());
    assert_eq!(
        read(&merges[0], "exit"),
        "0\n",
        "{}",
        read(&merges[0], "output")
    );
    // Nothing was pushed, so no check and no review ran a second time, and there is no fix.
    assert_eq!(scene.remote_slice(), head, "nothing was pushed");
    assert!(
        scene.project.gh_calls().contains(&format!(
            "pr merge 7 --merge --match-head-commit {head} --repo {REPO}"
        )),
        "{:?}",
        scene.project.gh_calls()
    );
    assert_eq!(scene.runs_of("review").len(), 1, "{:?}", scene.runs());
    assert!(scene.runs_of("fix").is_empty());
    assert!(!scene.workspace().exists(), "the workspace is dropped");
}

#[test]
fn a_forge_refusal_still_gets_nowhere_and_asks() {
    let mut scene = Scene::published("one\nslice\n");
    scene
        .project
        .fail_first("pr merge *", "Pull request #7 has failing required checks");
    scene.project.on_first(
        "issue create *",
        "https://github.com/example/tracker/issues/60\n",
    );
    scene.green();

    // The review is clean, and the merge the forge refuses runs again, never the fix.
    scene.spawn_phase("review");
    let merges = scene.runs_of("merge");
    assert_eq!(merges.len(), 1, "{:?}", scene.runs());
    assert_eq!(
        read(&merges[0], "exit"),
        "4\n",
        "{}",
        read(&merges[0], "output")
    );
    let placed = scene.placement();
    assert_eq!(placed["next"], "merge", "{placed}");

    scene.tick();
    scene.tick();
    let merges = scene.runs_of("merge");
    assert_eq!(merges.len(), 3, "{:?}", scene.runs());
    assert!(scene.runs_of("fix").is_empty(), "a refusal is no fix");

    // Three refusals at one head: the next tick asks the Engineer and runs nothing.
    let stdout = scene.tick();
    assert!(stdout.contains("merge got nowhere three times"), "{stdout}");
    assert_eq!(scene.runs_of("merge").len(), 3);
    assert!(scene.runs_of("fix").is_empty());
}

#[test]
fn merge_question_names_head_not_held() {
    let mut scene = Scene::published("one\nslice\n");
    scene.project.on_first(
        "issue create *",
        "https://github.com/example/tracker/issues/60\n",
    );
    let head = scene.remote_slice();
    scene.green();
    // The workspace goes ahead of the pull request's head: a commit nothing pushed.
    write(&scene.workspace(), "notes.txt", "one\nslice\nmore\n");
    jj(&scene.workspace(), &["commit", "-m", "Ahead of the head"]);
    let latest = scene.head();
    assert_ne!(latest, head);

    let before = scene.remote_slice();
    scene.spawn_phase("review");
    let merges = scene.runs_of("merge");
    assert_eq!(merges.len(), 1, "{:?}", scene.runs());
    let output = read(&merges[0], "output");
    assert_eq!(read(&merges[0], "exit"), "4\n", "{output}");
    let reason = format!("the workspace does not hold the pull request's head {head}");
    assert!(output.contains(&reason), "{output}");
    assert!(output.contains(&latest), "{output}");
    assert!(scene.runs_of("fix").is_empty(), "a refusal is no fix");

    scene.tick();
    scene.tick();
    // Three of them at one head: the question carries the same words.
    let stdout = scene.tick();
    assert!(stdout.contains("merge got nowhere three times"), "{stdout}");
    assert!(
        scene.project.gh_calls().iter().any(|call| {
            call.starts_with("issue create") && call.contains(&reason) && call.contains(&latest)
        }),
        "{:?}",
        scene.project.gh_calls()
    );
    // Nothing moved: the head is where it was and the workspace holds what it held.
    assert_eq!(scene.remote_slice(), before);
    assert_eq!(scene.head(), latest);
}

/// Question `number` raised on task 2, closed with `ruling` at `closed`.
fn ruled(number: u64, text: &str, ruling: &str, closed: &str) -> (serde_json::Value, String) {
    let issue = serde_json::json!({
        "number": number,
        "title": "Question on #2",
        "body": format!("Raised on #2.\n\n{text}"),
        "labels": [{"name": "question"}],
        "state": "CLOSED",
        "comments": [{"body": format!("<!-- zaf:ruling -->\n{ruling}")}],
    });
    let standing = serde_json::json!({
        "number": number,
        "state": "CLOSED",
        "stateReason": "COMPLETED",
        "labels": [{"name": "question"}],
        "url": format!("https://github.com/{REPO}/issues/{number}"),
        "closedAt": closed,
    });
    (issue, standing.to_string())
}

#[test]
fn the_fix_brief_carries_the_rulings_made_since_the_last_model_run() {
    let mut scene = Scene::published("one\nslice\n");
    scene.green();
    // A ruling the last model run had, one made since, and the question the review raised.
    let (old, old_standing) = ruled(30, "Which delimiter?", "A comma.", "2020-01-01T00:00:00Z");
    let (new, new_standing) = ruled(
        31,
        "Quote fields?",
        "Quote every field.",
        "2099-01-01T00:00:00Z",
    );
    let (asked, asked_standing) = ruled(45, "Sort the rows?", "By title.", "2020-01-02T00:00:00Z");
    scene
        .project
        .on_first(
            "issue list --label question --state all *",
            &serde_json::json!([old, new, asked]).to_string(),
        )
        .on_first("issue view 30 *", &old_standing)
        .on_first("issue view 31 *", &new_standing)
        .on_first("issue view 45 *", &asked_standing);
    // The last model run, then the review that raised #45 and exited 0.
    scene.ran("implement", 0, &[]);
    scene.ran("review", 0, &[45]);

    let placed = scene.placement();
    assert_eq!(placed["next"], "fix", "{placed}");
    let pack = scene.fix_brief();

    let why = pack
        .split_once("## Why this run\n")
        .map(|(_, rest)| rest.split("\n## Rulings").next().unwrap_or(rest))
        .unwrap_or_else(|| panic!("no Why this run: {pack}"));
    assert!(why.contains("Its run raised #45, ruled since"), "{why}");
    assert!(
        why.contains("#45 Sort the rows?\nruling: By title."),
        "{why}"
    );
    assert!(
        why.contains("#31 Quote fields?\nruling: Quote every field."),
        "{why}"
    );
    assert!(!why.contains("#30"), "the last model run had it: {why}");
    // The pack's Rulings still hold every one.
    assert!(
        pack.contains("#30 Which delimiter?\nruling: A comma."),
        "{pack}"
    );
}

#[test]
fn a_ruling_on_a_review_question_reaches_the_fix_once_and_the_review_applies_again() {
    let mut scene = Scene::published("one\nslice\n");
    scene.green();
    let (asked, asked_standing) = ruled(45, "Sort the rows?", "By title.", "2099-01-01T00:00:00Z");
    scene
        .project
        .on_first(
            "issue list --label question --state all *",
            &serde_json::json!([asked]).to_string(),
        )
        .on_first("issue view 45 *", &asked_standing);
    scene.ran("review", 0, &[45]);
    assert_eq!(scene.placement()["next"], "fix");

    // The fix found nothing to change: the head did not move. The review applies again, and the
    // fix is not run until it gets nowhere.
    scene.ran("fix", 0, &[]);
    let placed = scene.placement();
    assert_eq!(placed["place"], "lane", "{placed}");
    assert_eq!(placed["next"], "review", "{placed}");
    let (code, _, stderr) = outcome(&scene.project.zaf(&["spawn", "phase", "2", "fix"]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("review is ready"), "{stderr}");
}

/// The command lines of the processes that work in `root`: those that run in it or name it.
/// Linux reads `/proc`; elsewhere `ps` gives the command lines only, so a process that runs in
/// the project without naming it is not seen.
fn working_in(root: &Path, root_text: &str) -> Vec<String> {
    if !Path::new("/proc/self/cmdline").exists() {
        let table = std::process::Command::new("ps")
            .args(["-axo", "command="])
            .output()
            .expect("the process table");
        return String::from_utf8_lossy(&table.stdout)
            .lines()
            .filter(|line| line.contains(root_text))
            .map(str::to_owned)
            .collect();
    }
    fs::read_dir("/proc")
        .expect("the process table")
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.chars().all(|c| c.is_ascii_digit()))
        })
        .filter_map(|entry| {
            let dir = entry.path();
            let cwd = fs::read_link(dir.join("cwd")).ok();
            let cmdline = fs::read(dir.join("cmdline")).ok()?;
            let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            let here = cwd.is_some_and(|cwd| cwd.starts_with(root)) || cmdline.contains(root_text);
            here.then_some(cmdline)
        })
        .collect()
}
