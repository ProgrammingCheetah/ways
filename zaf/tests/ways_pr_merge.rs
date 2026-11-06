//! The ways repository's `pr-merge` action (docs/zaf-actions.md §4, §5, §7): run through `zaf do`
//! against a fake `gh`, it merges a pull request with a merge commit or squashed, one already
//! merged is quiet, and a refused merge exits 1 after retrying a moved base.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, REPO, outcome, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"
"#;

/// The action as the ways repository ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions/pr-merge");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project with the `pr-merge` action in its actions folder, its gh scene empty.
fn project() -> Project {
    let project = Project::new(MANIFEST);
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = project.root.join(".zaf/actions");
    write(&actions, "pr-merge", &action_text());
    fs::set_permissions(actions.join("pr-merge"), fs::Permissions::from_mode(0o750)).expect("mode");
    project
}

/// `zaf do pr-merge <args>` with its whole log: the exit code and the lines.
fn merge(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", "pr-merge"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

fn view_call(pr: &str) -> String {
    format!("pr view {pr} --json state --jq .state --repo {REPO}")
}

fn merge_call(pr: &str, strategy: &str) -> String {
    format!("pr merge {pr} {strategy} --repo {REPO}")
}

#[test]
fn pr_merge_merges_with_a_merge_commit() {
    let mut project = project();
    project.on("pr view *", "OPEN\n").on("pr merge *", "");

    let (code, text) = merge(&project, &["7"]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(
        project.gh_calls(),
        [view_call("7"), merge_call("7", "--merge")]
    );
}

#[test]
fn pr_merge_pins_the_head_it_is_given() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    for (args, strategy) in [
        (vec!["7", "--match-head-commit", sha], "--merge"),
        (
            vec!["7", "--squash", "--match-head-commit", sha],
            "--squash",
        ),
        (
            vec!["7", "--match-head-commit", sha, "--squash"],
            "--squash",
        ),
    ] {
        let mut project = project();
        project.on("pr view *", "OPEN\n").on("pr merge *", "");

        let (code, text) = merge(&project, &args);

        assert_eq!(code, 0, "{args:?}: {text}");
        assert_eq!(
            project.gh_calls(),
            [
                view_call("7"),
                format!("pr merge 7 {strategy} --match-head-commit {sha} --repo {REPO}")
            ]
        );
    }

    // A commit that is not one, or none at all, is refused before gh is called.
    let project = project();
    for args in [
        &["7", "--match-head-commit"][..],
        &["7", "--match-head-commit", ""][..],
        &["7", "--match-head-commit", "main"][..],
        &["7", "--match-head-commit", "abc;1"][..],
    ] {
        let (code, text) = merge(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn pr_merge_squash_squashes() {
    let mut project = project();
    project.on("pr view *", "OPEN\n").on("pr merge *", "");

    let (code, text) = merge(&project, &["7", "--squash"]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(
        project.gh_calls(),
        [view_call("7"), merge_call("7", "--squash")]
    );
}

#[test]
fn pr_merge_already_merged_is_quiet() {
    let mut project = project();
    project.on("pr view *", "MERGED\n");

    let (code, stdout, stderr) = outcome(&project.zaf(&["do", "pr-merge", "7"]));

    // Exit 0 and a plain run prints nothing, as a pass does; no merge was tried.
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(format!("{stdout}{stderr}"), "");
    assert_eq!(project.gh_calls(), [view_call("7")]);

    let (code, text) = merge(&project, &["7", "--squash"]);
    assert_eq!(code, 0, "{text}");
    assert_eq!(project.gh_calls(), [view_call("7"), view_call("7")]);
}

#[test]
fn pr_merge_refusal_exits_1() {
    // A refusal that is not a moved base is final: one try, no waiting.
    let mut project = project();
    project
        .on("pr view *", "OPEN\n")
        .fail_on("pr merge *", "Pull request #7 has failing required checks");
    let (code, text) = merge(&project, &["7"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("failing required checks"), "{text}");
    assert_eq!(
        project.gh_calls(),
        [view_call("7"), merge_call("7", "--merge")]
    );

    // A pull request closed unmerged is refused, and nothing is merged.
    let mut project = self::project();
    project.on("pr view *", "CLOSED\n");
    let (code, text) = merge(&project, &["7"]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("closed unmerged"), "{text}");
    assert_eq!(project.gh_calls(), [view_call("7")]);

    // Arguments it cannot use are refused before gh is called.
    let project = self::project();
    for args in [
        &[][..],
        &["7", "--merge"][..],
        &["7", "--squash", "extra"][..],
        &["", "--squash"][..],
        &["seven"][..],
        &["-7"][..],
        &["7;1"][..],
        &["--squash", "7"][..],
    ] {
        let (code, text) = merge(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn pr_merge_fails_with_2_when_gh_cannot_read_the_pull_request() {
    let mut project = project();
    project.fail_on("pr view *", "HTTP 502");
    let (code, text) = merge(&project, &["7"]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("HTTP 502"), "{text}");
    assert_eq!(project.gh_calls(), [view_call("7")]);

    // A state gh does not print is a backend failure too.
    let mut project = self::project();
    project.on("pr view *", "");
    let (code, text) = merge(&project, &["7"]);
    assert_eq!(code, 2, "{text}");
    assert!(text.contains("no state"), "{text}");
}

/// A directory holding a `gh` that refuses the first `refusals` merges with `message` and then
/// merges, and a `sleep` that waits for nothing and records what it was asked to wait. Put first on
/// `PATH`, it stands in for the fixtures' scripted gh, so a refusal can change between calls.
fn bin(dir: &Path, refusals: u32, message: &str) {
    let gh = format!(
        r#"#!/bin/sh
here=$(dirname "$0")
echo "$*" >> "$here/calls"
case "$1 $2" in
    "pr view") echo OPEN ;;
    "pr merge")
        count=$(cat "$here/merges" 2>/dev/null || echo 0)
        count=$((count + 1))
        echo "$count" > "$here/merges"
        if [ "$count" -le {refusals} ]; then
            echo "{message}" >&2
            exit 1
        fi
        ;;
    *) echo "unexpected: $*" >&2; exit 97 ;;
esac
"#
    );
    let sleep = "#!/bin/sh\necho \"$*\" >> \"$(dirname \"$0\")/sleeps\"\n";
    for (name, text) in [("gh", gh.as_str()), ("sleep", sleep)] {
        write(dir, name, text);
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o755)).expect("mode");
    }
}

/// The lines a bin script recorded, in order.
fn recorded(dir: &Path, file: &str) -> Vec<String> {
    fs::read_to_string(dir.join(file))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A project whose `PATH` starts with a bin directory, which it returns.
fn project_with_bin(refusals: u32, message: &str) -> (Project, PathBuf) {
    let mut project = project();
    let dir = project.temp.path().join("bin");
    bin(&dir, refusals, message);
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path =
        std::env::join_paths(std::iter::once(dir.clone()).chain(std::env::split_paths(&inherited)))
            .expect("search path");
    project.env("PATH", &search_path.to_string_lossy());
    (project, dir)
}

#[test]
fn pr_merge_retries_a_moved_base() {
    for message in [
        "X Pull request #7 Base branch was modified. Review and try the merge again.",
        "X Pull request #7 is not mergeable: the merge commit cannot be cleanly created",
    ] {
        // Two refusals and then a merge: exit 0, three tries, two waits of 15 seconds.
        let (project, dir) = project_with_bin(2, message);
        let (code, text) = merge(&project, &["7", "--squash"]);
        assert_eq!(code, 0, "{message}: {text}");
        let merges = vec!["pr merge 7 --squash --repo example/tracker"; 3];
        assert_eq!(recorded(&dir, "merges"), ["3"]);
        assert_eq!(
            recorded(&dir, "calls")
                .iter()
                .filter(|call| call.starts_with("pr merge"))
                .collect::<Vec<_>>(),
            merges
        );
        assert_eq!(recorded(&dir, "sleeps"), ["15", "15"]);

        // Refused every time: the first try and three retries, three waits, then exit 1.
        let (project, dir) = project_with_bin(99, message);
        let (code, text) = merge(&project, &["7"]);
        assert_eq!(code, 1, "{message}: {text}");
        assert!(text.contains("after 3 retries"), "{text}");
        assert_eq!(recorded(&dir, "merges"), ["4"]);
        assert_eq!(recorded(&dir, "sleeps"), ["15", "15", "15"]);
    }

    // Any other refusal is not waited for.
    let (project, dir) = project_with_bin(99, "X Pull request #7 has failing required checks");
    let (code, text) = merge(&project, &["7"]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(recorded(&dir, "merges"), ["1"]);
    assert_eq!(recorded(&dir, "sleeps"), Vec::<String>::new());
}
