//! `zaf update` reads zaf's releases over `curl` alone: these runs have a `PATH` that holds a
//! stand-in `curl` and the few tools it uses, and no `gh`.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const RELEASES: &str =
    "GET https://api.github.com/repos/ProgrammingCheetah/ways/releases?per_page=100";

/// A scene for the fake `curl` of `tests/fixtures/forgejo/`, and a `PATH` of it alone.
struct Scene {
    temp: tempfile::TempDir,
}

impl Scene {
    /// A scene whose releases listing answers `status` with `body`.
    fn new(status: u16, body: &str) -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let scene = temp.path().join("scene");
        let bin = temp.path().join("bin");
        fs::create_dir_all(&scene).expect("scene");
        fs::create_dir_all(&bin).expect("bin");
        fs::write(scene.join("response"), body).expect("response");
        fs::write(
            scene.join("rules"),
            format!("{RELEASES}\t{status}\tresponse\n"),
        )
        .expect("rules");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/forgejo/curl");
        symlink(fixture, bin.join("curl")).expect("curl");
        // The tools the stand-in uses; never `gh`.
        for tool in ["mkdir", "rmdir", "sleep", "find", "wc", "cat", "grep"] {
            symlink(on_path(tool), bin.join(tool)).expect("tool");
        }
        assert!(!bin.join("gh").exists());
        Self { temp }
    }

    fn zaf(&self, args: &[&str], token: Option<&str>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_zaf"));
        command
            .args(args)
            .current_dir(self.temp.path())
            .env_clear()
            .env("PATH", self.temp.path().join("bin"))
            .env("FAKE_CURL_DIR", self.temp.path().join("scene"))
            .env("ZAF_HOME", self.temp.path().join("home"))
            .env("HOME", self.temp.path());
        if let Some(token) = token {
            command.env("GITHUB_TOKEN", token);
        }
        command.output().expect("run zaf")
    }

    /// Every argument of every curl call, and everything it read on stdin.
    fn calls(&self) -> (String, String) {
        let calls = self.temp.path().join("scene/calls");
        let (mut argv, mut config) = (String::new(), String::new());
        for entry in fs::read_dir(calls).expect("calls") {
            let path = entry.expect("entry").path();
            let text = String::from_utf8_lossy(&fs::read(&path).expect("call")).into_owned();
            if path.extension().is_some() {
                config.push_str(&text);
            } else {
                argv.push_str(&text);
            }
        }
        (argv, config)
    }
}

/// Where the inherited `PATH` has `tool`.
fn on_path(tool: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").expect("PATH"))
        .map(|dir| dir.join(tool))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("no {tool} on PATH"))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn newest_release() -> String {
    serde_json::json!([
        {"tag_name": "zaf-v99.0.0", "draft": false, "prerelease": false, "body": "x", "assets": []},
        {"tag_name": "zaf-v100.0.0", "draft": true, "prerelease": false, "body": "x", "assets": []},
        {"tag_name": "zaf-v0.0.1", "draft": false, "prerelease": false, "body": "", "assets": []},
    ])
    .to_string()
}

#[test]
fn update_needs_no_gh() {
    let scene = Scene::new(200, &newest_release());

    let output = scene.zaf(&["update", "--check"], None);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output.stderr));
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(
        text(&output.stdout),
        format!("zaf {version}, newest 99.0.0: run zaf update\n")
    );
    assert_eq!(text(&output.stderr), "");

    let output = scene.zaf(&["--json", "update", "--check"], None);
    assert_eq!(output.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(
        json,
        serde_json::json!({"current": version, "newest": "99.0.0", "updated": false})
    );
}

#[test]
fn the_token_reaches_neither_the_command_line_nor_the_output() {
    let scene = Scene::new(200, &newest_release());

    let output = scene.zaf(&["update", "--check"], Some("secret-xyz"));

    assert_eq!(output.status.code(), Some(1));
    let (argv, config) = scene.calls();
    assert!(!argv.contains("secret-xyz"), "{argv:?}");
    assert!(config.contains("secret-xyz"), "the token goes by stdin");
    assert!(!text(&output.stdout).contains("secret-xyz"));
    assert!(!text(&output.stderr).contains("secret-xyz"));
}

#[test]
fn a_rate_limit_ends_the_update_with_exit_2_and_the_binary_as_it_was() {
    let scene = Scene::new(
        403,
        r#"{"message": "API rate limit exceeded for 203.0.113.9."}"#,
    );
    let binary = Path::new(env!("CARGO_BIN_EXE_zaf"));
    let before = fs::read(binary).expect("binary");

    let output = scene.zaf(&["update"], Some("secret-xyz"));

    assert_eq!(output.status.code(), Some(2), "{}", text(&output.stderr));
    let stderr = text(&output.stderr);
    assert!(stderr.contains("rate limit"), "{stderr}");
    assert!(!stderr.contains("secret-xyz"));
    assert_eq!(text(&output.stdout), "");
    assert_eq!(fs::read(binary).expect("binary"), before);
    let leftovers = fs::read_dir(binary.parent().expect("folder"))
        .expect("folder")
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".zaf-update-")
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn no_network_ends_the_update_with_exit_2_naming_it() {
    let scene = Scene::new(200, "[]");
    // No curl answers: the stand-in is taken off the PATH.
    fs::remove_file(scene.temp.path().join("bin/curl")).expect("remove");

    let output = scene.zaf(&["update", "--check"], None);

    assert_eq!(output.status.code(), Some(2));
    assert!(
        text(&output.stderr).contains("curl"),
        "{}",
        text(&output.stderr)
    );
}
