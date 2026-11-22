use std::fs;
use std::io::Cursor;
use std::process::Command;

use super::*;
use crate::manifest::VcsKind;

/// Marks a folder that is no repository as a jj one, so the tests that are not about the kind ask.
fn marked(dir: &Path) {
    if !dir.join(".git").exists() && !dir.join(".jj").exists() {
        fs::create_dir(dir.join(".jj")).expect("marker");
    }
}

fn run(dir: &Path, answers: &str) -> (Result<PathBuf, InitError>, String) {
    marked(dir);
    let mut output = Vec::new();
    let mut learn = || Ok(TAG.to_owned());
    let result = init(
        dir,
        &mut learn,
        &mut Cursor::new(answers.to_owned()),
        &mut output,
    );
    (result, String::from_utf8(output).expect("utf-8 prompts"))
}

const TAG: &str = "teams-v2.0.0";

fn run_standard(dir: &Path) -> PathBuf {
    run(dir, "bookshelf\nexample/shelf\n\ncargo test\n")
        .0
        .expect("written")
}

#[test]
fn init_writes_standard_manifest() {
    use crate::domain::Profile;
    use crate::domain::RoleName;
    use crate::manifest::{CheckTarget, Effort, RunnerName};

    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = Manifest::load(&run_standard(dir.path())).expect("loads");

    let runner = manifest
        .runners
        .get(&RunnerName::from("claude"))
        .expect("a claude runner");
    assert_eq!(
        runner.command.as_ref(),
        "claude -p {prompt} --model {model} --add-dir {root}"
    );
    assert_eq!(runner.timeout_minutes, 45);

    for (name, model, effort, profile, turns) in [
        ("programmer", "sonnet", Effort::High, Profile::Lean, 150),
        ("implement", "sonnet", Effort::High, Profile::Lean, 150),
        ("fix", "sonnet", Effort::High, Profile::Lean, 150),
        ("merge", "sonnet", Effort::High, Profile::Lean, 1),
        ("release", "opus", Effort::Xhigh, Profile::Build, 200),
    ] {
        let role = manifest
            .roles
            .get(&RoleName::from(name))
            .unwrap_or_else(|| panic!("{name}"));
        assert_eq!(role.model.as_ref(), model, "{name}");
        assert_eq!(role.effort, effort, "{name}");
        assert_eq!(role.profile, profile, "{name}");
        assert_eq!(role.turns, turns, "{name}");
    }

    let pr = manifest
        .check
        .targets
        .get(&CheckTarget::Pr)
        .expect("[check.pr]");
    assert_eq!(pr.steps, ["ci"]);
    let phases = ["implement", "ci", "review", "merge"];
    for task_type in ["Task", "Bug"] {
        let list = manifest.phases.get(task_type).expect(task_type);
        let names: Vec<&str> = list.iter().map(|name| name.as_str()).collect();
        assert_eq!(names, phases, "{task_type}");
    }
    let pack = manifest.pack.expect("[pack]");
    assert_eq!(pack.tag.as_deref(), Some(TAG));
    assert!(pack.path.is_none());
    assert!(manifest.gate.is_some(), "the gate answer is still kept");

    // The ruling on #543: the tools the standard actions need, and the merge denies.
    let required: Vec<&str> = manifest.requires.keys().map(AsRef::as_ref).collect();
    assert_eq!(required, ["gh", "jj"]);
    let denied: Vec<&str> = manifest
        .forbidden
        .commands
        .iter()
        .map(AsRef::as_ref)
        .collect();
    assert_eq!(
        denied,
        [
            "gh pr merge",
            "jj git push",
            "zaf forge merge",
            "zaf do pr-merge",
            "zaf do --tier machine pr-merge"
        ]
    );
}

#[test]
fn a_tag_that_cannot_be_learned_writes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    marked(dir.path());
    let mut learn = || {
        Err(ProfileError::Install {
            line: "claude plugin install".to_owned(),
            output: "offline".to_owned(),
        })
    };
    let result = init(
        dir.path(),
        &mut learn,
        &mut Cursor::new("a\nb/c\n\n\n".to_owned()),
        &mut Vec::new(),
    );
    assert!(matches!(result, Err(InitError::Pack(_))));
    assert!(!dir.path().join(MANIFEST_FILE).exists());
}

/// Writes an action named `name` into the project's tier with `extra` metadata lines.
fn write_action(root: &Path, name: &str, extra: &str) {
    use std::os::unix::fs::PermissionsExt;

    let dir = root.join(".zaf/actions");
    fs::create_dir_all(&dir).expect("actions dir");
    let file = dir.join(name);
    fs::write(
        &file,
        format!("#!/bin/sh\n# zaf description: {name}\n# zaf 1: failed\n{extra}true\n"),
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
}

#[test]
fn standard_manifest_passes_doctor_manifest_checks() {
    use crate::app::doctor::{doctor_with, fake::FakeCommands};
    use std::time::SystemTime;

    let dir = tempfile::tempdir().expect("temp dir");
    let manifest = Manifest::load(&run_standard(dir.path())).expect("loads");

    // Actions that lean on the standard roles and phase lists, and nothing else.
    let groups = "# zaf group: implement, fix, merge, release, programmer\n";
    for name in ["slice-open", "slice-pr", "slice-merge", "slice-drop"] {
        write_action(dir.path(), name, groups);
    }
    write_action(
        dir.path(),
        "implement",
        "# zaf kind: model\n# zaf ends: pr-open\n",
    );
    write_action(dir.path(), "fix", "# zaf kind: model\n# zaf ends: exit\n");
    write_action(dir.path(), "ci", "# zaf kind: wait\n# zaf ends: ci-done\n");
    write_action(
        dir.path(),
        "review",
        "# zaf kind: wait\n# zaf ends: review-posted\n",
    );
    write_action(dir.path(), "merge", "# zaf kind: zaf\n# zaf ends: merged\n");

    let commands = FakeCommands::new().with("env", "{\"loggedIn\": true}\n");
    let report = doctor_with(
        dir.path(),
        None,
        &manifest,
        &commands,
        None,
        SystemTime::now(),
    );
    let text = report.lines().join("\n");
    assert!(!text.contains("names no role"), "{text}");
    assert!(!text.contains("no runner"), "{text}");
    assert!(report.actions.iter().all(|check| check.ok), "{text}");
    assert_eq!(report.phases.len(), 8, "{text}");
    assert!(report.phases.iter().all(|check| check.ok), "{text}");
}

#[test]
fn scripted_answers_write_a_manifest_the_loader_accepts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, prompts) = run(dir.path(), "bookshelf\nexample/shelf\n\ncargo test --all\n");
    let path = result.expect("written");
    assert_eq!(path, dir.path().join(MANIFEST_FILE));
    assert!(
        prompts.contains("  Project name   what zaf calls this project\n  › ["),
        "{prompts}"
    );
    let manifest = Manifest::load(&path).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "bookshelf");
    assert_eq!(
        manifest.board.repo.as_ref().map(AsRef::as_ref),
        Some("example/shelf")
    );
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(
        forge.repo.as_ref().map(AsRef::as_ref),
        Some("example/shelf"),
        "a blank forge answer takes the board repo"
    );
    assert!(manifest.vcs.is_some());
    assert_eq!(
        manifest.gate.expect("a gate").full.as_ref(),
        "cargo test --all"
    );
}

#[test]
fn a_separate_forge_repo_and_a_blank_gate_are_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, _) = run(dir.path(), "bookshelf\nexample/board\nexample/code\n\n");
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(forge.repo.as_ref().map(AsRef::as_ref), Some("example/code"));
    assert!(manifest.gate.is_none());
}

#[test]
fn quotes_in_an_answer_survive() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, _) = run(
        dir.path(),
        "the \"shelf\"\nexample/b\n\necho \"hi\" \\ done\n",
    );
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "the \"shelf\"");
    assert_eq!(
        manifest.gate.expect("a gate").full.as_ref(),
        "echo \"hi\" \\ done"
    );
}

#[test]
fn an_existing_manifest_is_refused_untouched_and_unasked() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join(MANIFEST_FILE);
    fs::write(&path, "keep me").expect("existing manifest");
    let (result, prompts) = run(dir.path(), "a\nb\n\n\n");
    assert!(matches!(result, Err(InitError::Exists(_))));
    assert_eq!(fs::read_to_string(&path).expect("read"), "keep me");
    assert_eq!(prompts, "", "refused before the first question");
}

#[test]
fn a_blank_name_or_board_and_a_short_input_write_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert!(matches!(
        run(dir.path(), "\n\n").0,
        Err(InitError::Blank(_))
    ));
    assert!(matches!(
        run(dir.path(), "a\n\n").0,
        Err(InitError::Blank(_))
    ));
    assert!(matches!(
        run(dir.path(), "a\nb/c\n").0,
        Err(InitError::Closed(_))
    ));
    assert!(!dir.path().join(MANIFEST_FILE).exists());
}

/// A folder `zabo` holding a git repo, with `origin` at `url` when given.
fn repo_folder(url: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("temp dir");
    let dir = root.path().join("zabo");
    fs::create_dir(&dir).expect("folder");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&dir)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    if let Some(url) = url {
        git(&["remote", "add", "origin", url]);
    }
    (root, dir)
}

#[test]
fn init_defaults_from_folder_and_remote() {
    for url in ["git@github.com:o/zabo.git", "https://github.com/o/zabo.git"] {
        let (_root, dir) = repo_folder(Some(url));
        let (result, prompts) = run(&dir, "\n\n\n\n");
        let manifest = Manifest::load(&result.expect("written")).expect("loads");
        assert_eq!(manifest.project.name.as_ref(), "zabo", "{url}");
        assert_eq!(
            manifest.board.repo.as_ref().map(AsRef::as_ref),
            Some("o/zabo"),
            "{url}"
        );
        let forge = manifest.forge.expect("a forge section");
        assert_eq!(
            forge.repo.as_ref().map(AsRef::as_ref),
            Some("o/zabo"),
            "{url}"
        );
        assert!(manifest.gate.is_none(), "{url}");
        assert!(prompts.contains("  › [zabo] "), "{prompts}");
        assert!(prompts.contains("  › [o/zabo] "), "{prompts}");
    }
}

#[test]
fn init_typed_answer_wins() {
    let (_root, dir) = repo_folder(Some("git@github.com:o/zabo.git"));
    let (result, _) = run(&dir, "shelf\nx/board\nx/code\ncargo test\n");
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "shelf");
    assert_eq!(
        manifest.board.repo.as_ref().map(AsRef::as_ref),
        Some("x/board")
    );
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(forge.repo.as_ref().map(AsRef::as_ref), Some("x/code"));
    assert_eq!(manifest.gate.expect("a gate").full.as_ref(), "cargo test");
}

#[test]
fn init_no_remote_requires_repo() {
    let (_root, dir) = repo_folder(None);
    let (result, _) = run(&dir, "\n\n\n\n");
    assert!(
        matches!(result, Err(InitError::Blank("Board"))),
        "a blank board repo is refused by name"
    );
    assert!(!dir.join(MANIFEST_FILE).exists());

    let (_root, dir) = repo_folder(Some("git@gitlab.com:o/zabo.git"));
    let (result, _) = run(&dir, "\n\n\n\n");
    assert!(
        matches!(result, Err(InitError::Blank(_))),
        "not GitHub: no default"
    );
    assert!(!dir.join(MANIFEST_FILE).exists());
}

#[test]
fn github_urls_read_as_owner_name() {
    for url in [
        "git@github.com:o/n.git",
        "git@github.com:o/n",
        "ssh://git@github.com/o/n.git",
        "https://github.com/o/n.git",
        "https://github.com/o/n/",
        "https://user:tok@github.com/o/n.git\n",
    ] {
        assert_eq!(github_repo(url).as_deref(), Some("o/n"), "{url}");
    }
    for url in [
        "git@gitlab.com:o/n.git",
        "https://github.com/o",
        "/srv/repo.git",
        "",
    ] {
        assert_eq!(github_repo(url), None, "{url}");
    }
}

fn other(url: &str) -> Remote {
    match parse_origin(url) {
        Origin::Other(remote) => remote,
        found => panic!("{url}: {found:?}"),
    }
}

#[test]
fn github_remotes_stay_github() {
    for url in [
        "git@github.com:o/n.git",
        "ssh://git@github.com/o/n",
        "https://github.com/o/n.git",
    ] {
        assert_eq!(parse_origin(url), Origin::Github("o/n".to_owned()), "{url}");
    }
    assert_eq!(parse_origin("https://github.com/o"), Origin::None);
}

#[test]
fn other_remotes_read_as_host_and_owner_name() {
    for (url, host, repo) in [
        (
            "ssh://git@yagdrassyl.local:2222/zuri/ways.git",
            "yagdrassyl.local",
            "zuri/ways",
        ),
        (
            "git@host.example:owner/name.git",
            "host.example",
            "owner/name",
        ),
        ("git@host.example:owner/name", "host.example", "owner/name"),
        (
            "https://host.example/owner/name.git",
            "host.example",
            "owner/name",
        ),
        (
            "https://host.example/owner/name",
            "host.example",
            "owner/name",
        ),
        (
            "http://u:tok@host.example:3000/owner/name.git\n",
            "host.example",
            "owner/name",
        ),
    ] {
        let remote = other(url);
        assert_eq!(remote.host(), host, "{url}");
        assert_eq!(remote.repo().as_str(), repo, "{url}");
    }
    for url in [
        "/srv/repo.git",
        "",
        "ssh://host/onlyone",
        "git@host.example:a/b/c.git",
        "file:///srv/a/b",
    ] {
        assert_eq!(parse_origin(url), Origin::None, "{url}");
    }
}

#[test]
fn candidates_start_with_the_remotes_own_address() {
    let urls = |url: &str| -> Vec<String> {
        other(url)
            .candidates()
            .iter()
            .map(|candidate| candidate.as_str().to_owned())
            .collect()
    };
    assert_eq!(
        urls("ssh://git@h.local:2222/o/n.git"),
        ["https://h.local", "http://h.local:3000", "http://h.local"]
    );
    assert_eq!(
        urls("http://h.local:8080/o/n.git"),
        [
            "http://h.local:8080",
            "https://h.local",
            "http://h.local:3000",
            "http://h.local"
        ]
    );
    assert_eq!(
        urls("https://h.local/o/n"),
        ["https://h.local", "http://h.local:3000", "http://h.local"]
    );
}

const FORGEJO_ORIGIN: &str = "ssh://git@yagdrassyl.local:2222/zuri/ways.git";

fn run_with_probe(
    dir: &Path,
    answers: &str,
    found: &str,
) -> (Result<PathBuf, InitError>, String, Vec<String>) {
    marked(dir);
    let mut output = Vec::new();
    let mut learn = || Ok(TAG.to_owned());
    let mut asked = Vec::new();
    let mut probe = |url: &ForgeUrl| {
        asked.push(url.as_str().to_owned());
        url.as_str() == found
    };
    let result = init_with_home(
        dir,
        None,
        &mut learn,
        &mut probe,
        &Question::plain,
        &mut Cursor::new(answers.to_owned()),
        &mut output,
    );
    (result, String::from_utf8(output).expect("utf-8"), asked)
}

#[test]
fn a_forgejo_origin_asks_the_url_and_writes_forgejo_kinds() {
    use crate::manifest::{BoardKind, ForgeKind};
    let (_root, dir) = repo_folder(Some(FORGEJO_ORIGIN));
    let (result, prompts, asked) =
        run_with_probe(&dir, "\n\n\n\n\n", "http://yagdrassyl.local:3000");
    let path = result.expect("written");
    assert_eq!(
        asked,
        ["https://yagdrassyl.local", "http://yagdrassyl.local:3000"],
        "probing stops at the first answer"
    );
    assert!(
        prompts.contains("  › [http://yagdrassyl.local:3000] "),
        "{prompts}"
    );
    let manifest = Manifest::load(&path).expect("loads");
    assert_eq!(manifest.board.kind, BoardKind::ForgejoIssues);
    assert_eq!(
        manifest.board.repo.as_ref().map(AsRef::as_ref),
        Some("zuri/ways")
    );
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(forge.kind, ForgeKind::Forgejo);
    assert_eq!(
        forge.url.as_ref().map(AsRef::as_ref),
        Some("http://yagdrassyl.local:3000")
    );
    assert_eq!(forge.repo.as_ref().map(AsRef::as_ref), Some("zuri/ways"));
    assert_eq!(forge.token_env().as_str(), "FORGEJO_TOKEN");
    assert_eq!(manifest.board.token_env().as_str(), "FORGEJO_TOKEN");
    let required: Vec<&str> = manifest.requires.keys().map(AsRef::as_ref).collect();
    assert!(required.is_empty(), "a plain git repository needs no jj");
    let denied: Vec<&str> = manifest
        .forbidden
        .commands
        .iter()
        .map(AsRef::as_ref)
        .collect();
    assert_eq!(
        denied,
        [
            "jj git push",
            "zaf forge merge",
            "zaf do pr-merge",
            "zaf do --tier machine pr-merge"
        ]
    );
    let text = fs::read_to_string(&path).expect("text");
    assert!(!text.contains("gh = ") && !text.contains("gh pr"), "{text}");
}

/// A shell no run may reach: the profiles below are generated with no pack.
struct NoShell;

impl crate::ports::Shell for NoShell {
    fn run(
        &self,
        line: &crate::manifest::CommandLine,
    ) -> Result<crate::ports::GateRun, crate::ports::PortError> {
        panic!("no pack, no shell: {}", line.as_ref());
    }
}

/// The Bash deny patterns of the lean and build profiles generated from the manifest at `path`.
fn profile_denies(path: &Path) -> Vec<(&'static str, Vec<String>)> {
    let mut manifest = Manifest::load(path).expect("loads");
    manifest.pack = None;
    let root = tempfile::tempdir().expect("temp dir");
    let state = root.path().join(".zaf");
    crate::app::profile::init_with(root.path(), &state, &manifest, &NoShell, None)
        .expect("generates");
    ["lean", "build"]
        .into_iter()
        .map(|profile| {
            let file = state.join("profiles").join(profile).join("settings.json");
            let text = fs::read_to_string(file).expect("settings");
            let settings: serde_json::Value = serde_json::from_str(&text).expect("parses");
            let patterns = settings["permissions"]["deny"]
                .as_array()
                .expect("a deny list")
                .iter()
                .filter_map(|rule| rule.as_str())
                .filter_map(|rule| rule.strip_prefix("Bash(")?.strip_suffix(')'))
                .map(str::to_owned)
                .collect();
            (profile, patterns)
        })
        .collect()
}

/// Asserts each profile denies `jj git push` and its flag forms, and still the merge denies.
fn assert_blocks_jj_git_push(path: &Path, merge_denies: &[&str]) {
    let denies = profile_denies(path);
    assert_eq!(denies.len(), 2);
    for (profile, patterns) in denies {
        for wanted in ["jj git push", "jj git push *", "jj -* git push *"]
            .into_iter()
            .chain(merge_denies.iter().copied())
        {
            assert!(
                patterns.iter().any(|pattern| pattern == wanted),
                "{profile} lacks {wanted}: {patterns:?}"
            );
        }
    }
}

#[test]
fn a_forgejo_profile_blocks_jj_git_push() {
    let (_root, dir) = repo_folder(Some(FORGEJO_ORIGIN));
    let (result, _, _) = run_with_probe(&dir, "\n\n\n\n\n", "http://yagdrassyl.local:3000");
    let path = result.expect("written");
    assert_blocks_jj_git_push(&path, &["zaf forge merge", "zaf do pr-merge"]);
}

#[test]
fn a_github_profile_blocks_jj_git_push() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = run_standard(dir.path());
    assert_blocks_jj_git_push(
        &path,
        &["gh pr merge", "zaf forge merge", "zaf do pr-merge"],
    );
}

#[test]
fn a_forgejo_origin_with_no_answering_instance_needs_the_url_typed() {
    let (_root, dir) = repo_folder(Some(FORGEJO_ORIGIN));
    let (result, prompts, _) = run_with_probe(&dir, "\n\n\n\n\n", "");
    assert!(
        matches!(result, Err(InitError::Blank("Forge address"))),
        "{prompts}"
    );
    assert!(!dir.join(MANIFEST_FILE).exists());

    let (result, _, _) = run_with_probe(&dir, "\n\n\nhttp://forge.test:3000/\n\n", "");
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    let url = manifest.forge.expect("forge").url;
    assert_eq!(
        url.as_ref().map(AsRef::as_ref),
        Some("http://forge.test:3000")
    );
}

#[test]
fn no_origin_asks_no_url() {
    let (_root, dir) = repo_folder(None);
    let (result, prompts, asked) = run_with_probe(&dir, "\nx/y\n\n\n", "");
    result.expect("written");
    assert!(asked.is_empty());
    assert!(!prompts.contains("Forge address"), "{prompts}");
}

fn labels(questions: &[Question]) -> Vec<&str> {
    questions.iter().map(|question| question.label).collect()
}

fn meaning<'a>(questions: &'a [Question], label: &str) -> &'a str {
    &questions
        .iter()
        .find(|question| question.label == label)
        .unwrap_or_else(|| panic!("{label}"))
        .meaning
}

#[test]
fn a_github_origin_asks_four_explained_questions() {
    for url in [Some("git@github.com:o/zabo.git"), None] {
        let (_root, dir) = repo_folder(url);
        let list = questions(&dir);
        assert_eq!(
            labels(&list),
            ["Project name", "Board", "Forge", "Check command"]
        );
        assert_eq!(
            meaning(&list, "Project name"),
            "what zaf calls this project"
        );
        assert_eq!(
            meaning(&list, "Forge"),
            "where pull requests open: owner/name"
        );
        assert_eq!(
            meaning(&list, "Check command"),
            "the command that proves the code is good, e.g. cargo test (empty: none)"
        );
        assert_eq!(list[0].default.as_deref(), Some("zabo"));
        assert_eq!(list[3].default, None);
        assert!(!list[3].required);
        let expected = url.map(|_| "o/zabo".to_owned());
        assert_eq!(list[1].default, expected);
        assert_eq!(list[2].default, expected);
    }
}

#[test]
fn a_forgejo_origin_asks_five() {
    let (_root, dir) = repo_folder(Some(FORGEJO_ORIGIN));
    let list = questions(&dir);
    assert_eq!(
        labels(&list),
        [
            "Project name",
            "Board",
            "Forge",
            "Forge address",
            "Check command"
        ]
    );
    assert_eq!(list[1].default.as_deref(), Some("zuri/ways"));
    assert_eq!(list[3].default, None, "the address is found when asked");
    assert!(list[3].required);
}

#[test]
fn the_board_question_names_its_kind() {
    let (_root, dir) = repo_folder(Some("git@github.com:o/zabo.git"));
    assert_eq!(
        meaning(&questions(&dir), "Board"),
        "where your tickets live: GitHub issues of owner/name"
    );
    let (_root, dir) = repo_folder(Some(FORGEJO_ORIGIN));
    assert_eq!(
        meaning(&questions(&dir), "Board"),
        "where your tickets live: Forgejo issues of owner/name"
    );
}

#[test]
fn a_question_is_drawn_plain_as_the_mock() {
    let (_root, dir) = repo_folder(None);
    let list = questions(&dir);
    assert_eq!(
        list[0].plain(),
        "  Project name   what zaf calls this project\n  › [zabo] "
    );
    assert_eq!(
        list[1].plain(),
        "  Board          where your tickets live: GitHub issues of owner/name\n  › "
    );
}

#[test]
fn explained_questions_write_the_same_manifest() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = run_standard(dir.path());
    let text = fs::read_to_string(&path).expect("text");
    assert!(text.starts_with(
        "[project]\nname = \"bookshelf\"\n\n[board]\nkind = \"github-issues\"\n\
         repo = \"example/shelf\"\n\n[vcs]\nkind = \"jj\"\n\n[forge]\nkind = \"github\"\n\
         repo = \"example/shelf\"\n\n[gate]\nfull = \"cargo test\"\n\n[runners.claude]\n"
    ));
    assert!(text.ends_with("\n[pack]\ntag = \"teams-v2.0.0\"\n"));
}

fn required(path: &Path) -> Vec<String> {
    let manifest = Manifest::load(path).expect("loads");
    manifest
        .requires
        .keys()
        .map(|name| name.as_ref().to_owned())
        .collect()
}

#[test]
fn plain_git_writes_kind_git() {
    let (_root, dir) = repo_folder(Some("git@github.com:o/zabo.git"));
    let path = run(&dir, "\n\n\n\n").0.expect("written");
    let manifest = Manifest::load(&path).expect("loads");
    assert_eq!(manifest.vcs.expect("[vcs]").kind, VcsKind::Git);
    assert!(
        fs::read_to_string(&path)
            .expect("text")
            .contains("[vcs]\nkind = \"git\"\n")
    );
}

#[test]
fn plain_git_requires_no_jj() {
    let (_root, dir) = repo_folder(Some("git@github.com:o/zabo.git"));
    let path = run(&dir, "\n\n\n\n").0.expect("written");
    assert_eq!(required(&path), ["gh"]);

    let (_root, dir) = repo_folder(Some(FORGEJO_ORIGIN));
    let (result, _, _) = run_with_probe(&dir, "\n\n\nhttps://forge.example\n\n", "none");
    assert!(required(&result.expect("written")).is_empty());
}

#[test]
fn colocated_is_jj() {
    let (_root, dir) = repo_folder(Some("git@github.com:o/zabo.git"));
    fs::create_dir(dir.join(".jj")).expect(".jj");
    let path = run(&dir, "\n\n\n\n").0.expect("written");
    let manifest = Manifest::load(&path).expect("loads");
    assert_eq!(manifest.vcs.expect("[vcs]").kind, VcsKind::Jj);
    assert_eq!(required(&path), ["gh", "jj"]);
}

#[test]
fn no_repository_is_refused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut output = Vec::new();
    let mut learn = || Ok(TAG.to_owned());
    let result = init(
        dir.path(),
        &mut learn,
        &mut Cursor::new("a\nb/c\n\n\n".to_owned()),
        &mut output,
    );
    let error = result.expect_err("refused");
    assert!(matches!(error, InitError::NoRepository(_)));
    let text = error.to_string();
    assert!(
        text.contains("git init") && text.contains("jj git init"),
        "{text}"
    );
    assert!(output.is_empty(), "refused before the first question");
    assert!(!dir.path().join(MANIFEST_FILE).exists());
}

#[test]
fn rerun_keeps_vcs() {
    let (_root, dir) = repo_folder(Some("git@github.com:o/zabo.git"));
    let path = dir.join(MANIFEST_FILE);
    let text = "[vcs]\nkind = \"jj\"\n";
    fs::write(&path, text).expect("manifest");
    let (result, prompts) = run(&dir, "\n\n\n\n");
    assert!(matches!(result, Err(InitError::Exists(_))));
    assert_eq!(fs::read_to_string(&path).expect("read"), text);
    assert_eq!(prompts, "");
}
