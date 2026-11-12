//! `zaf setup` from nothing, end to end: in an empty folder with an `origin` remote and an empty
//! zaf home it asks for the manifest, writes it, installs the pack's actions and generates the
//! profiles, and `zaf doctor` passes with no other command between; a second `zaf setup` asks
//! and changes nothing. ways' own tree passes `zaf doctor` with its own actions too.
//!
//! `claude`, `gh` and `jj` are stand-ins in a temp folder first on PATH. The stand-in `claude`
//! records the pack install the way Claude does, at the real `plugins/teams` folder, so the
//! actions setup installs are the real standard actions.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The stand-in `claude`: logged in; a marketplace added is listed; an install records the pack
/// (`FAKE_PACK_VERSION` at `FAKE_PACK_PATH`) in the profile's `plugins/installed_plugins.json`.
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
    *) ;;
esac
"#;

/// A stand-in for `gh` and `jj`: installed, recent and logged in, doing nothing else.
const FAKE_TOOL: &str =
    "#!/bin/sh\nset -eu\ncase \"$*\" in\n    \"--version\") echo \"9.9.9\" ;;\n    *) ;;\nesac\n";

/// An empty folder with an `origin` remote, an empty zaf home and the stand-in tools.
struct Scene {
    temp: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

/// The ways repository's root, one folder above the crate.
fn ways_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_owned()
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).expect("write tool");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

impl Scene {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let (root, home) = (temp.path().join("tracker"), temp.path().join("zaf-home"));
        let bin = temp.path().join("bin");
        for dir in [&root, &home, &bin, &temp.path().join("user-home")] {
            fs::create_dir_all(dir).expect("dir");
        }
        executable(&bin.join("claude"), FAKE_CLAUDE);
        executable(&bin.join("gh"), FAKE_TOOL);
        executable(&bin.join("jj"), FAKE_TOOL);
        for args in [
            &["init", "--quiet"][..],
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:example/tracker.git",
            ],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?}");
        }
        Self { temp, root, home }
    }

    /// Runs `zaf <args>` in the folder with `stdin` on its input (none: closed input).
    fn zaf(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let search = std::env::join_paths(
            std::iter::once(self.temp.path().join("bin")).chain(std::env::split_paths(&inherited)),
        )
        .expect("search path");
        let mut command = Command::new(env!("CARGO_BIN_EXE_zaf"));
        command
            .args(args)
            .current_dir(&self.root)
            .env("PATH", search)
            .env("ZAF_HOME", &self.home)
            .env("HOME", self.temp.path().join("user-home"))
            .env("FAKE_PACK_VERSION", "2.1.1")
            .env("FAKE_PACK_PATH", ways_root().join("plugins/teams"))
            .env_remove("ZAF_INSTANCE")
            .env_remove("ZAF_VERSION")
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
}

/// A path's bytes (none for a folder), mode bits and link target.
type Tree = BTreeMap<PathBuf, (Option<Vec<u8>>, u32, Option<PathBuf>)>;

/// The exit code, stdout and stderr of a run.
fn outcome(output: &Output) -> (i32, String, String) {
    (
        output.status.code().expect("exit code"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Every path under `dir` with its bytes (`None` for a folder), its mode bits and, for a link,
/// its target, relative to `dir`. The `.git` folder is git's own and left out.
fn tree(dir: &Path) -> Tree {
    fn walk(base: &Path, dir: &Path, found: &mut Tree) {
        for entry in fs::read_dir(dir).expect("read dir") {
            let path = entry.expect("entry").path();
            let relative = path.strip_prefix(base).expect("under base").to_owned();
            if relative == Path::new(".git") {
                continue;
            }
            let meta = fs::symlink_metadata(&path).expect("metadata");
            let mode = meta.permissions().mode();
            if meta.is_symlink() {
                found.insert(relative, (None, mode, fs::read_link(&path).ok()));
            } else if meta.is_dir() {
                found.insert(relative, (None, mode, None));
                walk(base, &path, found);
            } else {
                found.insert(
                    relative,
                    (Some(fs::read(&path).expect("bytes")), mode, None),
                );
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(dir, dir, &mut found);
    found
}

/// In an empty folder one `zaf setup` with blank answers (every default taken) writes the
/// manifest, installs the pack's standard actions into the zaf home and generates the profiles,
/// and `zaf doctor` right after exits 0.
#[test]
fn setup_empty_folder_doctor_passes() {
    let scene = Scene::new();
    assert!(!scene.root.join(".zaf.toml").exists());

    let (code, stdout, stderr) = outcome(&scene.zaf(&["setup"], Some("\n\n\n\n")));
    assert_eq!(code, 0, "setup:\n{stdout}{stderr}");
    assert!(scene.root.join(".zaf.toml").is_file(), "{stdout}");
    let installed: Vec<String> = fs::read_dir(scene.home.join("actions"))
        .expect("the home's actions")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(installed.contains(&"commit".to_owned()), "{installed:?}");

    let (code, stdout, stderr) = outcome(&scene.zaf(&["doctor"], None));
    assert_eq!(code, 0, "doctor after setup:\n{stdout}{stderr}");
}

/// After one setup, a second `zaf setup` reads nothing from stdin (it is closed) and leaves every
/// file under the project folder and the zaf home byte for byte the same, a machine action the
/// Engineer edited included.
#[test]
fn setup_rerun_changes_nothing() {
    let scene = Scene::new();
    let (code, stdout, stderr) = outcome(&scene.zaf(&["setup"], Some("\n\n\n\n")));
    assert_eq!(code, 0, "first setup:\n{stdout}{stderr}");
    let action = scene.home.join("actions/commit");
    let mut edited = fs::read(&action).expect("an installed action");
    edited.extend_from_slice(b"\n# edited by the Engineer\n");
    fs::write(&action, &edited).expect("edit");
    let before = (tree(&scene.root), tree(&scene.home));

    let (code, stdout, stderr) = outcome(&scene.zaf(&["setup"], None));
    assert_eq!(code, 0, "second setup:\n{stdout}{stderr}");
    assert_eq!(
        (tree(&scene.root), tree(&scene.home)),
        before,
        "the second setup changed a file"
    );
    assert_eq!(fs::read(&action).expect("action"), edited);
}

/// `zaf doctor` exits 0 on a copy of ways' own `.zaf.toml` and `.zaf/actions/` with an empty zaf
/// home, so it is ways' own actions that pass, none taken from the machine. The profiles ways
/// keeps per machine are generated first, with `zaf profile init`.
#[test]
fn ways_doctor_passes_with_own_actions() {
    let scene = Scene::new();
    let ways = ways_root();
    fs::copy(ways.join(".zaf.toml"), scene.root.join(".zaf.toml")).expect("manifest");
    copy_dir(&ways.join(".zaf/actions"), &scene.root.join(".zaf/actions"));
    // The design pages, the marketplace file and the pack, which doctor reads as ways has them.
    for folder in ["docs", ".claude-plugin", "plugins/teams"] {
        copy_dir(&ways.join(folder), &scene.root.join(folder));
    }
    let (code, stdout, stderr) = outcome(&scene.zaf(&["profile", "init"], None));
    assert_eq!(code, 0, "profile init:\n{stdout}{stderr}");

    let (code, stdout, stderr) = outcome(&scene.zaf(&["doctor"], None));
    assert_eq!(code, 0, "doctor on ways:\n{stdout}{stderr}");
}

/// Copies `from` into `to`, files and folders alike, keeping each file's mode bits.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create dir");
    for entry in fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy");
        }
    }
}
