//! The pack's two Designer actions, run as files with stand-ins for `zaf` and `claude` first on
//! PATH (docs/zaf-actions.md §7): `design` opens the Engineer's session in their own profile, and
//! `review-epic` checks the epic is open, then runs the Designer once, headless, in the build
//! profile. Outside the ways repository, where the pack is not beside the crate, nothing runs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;
use common::outcome;

/// A stand-in `claude`: records its arguments one per line, its profile and its directory, then
/// exits `FAKE_CLAUDE_EXIT`.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
printf '%s\n' "$@" > "$FAKE_DIR/claude.args"
printf '%s' "${CLAUDE_CONFIG_DIR-unset}" > "$FAKE_DIR/claude.profile"
pwd > "$FAKE_DIR/claude.cwd"
exit "${FAKE_CLAUDE_EXIT:-0}"
"#;

/// A stand-in `zaf`: records each call's arguments, prints one line and exits `FAKE_ZAF_EXIT`.
const FAKE_ZAF: &str = r##"#!/bin/sh
printf '%s\n' "$*" >> "$FAKE_DIR/zaf.calls"
echo "#12 signed 0 of 0  an epic"
exit "${FAKE_ZAF_EXIT:-0}"
"##;

/// A project root and a folder of stand-ins, in one temp folder.
struct Scene {
    _temp: tempfile::TempDir,
    root: PathBuf,
    fakes: PathBuf,
}

impl Scene {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("project");
        let fakes = temp.path().join("fakes");
        fs::create_dir_all(&root).expect("root");
        for (name, text) in [("claude", FAKE_CLAUDE), ("zaf", FAKE_ZAF)] {
            let file = fakes.join(name);
            fs::create_dir_all(&fakes).expect("fakes");
            fs::write(&file, text).expect("fake");
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
        }
        Self {
            _temp: temp,
            root: real(&root),
            fakes,
        }
    }

    /// Runs the pack's action `name` with `args`, `ZAF_ROOT` set (unless `root` is false) and the
    /// stand-ins first on PATH, from the temp folder, never the root. `env` is set on top.
    fn run(&self, name: &str, args: &[&str], root: bool, env: &[(&str, &str)]) -> (i32, String) {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(
            std::iter::once(self.fakes.clone()).chain(std::env::split_paths(&inherited)),
        )
        .expect("search path");
        let mut command = Command::new(pack().join(name));
        command
            .args(args)
            .current_dir(&self.fakes)
            .env("PATH", path)
            .env("FAKE_DIR", &self.fakes)
            .env_remove("ZAF_ROOT")
            .env_remove("CLAUDE_CONFIG_DIR")
            .envs(env.iter().copied());
        if root {
            command.env("ZAF_ROOT", &self.root);
        }
        let (code, stdout, stderr) = outcome(&command.output().expect("run the action"));
        (code, format!("{stdout}{stderr}"))
    }

    /// The stand-in's record `file`, none when it never ran.
    fn record(&self, file: &str) -> Option<String> {
        fs::read_to_string(self.fakes.join(file)).ok()
    }

    /// The arguments `claude` was given, one per entry.
    fn claude_args(&self) -> Vec<String> {
        self.record("claude.args")
            .expect("claude ran")
            .lines()
            .map(str::to_owned)
            .collect()
    }
}

/// The pack's actions folder.
fn pack() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/teams/actions")
}

fn real(path: &Path) -> PathBuf {
    path.canonicalize().expect("canonical")
}

#[test]
fn design_opens_the_designer_in_the_engineers_own_profile() {
    if !pack().is_dir() {
        return;
    }
    let scene = Scene::new();
    let (code, said) = scene.run(
        "design",
        &[],
        true,
        &[("CLAUDE_CONFIG_DIR", "/own/profile")],
    );
    assert_eq!(code, 0, "{said}");
    let args = scene.claude_args();
    assert_eq!(args[..2], ["--agent", "teams:designer"], "{args:?}");
    assert_eq!(args.len(), 3, "{args:?}");
    for words in [
        "design session",
        &scene.root.display().to_string(),
        "CLAUDE.md",
        "ask me what I want to design",
    ] {
        assert!(args[2].contains(words), "{words}: {}", args[2]);
    }
    assert_eq!(
        scene.record("claude.profile").as_deref(),
        Some("/own/profile"),
        "the Engineer's profile is left as it is"
    );
    assert_eq!(
        scene.record("claude.cwd").map(|cwd| cwd.trim().to_owned()),
        Some(scene.root.display().to_string()),
        "the session starts at the project root"
    );
}

#[test]
fn design_takes_its_arguments_as_the_opening_prompt() {
    if !pack().is_dir() {
        return;
    }
    let scene = Scene::new();
    let (code, said) = scene.run("design", &["/teams:objectives", "v0.2"], true, &[]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(
        scene.claude_args(),
        ["--agent", "teams:designer", "/teams:objectives v0.2"]
    );
    assert_eq!(
        scene.record("claude.profile").as_deref(),
        Some("unset"),
        "no profile is set for it"
    );
}

#[test]
fn design_exits_with_claudes_code_and_refuses_without_a_root() {
    if !pack().is_dir() {
        return;
    }
    let scene = Scene::new();
    let (code, _) = scene.run("design", &[], true, &[("FAKE_CLAUDE_EXIT", "7")]);
    assert_eq!(code, 7, "claude's own code passes through");

    let scene = Scene::new();
    let (code, said) = scene.run("design", &[], false, &[]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("usage: design"), "{said}");
    assert_eq!(scene.record("claude.args"), None, "claude never ran");
}

#[test]
fn review_epic_runs_the_designer_once_in_the_build_profile() {
    if !pack().is_dir() {
        return;
    }
    let scene = Scene::new();
    let (code, said) = scene.run("review-epic", &["#12"], true, &[]);
    assert_eq!(code, 0, "{said}");
    assert_eq!(scene.record("zaf.calls").as_deref(), Some("epic show 12\n"));
    let root = scene.root.display().to_string();
    assert_eq!(
        scene.record("claude.profile"),
        Some(format!("{root}/.zaf/profiles/build"))
    );
    let args = scene.claude_args();
    assert_eq!(args[0], "-p", "{args:?}");
    assert_eq!(
        args[2..],
        [
            "--agent",
            "teams:designer",
            "--model",
            "opus",
            "--effort",
            "xhigh",
            "--add-dir",
            &root
        ],
        "{args:?}"
    );
    for words in [
        "challenger of a document the Engineer signs",
        "epic #12",
        "`zaf epic show 12 --more`",
        "`zaf board question 12 \"",
        "`--door`",
        "Never edit the epic, never sign it",
        "one-screen summary",
    ] {
        assert!(args[1].contains(words), "{words}: {}", args[1]);
    }
}

#[test]
fn review_epic_refuses_what_is_not_one_epic() {
    if !pack().is_dir() {
        return;
    }
    for args in [&[][..], &["12", "13"], &["twelve"], &["#"], &["12x"]] {
        let scene = Scene::new();
        let (code, said) = scene.run("review-epic", args, true, &[]);
        assert_eq!(code, 1, "{args:?}: {said}");
        assert_eq!(scene.record("zaf.calls"), None, "{args:?}: nothing asked");
        assert_eq!(scene.record("claude.args"), None, "{args:?}: no Designer");
    }
    let scene = Scene::new();
    let (code, said) = scene.run("review-epic", &["12"], false, &[]);
    assert_eq!(code, 1, "{said}");
    assert!(said.contains("usage: review-epic <epic>"), "{said}");
}

#[test]
fn review_epic_passes_on_the_epics_refusal_and_a_backend_failure() {
    if !pack().is_dir() {
        return;
    }
    for (zaf_exit, claude_exit, expected, designer) in [
        ("1", "0", 1, false),
        ("2", "0", 2, false),
        ("125", "0", 2, false),
        ("0", "3", 2, true),
        ("0", "1", 2, true),
    ] {
        let scene = Scene::new();
        let env = [
            ("FAKE_ZAF_EXIT", zaf_exit),
            ("FAKE_CLAUDE_EXIT", claude_exit),
        ];
        let (code, said) = scene.run("review-epic", &["12"], true, &env);
        assert_eq!(
            code, expected,
            "zaf {zaf_exit}, claude {claude_exit}: {said}"
        );
        assert_eq!(
            scene.record("claude.args").is_some(),
            designer,
            "zaf {zaf_exit}: the Designer runs only on an open epic"
        );
    }
}
