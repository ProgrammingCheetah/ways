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
    /// The zaf home every run gets (`ZAF_HOME`), empty, so the machine's own
    /// `~/.zaf/config.toml` never reaches a test.
    pub home: PathBuf,
    scene: PathBuf,
    responses: usize,
    env: Vec<(String, String)>,
}

impl Project {
    /// An empty project root with `manifest` as its `.zaf.toml`. A test that exercises the
    /// retired `ZAF.toml` writes that file itself.
    pub fn new(manifest: &str) -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("repo");
        let scene = temp.path().join("gh-scene");
        let home = temp.path().join("zaf-home");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&root).expect("root");
        fs::create_dir_all(&scene).expect("scene");
        fs::write(scene.join("rules"), "").expect("rules");
        fs::write(root.join(".zaf.toml"), manifest).expect("manifest");
        Self {
            temp,
            root,
            home,
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

    /// Answers `gh` calls matching `pattern` with `stdout` before any rule given earlier, so a
    /// scene can change between two runs of the binary.
    pub fn on_first(&mut self, pattern: &str, stdout: &str) -> &mut Self {
        self.rule_first(pattern, 0, stdout)
    }

    /// Fails `gh` calls matching `pattern` with exit code 1 and `stderr`, before any rule given
    /// earlier.
    pub fn fail_first(&mut self, pattern: &str, stderr: &str) -> &mut Self {
        self.rule_first(pattern, 1, stderr)
    }

    fn rule_first(&mut self, pattern: &str, status: u8, text: &str) -> &mut Self {
        let response = format!("response-{}", self.responses);
        self.responses += 1;
        fs::write(self.scene.join(&response), text).expect("response");
        let rules = self.scene.join("rules");
        let earlier = fs::read_to_string(&rules).expect("rules");
        fs::write(
            &rules,
            format!("{pattern}\t{status}\t{response}\n{earlier}"),
        )
        .expect("rules");
        self
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
        self.command_in(dir, args).output().expect("run zaf")
    }

    /// The `zaf` command `zaf_in` runs, not yet started: for a test that must not block on it.
    pub fn command_in(&self, dir: &Path, args: &[&str]) -> Command {
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
            .env("ZAF_HOME", &self.home)
            .env_remove("ZAF_INSTANCE")
            .env_remove("ZAF_VERSION");
        // A suite run inside an action (the gate is one) must not hand its caller's
        // action state to the runs it makes.
        for var in [
            "ZAF_ROOT",
            "ZAF_ACTION",
            "ZAF_DEPTH",
            "ZAF_ACTION_CHAIN",
            "ZAF_ACTION_LOG",
        ] {
            command.env_remove(var);
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
    }

    /// Writes the four `slice-*` actions into the project's `.zaf/actions`: zaf ships none, and
    /// `zaf doctor` (and so `zaf setup`) fails while a needed name has no provider.
    pub fn provide_slice_actions(&self) {
        use std::os::unix::fs::PermissionsExt;
        let actions = self.root.join(".zaf/actions");
        for name in ["slice-open", "slice-pr", "slice-merge", "slice-drop"] {
            write(
                &actions,
                name,
                "#!/bin/sh\n# zaf description: a slice step\n# zaf 1: failed\ntrue\n",
            );
            fs::set_permissions(actions.join(name), fs::Permissions::from_mode(0o755))
                .expect("mode");
        }
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

/// The folder of the fake `gh` and the other fixtures.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A `PATH` of `dirs` in order, then the built `zaf`'s folder, then the one inherited: an action
/// that says `zaf …` runs the binary under test, never one installed on the machine.
pub fn path_with_zaf(dirs: &[&Path]) -> String {
    let zaf = Path::new(env!("CARGO_BIN_EXE_zaf"))
        .parent()
        .expect("the binary's folder")
        .to_owned();
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let all = dirs
        .iter()
        .map(|dir| dir.to_path_buf())
        .chain(std::iter::once(zaf))
        .chain(std::env::split_paths(&inherited));
    std::env::join_paths(all)
        .expect("search path")
        .to_string_lossy()
        .into_owned()
}

/// A scene for the fake `curl` in `tests/fixtures/forgejo/`.
pub struct Curl {
    /// The scene's folder: `FAKE_CURL_DIR`.
    pub dir: PathBuf,
    count: usize,
}

impl Curl {
    /// A scene under `temp` whose instance pages 50 items at a time.
    pub fn new(temp: &Path) -> Self {
        let dir = temp.join("curl-scene");
        fs::create_dir_all(&dir).expect("scene");
        fs::write(dir.join("rules"), "").expect("rules");
        let mut curl = Self { dir, count: 0 };
        curl.on(
            "GET */api/v1/settings/api",
            200,
            r#"{"max_response_items":50}"#,
        );
        curl
    }

    /// Answers calls whose `METHOD URL` matches the glob `pattern` with `status` and `body`.
    pub fn on(&mut self, pattern: &str, status: u16, body: &str) -> &mut Self {
        let response = format!("response-{}", self.count);
        self.count += 1;
        fs::write(self.dir.join(&response), body).expect("response");
        let mut rules = OpenOptions::new()
            .append(true)
            .open(self.dir.join("rules"))
            .expect("rules");
        writeln!(rules, "{pattern}\t{status}\t{response}").expect("rule");
        self
    }

    /// Every call's `METHOD URL` but the page-size read, in order.
    pub fn asked(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.dir.join("calls")) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .map(|entry| entry.expect("call").path())
            .filter(|path| path.extension().is_none())
            .collect();
        files.sort();
        files
            .iter()
            .map(|file| {
                let argv: Vec<String> = fs::read_to_string(file)
                    .expect("call")
                    .split_terminator('\0')
                    .map(str::to_owned)
                    .collect();
                let method = argv
                    .iter()
                    .position(|arg| arg == "--request")
                    .map_or("GET", |at| argv[at + 1].as_str());
                format!("{method} {}", argv.last().expect("a URL"))
            })
            .filter(|asked| !asked.ends_with("/settings/api"))
            .collect()
    }
}
