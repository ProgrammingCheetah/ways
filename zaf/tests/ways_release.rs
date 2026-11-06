//! The ways repository's `release` action (docs/zaf-actions.md §4, §7): run through `zaf do`
//! against a fake `gh` and stand-ins for the three steps it composes, it checks `zaf/Cargo.toml`
//! is at the version and `ZAF_BASE` has an open pull request, changing nothing when not, and
//! else runs `pr-merge <pr> --squash`, `sync main` and `tag zaf-v<version>` in that order,
//! stopping at the first that fails. Only the release role runs it.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, REPO, outcome, write};

const VERSION: &str = "0.2.0";
const BASE: &str = "feature/0.2.0";
const RELEASE_INSTANCE: &str = "release instance 1 on #103";

/// The integration branch's pull request into main, as `gh pr list` prints it: its number and head.
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const LISTED: &str = "7 0123456789abcdef0123456789abcdef01234567\n";

/// The merge the release runs: squashed, and pinned to the head it listed.
const PINNED_MERGE: &str =
    "pr-merge 7 --squash --match-head-commit 0123456789abcdef0123456789abcdef01234567";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"

[roles.release]
model = "opus"
effort = "high"
profile = "build"
turns = 100
"#;

/// A crate manifest at `version`, with a dependency table that also has a `version` key.
fn cargo_toml(version: &str) -> String {
    format!(
        "[package]\nname = \"zaf\"\nversion = \"{version}\"\nedition = \"2024\"\n\n\
         [dependencies.decoy]\nversion = \"9.9.9\"\n"
    )
}

/// An action as the ways repository ships it.
fn action_text(name: &str) -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions")
        .join(name);
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A stand-in for a step: it appends its name and arguments to `steps` at the root, and exits
/// with the code in a `fail-<name>` file there when the file exists. It is as open as the real
/// step, so only `release`'s own mode decides who gets that far.
fn stand_in(name: &str) -> String {
    format!(
        "#!/bin/sh\n# zaf description: a stand-in for {name}\n# zaf 1: refused\n# zaf 2: failed\n\
         # zaf mode: 750\n# zaf group: team, release\n\
         echo \"{name} $*\" >> \"$ZAF_ROOT/steps\"\n\
         if [ -f \"$ZAF_ROOT/fail-{name}\" ]; then\n    exit \"$(cat \"$ZAF_ROOT/fail-{name}\")\"\nfi\n\
         exit 0\n"
    )
}

fn install(project: &Project, name: &str, text: &str) {
    let actions = project.root.join(".zaf/actions");
    write(&actions, name, text);
    fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o750)).expect("mode");
}

/// A project with `release`, stand-ins for `pr-merge`, `sync` and `tag`, a crate at `VERSION`,
/// and `zaf` itself on PATH for the nested calls; no version is set, so `ZAF_BASE` is not.
fn unversioned_project() -> Project {
    let mut project = Project::new(MANIFEST);
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    install(&project, "release", &action_text("release"));
    for name in ["pr-merge", "sync", "tag"] {
        install(&project, name, &stand_in(name));
    }
    write(&project.root, "zaf/Cargo.toml", &cargo_toml(VERSION));

    let bin = Path::new(env!("CARGO_BIN_EXE_zaf"))
        .parent()
        .expect("the binary's folder");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        [bin.to_owned(), fixtures]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    project.env("PATH", &search_path.to_string_lossy());
    project
}

/// The same with the version set, so `ZAF_BASE` is `BASE`.
fn project() -> Project {
    let mut project = unversioned_project();
    project.env("ZAF_VERSION", VERSION);
    project
}

/// `zaf do --more release <args>` with its whole log: the exit code and the lines.
fn release(project: &Project, args: &[&str]) -> (i32, String) {
    let mut command = vec!["do", "--more", "release"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    (code, format!("{stdout}{stderr}"))
}

/// The steps the stand-ins ran, in order, one line each.
fn steps(project: &Project) -> Vec<String> {
    fs::read_to_string(project.root.join("steps"))
        .map(|text| text.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

fn list_call() -> String {
    format!(
        "pr list --head {BASE} --base main --state open --json number,headRefOid \
         --jq .[] | \"\\(.number) \\(.headRefOid)\" --repo {REPO}"
    )
}

#[test]
fn release_pins_the_head_and_lists_only_prs_into_main() {
    let mut project = project();
    project
        .on("pr list *", LISTED)
        .env("ZAF_INSTANCE", RELEASE_INSTANCE);

    let (code, text) = release(&project, &[VERSION]);

    assert_eq!(code, 0, "{text}");
    // Only pull requests into main are listed, so a pull request from the base into another
    // branch is never the release.
    let calls = project.gh_calls();
    assert_eq!(calls, [list_call()]);
    assert!(calls[0].contains(" --base main "), "{calls:?}");
    // The merge names the head that was listed, the commit CI passed.
    assert_eq!(steps(&project)[0], PINNED_MERGE);
    assert!(steps(&project)[0].ends_with(HEAD));

    // A list with no head commit is a backend failure: nothing merges.
    let mut project = self::project();
    project.on("pr list *", "7\n");
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 2, "{text}");
    assert_eq!(steps(&project), Vec::<String>::new());
}

#[test]
fn release_checks_the_version() {
    let mut project = project();
    project.on("pr list *", LISTED);

    // The crate is at another version: refused before gh is called, and nothing changes.
    write(&project.root, "zaf/Cargo.toml", &cargo_toml("0.1.9"));
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("0.1.9"), "{text}");
    assert_eq!(project.gh_calls(), Vec::<String>::new());
    assert_eq!(steps(&project), Vec::<String>::new());

    // The crate's dependencies are not the crate: a decoy `version` there is never read.
    write(
        &project.root,
        "zaf/Cargo.toml",
        "[package]\nname = \"zaf\"\n\n[dependencies.decoy]\nversion = \"0.2.0\"\n",
    );
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 1, "{text}");

    // No manifest at all is not at the version either.
    fs::remove_file(project.root.join("zaf/Cargo.toml")).expect("remove");
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 1, "{text}");

    // An argument that is not one usable version is refused the same way.
    write(&project.root, "zaf/Cargo.toml", &cargo_toml(VERSION));
    for args in [
        &[][..],
        &[VERSION, "extra"][..],
        &[""][..],
        &["-0.2.0"][..],
        &["0.2.0;1"][..],
        &["0.2.0 "][..],
    ] {
        let (code, text) = release(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }
    assert_eq!(project.gh_calls(), Vec::<String>::new());
    assert_eq!(steps(&project), Vec::<String>::new());

    // At the version it goes on.
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 0, "{text}");
}

#[test]
fn release_without_a_pr_changes_nothing() {
    // ZAF_BASE unset: with no version in the environment zaf sets none. Nothing is asked of gh.
    let mut project = unversioned_project();
    project.on("pr list *", LISTED);
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("ZAF_BASE"), "{text}");
    assert_eq!(project.gh_calls(), Vec::<String>::new());
    assert_eq!(steps(&project), Vec::<String>::new());

    // The base has no open pull request: gh is asked, and nothing runs.
    let mut project = self::project();
    project.on("pr list *", "");
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("no open pull request"), "{text}");
    assert_eq!(project.gh_calls(), [list_call()]);
    assert_eq!(steps(&project), Vec::<String>::new());

    // gh refusing to list is a backend failure, and nothing runs either.
    let mut project = self::project();
    project.fail_on("pr list *", "HTTP 502");
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 2, "{text}");
    assert_eq!(steps(&project), Vec::<String>::new());
}

#[test]
fn release_merges_syncs_tags_in_order() {
    let mut project = project();
    project
        .on("pr list *", LISTED)
        .env("ZAF_INSTANCE", RELEASE_INSTANCE);

    let (code, text) = release(&project, &[VERSION]);

    assert_eq!(code, 0, "{text}");
    assert_eq!(project.gh_calls(), [list_call()]);
    assert_eq!(
        steps(&project),
        [PINNED_MERGE, "sync main", &format!("tag zaf-v{VERSION}")]
    );
}

#[test]
fn release_stops_on_a_refused_merge() {
    // The merge is refused: nothing after it runs, and its code is the release's.
    let mut project = project();
    project.on("pr list *", LISTED);
    write(&project.root, "fail-pr-merge", "1");
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(steps(&project), [PINNED_MERGE]);

    // The sync fails: the merge ran, the tag does not.
    let mut project = self::project();
    project.on("pr list *", LISTED);
    write(&project.root, "fail-sync", "2");
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 2, "{text}");
    assert_eq!(steps(&project), [PINNED_MERGE, "sync main"]);

    // The tag is refused: all three ran, and the release says so.
    let mut project = self::project();
    project.on("pr list *", LISTED);
    write(&project.root, "fail-tag", "1");
    let (code, text) = release(&project, &[VERSION]);
    assert_eq!(code, 1, "{text}");
    assert_eq!(steps(&project).len(), 3);
}

#[test]
fn release_is_refused_for_a_programmer() {
    for instance in ["programmer instance 1 on #9", "team instance 1 on #9"] {
        let mut project = project();
        project
            .on("pr list *", LISTED)
            .env("ZAF_INSTANCE", instance);
        let role = instance.split(' ').next().expect("a role");

        let (code, stdout, stderr) = outcome(&project.zaf(&["do", "release", VERSION]));

        assert_eq!(code, 125, "{stdout}{stderr}");
        assert_eq!(
            stderr,
            format!("NOT_RUN release: not permitted for {role}\n")
        );
        // Nothing was asked of gh and nothing merged.
        assert_eq!(project.gh_calls(), Vec::<String>::new());
        assert_eq!(steps(&project), Vec::<String>::new());
    }

    // A role that reaches the nested `tag` is refused the same way: the real `tag` is not
    // open to a role outside its group, and the release role's own run of it goes through.
    let mut project = project();
    install(&project, "tag", &action_text("tag"));
    project.env("ZAF_INSTANCE", "programmer instance 1 on #9");
    let (code, stdout, stderr) = outcome(&project.zaf(&["do", "tag", "zaf-v0.2.0"]));
    assert_eq!(code, 125, "{stdout}{stderr}");
    assert_eq!(stderr, "NOT_RUN tag: not permitted for programmer\n");
}
