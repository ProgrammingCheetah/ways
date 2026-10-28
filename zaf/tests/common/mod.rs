//! What the command-line tests share: a project in a temp dir, the fake `gh`
//! scene, and running the built binary inside it.

#![allow(dead_code, reason = "each test crate uses its own part of this module")]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The repository the manifest names for the board and the forge.
pub const REPO: &str = "example/tracker";

/// A temp dir holding a project root and a scene for the fake `gh` in `tests/fixtures/`.
pub struct Project {
    pub temp: tempfile::TempDir,
    pub root: PathBuf,
    scene: PathBuf,
    responses: usize,
    env: Vec<(String, String)>,
}

impl Project {
    /// An empty project root with `manifest` as its `ZAF.toml`.
    pub fn new(manifest: &str) -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("repo");
        let scene = temp.path().join("gh-scene");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&scene).expect("scene");
        fs::write(scene.join("rules"), "").expect("rules");
        fs::write(root.join("ZAF.toml"), manifest).expect("manifest");
        Self {
            temp,
            root,
            scene,
            responses: 0,
            env: Vec::new(),
        }
    }

    /// Answers `gh` calls whose joined argv matches the glob `pattern` with `stdout`.
    pub fn on(&mut self, pattern: &str, stdout: &str) -> &mut Self {
        self.rule(pattern, 0, stdout)
    }

    /// Fails `gh` calls matching `pattern` with exit code 1 and `stderr`.
    pub fn fail_on(&mut self, pattern: &str, stderr: &str) -> &mut Self {
        self.rule(pattern, 1, stderr)
    }

    fn rule(&mut self, pattern: &str, status: u8, text: &str) -> &mut Self {
        let response = format!("response-{}", self.responses);
        self.responses += 1;
        fs::write(self.scene.join(&response), text).expect("response");
        let mut rules = OpenOptions::new()
            .append(true)
            .open(self.scene.join("rules"))
            .expect("rules");
        writeln!(rules, "{pattern}\t{status}\t{response}").expect("rule");
        self
    }

    /// Sets an environment variable for every later run.
    pub fn env(&mut self, key: &str, value: &str) -> &mut Self {
        self.env.push((key.to_owned(), value.to_owned()));
        self
    }

    /// Runs the built `zaf` at the project root with the fake `gh` first on PATH.
    pub fn zaf(&self, args: &[&str]) -> Output {
        self.zaf_in(&self.root, args)
    }

    /// Runs the built `zaf` in `dir`.
    pub fn zaf_in(&self, dir: &Path, args: &[&str]) -> Output {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let search_path = std::env::join_paths(
            std::iter::once(fixtures).chain(std::env::split_paths(&inherited)),
        )
        .expect("search path");
        let mut command = Command::new(env!("CARGO_BIN_EXE_zaf"));
        command
            .args(args)
            .current_dir(dir)
            .env("PATH", search_path)
            .env("FAKE_GH_DIR", &self.scene)
            .env_remove("ZAF_INSTANCE")
            .env_remove("ZAF_VERSION");
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command.output().expect("run zaf")
    }

    /// Every `gh` call's argv, joined by spaces, in order.
    pub fn gh_calls(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.scene.join("calls")) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries.map(|entry| entry.expect("entry").path()).collect();
        files.sort();
        files
            .iter()
            .map(|file| {
                let argv = fs::read(file).expect("call");
                let text = String::from_utf8_lossy(&argv);
                text.trim_end_matches('\0').replace('\0', " ")
            })
            .collect()
    }
}

/// The exit code, stdout and stderr of a run, for asserting on.
pub fn outcome(output: &Output) -> (i32, String, String) {
    (
        output.status.code().expect("exit code"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Runs a program in `dir` and returns its stdout; a failure fails the test.
pub fn run(dir: &Path, program: &str, args: &[&str]) -> String {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{program} {args:?} failed: {stderr}"
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Runs `jj` in `dir` without a pager or colour.
pub fn jj(dir: &Path, args: &[&str]) -> String {
    let mut full_args = vec!["--no-pager", "--color=never"];
    full_args.extend_from_slice(args);
    run(dir, "jj", &full_args)
}

/// Writes a file under `dir`, creating its parents.
pub fn write(dir: &Path, path: &str, content: &str) {
    let file = dir.join(path);
    fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
    fs::write(file, content).expect("write");
}
