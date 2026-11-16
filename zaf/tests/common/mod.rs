//! What the command-line tests share: a project in a temp dir, the fake `gh`
//! scene, and running the built binary inside it.

#![allow(dead_code, reason = "each test crate uses its own part of this module")]

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
        self.command_of(Path::new(env!("CARGO_BIN_EXE_zaf")), dir, args)
    }

    /// The command `command_in` makes, of the zaf binary at `program`: a copy, for a test that
    /// replaces the binary a scheduler runs.
    pub fn command_of(&self, program: &Path, dir: &Path, args: &[&str]) -> Command {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let inherited = std::env::var_os("PATH").unwrap_or_default();
        let search_path = std::env::join_paths(
            std::iter::once(fixtures).chain(std::env::split_paths(&inherited)),
        )
        .expect("search path");
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(dir)
            .env("PATH", search_path)
            .env("FAKE_GH_DIR", &self.scene)
            .env("ZAF_HOME", &self.home)
            .env_remove("ZAF_INSTANCE")
            .env_remove("ZAF_VERSION");
        // A suite run inside an action (the gate is one) must not hand its caller's
        // action state to the runs it makes, nor its phase run, which records the questions
        // asked inside it.
        for var in [
            "ZAF_ROOT",
            "ZAF_ACTION",
            "ZAF_DEPTH",
            "ZAF_ACTION_CHAIN",
            "ZAF_ACTION_LOG",
            "ZAF_RUN",
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

/// Holds the scheduler lock of `project` as a running scheduler does, until the returned file is
/// dropped: its `flock`, which says a scheduler runs, and its line naming this test's process,
/// which says where (`pid <this test>, since 2027-01-15T08:00:00Z (zaf 0.3.1, --version 0.3.0)`).
pub fn scheduler_up(project: &Project) -> fs::File {
    let me = zaf::adapters::process::holder_of(std::process::id()).expect("this process");
    let line = serde_json::json!({
        "pid": me.pid,
        "start": me.start,
        "since": "2027-01-15T08:00:00Z",
        "zaf": "0.3.1",
        "version": "0.3.0",
    });
    let state = project.root.join(".zaf");
    fs::create_dir_all(&state).expect("state");
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(state.join("scheduler.lock"))
        .expect("the lock file");
    file.lock().expect("the lock");
    file.write_all(format!("{line}\n").as_bytes())
        .expect("the lock line");
    file
}

/// The exit code, stdout and stderr of a run, for asserting on.
pub fn outcome(output: &Output) -> (i32, String, String) {
    (
        output.status.code().expect("exit code"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A tick's stdout without its first line when that line is the `tick HH:MM` header, which reads
/// the wall clock: what is left is what two ticks of the same scene print alike, whichever minute
/// each began in.
pub fn without_tick_line(stdout: &str) -> &str {
    match stdout.split_once('\n') {
        Some((first, rest)) if first.starts_with("tick ") => rest,
        None if stdout.starts_with("tick ") => "",
        _ => stdout,
    }
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

/// How long a test waits for a long-lived process to say something before it fails.
pub const PATIENCE: Duration = Duration::from_secs(30);

/// A long-lived `zaf` (the scheduler) started by a test: its stdout and stderr gathered as they
/// come, and killed when dropped, so no test leaves it running whatever path it takes.
pub struct Running {
    child: Child,
    out: Arc<Mutex<String>>,
    err: Arc<Mutex<String>>,
}

impl Running {
    /// Starts `command` with its output piped. A binary just written may be held open for writing
    /// by a child another test thread is forking (`ETXTBSY`) for an instant: the start is tried
    /// again then.
    pub fn start(mut command: Command) -> Self {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut child = loop {
            match command.spawn() {
                Ok(child) => break child,
                Err(error)
                    if error.raw_os_error() == Some(libc::ETXTBSY) && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => panic!("cannot start zaf: {error}"),
            }
        };
        let out = gather(child.stdout.take().expect("stdout"));
        let err = gather(child.stderr.take().expect("stderr"));
        Self { child, out, err }
    }

    /// Its process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// What it printed on stdout so far.
    pub fn stdout(&self) -> String {
        self.out.lock().expect("stdout").clone()
    }

    /// What it printed on stderr so far.
    pub fn stderr(&self) -> String {
        self.err.lock().expect("stderr").clone()
    }

    /// Waits up to [`PATIENCE`] for `done(stdout, stderr)`, failing the test with both and `what`
    /// past that.
    pub fn wait_for(&self, what: &str, done: impl Fn(&str, &str) -> bool) {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let (out, err) = (self.stdout(), self.stderr());
            if done(&out, &err) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "never {what}:\n--- stdout\n{out}\n--- stderr\n{err}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits for stdout to hold `count` lines that contain `text`.
    pub fn wait_for_lines(&self, text: &str, count: usize) {
        self.wait_for(&format!("{count} lines with {text:?}"), |out, _| {
            out.lines().filter(|line| line.contains(text)).count() >= count
        });
    }

    /// Sends it the signal `name` (`TERM`, `INT`).
    pub fn signal(&self, name: &str) {
        let status = Command::new("kill")
            .args([format!("-{name}"), self.pid().to_string()])
            .status()
            .expect("kill");
        assert!(status.success(), "kill -{name} failed");
    }

    /// Waits up to [`PATIENCE`] for it to end, and gives its exit code; none when a signal ended
    /// it. Past that it is killed and the test fails.
    pub fn wait_exit(&mut self) -> Option<i32> {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                // Let the readers take the last of the output.
                std::thread::sleep(Duration::from_millis(100));
                return status.code();
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!(
                    "it never ended:\n--- stdout\n{}\n--- stderr\n{}",
                    self.stdout(),
                    self.stderr()
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Whether it still runs.
    pub fn runs(&mut self) -> bool {
        self.child.try_wait().expect("try_wait").is_none()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Gathers what `stream` prints, line by line, as it comes.
fn gather(stream: impl Read + Send + 'static) -> Arc<Mutex<String>> {
    let text = Arc::new(Mutex::new(String::new()));
    let filled = Arc::clone(&text);
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while matches!(reader.read_line(&mut line), Ok(read) if read > 0) {
            filled.lock().expect("output").push_str(&line);
            line.clear();
        }
    });
    text
}
