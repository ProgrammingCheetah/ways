//! The ways repository's `release` action (docs/zaf-actions.md §4, §7): run through `zaf do` on a
//! throwaway jj repository with two bare remotes (`origin`, the forge's, and `github`, the
//! mirror) and a stand-in `gh` for the forge verbs it asks, it refuses, changing neither remote,
//! unless the integration branch's pull request has green checks at the branch's head; else it
//! makes main one squash commit (the branch's files, on the main before it, the message from a
//! file), tags it `zaf-v<version>` and pushes main and the tag to origin, then to the mirror. A
//! run that stopped is picked up by the next. Only the release role runs it.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use common::{Project, jj, outcome, run, write};

const VERSION: &str = "0.4.1";
const BASE: &str = "feature/0.4.1";
const TAG: &str = "zaf-v0.4.1";
const RELEASE_INSTANCE: &str = "release instance 1 on #103";

/// The pull request the stand-in forge names for the branch.
const PR: &str = "9";

const MESSAGE: &str = "zaf 0.4.1: the fixes\n\n- one\n- two\n";

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

/// The action as the ways repository ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".zaf/actions/release");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The stand-in `gh` the action's `zaf forge …` calls reach (the built `zaf` is first on an
/// action's path, so the verbs are its own): `pr list` prints the `pr` file as the one open pull
/// request, `pr view` the `head` file and `pr checks` the `checks` file, all beside it, and
/// `pr checks` fails when a `checks-fail` file is there. Every call is recorded in `calls`, as its
/// first two words.
const FORGE: &str = r#"#!/bin/sh
here=$(dirname "$0")
printf '%s %s\n' "$1" "$2" >> "$here/calls"
case "$1 $2" in
    "pr list") printf '%s\thttp://forge.example/pull/%s\n' "$(cat "$here/pr")" "$(cat "$here/pr")" ;;
    "pr view") cat "$here/head" ;;
    "pr checks")
        if [ -f "$here/checks-fail" ]; then
            echo "HTTP 502: the forge is down" >&2
            exit 1
        fi
        cat "$here/checks"
        ;;
    *) echo "unexpected: $*" >&2; exit 97 ;;
esac
"#;

/// One check line as `zaf forge checks` prints it: bucket, name and link.
fn check(bucket: &str, name: &str) -> String {
    format!("{bucket}\t{name}\thttp://forge.example/{name}\n")
}

fn forge_dir(project: &Project) -> PathBuf {
    project.temp.path().join("forge")
}

fn remote_dir(project: &Project, remote: &str) -> PathBuf {
    project.temp.path().join(format!("{remote}.git"))
}

fn remote_git(project: &Project, remote: &str, args: &[&str]) -> String {
    let dir = remote_dir(project, remote);
    let mut full = vec!["--git-dir", dir.to_str().expect("utf-8 path")];
    full.extend_from_slice(args);
    run(project.temp.path(), "git", &full).trim().to_owned()
}

/// The commit a remote's ref is at, or nothing when it has none.
fn remote_ref(project: &Project, remote: &str, name: &str) -> Option<String> {
    let listed = remote_git(
        project,
        remote,
        &["for-each-ref", "--format=%(objectname)", name],
    );
    (!listed.is_empty()).then_some(listed)
}

/// The commit a revset names in the root repository.
fn commit(root: &Path, revset: &str) -> String {
    jj(
        root,
        &[
            "--ignore-working-copy",
            "log",
            "--no-graph",
            "-r",
            revset,
            "-T",
            "commit_id",
        ],
    )
}

/// A project whose root is a jj repository with two bare remotes. Main is one release commit, on
/// both; the integration branch starts from a commit with main's files (main holds only squash
/// commits, so it is not among the branch's) and has work at `VERSION`, on origin. The forge
/// stand-in names the branch's pull request at the branch's head with green checks, the message
/// file is beside the root, and no version is set, so `ZAF_BASE` is not.
fn unversioned_project() -> Project {
    let mut project = Project::new(MANIFEST);
    let temp = project.temp.path().to_owned();
    let root = project.root.clone();
    run(&temp, "git", &["init", "-q", "--bare", "origin.git"]);
    run(&temp, "git", &["init", "-q", "--bare", "github.git"]);
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
    for remote in ["origin", "github"] {
        let dir = remote_dir(&project, remote);
        jj(
            &root,
            &["git", "remote", "add", remote, &dir.to_string_lossy()],
        );
    }
    // The action is the project's own file, so it sits in every commit.
    write(&root, ".gitignore", ".zaf/*\n!.zaf/actions/\n");
    write(&root, "notes.txt", "one\n");
    write(&root, "zaf/Cargo.toml", &cargo_toml("0.4.0"));
    let actions = root.join(".zaf/actions");
    write(&actions, "release", &action_text());
    fs::set_permissions(actions.join("release"), fs::Permissions::from_mode(0o750)).expect("mode");
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run,
    // and before the first commit so what it writes is in every commit.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    jj(&root, &["commit", "-m", "zaf 0.4.0"]);
    jj(&root, &["bookmark", "create", "main", "-r", "@-"]);
    jj(
        &root,
        &["git", "push", "--remote", "origin", "--bookmark", "main"],
    );
    jj(&root, &["bookmark", "track", "main@github"]);
    jj(
        &root,
        &["git", "push", "--remote", "github", "--bookmark", "main"],
    );

    jj(&root, &["new", "root()"]);
    jj(&root, &["restore", "--from", "main"]);
    jj(
        &root,
        &["commit", "-m", "Start 0.4.1 from the files of 0.4.0"],
    );
    write(&root, "notes.txt", "one\ntwo\n");
    write(&root, "zaf/Cargo.toml", &cargo_toml(VERSION));
    jj(&root, &["commit", "-m", "The work of 0.4.1"]);
    jj(&root, &["bookmark", "create", BASE, "-r", "@-"]);
    jj(
        &root,
        &["git", "push", "--remote", "origin", "--bookmark", BASE],
    );
    // The checkout is left on main, with a file of its own that no release may touch.
    jj(&root, &["new", "main"]);
    write(&root, "scratch.txt", "the Engineer's\n");

    let forge = forge_dir(&project);
    write(&forge, "gh", FORGE);
    fs::set_permissions(forge.join("gh"), fs::Permissions::from_mode(0o755)).expect("mode");
    write(&forge, "pr", &format!("{PR}\n"));
    write(
        &forge,
        "head",
        &commit(&root, &format!("bookmarks(exact:\"{BASE}\")")),
    );
    write(
        &forge,
        "checks",
        &[check("pass", "test"), check("skipping", "release")].concat(),
    );
    write(&temp, "message", MESSAGE);

    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path =
        std::env::join_paths(std::iter::once(forge).chain(std::env::split_paths(&inherited)))
            .expect("search path");
    project.env("PATH", &search_path.to_string_lossy());
    project
}

/// The same with the version set, so `ZAF_BASE` is `BASE`, run by the release instance.
fn project() -> Project {
    let mut project = unversioned_project();
    project
        .env("ZAF_VERSION", VERSION)
        .env("ZAF_INSTANCE", RELEASE_INSTANCE);
    project
}

/// `zaf do --more release <args>`: the exit code, the whole log (stdout) and everything printed.
fn release_with(project: &Project, args: &[&str]) -> (i32, String, String) {
    let mut command = vec!["do", "--more", "release"];
    command.extend_from_slice(args);
    let (code, stdout, stderr) = outcome(&project.zaf(&command));
    let text = format!("{stdout}{stderr}");
    (code, stdout, text)
}

fn message_path(project: &Project) -> String {
    project
        .temp
        .path()
        .join("message")
        .to_string_lossy()
        .into_owned()
}

fn release(project: &Project) -> (i32, String, String) {
    release_with(project, &[VERSION, &message_path(project)])
}

/// The last non-empty line of a log: what the action handed back.
fn last_line(log: &str) -> &str {
    log.lines()
        .rfind(|line| !line.trim().is_empty())
        .expect("a line")
}

/// What the two remotes hold of a release: each one's main and tag.
fn remotes(project: &Project) -> [Option<String>; 4] {
    [
        remote_ref(project, "origin", "refs/heads/main"),
        remote_ref(project, "origin", &format!("refs/tags/{TAG}")),
        remote_ref(project, "github", "refs/heads/main"),
        remote_ref(project, "github", &format!("refs/tags/{TAG}")),
    ]
}

/// The calls the stand-in forge recorded, in order.
fn forge_calls(project: &Project) -> Vec<String> {
    fs::read_to_string(forge_dir(project).join("calls"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A remote that refuses every push from here on, or takes them again.
fn refuse_pushes(project: &Project, remote: &str, refuse: bool) {
    let hook = remote_dir(project, remote).join("hooks/pre-receive");
    if refuse {
        fs::write(&hook, "#!/bin/sh\nexit 1\n").expect("hook");
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("mode");
    } else {
        fs::remove_file(&hook).expect("remove the hook");
    }
}

#[test]
fn release_squashes_tags_and_pushes_to_origin_then_the_mirror() {
    let project = project();
    let before = remotes(&project);
    let main = before[0].clone().expect("main");
    let branch = remote_ref(&project, "origin", &format!("refs/heads/{BASE}")).expect("branch");

    let (code, log, text) = release(&project);

    assert_eq!(code, 0, "{text}");
    let made = remote_ref(&project, "origin", "refs/heads/main").expect("main");
    assert_ne!(made, main);
    assert_eq!(last_line(&log), made, "{log}");
    // One commit on the main before it, with the branch's files and the message given.
    assert_eq!(
        remote_git(&project, "origin", &["log", "-1", "--format=%P", &made]),
        main
    );
    assert_eq!(
        remote_git(
            &project,
            "origin",
            &["rev-parse", &format!("{made}^{{tree}}")]
        ),
        remote_git(
            &project,
            "origin",
            &["rev-parse", &format!("{branch}^{{tree}}")]
        ),
    );
    assert_eq!(
        remote_git(&project, "origin", &["log", "-1", "--format=%B", &made]),
        MESSAGE.trim()
    );
    // Main and the tag are on both remotes, at that commit.
    assert_eq!(remotes(&project), [(); 4].map(|()| Some(made.clone())));
    // The log says what it did, origin before the mirror.
    let origin = log.find(&format!("pushed main and {TAG} to origin"));
    let mirror = log.find(&format!("pushed main and {TAG} to github"));
    assert!(origin.is_some() && origin < mirror, "{log}");
    assert!(
        log.contains(&format!("pull request #{PR} stays open")),
        "{log}"
    );
    // The forge was asked for the branch's pull request into main, its head and its checks.
    assert_eq!(
        forge_calls(&project),
        [
            "pr list".to_owned(),
            "pr view".to_owned(),
            "pr checks".to_owned(),
        ]
    );
    // The checkout is where it was, its own file with it.
    let root = &project.root;
    assert_eq!(commit(root, "@-"), main);
    assert_eq!(
        fs::read_to_string(root.join("scratch.txt")).expect("the file"),
        "the Engineer's\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("notes.txt")).expect("the file"),
        "one\n"
    );

    // A second run finds it made and changes nothing.
    let (code, log, text) = release(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(last_line(&log), made, "{log}");
    assert_eq!(remotes(&project), [(); 4].map(|()| Some(made.clone())));
}

#[test]
fn release_refuses_checks_that_are_not_green() {
    let project = project();
    let before = remotes(&project);
    let forge = forge_dir(&project);

    // Running, failed and cancelled checks are named; none at all, or only skipped ones, is not
    // green either.
    for (checks, said) in [
        (
            [check("pass", "test"), check("pending", "build")].concat(),
            "not green: build (pending)",
        ),
        (check("fail", "test"), "not green: test (fail)"),
        (
            [check("pass", "build"), check("cancel", "test")].concat(),
            "not green: test (cancel)",
        ),
        (String::new(), "no passed check"),
        (check("skipping", "release"), "no passed check"),
    ] {
        write(&forge, "checks", &checks);
        let (code, _, text) = release(&project);
        assert_eq!(code, 1, "{checks:?}: {text}");
        assert!(text.contains(said), "{checks:?}: {text}");
        assert_eq!(remotes(&project), before, "{checks:?}");
    }

    // Green checks of another head are not the branch's.
    write(&forge, "checks", &check("pass", "test"));
    write(&forge, "head", "0123456789abcdef0123456789abcdef01234567\n");
    let (code, _, text) = release(&project);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("is at 0123456789abcdef"), "{text}");
    assert_eq!(remotes(&project), before);

    // A forge that fails is a backend failure, and nothing is released either.
    write(
        &forge,
        "head",
        &commit(&project.root, &format!("bookmarks(exact:\"{BASE}\")")),
    );
    write(&forge, "checks-fail", "");
    let (code, _, text) = release(&project);
    assert_eq!(code, 2, "{text}");
    assert_eq!(remotes(&project), before);

    // No release commit was left behind, and the local main did not move.
    let root = &project.root;
    assert_eq!(
        commit(root, "bookmarks(exact:\"main\")"),
        before[0].clone().expect("main")
    );
    assert_eq!(commit(root, "description(glob:\"zaf release *\")"), "");
}

#[test]
fn release_checks_the_version_on_the_branch_and_its_arguments() {
    let project = project();
    let before = remotes(&project);
    let message = message_path(&project);

    // The branch's crate is at another version than the one asked for; the checkout's own
    // manifest, at 0.4.0, is never the one read.
    let (code, _, text) = release_with(&project, &["0.4.0", &message]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("is at 0.4.1, not 0.4.0"), "{text}");
    // The crate's dependencies are not the crate: the decoy's version is never read.
    let (code, _, text) = release_with(&project, &["9.9.9", &message]);
    assert_eq!(code, 1, "{text}");

    // Arguments that are not one usable version and one readable, non-empty message file.
    write(project.temp.path(), "blank", "\n\n");
    let blank = project.temp.path().join("blank");
    for args in [
        &[][..],
        &[VERSION][..],
        &[VERSION, &message, "extra"][..],
        &["", &message][..],
        &["-0.4.1", &message][..],
        &["0.4.1;1", &message][..],
        &[VERSION, "no-such-file"][..],
        &[VERSION, &blank.to_string_lossy()][..],
    ] {
        let (code, _, text) = release_with(&project, args);
        assert_eq!(code, 1, "{args:?}: {text}");
    }
    // None of it reached the forge or a remote.
    assert_eq!(forge_calls(&project), Vec::<String>::new());
    assert_eq!(remotes(&project), before);

    // A relative message file is read from the caller's directory.
    write(&project.root, "zaf/notes/message", MESSAGE);
    let mut command = project.command_in(
        &project.root.join("zaf/notes"),
        &["do", "release", VERSION, "message"],
    );
    let (code, stdout, stderr) = outcome(&command.output().expect("run zaf"));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(remotes(&project)[1].is_some());
}

#[test]
fn release_without_an_integration_branch_changes_nothing() {
    // ZAF_BASE unset: with no version in the environment zaf sets none.
    let mut project = unversioned_project();
    project.env("ZAF_INSTANCE", RELEASE_INSTANCE);
    let before = remotes(&project);
    let (code, _, text) = release(&project);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("ZAF_BASE"), "{text}");
    assert_eq!(forge_calls(&project), Vec::<String>::new());
    assert_eq!(remotes(&project), before);

    // A version whose branch origin lacks.
    project.env("ZAF_VERSION", "0.9.9");
    let (code, _, text) = release_with(&project, &["0.9.9", &message_path(&project)]);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("feature/0.9.9 is not on origin"), "{text}");
    assert_eq!(remotes(&project), before);

    // No mirror to push to.
    let project = self::project();
    let before = remotes(&project);
    jj(&project.root, &["git", "remote", "remove", "github"]);
    let (code, _, text) = release(&project);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("no remote named github"), "{text}");
    assert_eq!(remotes(&project)[..2], before[..2]);
}

#[test]
fn release_refuses_a_branch_that_does_not_carry_main() {
    // Main got a commit the branch never saw: the branch's files on top would undo it.
    let project = project();
    let root = &project.root;
    write(root, "hotfix.txt", "on main only\n");
    jj(root, &["commit", "-m", "A fix straight on main"]);
    jj(root, &["bookmark", "set", "main", "-r", "@-"]);
    jj(
        root,
        &["git", "push", "--remote", "origin", "--bookmark", "main"],
    );
    jj(
        root,
        &["git", "push", "--remote", "github", "--bookmark", "main"],
    );
    let before = remotes(&project);

    let (code, _, text) = release(&project);

    assert_eq!(code, 1, "{text}");
    assert!(text.contains("does not carry main"), "{text}");
    assert_eq!(forge_calls(&project), Vec::<String>::new());
    assert_eq!(remotes(&project), before);

    // With main merged into the branch it is among the branch's commits, and the release goes on.
    let base = format!("bookmarks(exact:\"{BASE}\")");
    jj(root, &["new", &base, "main", "-m", "Merge main"]);
    jj(root, &["restore", "--from", &base]);
    write(root, "hotfix.txt", "on main only\n");
    jj(root, &["bookmark", "set", BASE, "-r", "@"]);
    jj(
        root,
        &["git", "push", "--remote", "origin", "--bookmark", BASE],
    );
    write(&forge_dir(&project), "head", &commit(root, &base));
    let (code, log, text) = release(&project);
    assert_eq!(code, 0, "{text}");
    let made = last_line(&log).to_owned();
    assert_eq!(
        remote_git(&project, "origin", &["show", &format!("{made}:hotfix.txt")]),
        "on main only"
    );
}

#[test]
fn release_refuses_a_tag_or_a_main_that_is_somewhere_else() {
    // The tag is on another commit, on origin only: a refusal, never a move.
    let project = project();
    let main = remote_ref(&project, "origin", "refs/heads/main").expect("main");
    remote_git(&project, "origin", &["tag", TAG, &main]);
    let before = remotes(&project);
    let (code, _, text) = release(&project);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("already on another commit"), "{text}");
    assert_eq!(remotes(&project), before);

    // The local main has work origin lacks.
    let project = self::project();
    let root = &project.root;
    write(root, "local.txt", "local\n");
    jj(root, &["commit", "-m", "Local only"]);
    jj(root, &["bookmark", "set", "main", "-r", "@-"]);
    let before = remotes(&project);
    let (code, _, text) = release(&project);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("push or reset it first"), "{text}");
    assert_eq!(remotes(&project), before);

    // The mirror's main has a commit origin's lacks.
    let project = self::project();
    let temp = project.temp.path();
    let mirror = remote_dir(&project, "github");
    run(
        temp,
        "git",
        // The branch is named: a bare remote's HEAD follows the machine's `init.defaultBranch`.
        &[
            "clone",
            "-q",
            "-b",
            "main",
            &mirror.to_string_lossy(),
            "other",
        ],
    );
    let other = temp.join("other");
    write(&other, "mirror.txt", "mirror only\n");
    run(&other, "git", &["add", "mirror.txt"]);
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
            "On the mirror only",
        ],
    );
    run(&other, "git", &["push", "-q", "origin", "HEAD:main"]);
    let before = remotes(&project);
    let (code, _, text) = release(&project);
    assert_eq!(code, 1, "{text}");
    assert!(text.contains("settle the mirror first"), "{text}");
    assert_eq!(remotes(&project), before);
}

#[test]
fn release_picks_up_a_run_that_stopped_at_a_push() {
    // The mirror refuses the push: origin has the release, and the code says the mirror lacks it.
    let project = project();
    let before = remotes(&project);
    refuse_pushes(&project, "github", true);
    let (code, _, text) = release(&project);
    assert_eq!(code, 3, "{text}");
    let after = remotes(&project);
    let made = after[0].clone().expect("main");
    assert_ne!(after[0], before[0]);
    assert_eq!(after[1], after[0]);
    assert_eq!(after[2..], before[2..]);
    // The next run makes no second commit: it pushes the one there is to the mirror.
    refuse_pushes(&project, "github", false);
    let (code, log, text) = release(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(last_line(&log), made, "{log}");
    assert_eq!(remotes(&project), [(); 4].map(|()| Some(made.clone())));

    // Origin refuses the push: neither remote has anything, and the next run pushes the commit
    // the first one made.
    let project = self::project();
    let before = remotes(&project);
    refuse_pushes(&project, "origin", true);
    let (code, _, text) = release(&project);
    assert_eq!(code, 2, "{text}");
    assert_eq!(remotes(&project), before);
    let made = commit(&project.root, "bookmarks(exact:\"main\")");
    refuse_pushes(&project, "origin", false);
    let (code, log, text) = release(&project);
    assert_eq!(code, 0, "{text}");
    assert_eq!(last_line(&log), made, "{log}");
    assert_eq!(remotes(&project), [(); 4].map(|()| Some(made.clone())));
}

#[test]
fn release_is_refused_for_a_programmer() {
    for instance in ["programmer instance 1 on #9", "team instance 1 on #9"] {
        let mut project = project();
        project.env("ZAF_INSTANCE", instance);
        let before = remotes(&project);
        let role = instance.split(' ').next().expect("a role");

        let (code, stdout, stderr) =
            outcome(&project.zaf(&["do", "release", VERSION, &message_path(&project)]));

        assert_eq!(code, 125, "{stdout}{stderr}");
        assert_eq!(
            stderr,
            format!("NOT_RUN release: not permitted for {role}\n")
        );
        // Nothing was asked of the forge and nothing was pushed.
        assert_eq!(forge_calls(&project), Vec::<String>::new());
        assert_eq!(remotes(&project), before);
    }
}
