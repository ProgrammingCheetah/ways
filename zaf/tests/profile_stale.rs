//! A profile older than the manifest or zaf, through the binary: `zaf spawn` refuses it, `zaf
//! doctor` fails on it and `zaf status` names it, each with the same line; `zaf profile init`
//! makes all three pass again.

mod common;

use std::fs;
use std::path::Path;

use common::{Project, outcome};

const HEAD: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n";

const PUSH: &str = "[forbidden]\ncommands = [\"git push\"]\n";
const MERGE: &str = "[forbidden]\ncommands = [\"gh pr merge\"]\n";

/// Every role's profile, lean and build, is stale in these projects, so they read as one sentence.
const STALE_FORBIDDEN: &str =
    "profiles lean and build are older than the manifest (forbidden changed)";
const NO_STAMP: &str = "profile build has no stamp: run zaf profile init";

const SPAWN: [&str; 4] = ["spawn", "--once", "--version", "0.2.1"];

/// A project whose profiles were generated from the `PUSH` manifest, with the board empty.
fn generated() -> Project {
    let mut project = Project::new(&format!("{HEAD}{PUSH}"));
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project.on("*", "[]");
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    project
}

/// Edits `[forbidden]` without running `zaf profile init`.
fn edit_forbidden(project: &Project) {
    fs::write(project.root.join(".zaf.toml"), format!("{HEAD}{MERGE}")).expect("manifest");
}

fn stamp(project: &Project) -> std::path::PathBuf {
    project.root.join(".zaf/profiles/build/.zaf-stamp.json")
}

fn launched(root: &Path) -> bool {
    root.join(".zaf/spawn.state").exists()
}

/// A project with two roles, so two generated profiles, both stamped from the `PUSH` manifest and
/// then made stale by the `MERGE` one.
fn generated_two() -> Project {
    let head = format!(
        "{HEAD}[roles.programmer]\nmodel = \"sonnet\"\neffort = \"high\"\nprofile = \"lean\"\nturns = 5\nbatch = 2\n\n"
    );
    let mut project = Project::new(&format!("{head}{PUSH}"));
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project.on("*", "[]");
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    fs::write(project.root.join(".zaf.toml"), format!("{head}{MERGE}")).expect("manifest");
    project
}

const BOTH: &str = STALE_FORBIDDEN;

fn count(text: &str, needle: &str) -> usize {
    text.lines().filter(|line| line.contains(needle)).count()
}

#[test]
fn spawn_refuses_with_one_line_for_stale_profiles() {
    let project = generated_two();
    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(count(&stderr, "older than"), 1, "{stderr}");
    assert!(stderr.contains(BOTH), "{stderr}");
    assert!(!launched(&project.root), "nothing was launched");
}

#[test]
fn doctor_prints_one_line_for_stale_profiles() {
    let project = generated_two();
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(count(&stdout, "older than"), 1, "{stdout}");
    assert!(stdout.contains(BOTH), "{stdout}");
}

#[test]
fn status_prints_one_line_for_stale_profiles() {
    let project = generated_two();
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(count(&stdout, "older than"), 1, "{stdout}");
    assert!(stdout.contains(BOTH), "{stdout}");
}

#[test]
fn a_current_profile_spawns() {
    let project = generated();
    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn spawn_refuses_a_stale_profile() {
    let project = generated();
    edit_forbidden(&project);

    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains(STALE_FORBIDDEN), "{stderr}");
    assert!(!launched(&project.root), "nothing was launched");

    let dry = ["spawn", "--once", "--version", "0.2.1", "--dry-run"];
    let (code, stdout, stderr) = outcome(&project.zaf(&dry));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains(STALE_FORBIDDEN), "{stderr}");

    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&dry));
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn spawn_refuses_a_profile_with_no_stamp_or_written_by_another_zaf() {
    let project = generated();
    let text = fs::read_to_string(stamp(&project)).expect("stamp");

    let other_zaf = text.replace(env!("CARGO_PKG_VERSION"), "0.0.1");
    fs::write(stamp(&project), &other_zaf).expect("other zaf");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 0, "the same generation is current: {stderr}");

    let current = format!("\"generation\": {}", zaf::app::profile::GENERATION);
    assert!(other_zaf.contains(&current), "{other_zaf}");
    fs::write(
        stamp(&project),
        other_zaf.replace(&current, "\"generation\": 0"),
    )
    .expect("old");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stderr}");
    assert!(
        stderr.contains("profile build is of another profile generation (written by zaf 0.0.1)"),
        "{stderr}"
    );

    fs::remove_file(stamp(&project)).expect("remove");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains(NO_STAMP), "{stderr}");
}

#[test]
fn spawn_refuses_a_profile_that_was_never_generated() {
    let project = generated();
    fs::remove_dir_all(project.root.join(".zaf/profiles/build")).expect("remove");
    let (code, _, stderr) = outcome(&project.zaf(&SPAWN));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains(NO_STAMP), "{stderr}");
}

#[test]
fn doctor_names_a_stale_profile() {
    let project = generated();
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("older than"), "{stdout}");

    edit_forbidden(&project);
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stdout.contains(STALE_FORBIDDEN), "{stdout}");

    fs::remove_file(stamp(&project)).expect("remove");
    let (code, stdout, _) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 1);
    assert!(stdout.contains("profile build has no stamp"), "{stdout}");

    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn status_names_a_stale_profile() {
    let project = generated();
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("older than"), "{stdout}");

    edit_forbidden(&project);
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains(STALE_FORBIDDEN), "{stdout}");

    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(!stdout.contains("older than"), "{stdout}");
}

#[test]
fn a_second_init_reports_nothing_written() {
    let project = generated();
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.trim(), "nothing changed");
}

// A git project's profile (#498): git's read verbs as exact rules, never `Bash(git *)`.

const GIT_VCS: &str = "[vcs]\nkind = \"git\"\n\n[requires]\ngit = \"*\"\n\n";
const JJ_VCS: &str = "[vcs]\nkind = \"jj\"\n\n[requires]\njj = \"*\"\n\n";

/// The permissions of the lean profile `zaf profile init` writes for `HEAD` plus `extra`.
fn init_permissions(extra: &str) -> serde_json::Value {
    let mut project = Project::new(&format!("{HEAD}{extra}"));
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    let (code, stdout, stderr) = outcome(&project.zaf(&["profile", "init"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let text = fs::read_to_string(project.root.join(".zaf/profiles/lean/settings.json"))
        .expect("settings");
    serde_json::from_str::<serde_json::Value>(&text).expect("json")["permissions"].clone()
}

fn rules(permissions: &serde_json::Value, key: &str) -> Vec<String> {
    permissions[key]
        .as_array()
        .expect("rules")
        .iter()
        .map(|rule| rule.as_str().expect("text").to_owned())
        .collect()
}

/// Claude Code's wildcard: `*` matches any text anywhere.
fn glob(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => text.strip_prefix(head).is_some_and(|tail| {
            (0..=tail.len())
                .filter(|at| tail.is_char_boundary(*at))
                .any(|at| glob(rest, &tail[at..]))
        }),
    }
}

/// Whether a rule matches `command`; a trailing ` *` also matches the bare command.
fn matches(rules: &[String], command: &str) -> bool {
    rules.iter().any(|rule| {
        let Some(pattern) = rule
            .strip_prefix("Bash(")
            .and_then(|rest| rest.strip_suffix(')'))
        else {
            return false;
        };
        pattern.strip_suffix(" *") == Some(command) || glob(pattern, command)
    })
}

fn permitted(permissions: &serde_json::Value, command: &str) -> bool {
    matches(&rules(permissions, "allow"), command) && !matches(&rules(permissions, "deny"), command)
}

#[test]
fn git_profile_allows_the_exact_verbs() {
    let permissions = init_permissions(GIT_VCS);
    // The Engineer's ruling on #521 (2026-11-19): git's read verbs only; commits are an action.
    for command in [
        "git status",
        "git status --short",
        "git diff HEAD",
        "git log -p main",
        "git show HEAD",
        "git blame src/main.rs",
        "git rev-parse HEAD",
    ] {
        assert!(permitted(&permissions, command), "{command}");
    }
    for command in [
        "git commit -m x",
        "git add .",
        "git restore x",
        "git checkout main",
        "git merge main",
        "git fetch",
        "git worktree add x",
        "git config core.pager x",
    ] {
        assert!(!permitted(&permissions, command), "{command}");
    }
}

#[test]
fn git_profile_denies_output_and_global_options() {
    let permissions = init_permissions(GIT_VCS);
    for command in [
        "git log --output=x",
        "git diff --output x",
        "git show --output=/tmp/x HEAD",
        "git -C /tmp status",
        "git -c core.pager=x log",
        "git --git-dir=/x log",
        "git --git-dir /x log",
        "git --work-tree=/x status",
        "git --exec-path=/x status",
        "git --config-env=a=B status",
        "GIT_CONFIG_COUNT=1 git log",
        "FOO=1 git log --output=x",
    ] {
        assert!(!permitted(&permissions, command), "{command}");
    }
}

#[test]
fn git_profile_denies_push() {
    let permissions = init_permissions(GIT_VCS);
    for command in [
        "git push",
        "git push origin main",
        "git -c push.default=current push",
    ] {
        assert!(!permitted(&permissions, command), "{command}");
    }
}

#[test]
fn git_profile_has_no_git_wildcard() {
    // Even a required `git` and a gate line that runs it add no `Bash(git *)`.
    let permissions = init_permissions(&format!(
        "{GIT_VCS}[gate]\nfull = \"git diff --check && cargo test\"\n"
    ));
    let allow = rules(&permissions, "allow");
    assert!(!allow.iter().any(|rule| rule == "Bash(git *)"), "{allow:?}");
    for rule in &allow {
        if let Some(verb) = rule.strip_prefix("Bash(git ") {
            let verb = verb.split([' ', ')']).next().expect("a verb");
            assert!(
                ["status", "diff", "log", "show", "blame", "rev-parse"].contains(&verb),
                "{rule}"
            );
        }
    }
}

#[test]
fn jj_profile_unchanged_by_git_rules() {
    let permissions = init_permissions(JJ_VCS);
    let allow = rules(&permissions, "allow");
    let git: Vec<&str> = allow
        .iter()
        .filter(|rule| rule.starts_with("Bash(git"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        git,
        [
            "Bash(git log *)",
            "Bash(git show *)",
            "Bash(git diff *)",
            "Bash(git status *)",
            "Bash(git blame *)",
        ]
    );
    assert_eq!(
        rules(&permissions, "deny"),
        [
            "Bash(git * --output*)",
            "Bash(git --config-env*)",
            "Bash(git --exec-path*)",
            "Bash(git --git-dir*)",
            "Bash(git --work-tree*)",
            "Bash(git -C *)",
            "Bash(git -c *)",
            "Bash(jj *--config*)",
            "Bash(jj *--repository*)",
            "Bash(jj *--tool*)",
            "Bash(jj *-R*)",
            "Bash(jj -* config edit *)",
            "Bash(jj -* config edit)",
            "Bash(jj -* config set *)",
            "Bash(jj -* config set)",
            "Bash(jj -* gerrit *)",
            "Bash(jj -* gerrit)",
            "Bash(jj -* git clone *)",
            "Bash(jj -* git clone)",
            "Bash(jj -* git init *)",
            "Bash(jj -* git init)",
            "Bash(jj -* git remote *)",
            "Bash(jj -* git remote)",
            "Bash(jj -* run *)",
            "Bash(jj -* run)",
            "Bash(jj -* util *)",
            "Bash(jj -* util)",
            "Bash(jj -* workspace add *)",
            "Bash(jj -* workspace add)",
            "Bash(jj config edit *)",
            "Bash(jj config edit)",
            "Bash(jj config set *)",
            "Bash(jj config set)",
            "Bash(jj gerrit *)",
            "Bash(jj gerrit)",
            "Bash(jj git clone *)",
            "Bash(jj git clone)",
            "Bash(jj git init *)",
            "Bash(jj git init)",
            "Bash(jj git remote *)",
            "Bash(jj git remote)",
            "Bash(jj run *)",
            "Bash(jj run)",
            "Bash(jj util *)",
            "Bash(jj util)",
            "Bash(jj workspace add *)",
            "Bash(jj workspace add)",
        ]
    );
}
