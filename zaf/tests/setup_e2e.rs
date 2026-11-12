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

mod common;
use common::Curl;

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
    /// Whether the search path is only the stand-in folder (a Forgejo scene), not that and the
    /// machine's own.
    sealed: bool,
    /// Environment set on every run.
    env: Vec<(String, String)>,
}

/// The ways repository's root, one folder above the crate.
fn ways_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_owned()
}

/// The version in the pack's own `plugin.json`, which the fake marketplace serves, so a pack
/// release needs no edit here.
fn pack_version() -> String {
    let text = fs::read_to_string(ways_root().join("plugins/teams/.claude-plugin/plugin.json"))
        .expect("the pack's plugin.json");
    let json: serde_json::Value = serde_json::from_str(&text).expect("plugin.json is JSON");
    json["version"]
        .as_str()
        .expect("plugin.json has a version")
        .to_owned()
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).expect("write tool");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
}

impl Scene {
    fn new() -> Self {
        Self::with_origin("git@github.com:example/tracker.git")
    }

    /// An empty folder whose `origin` is `ssh://git@yagdrassyl.local:2222/zuri/ways.git` and
    /// whose PATH holds no `gh`: the machine's tools but that one, the stand-in `claude` and
    /// `jj`, and the fake `curl` answering as a Forgejo instance at `http://yagdrassyl.local:3000`.
    fn forgejo() -> Self {
        let mut scene = Self::with_origin("ssh://git@yagdrassyl.local:2222/zuri/ways.git");
        let bin = scene.temp.path().join("bin");
        fs::remove_file(bin.join("gh")).expect("no gh");
        executable(
            &bin.join("curl"),
            &fs::read_to_string(common::fixtures().join("forgejo/curl")).expect("fake curl"),
        );
        // Every tool the machine's PATH offers but `gh`, so nothing but the stand-ins differ.
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        for dir in std::env::split_paths(&inherited) {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                if name != "gh" {
                    let _ = std::os::unix::fs::symlink(entry.path(), bin.join(&name));
                }
            }
        }
        let mut curl = Curl::new(scene.temp.path());
        curl.on(
            "GET http://yagdrassyl.local:3000/api/v1/version",
            200,
            r#"{"version":"15.0.11+gitea-1.22.0"}"#,
        )
        .on(
            "GET http://yagdrassyl.local:3000/api/v1/repos/zuri/ways",
            200,
            r#"{"permissions":{"pull":true,"push":true}}"#,
        )
        .on(
            "POST http://yagdrassyl.local:3000/api/v1/repos/zuri/ways/*",
            422,
            r#"{"message":"invalid"}"#,
        )
        .on(
            "GET http://yagdrassyl.local:3000/api/v1/repos/zuri/ways/*",
            200,
            "[]",
        );
        scene.sealed = true;
        scene
            .env
            .push(("FAKE_CURL_DIR".to_owned(), curl.dir.display().to_string()));
        scene
    }

    fn with_origin(origin: &str) -> Self {
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
            &["remote", "add", "origin", origin],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .expect("git");
            assert!(status.success(), "git {args:?}");
        }
        Self {
            temp,
            root,
            home,
            sealed: false,
            env: Vec::new(),
        }
    }

    /// Runs `zaf <args>` in the folder with `stdin` on its input (none: closed input).
    fn zaf(&self, args: &[&str], stdin: Option<&str>) -> Output {
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let inherited = if self.sealed {
            Default::default()
        } else {
            inherited
        };
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
            .env("FAKE_PACK_VERSION", pack_version())
            .env("FAKE_PACK_PATH", ways_root().join("plugins/teams"))
            .env_remove("ZAF_INSTANCE")
            .env_remove("ZAF_VERSION")
            .envs(self.env.iter().map(|(key, value)| (key, value)))
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

/// In an empty folder whose `origin` is not a GitHub remote, `zaf setup` with every default taken
/// finds the instance by probing the remote's host, and writes a Forgejo manifest with the URL
/// it found; while the token variable is unset setup exits 1 and says what to export.
#[test]
fn setup_writes_a_forgejo_manifest_from_origin() {
    let mut scene = Scene::forgejo();
    scene.env.push(("FORGEJO_TOKEN".to_owned(), String::new()));
    let (code, stdout, stderr) = outcome(&scene.zaf(&["setup"], Some("\n\n\n\n\n")));
    assert!(
        stdout.contains("forge url [http://yagdrassyl.local:3000]: "),
        "{stdout}{stderr}"
    );
    let manifest = fs::read_to_string(scene.root.join(".zaf.toml")).expect("manifest");
    for line in [
        "kind = \"forgejo-issues\"",
        "kind = \"forgejo\"",
        "url = \"http://yagdrassyl.local:3000\"",
        "repo = \"zuri/ways\"",
        "token_env = \"FORGEJO_TOKEN\"",
    ] {
        assert!(manifest.contains(line), "{line}:\n{manifest}");
    }
    assert!(!manifest.contains("gh = ") && !manifest.contains("gh pr"));
    assert_eq!(code, 1, "no token, so doctor fails:\n{stdout}{stderr}");
    assert!(
        stdout.contains("`FORGEJO_TOKEN` is not set")
            && stdout.contains("read and write on the repository's issues and pull requests"),
        "{stdout}{stderr}"
    );
}

/// The same project with the token variable set and the instance answering: setup exits 0 and
/// `zaf doctor` exits 0, with no `gh` on the search path at all.
#[test]
fn setup_forgejo_project_doctor_passes_without_gh() {
    let mut scene = Scene::forgejo();
    scene
        .env
        .push(("FORGEJO_TOKEN".to_owned(), "secret".to_owned()));
    let bin = scene.temp.path().join("bin");
    assert!(!bin.join("gh").exists());
    let (code, stdout, stderr) = outcome(&scene.zaf(&["setup"], Some("\n\n\n\n\n")));
    assert_eq!(code, 0, "setup:\n{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&scene.zaf(&["doctor"], None));
    assert_eq!(code, 0, "doctor:\n{stdout}{stderr}");
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
