//! A plain git repository takes one ticket from ready to merged. The folder is made with
//! `git init` and has a bare `origin`; `zaf setup` writes `kind = "git"` and `zaf doctor` passes;
//! then one ready task goes through the phases (claim and implement, ci, review, merge) with the
//! fake Forgejo (`tests/fixtures/forgejo/curl`) and a fake `claude` programmer that edits one
//! file, until the change is on `origin`'s integration branch, the task is done and its worktree
//! is gone.
//!
//! No `jj` and no `gh` is on the search path: a git project needs neither.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

mod common;
use common::{Curl, outcome};

const FORGEJO: &str = "http://yagdrassyl.local:3000";
const ADDRESS: &str = "ssh://git@yagdrassyl.local:2222/zuri/ways.git";
const API: &str = "http://yagdrassyl.local:3000/api/v1/repos/zuri/ways";

/// The stand-in `claude`: logged in, the plugin commands succeed and record the pack at the real
/// `plugins/teams` folder; run as the programmer (`-p`) it appends a line to `feature.txt` in
/// the workspace its prompt names.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
set -eu
case "$*" in
    "auth status"*) printf '{"loggedIn": true}\n' ;;
    "plugin marketplace list")
        if [ -e "$CLAUDE_CONFIG_DIR/.fake-marketplace" ]; then echo "ways"; else echo "No marketplaces configured"; fi ;;
    "plugin marketplace add"*)
        mkdir -p "$CLAUDE_CONFIG_DIR"; : > "$CLAUDE_CONFIG_DIR/.fake-marketplace" ;;
    "plugin install "*)
        mkdir -p "$CLAUDE_CONFIG_DIR/plugins"
        printf '{"version": 2, "plugins": {"%s": [{"scope": "user", "version": "%s", "installPath": "%s"}]}}\n' \
            "$3" "$FAKE_PACK_VERSION" "$FAKE_PACK_PATH" > "$CLAUDE_CONFIG_DIR/plugins/installed_plugins.json" ;;
    "plugin list") echo "No plugins installed" ;;
    "-p "*)
        workspace=$(printf '%s' "$2" | sed -n 's/.*Workspace: \(.*\)\.$/\1/p')
        [ -d "$workspace" ] || { echo "no workspace in: $2" >&2; exit 1; }
        echo "the programmer's change" >> "$workspace/feature.txt" ;;
    *) ;;
esac
"#;

fn executable(path: &Path, text: &str) {
    fs::write(path, text).expect("write tool");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// The ways repository's root, one folder above the crate.
fn ways_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_owned()
}

/// The version in the pack's own `plugin.json`, which the fake marketplace serves.
fn pack_version() -> String {
    let text = fs::read_to_string(ways_root().join("plugins/teams/.claude-plugin/plugin.json"))
        .expect("the pack's plugin.json");
    let json: serde_json::Value = serde_json::from_str(&text).expect("plugin.json is JSON");
    json["version"]
        .as_str()
        .expect("plugin.json has a version")
        .to_owned()
}

struct Scene {
    temp: tempfile::TempDir,
    root: PathBuf,
    origin: PathBuf,
    home: PathBuf,
    curl: Curl,
}

impl Scene {
    /// A folder made with `git init`, a bare `origin` it pushes `main` to, the stand-in `claude`
    /// and `curl` first on a search path that holds no `gh` and no `jj`.
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let (root, origin, home) = (
            temp.path().join("tracker"),
            temp.path().join("origin.git"),
            temp.path().join("zaf-home"),
        );
        let (bin, user) = (temp.path().join("bin"), temp.path().join("user-home"));
        for dir in [&root, &home, &bin, &user] {
            fs::create_dir_all(dir).expect("dir");
        }
        executable(&bin.join("claude"), FAKE_CLAUDE);
        executable(
            &bin.join("curl"),
            &fs::read_to_string(common::fixtures().join("forgejo/curl")).expect("fake curl"),
        );
        // Every tool the machine offers but the ones a git project must not need.
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&inherited) {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                if !["gh", "jj", "claude", "curl"].contains(&name.to_string_lossy().as_ref()) {
                    let _ = std::os::unix::fs::symlink(entry.path(), bin.join(&name));
                }
            }
        }
        let scene = Self {
            curl: Curl::new(temp.path()),
            temp,
            root,
            origin,
            home,
        };
        scene.write_gitconfig(false);
        scene.git(scene.temp.path(), &["init", "-q", "--bare", "origin.git"]);
        scene.git(&scene.root, &["init", "-q"]);
        // The remote is a Forgejo's address, so setup finds the instance.
        scene.git(&scene.root, &["remote", "add", "origin", ADDRESS]);
        scene
    }

    /// The git configuration every git of the test reads: an author, `main` as the first branch,
    /// and with `routed` the rewrite that makes the Forgejo address the remote names reach the
    /// bare repository beside the project. `git remote get-url` applies the rewrite too, so it is
    /// written once setup has read the address.
    fn write_gitconfig(&self, routed: bool) {
        let mut text = "[user]\n\tname = Test Author\n\temail = author@example.com\n\
                        [init]\n\tdefaultBranch = main\n"
            .to_owned();
        if routed {
            text.push_str(&format!(
                "[url \"{}\"]\n\tinsteadOf = {ADDRESS}\n",
                self.origin.display()
            ));
        }
        fs::write(self.temp.path().join("user-home/.gitconfig"), text).expect("gitconfig");
    }

    /// Runs `git` in `dir` under the test's own configuration, failing the test when it fails.
    fn git(&self, dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("HOME", self.temp.path().join("user-home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .expect("git");
        assert!(
            output.status.success(),
            "git {args:?} in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    /// Runs `zaf <args>` in `dir` with `stdin` on its input (none: closed input) and `env` set.
    fn zaf_in(
        &self,
        dir: &Path,
        args: &[&str],
        stdin: Option<&str>,
        env: &[(&str, &str)],
    ) -> Output {
        let bin = self.temp.path().join("bin");
        let search = std::env::join_paths([bin].into_iter().chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .expect("search path");
        let mut command = Command::new(env!("CARGO_BIN_EXE_zaf"));
        command
            .args(args)
            .current_dir(dir)
            .env("PATH", search)
            .env("ZAF_HOME", &self.home)
            .env("HOME", self.temp.path().join("user-home"))
            .env("FAKE_CURL_DIR", &self.curl.dir)
            .env("FAKE_PACK_VERSION", pack_version())
            .env("FAKE_PACK_PATH", ways_root().join("plugins/teams"))
            .env("FORGEJO_TOKEN", "secret")
            .env_remove("ZAF_INSTANCE")
            .env_remove("ZAF_VERSION")
            .env_remove("ZAF_VCS")
            .envs(env.iter().copied())
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().expect("run zaf");
        if let Some(text) = stdin {
            child
                .stdin
                .take()
                .expect("stdin")
                .write_all(text.as_bytes())
                .expect("answers");
        }
        child.wait_with_output().expect("zaf output")
    }

    fn zaf(&self, args: &[&str], stdin: Option<&str>) -> Output {
        self.zaf_in(&self.root, args, stdin, &[])
    }
}

#[test]
fn plain_git_ticket_ready_to_merged() {
    let mut scene = Scene::new();
    scene
        .curl
        .on(
            &format!("GET {FORGEJO}/api/v1/version"),
            200,
            r#"{"version":"15.0.11+gitea-1.22.0"}"#,
        )
        .on(
            &format!("GET {API}"),
            200,
            r#"{"permissions":{"pull":true,"push":true},"default_branch":"main"}"#,
        )
        .on(&format!("POST {API}/*"), 422, r#"{"message":"invalid"}"#)
        .on(&format!("GET {API}/*"), 200, "[]");

    // The project: a first commit on main, pushed to origin.
    fs::write(scene.root.join("feature.txt"), "first\n").expect("file");
    fs::write(scene.root.join(".gitignore"), ".zaf/*\n!.zaf/actions/\n").expect("ignore");
    let (code, stdout, stderr) = outcome(&scene.zaf(&["setup"], Some("\n\n\n\n\n")));
    assert_eq!(code, 0, "setup:\n{stdout}{stderr}");
    let manifest = fs::read_to_string(scene.root.join(".zaf.toml")).expect("manifest");
    assert!(manifest.contains("[vcs]\nkind = \"git\""), "{manifest}");
    let (code, stdout, stderr) = outcome(&scene.zaf(&["doctor"], None));
    assert_eq!(code, 0, "doctor:\n{stdout}{stderr}");

    // The project's first commit goes to origin's main.
    scene.write_gitconfig(true);
    scene.git(&scene.root, &["add", "-A"]);
    scene.git(&scene.root, &["commit", "-qm", "Start the project"]);
    scene.git(&scene.root, &["push", "-q", "origin", "main"]);

    // The version's integration branch is made on origin, and the task is ready.
    scene.forgejo(&[(1, "task"), (3, "state/ready")], &[], &[]);
    let (code, stdout, stderr) = outcome(&scene.zaf_in(
        &scene.root,
        &["do", "--more", "base-open"],
        None,
        &[("ZAF_VERSION", VERSION)],
    ));
    assert_eq!(code, 0, "base-open:\n{stdout}{stderr}");
    let opened = vec![
        (format!("GET {API}/pulls[?]*"), 200, "[]".to_owned()),
        (
            format!("POST {API}/pulls"),
            201,
            pull_request("0000000000000000000000000000000000000000", false).to_string(),
        ),
    ];
    // The claim: the board then reads the task as claimed, with the claim's note on it.
    scene.forgejo(&[(1, "task"), (3, "state/ready")], &[], &opened);
    let (code, stdout, stderr) = scene.work(&scene.root, &["board", "claim", "511"]);
    assert_eq!(code, 0, "claim:\n{stdout}{stderr}");
    let note = format!("claimed by {INSTANCE} at 2026-11-21T22:00:00Z");
    scene.forgejo(&[(1, "task"), (4, "state/claimed")], &[&note], &opened);
    let (code, stdout, stderr) = scene.work(&scene.root, &["spawn", "phase", "511", "implement"]);
    assert_eq!(code, 0, "spawn phase implement:\n{stdout}{stderr}");
    scene.await_phase("implement");
    let head = scene.git(
        &scene.origin,
        &["rev-parse", &format!("refs/heads/{SLICE}")],
    );

    // ci: the pull request is open at that head and its check is still running, so the lane holds
    // the task at ci; once the check passes ci's end fact holds and the next phase is review.
    scene.on_pull(&head, &note, "pending", false);
    let (code, stdout, stderr) = scene.work(&scene.root, &["spawn", "phase", "511", "ci"]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stderr.contains("the checks at") && stderr.contains("have not ended"),
        "{stderr}"
    );
    scene.on_pull(&head, &note, "success", false);
    scene.forge_merges(&head, &note);

    // review and merge: the check is green at the head; the merge lands the slice.
    for action in ["review", "merge"] {
        scene.start_phase(action);
        scene.await_phase(action);
    }

    // The change is on origin's integration branch, whole: the slice's head is in its history and
    // the file holds the programmer's line.
    scene.git(
        &scene.origin,
        &[
            "merge-base",
            "--is-ancestor",
            &head,
            &format!("refs/heads/{BASE}"),
        ],
    );
    let landed = scene.git(&scene.origin, &["show", &format!("{BASE}:feature.txt")]);
    assert_eq!(
        landed, "first\nthe programmer's change",
        "{BASE}:feature.txt"
    );

    // The task is done: the board closed its issue; and its worktree is gone, so the project's
    // checkout is the only one left.
    assert!(
        scene
            .curl
            .asked()
            .contains(&format!("PATCH {API}/issues/{TASK}")),
        "the task's issue was never closed: {:#?}",
        scene.curl.asked()
    );
    assert!(
        !scene.root.join(".zaf/workspaces").join(SLICE).exists(),
        "the worktree is still there"
    );
    let worktrees = scene.git(&scene.root, &["worktree", "list", "--porcelain"]);
    assert_eq!(worktrees.matches("worktree ").count(), 1, "{worktrees}");
}

const TASK: u64 = 511;
const EPIC: u64 = 424;
const VERSION: &str = "1.0.0";
const BASE: &str = "feature/1.0.0";
const SLICE: &str = "feature/1.0.0-424-511";
const INSTANCE: &str = "implement instance 1 on #424";

const TASK_BODY: &str = "Goal: A plain git repository takes one ticket.\n\
    Owned paths: feature.txt\n\
    Interfaces: none\n\
    Design pages: none\n\
    Architecture: none\n\
    Acceptance:\n  - feature.txt holds the programmer's line\n\
    Tests: none\n\
    Security trigger: no\n\
    Model: sonnet\n\
    Size: S\n\
    Blocked by: none\n";

const PR: u64 = 601;

/// Pull request 601 as Forgejo prints it with the slice's head at `head`.
fn pull_request(head: &str, merged: bool) -> serde_json::Value {
    serde_json::json!({
        "number": PR,
        "state": if merged { "closed" } else { "open" },
        "merged": merged,
        "mergeable": !merged,
        "html_url": format!("{FORGEJO}/zuri/ways/pulls/{PR}"),
        "created_at": "2026-11-21T22:00:00Z",
        "merged_at": if merged { Some("2026-11-21T22:30:00Z") } else { None },
        "head": {"ref": SLICE, "sha": head},
        "base": {"ref": BASE, "sha": head},
    })
}

/// The repository's labels, which the board reads to find the one it adds.
fn repo_labels() -> serde_json::Value {
    serde_json::json!([
        {"id": 1, "name": "task"},
        {"id": 2, "name": "epic"},
        {"id": 3, "name": "state/ready", "exclusive": true},
        {"id": 4, "name": "state/claimed", "exclusive": true},
        {"id": 5, "name": "state/done", "exclusive": true},
    ])
}

impl Scene {
    /// What the instance answers while the task wears `labels`: the answers a stage names come
    /// first, then the task, its epic, the repository and the fallbacks. A stage's rules replace
    /// the last one's, as a Forgejo's answers change with the work.
    fn forgejo(
        &mut self,
        labels: &[(u64, &str)],
        comments: &[&str],
        answers: &[(String, u16, String)],
    ) {
        let comments: Vec<_> = comments
            .iter()
            .enumerate()
            .map(|(at, body)| serde_json::json!({"id": at + 1, "body": body}))
            .collect();
        self.curl = Curl::new(self.temp.path());
        fs::write(self.curl.dir.join("rules"), "").expect("rules");
        self.curl.on(
            "GET */api/v1/settings/api",
            200,
            r#"{"max_response_items":50}"#,
        );
        let mut on_task: Vec<_> = labels
            .iter()
            .map(|(id, name)| {
                serde_json::json!({"id": id, "name": name, "exclusive": name.starts_with("state/")})
            })
            .collect();
        on_task.push(serde_json::json!({"id": 6, "name": "type/Task", "exclusive": true}));
        let task = serde_json::json!({
            "number": TASK,
            "title": "A plain git repository takes one ticket from ready to merged",
            "body": TASK_BODY, "labels": on_task, "state": "open",
            "html_url": format!("{FORGEJO}/zuri/ways/issues/{TASK}"),
        });
        let epic = serde_json::json!({
            "number": EPIC, "title": "A plain git repository", "body": "An epic.\n",
            "labels": [{"id": 2, "name": "epic"}], "state": "open",
            "html_url": format!("{FORGEJO}/zuri/ways/issues/{EPIC}"),
        });
        // The epic is signed, planned and challenged, so its tasks are for building.
        let hash = zaf::domain::BodyHash::of(&zaf::domain::TicketBody::from("An epic.\n"));
        let epic_comments = serde_json::json!([
            {"id": 1, "created_at": "2026-11-21T20:00:00Z",
             "body": format!("<!-- zaf:signature sha256={} at=2026-11-21T20:00:00Z -->\nSigned.", hash.as_str())},
            {"id": 2, "created_at": "2026-11-21T20:10:00Z",
             "body": "<!-- zaf:exit planning instance 1 on #424 done -->\nPlanned."},
            {"id": 3, "created_at": "2026-11-21T20:20:00Z",
             "body": "<!-- zaf:report challenger instance 1 on #424 -->\nChallenged."},
        ]);
        let curl = &mut self.curl;
        for (pattern, status, body) in answers {
            curl.on(pattern, *status, body);
        }
        curl.on(
            &format!("GET {API}/issues/{EPIC}/comments*"),
            200,
            &epic_comments.to_string(),
        )
        .on(
            &format!("GET {API}/issues/{EPIC}/dependencies*"),
            200,
            &format!("[{task}]"),
        )
        .on(
            &format!("GET {API}/issues[?]*labels=epic*"),
            200,
            &format!("[{epic}]"),
        );
        // The lists the board reads: the open tasks, and those in each state the task wears.
        let listed = format!("[{task}]");
        for (_, name) in labels.iter().filter(|(_, name)| name.starts_with("state/")) {
            let label = name.replace('/', "%2F");
            curl.on(
                &format!("GET {API}/issues[?]*labels={label}*"),
                200,
                &listed,
            );
        }
        curl.on(&format!("GET {API}/issues[?]*labels=task*"), 200, &listed);
        curl.on(&format!("GET {API}/issues/{TASK}"), 200, &task.to_string())
            .on(
                &format!("GET {API}/issues/{TASK}/blocks*"),
                200,
                &format!("[{epic}]"),
            )
            .on(&format!("GET {API}/issues/{EPIC}"), 200, &epic.to_string())
            .on(
                &format!("GET {API}/issues/{TASK}/comments*"),
                200,
                &serde_json::Value::Array(comments).to_string(),
            )
            .on(
                &format!("GET {API}/labels*"),
                200,
                &repo_labels().to_string(),
            )
            .on(
                &format!("POST {API}/issues/{TASK}/comments"),
                201,
                r#"{"id":1,"body":""}"#,
            )
            .on(
                &format!("POST {API}/issues/{TASK}/labels"),
                200,
                &repo_labels().to_string(),
            )
            .on(&format!("DELETE {API}/issues/{TASK}/labels/*"), 204, "")
            .on(
                &format!("PATCH {API}/issues/{TASK}"),
                201,
                &task.to_string(),
            )
            .on(
                &format!("GET {FORGEJO}/api/v1/version"),
                200,
                r#"{"version":"15.0.11+gitea-1.22.0"}"#,
            )
            .on(
                &format!("GET {API}"),
                200,
                r#"{"permissions":{"pull":true,"push":true},"default_branch":"main"}"#,
            )
            .on(&format!("POST {API}/*"), 422, r#"{"message":"invalid"}"#)
            .on(&format!("GET {API}/*"), 200, "[]");
    }

    /// The forge with the task claimed (`note` the claim) and its pull request open at `head`,
    /// where the one check `ci / gate` stands at `status`.
    fn on_pull(&mut self, head: &str, note: &str, status: &str, merged: bool) {
        let statuses = serde_json::json!([{
            "id": 1, "status": status, "context": "ci / gate (pull_request)",
            "created_at": "2026-11-21T22:05:00Z",
        }]);
        let open = pull_request(head, merged).to_string();
        let answers = vec![
            (format!("POST {API}/pulls/{PR}/merge"), 200, String::new()),
            (format!("GET {API}/pulls/{PR}"), 200, open.clone()),
            (
                format!("GET {API}/commits/{head}/statuses*"),
                200,
                statuses.to_string(),
            ),
            (format!("GET {API}/pulls[?]*"), 200, format!("[{open}]")),
            (
                format!("GET {API}/pulls/{PR}/files*"),
                200,
                r#"[{"filename":"feature.txt","status":"modified"}]"#.to_owned(),
            ),
        ];
        self.forgejo(&[(1, "task"), (4, "state/claimed")], &[note], &answers);
    }

    /// Makes the forge merge as Forgejo does: when the merge call lands, the pull request's
    /// branch is merged into its base in `origin`, and from then on the pull request reads as
    /// merged. Call it with the rules of the open pull request in place.
    fn forge_merges(&mut self, head: &str, note: &str) {
        self.on_pull(head, note, "success", true);
        let rules = self.curl.dir.join("rules");
        fs::rename(&rules, self.curl.dir.join("rules.merged")).expect("merged rules");
        self.on_pull(head, note, "success", false);
        let hook = format!(
            r#"#!/bin/sh
set -eu
case "$1" in
    "POST {API}/pulls/{PR}/merge")
        repo={origin}
        tree=$(git -C "$repo" merge-tree --write-tree "refs/heads/{BASE}" "refs/heads/{SLICE}")
        commit=$(git -C "$repo" -c user.name=Forgejo -c user.email=forgejo@example.com \
            commit-tree "$tree" -p "refs/heads/{BASE}" -p "refs/heads/{SLICE}" -m "Merge pull request #{PR}")
        git -C "$repo" update-ref "refs/heads/{BASE}" "$commit"
        cp "$2/rules.merged" "$2/rules"
        ;;
esac
"#,
            origin = self.origin.display()
        );
        executable(&self.curl.dir.join("hook"), &hook);
    }

    /// Starts the task's current phase, which must be `action`; it runs as a process of its own.
    fn start_phase(&self, action: &str) {
        let (code, stdout, stderr) = self.work(&self.root, &["spawn", "phase", "511", action]);
        assert_eq!(code, 0, "spawn phase {action}:\n{stdout}{stderr}");
    }

    /// Waits for the latest run of the phase action `action` to end and gives what it exited
    /// with; the run is a process of its own, which `zaf spawn phase` does not wait for.
    fn await_phase(&self, action: &str) -> String {
        let dir = self.root.join(".zaf").join(zaf::app::actions::PHASES_DIR);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let mut ended: Vec<(std::time::SystemTime, String, PathBuf)> = Vec::new();
            for run in fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = run.path();
                let is_action =
                    fs::read_to_string(path.join("action")).is_ok_and(|text| text.trim() == action);
                if let (true, Ok(exit)) = (is_action, fs::read_to_string(path.join("exit"))) {
                    let at = fs::metadata(path.join("exit"))
                        .and_then(|meta| meta.modified())
                        .unwrap_or(std::time::UNIX_EPOCH);
                    ended.push((at, exit.trim().to_owned(), path));
                }
            }
            if let Some((_, exit, run)) = ended.into_iter().max() {
                let files: Vec<String> = fs::read_dir(&run)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .map(|file| {
                        let text = fs::read_to_string(file.path()).unwrap_or_default();
                        format!("--- {}\n{text}", file.file_name().to_string_lossy())
                    })
                    .collect();
                assert_eq!(exit, "0", "the {action} run:\n{}", files.join("\n"));
                return exit;
            }
            assert!(
                Instant::now() < deadline,
                "the {action} run never ended: {:?}",
                fs::read_dir(&dir)
                    .map(|runs| runs.flatten().map(|run| run.path()).collect::<Vec<_>>())
            );
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Runs `zaf <args>` in `dir` as the programmer of the epic's version.
    fn work(&self, dir: &Path, args: &[&str]) -> (i32, String, String) {
        outcome(&self.zaf_in(
            dir,
            args,
            None,
            &[("ZAF_VERSION", VERSION), ("ZAF_INSTANCE", INSTANCE)],
        ))
    }
}
