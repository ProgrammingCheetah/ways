//! `zaf slice open` → `zaf changes` → `zaf slice pr` → `zaf slice merge`, end to end:
//! the built binary on a temp manifest, a throwaway jj repository with a bare
//! remote, and the fake `gh` standing in for the board and the forge.

mod common;

use std::path::{Path, PathBuf};

use common::{Project, REPO, jj, outcome, run, write};

const BASE: &str = "feature/v1";
const SLICE: &str = "feature/v1-1-2";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[vcs]
kind = "jj"

[forge]
kind = "github"
repo = "example/tracker"

[gate]
full = "true"
light = "test -f src/export.txt"
"#;

/// Task 2 of epic 1, claimed, as `gh issue view` prints it.
const TASK: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"claimed"}],"parent":{"number":1},"state":"OPEN","comments":[]}"#;

/// A project whose root is a jj repository with its base bookmark pushed to a bare remote,
/// and a fake `gh` that knows task 2 and opens pull request 7.
fn project() -> (Project, PathBuf) {
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
    write(&root, ".gitignore", ".zaf/\n");
    write(&root, "notes.txt", "one\ntwo\n");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", BASE]);
    project
        .on("issue view 2 *", TASK)
        .on(&format!("pr list --head {SLICE} *"), "[]")
        .on(
            "pr create *",
            &format!("https://github.com/{REPO}/pull/7\n"),
        )
        .on("pr view 7 *", r#"{"state":"OPEN"}"#)
        .on("pr merge 7 *", "")
        .on("issue edit 2 *", "")
        .env("ZAF_VERSION", "v1");
    (project, remote)
}

fn workspace(project: &Project) -> PathBuf {
    project.root.join(".zaf/workspaces").join(SLICE)
}

/// The commit a ref points at in the bare remote.
fn remote_ref(remote: &Path, name: &str) -> String {
    let git_dir = remote.to_string_lossy();
    let reference = format!("refs/heads/{name}");
    run(
        remote,
        "git",
        &["--git-dir", &git_dir, "rev-parse", &reference],
    )
    .trim()
    .to_owned()
}

/// Moves the base one commit ahead from the root's own working copy.
fn advance_base(root: &Path) -> String {
    let base = format!("bookmarks(exact:\"{BASE}\")");
    jj(root, &["new", &base]);
    write(root, "later.txt", "later\n");
    jj(root, &["commit", "-m", "Advance the base"]);
    jj(root, &["bookmark", "set", BASE, "-r", "@-"]);
    jj(root, &["log", "--no-graph", "-r", &base, "-T", "commit_id"])
}

#[test]
fn a_slice_goes_from_open_to_merged() {
    let (project, remote) = project();

    let (code, stdout, stderr) = outcome(&project.zaf(&["slice", "open", "2"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.starts_with(&format!("{SLICE} opened on {BASE}")),
        "{stdout}"
    );
    let dir = workspace(&project);
    assert!(
        dir.join("notes.txt").is_file(),
        "the workspace holds the base"
    );
    let (code, stdout, _) = outcome(&project.zaf(&["slice", "open", "2"]));
    assert_eq!(code, 0);
    assert!(stdout.contains("already open"), "{stdout}");

    write(&dir, "src/export.txt", "title\nauthor\nyear\n");
    let (code, stdout, stderr) = outcome(&project.zaf(&["changes", "2"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, "src/export.txt +3 -0\n1 files +3 -0\n");
    let (code, stdout, _) = outcome(&project.zaf(&["--json", "changes", "2"]));
    assert_eq!(code, 0);
    let stat: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(stat["files"][0]["path"], "src/export.txt");
    assert_eq!(stat["added"], 3);
    let (code, stdout, _) = outcome(&project.zaf(&["changes", "2", "--path", "src/export.txt"]));
    assert_eq!(code, 0);
    assert!(stdout.contains("+author"), "{stdout}");
    let (code, _, _) = outcome(&project.zaf(&["changes", "2", "--path", "README.md"]));
    assert_eq!(code, 1, "a file the slice did not change is not found");

    let (code, stdout, stderr) = outcome(&project.zaf(&["slice", "pr", "2"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, format!("#7 for {SLICE} into {BASE}\n"));
    let published = remote_ref(&remote, SLICE);
    assert!(!published.is_empty());
    let create = project
        .gh_calls()
        .into_iter()
        .find(|call| call.starts_with("pr create"))
        .expect("a pr create call");
    assert!(
        create.contains(&format!(
            "--head {SLICE} --base {BASE} --title Export the list (#2)"
        )),
        "{create}"
    );

    let base_tip = advance_base(&project.root);
    let (code, stdout, stderr) = outcome(&project.zaf(&["slice", "merge", "2"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, format!("#7 merged into {BASE}; #2 done\n"));
    let calls = project.gh_calls();
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("pr merge 7 --merge"))
    );
    assert!(
        calls
            .iter()
            .any(|call| call.starts_with("issue edit 2 --add-label done --remove-label claimed")),
        "{calls:?}"
    );
    assert!(!dir.exists(), "the workspace is dropped");
    let merged = remote_ref(&remote, SLICE);
    let git_dir = remote.to_string_lossy();
    run(
        &remote,
        "git",
        &[
            "--git-dir",
            &git_dir,
            "merge-base",
            "--is-ancestor",
            &base_tip,
            &merged,
        ],
    );
}

#[test]
fn merge_stops_at_the_failing_gate_and_says_so() {
    let (project, _remote) = project();
    assert_eq!(outcome(&project.zaf(&["slice", "open", "2"])).0, 0);
    write(&workspace(&project), "src/other.txt", "not the export\n");
    assert_eq!(outcome(&project.zaf(&["slice", "pr", "2"])).0, 0);

    let (code, stdout, stderr) = outcome(&project.zaf(&["--json", "slice", "merge", "2"]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("gate: the light gate failed"), "{stderr}");
    let failure: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(failure["step"], "gate");
    let calls = project.gh_calls();
    assert!(!calls.iter().any(|call| call.starts_with("pr merge")));
    assert!(!calls.iter().any(|call| call.starts_with("issue edit")));
    assert!(
        workspace(&project).exists(),
        "the workspace stays for the fix"
    );

    let (code, stdout, stderr) = outcome(&project.zaf(&["slice", "drop", "2"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, format!("{SLICE} dropped\n"));
    assert!(!workspace(&project).exists());
    assert_eq!(outcome(&project.zaf(&["slice", "drop", "2"])).0, 0);
}

#[test]
fn a_missing_version_or_backend_exits_two() {
    let (mut project, _remote) = project();
    project.env("ZAF_VERSION", "");
    let (code, _, stderr) = outcome(&project.zaf(&["slice", "open", "2"]));
    assert_eq!(code, 2);
    assert!(stderr.contains("ZAF_VERSION"), "{stderr}");
    project
        .env("ZAF_VERSION", "v1")
        .fail_on("issue view 3 *", "HTTP 502");
    let (code, _, stderr) = outcome(&project.zaf(&["slice", "open", "3"]));
    assert_eq!(code, 2);
    assert!(stderr.contains("ticket:"), "{stderr}");
}
