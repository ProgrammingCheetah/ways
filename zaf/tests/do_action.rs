//! `zaf do` through the built binary, and the runner itself where a kill would otherwise
//! take a whole minute. Every run gets a temp project and a temp zaf home.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use common::{Project, outcome, write};
use zaf::app::actions::{Caller, MINUTE, Nesting, RunRequest, RunResult, Tier, run_action};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

/// The variables a run reads from its caller: a test suite run inside an action must not
/// hand them to the runs it makes.
const CALLER_VARS: [&str; 7] = [
    "ZAF_ROOT",
    "ZAF_ACTION",
    "ZAF_DEPTH",
    "ZAF_ACTION_CHAIN",
    "ZAF_ACTION_LOG",
    "ZAF_INSTANCE",
    "ZAF_VERSION",
];

/// A project whose homes exist, so the first-run notice never reaches a test's output.
fn project() -> Project {
    let project = Project::new(MANIFEST);
    set_up(&project);
    project
}

/// Runs `zaf setup`, which is ready only while the four `slice-*` actions exist: they are
/// provided for the run and taken away after it, so a test sees only the actions it writes.
fn set_up(project: &Project) {
    project.provide_slice_actions();
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    for name in ["slice-open", "slice-pr", "slice-merge", "slice-drop"] {
        fs::remove_file(project.root.join(".zaf/actions").join(name)).expect("take it away");
    }
}

/// The built `zaf` in `dir`, itself first on PATH so a body can say `zaf do`.
fn zaf_command(project: &Project, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Command {
    let bin = Path::new(env!("CARGO_BIN_EXE_zaf"))
        .parent()
        .expect("the binary's folder");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        std::iter::once(bin.to_owned()).chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    let mut command = Command::new(env!("CARGO_BIN_EXE_zaf"));
    command
        .args(args)
        .current_dir(dir)
        .env("PATH", search_path)
        .env("ZAF_HOME", &project.home);
    for var in CALLER_VARS {
        command.env_remove(var);
    }
    command.envs(env.iter().copied());
    command
}

/// Runs the built `zaf` in `dir`.
fn zaf_in(project: &Project, dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Output {
    zaf_command(project, dir, args, env)
        .output()
        .expect("run zaf")
}

fn zaf(project: &Project, args: &[&str]) -> Output {
    zaf_in(project, &project.root, args, &[])
}

/// An executable file at `dir/name` holding exactly `text`.
fn executable(dir: &Path, name: &str, text: &str) -> PathBuf {
    write(dir, name, text);
    let file = dir.join(name);
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("chmod");
    file
}

/// The text of an action with the usual metadata, `extra` lines of it and then `body`.
fn action_text(name: &str, extra: &str, body: &str) -> String {
    format!("#!/bin/sh\n# zaf description: the {name} action\n# zaf 1: it failed\n{extra}{body}\n")
}

/// A project-tier action.
fn action(project: &Project, name: &str, extra: &str, body: &str) -> PathBuf {
    executable(
        &project.root.join(".zaf/actions"),
        name,
        &action_text(name, extra, body),
    )
}

/// A machine-tier action.
fn machine_action(project: &Project, name: &str, body: &str) -> PathBuf {
    executable(
        &project.home.join("actions"),
        name,
        &action_text(name, "", body),
    )
}

/// The files in `dir`, sorted; none when it does not exist.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries.map(|entry| entry.expect("entry").path()).collect();
    files.sort();
    files
}

/// The project's logs of `name`, as the paths zaf prints: a run takes the root from its real
/// working directory, and on macOS the temp dir sits behind the `/var` symlink.
fn logs_of(project: &Project, name: &str) -> Vec<PathBuf> {
    let suffix = format!("_{name}.");
    files_in(&real(&project.root).join(".zaf/logs"))
        .into_iter()
        .filter(|file| file.to_string_lossy().contains(&suffix))
        .collect()
}

/// The one log of `name`.
fn log_of(project: &Project, name: &str) -> PathBuf {
    let logs = logs_of(project, name);
    assert_eq!(logs.len(), 1, "{name}: {logs:?}");
    logs.into_iter().next().expect("one log")
}

fn read(file: &Path) -> String {
    fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

fn real(path: &Path) -> PathBuf {
    path.canonicalize().expect("canonical")
}

/// The run's whole report, whichever stream the lines went to.
fn said(output: &Output) -> String {
    let (_, stdout, stderr) = outcome(output);
    format!("{stdout}{stderr}")
}

/// The `KEY=value` lines of `env` output.
fn env_lines(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect()
}

#[test]
fn do_runs_in_the_callers_directory() {
    let project = project();
    let deep = project.root.join("src/deep");
    fs::create_dir_all(&deep).expect("deep");
    action(
        &project,
        "where",
        "",
        "pwd > \"$ZAF_ROOT/where\"\nprintf '%s\\n' \"$@\" > \"$ZAF_ROOT/argv\"",
    );

    let run = zaf_in(
        &project,
        &deep,
        &["do", "where", "--json", "two words", "-x", "--more"],
        &[],
    );
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "", "--json after the name is the body's");
    assert_eq!(
        real(Path::new(read(&project.root.join("where")).trim())),
        real(&deep)
    );
    assert_eq!(
        read(&project.root.join("argv")),
        "--json\ntwo words\n-x\n--more\n"
    );
}

#[test]
fn do_sets_the_environment() {
    let project = project();
    action(&project, "dump", "", "env > \"$ZAF_ROOT/env\"");

    let run = zaf_in(
        &project,
        &project.root,
        &["do", "dump"],
        &[
            ("ZAF_INSTANCE", "team instance 1 on #9"),
            ("ZAF_VERSION", "1.2.3"),
        ],
    );
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));

    let env = env_lines(&read(&project.root.join("env")));
    let path_of = |key: &str| real(Path::new(&env[key]));
    assert_eq!(path_of("ZAF_ROOT"), real(&project.root));
    assert_eq!(path_of("ZAF_HOME"), real(&project.home));
    assert_eq!(env["ZAF_ACTION"], "dump");
    assert_eq!(env["ZAF_DEPTH"], "0");
    assert_eq!(env["ZAF_INSTANCE"], "team instance 1 on #9");
    assert_eq!(env["ZAF_VERSION"], "1.2.3");

    // A caller without them hands none on.
    let run = zaf(&project, &["do", "dump"]);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let env = env_lines(&read(&project.root.join("env")));
    assert!(!env.contains_key("ZAF_INSTANCE"), "{env:?}");
    assert!(!env.contains_key("ZAF_VERSION"), "{env:?}");
}

/// A project whose manifest has an `[env]` table, set up like [`project`].
fn project_with_env(table: &str) -> Project {
    let project = Project::new(&format!("{MANIFEST}\n[env]\n{table}"));
    set_up(&project);
    project
}

#[test]
fn do_runs_with_env() {
    let project = project_with_env(
        "EXTRA = \"from the table\"\nINHERITED = \"from the table\"\nWHERE = \"{root}/x\"\nBRACES = \"{keep}\"\n",
    );
    action(&project, "dump", "", "env > \"$ZAF_ROOT/env\"");

    let run = zaf_in(
        &project,
        &project.root,
        &["do", "dump"],
        &[
            ("INHERITED", "from the caller"),
            ("UNTOUCHED", "from the caller"),
            ("ZAF_INSTANCE", "team instance 1 on #9"),
            ("ZAF_VERSION", "1.2.3"),
        ],
    );
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));

    let env = env_lines(&read(&project.root.join("env")));
    assert_eq!(env["EXTRA"], "from the table", "set in addition");
    assert_eq!(env["INHERITED"], "from the table", "the table wins");
    assert_eq!(env["UNTOUCHED"], "from the caller", "the rest is inherited");
    assert_eq!(env["BRACES"], "{keep}");
    assert_eq!(
        real(Path::new(env["WHERE"].strip_suffix("/x").expect("suffix"))),
        real(&project.root),
        "{{root}} is the project root"
    );
    assert_eq!(env["ZAF_INSTANCE"], "team instance 1 on #9");
    assert_eq!(env["ZAF_VERSION"], "1.2.3");
}

#[test]
fn do_env_fills_the_version_or_starts_nothing() {
    let project = project_with_env("RELEASE = \"v{version}\"\n");
    action(
        &project,
        "dump",
        "",
        "env > \"$ZAF_ROOT/env\"\n: > \"$ZAF_ROOT/ran\"",
    );

    let run = zaf_in(
        &project,
        &project.root,
        &["do", "dump"],
        &[("ZAF_VERSION", "1.2.3")],
    );
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert_eq!(
        env_lines(&read(&project.root.join("env")))["RELEASE"],
        "v1.2.3"
    );

    fs::remove_file(project.root.join("ran")).expect("reset");
    let run = zaf(&project, &["do", "dump"]);
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert!(said(&run).contains("{version}"), "{}", said(&run));
    assert!(!project.root.join("ran").exists(), "the body ran");
}

#[test]
fn do_env_cannot_set_a_zaf_variable() {
    // The manifest refuses such a table when it loads; this is the runner holding on its own,
    // for a table that reached it some other way.
    let project = project();
    action(&project, "dump", "", "env > \"$ZAF_ROOT/env\"");
    let mut request = request(&project, "dump", 1, MINUTE);
    request.env = BTreeMap::from([
        ("ZAF_INSTANCE".to_owned(), "engineer".to_owned()),
        ("zaf_action".to_owned(), "other".to_owned()),
        ("ZAF_ROOT".to_owned(), "/elsewhere".to_owned()),
        ("FINE".to_owned(), "yes".to_owned()),
    ]);
    let outcome = run_action(&request);
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");

    let env = env_lines(&read(&project.root.join("env")));
    assert_eq!(env["FINE"], "yes");
    // This process may itself run under an instance: only the table's value must be missing.
    assert_ne!(
        env.get("ZAF_INSTANCE").map(String::as_str),
        Some("engineer")
    );
    assert!(!env.contains_key("zaf_action"), "{env:?}");
    assert_eq!(env["ZAF_ACTION"], "dump");
    assert_eq!(real(Path::new(&env["ZAF_ROOT"])), real(&project.root));
}

#[test]
fn do_logs_and_prints_nothing_on_success() {
    let project = project();
    action(&project, "say", "", "echo to-stdout\necho to-stderr >&2");

    let (code, stdout, stderr) = outcome(&zaf(&project, &["do", "say"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");

    let log = log_of(&project, "say");
    let text = read(&log);
    assert!(text.contains("to-stdout\n"), "{text}");
    assert!(text.contains("to-stderr\n"), "{text}");
    let file_name = log
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    let stamp = file_name.strip_suffix("_say.log").expect(&file_name);
    let (date, time) = stamp.split_once('T').expect(stamp);
    let time = time.strip_suffix('Z').expect(stamp);
    assert_eq!(date.len(), 8, "{stamp}");
    assert_eq!(time.len(), 6, "{stamp}");
    assert!(
        date.chars().chain(time.chars()).all(|c| c.is_ascii_digit()),
        "{stamp}"
    );

    // `--more` prints the whole log after the run.
    let (code, stdout, stderr) = outcome(&zaf(&project, &["do", "--more", "say"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("to-stdout\n"), "{stdout}");
    assert!(stdout.contains("to-stderr\n"), "{stdout}");
}

#[test]
fn do_reports_a_declared_failure() {
    let project = project();
    action(
        &project,
        "boom",
        "# zaf 3: the disk is full\n",
        "for n in 01 02 03 04 05 06 07 08 09 10 11 12; do echo \"line-$n\"; done\nexit 3",
    );

    let run = zaf(&project, &["do", "boom"]);
    assert_eq!(outcome(&run).0, 3, "the code passes through");
    let text = said(&run);
    let log = log_of(&project, "boom");
    let headline = format!(
        "FAIL boom: exit 3, the disk is full, log {}\n",
        log.display()
    );
    assert!(text.starts_with(&headline), "{text}");
    let tail: Vec<&str> = text.lines().skip(1).collect();
    assert_eq!(
        tail,
        [
            "line-03", "line-04", "line-05", "line-06", "line-07", "line-08", "line-09", "line-10",
            "line-11", "line-12",
        ],
        "the last ten lines"
    );

    // `--more` prints the whole log, the early lines too.
    let run = zaf(&project, &["do", "--more", "boom"]);
    assert_eq!(outcome(&run).0, 3);
    let text = said(&run);
    assert!(
        text.contains("FAIL boom: exit 3, the disk is full"),
        "{text}"
    );
    assert!(text.contains("line-01\n"), "{text}");
    assert!(text.contains("line-12\n"), "{text}");
}

#[test]
fn do_reports_an_undeclared_code() {
    let project = project();
    action(&project, "odd", "", "exit 7");

    let run = zaf(&project, &["do", "odd"]);
    assert_eq!(outcome(&run).0, 7);
    let log = log_of(&project, "odd");
    assert!(
        said(&run).starts_with(&format!(
            "FAIL odd: exit 7, undeclared, log {}\n",
            log.display()
        )),
        "{}",
        said(&run)
    );
}

#[test]
fn do_reports_on_stderr() {
    let project = project();
    action(&project, "boom", "", "echo the-tail\nexit 1");

    let (code, stdout, stderr) = outcome(&zaf(&project, &["do", "boom"]));
    assert_eq!(code, 1);
    assert_eq!(stdout, "", "stdout stays clean");
    assert_eq!(
        stderr,
        format!(
            "FAIL boom: exit 1, it failed, log {}\nthe-tail\n",
            log_of(&project, "boom").display()
        ),
        "the line and the log's tail"
    );

    let (code, stdout, stderr) = outcome(&zaf(&project, &["do", "ghost"]));
    assert_eq!(code, 125);
    assert_eq!(stdout, "", "stdout stays clean");
    assert_eq!(stderr, "NOT_RUN ghost: no such action\n");
}

#[test]
fn do_closes_the_bodys_stdin() {
    let project = project();
    action(&project, "reader", "", "cat > \"$ZAF_ROOT/seen\"");

    let mut child = zaf_command(&project, &project.root, &["do", "reader"], &[])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start zaf");
    let mut stdin = child.stdin.take().expect("a pipe");
    // This write fails only when zaf has already gone, which a body reading the pipe prevents.
    let _ = stdin.write_all(b"typed by the caller\n");
    drop(stdin);
    let run = child.wait_with_output().expect("wait for zaf");

    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert_eq!(read(&project.root.join("seen")), "", "the caller's input");
}

#[test]
fn do_missing_name_is_not_run() {
    let project = project();

    let run = zaf(&project, &["do", "ghost"]);
    assert_eq!(outcome(&run).0, 125);
    assert_eq!(said(&run), "NOT_RUN ghost: no such action\n");
    assert!(logs_of(&project, "ghost").is_empty(), "no body, no log");

    let run = zaf(&project, &["do", "../ZAF.toml"]);
    assert_eq!(outcome(&run).0, 125);
    assert!(
        said(&run).starts_with("NOT_RUN ../ZAF.toml: bad action name"),
        "{}",
        said(&run)
    );
}

#[test]
fn do_refuses_a_file_without_metadata() {
    let project = project();
    executable(
        &project.root.join(".zaf/actions"),
        "plain",
        "#!/bin/sh\ntouch \"$ZAF_ROOT/ran\"\n",
    );

    let run = zaf(&project, &["do", "plain"]);
    assert_eq!(outcome(&run).0, 125);
    let text = said(&run);
    assert!(text.starts_with("NOT_RUN plain: "), "{text}");
    assert!(text.contains("plain:"), "the file is named: {text}");
    assert!(!project.root.join("ran").exists(), "the body never ran");
    assert!(logs_of(&project, "plain").is_empty());

    // A declared 125 is no valid metadata either, and the bad line is named.
    executable(
        &project.root.join(".zaf/actions"),
        "reserved",
        "#!/bin/sh\n# zaf description: claims the refusal\n# zaf 125: nope\n",
    );
    let run = zaf(&project, &["do", "reserved"]);
    assert_eq!(outcome(&run).0, 125);
    let text = said(&run);
    assert!(text.starts_with("NOT_RUN reserved: "), "{text}");
    assert!(text.contains("reserved:3:"), "{text}");
}

#[test]
fn do_unstartable_file_is_not_run() {
    let project = project();
    let actions = project.root.join(".zaf/actions");

    // Not executable.
    let file = action(&project, "stiff", "", "touch \"$ZAF_ROOT/ran\"");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("chmod");
    // An interpreter that is not there.
    executable(
        &actions,
        "orphan",
        "#!/no/such/interpreter\n# zaf description: never starts\n# zaf 1: no\n",
    );
    // An interpreter that is a folder: refused by the kernel on Linux and macOS alike. A file
    // with no shebang would not do: macOS's `posix_spawnp` retries it under `/bin/sh`.
    executable(
        &actions,
        "hollow",
        "#!/usr\n# zaf description: never starts\n# zaf 1: no\n",
    );

    for (name, reason) in [
        ("stiff", "is not executable"),
        ("orphan", "cannot start"),
        ("hollow", "cannot start"),
    ] {
        let run = zaf(&project, &["do", name]);
        assert_eq!(outcome(&run).0, 125, "{name}: {}", said(&run));
        let text = said(&run);
        assert!(text.starts_with(&format!("NOT_RUN {name}: ")), "{text}");
        assert!(text.contains(reason), "{name}: {text}");
        assert!(logs_of(&project, name).is_empty(), "{name}: no log is left");
    }
    assert!(!project.root.join("ran").exists(), "no body ran");
}

#[test]
fn do_nested_not_run_is_fail_125() {
    let project = project();
    action(&project, "outer", "", "zaf do ghost\nexit $?");

    let run = zaf(&project, &["do", "outer"]);
    assert_eq!(outcome(&run).0, 125, "125 passes through");
    let log = log_of(&project, "outer");
    assert!(
        said(&run).starts_with(&format!(
            "FAIL outer: exit 125, a nested action did not run, log {}\n",
            log.display()
        )),
        "{}",
        said(&run)
    );
    assert!(
        said(&run).contains("NOT_RUN ghost: no such action"),
        "the log's tail"
    );
    assert!(read(&log).contains("NOT_RUN ghost: no such action"));
}

/// A request at the top of the chain, its minute shortened to `minute`.
fn request<'a>(
    project: &'a Project,
    name: &'a str,
    minutes: u64,
    minute: Duration,
) -> RunRequest<'a> {
    RunRequest {
        name,
        args: &[],
        cwd: &project.root,
        project_root: Some(&project.root),
        home: &project.home,
        timeout_minutes: minutes,
        minute,
        nesting: Nesting::default(),
        caller: Caller::Engineer,
        only: None,
        slice: None,
        env: BTreeMap::new(),
    }
}

#[test]
fn do_times_out_with_children() {
    let project = project();
    let zaf_bin = env!("CARGO_BIN_EXE_zaf");
    // The outer body leaves a child behind in its own group and calls a nested run, whose
    // body sits in a group of its own with a child of its own. Every one of them would leave a
    // mark once the sleep ends, so a mark proves a survivor.
    action(
        &project,
        "inner",
        "",
        "(sleep 4; touch \"$ZAF_ROOT/inner-child-survived\") &\n\
         touch \"$ZAF_ROOT/inner-started\"\nsleep 4\ntouch \"$ZAF_ROOT/inner-survived\"",
    );
    action(
        &project,
        "outer",
        "# zaf timeout: 1\n",
        &format!(
            "(sleep 4; touch \"$ZAF_ROOT/outer-child-survived\") &\n\
             \"{zaf_bin}\" do inner\ntouch \"$ZAF_ROOT/outer-survived\""
        ),
    );

    // The file's `timeout: 1` wins over the manifest's far longer bound.
    let started = Instant::now();
    let run = run_action(&request(
        &project,
        "outer",
        1000,
        Duration::from_millis(1500),
    ));
    assert!(
        started.elapsed() < Duration::from_millis(3500),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(run.result, RunResult::Fail);
    assert_eq!(run.exit, 2);
    assert_eq!(run.tier, Some(Tier::Project));
    let log = run.log.clone().expect("a log");
    assert_eq!(
        run.headline().expect("a line"),
        format!(
            "FAIL outer: exit 2, timed out after 1 min, log {}",
            log.display()
        )
    );
    assert!(
        project.root.join("inner-started").exists(),
        "the nested body ran"
    );

    // Past the point every sleep would have ended.
    std::thread::sleep(Duration::from_millis(4500).saturating_sub(started.elapsed()));
    for mark in [
        "outer-child-survived",
        "outer-survived",
        "inner-survived",
        "inner-child-survived",
    ] {
        assert!(!project.root.join(mark).exists(), "{mark}");
    }

    // A file without `timeout` takes the manifest's.
    action(&project, "slow", "", "sleep 20");
    let started = Instant::now();
    let run = run_action(&request(&project, "slow", 2, Duration::from_millis(200)));
    assert_eq!((run.result, run.exit), (RunResult::Fail, 2));
    assert!(
        started.elapsed() >= Duration::from_millis(400),
        "{:?}",
        started.elapsed()
    );
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(MINUTE, Duration::from_secs(60), "every real run's minute");
}

#[test]
fn do_nests_with_two_logs() {
    let project = project();
    action(
        &project,
        "inner",
        "",
        "echo \"inner depth $ZAF_DEPTH named $ZAF_ACTION\"",
    );
    action(
        &project,
        "outer",
        "",
        "echo \"outer depth $ZAF_DEPTH\"\nzaf do inner\nzaf do inner > /dev/null 2>&1\necho value",
    );

    let (code, stdout, stderr) = outcome(&zaf(&project, &["do", "outer"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    assert_eq!(stderr, "");

    let outer = read(&log_of(&project, "outer"));
    assert!(outer.contains("outer depth 0\n"), "{outer}");
    assert!(
        !outer.contains("inner depth"),
        "the inner body has its own log: {outer}"
    );
    let inner = logs_of(&project, "inner");
    assert_eq!(inner.len(), 2, "{inner:?}");
    for log in &inner {
        assert!(read(log).contains("inner depth 1 named inner\n"));
        assert!(
            outer.contains(&log.display().to_string()),
            "a silenced call too is in the outer log: {outer}"
        );
    }
    assert_eq!(
        outer.lines().rfind(|line| !line.trim().is_empty()),
        Some("value"),
        "the body's last line stays the action's value"
    );
}

#[test]
fn do_stops_at_depth_eight() {
    let project = project();
    action(
        &project,
        "deep",
        "",
        "echo \"$ZAF_DEPTH\" >> \"$ZAF_ROOT/depths\"\nzaf do deep\nexit $?",
    );

    let run = zaf(&project, &["do", "deep"]);
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert!(
        said(&run).starts_with("FAIL deep: exit 125"),
        "{}",
        said(&run)
    );
    assert_eq!(
        read(&project.root.join("depths")),
        "0\n1\n2\n3\n4\n5\n6\n7\n",
        "the call with eight above it starts nothing"
    );
    let chain = ["deep"; 9].join(" > ");
    let refused = format!("NOT_RUN deep: depth 8 reached: {chain}");
    assert!(
        logs_of(&project, "deep")
            .iter()
            .any(|log| read(log).contains(&refused)),
        "the innermost log names the chain"
    );

    // Called directly from such a depth: nothing starts, nothing is logged, the chain is named.
    let plain = action(&project, "plain", "", "touch \"$ZAF_ROOT/ran\"");
    let run = zaf_in(
        &project,
        &project.root,
        &["do", "plain"],
        &[("ZAF_DEPTH", "8"), ("ZAF_ACTION_CHAIN", "first > second")],
    );
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert_eq!(
        said(&run),
        "NOT_RUN plain: depth 8 reached: first > second > plain\n"
    );
    assert!(!project.root.join("ran").exists());
    assert!(logs_of(&project, "plain").is_empty());
    assert!(plain.is_file());

    // A depth that is no number fails closed.
    let run = zaf_in(
        &project,
        &project.root,
        &["do", "plain"],
        &[("ZAF_DEPTH", "many")],
    );
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert!(
        said(&run).starts_with("NOT_RUN plain: ZAF_DEPTH is"),
        "{}",
        said(&run)
    );
    assert!(!project.root.join("ran").exists());
}

#[test]
fn do_same_second_runs_keep_both_logs() {
    let project = project();
    action(&project, "tick", "", "echo \"run $1\"");

    // Runs until two fall in one second; a slow machine only takes more of them.
    let mut second = None;
    for run in 0..300 {
        let (code, stdout, stderr) = outcome(&zaf(&project, &["do", "tick", &run.to_string()]));
        assert_eq!(code, 0, "{stdout}{stderr}");
        second = logs_of(&project, "tick")
            .into_iter()
            .find(|log| log.to_string_lossy().ends_with("_tick.2.log"));
        if second.is_some() {
            break;
        }
    }
    let second = second.expect("two runs within a second");
    let first = PathBuf::from(second.to_string_lossy().replace("_tick.2.log", "_tick.log"));
    assert!(first.is_file(), "{}", first.display());
    let (first, second) = (read(&first), read(&second));
    assert!(first.starts_with("run "), "{first}");
    assert!(second.starts_with("run "), "{second}");
    assert_ne!(first, second, "each run kept its own log");
}

#[test]
fn do_lists_with_tiers() {
    let project = project();
    action(&project, "alpha", "", "true");
    machine_action(&project, "beta", "true");
    action(&project, "both", "", "true");
    machine_action(&project, "both", "true");
    executable(
        &project.root.join(".zaf/actions"),
        "broken",
        "#!/bin/sh\necho no metadata\n",
    );

    let (code, stdout, stderr) = outcome(&zaf(&project, &["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let rows: Vec<Vec<&str>> = stdout
        .lines()
        .map(|line| line.split_whitespace().take(2).collect())
        .collect();
    assert_eq!(
        rows,
        [
            ["alpha", "project"],
            ["beta", "machine"],
            ["both", "project"],
            ["broken", "project"],
        ]
    );
    assert!(stdout.contains("the alpha action"), "{stdout}");
    assert!(stdout.contains("the beta action"), "{stdout}");
    assert!(stdout.contains("invalid metadata"), "{stdout}");

    let (code, stdout, stderr) = outcome(&zaf(&project, &["do", "--json"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let listed: serde_json::Value = serde_json::from_str(&stdout).expect(&stdout);
    assert_eq!(
        listed[0],
        serde_json::json!({
            "name": "alpha",
            "tier": "project",
            "description": "the alpha action",
        })
    );
    assert_eq!(listed[1]["tier"], "machine");
    assert_eq!(listed.as_array().expect("an array").len(), 4);
}

#[test]
fn do_json_shows_a_declared_kind() {
    let project = project();
    action(&project, "plain", "", "true");
    action(&project, "waiter", "# zaf kind: wait\n", "true");

    let (code, stdout, stderr) = outcome(&zaf(&project, &["do", "--json"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let listed: serde_json::Value = serde_json::from_str(&stdout).expect(&stdout);
    assert_eq!(listed[0]["name"], "plain");
    assert!(listed[0].get("kind").is_none(), "{stdout}");
    assert_eq!(listed[1]["name"], "waiter");
    assert_eq!(listed[1]["kind"], "wait");
}

#[test]
fn do_show_prints_the_file() {
    let project = project();
    let bytes = b"#!/bin/sh\n# zaf description: odd bytes\n# zaf 1: no\n# \xff\n  echo no newline at the end";
    write(&project.root, ".zaf/actions/odd", "");
    fs::write(project.root.join(".zaf/actions/odd"), bytes).expect("odd");

    let output = zaf(&project, &["do", "--show", "odd"]);
    assert_eq!(output.status.code(), Some(0), "{}", said(&output));
    assert_eq!(output.stdout, bytes, "byte for byte");
    assert!(logs_of(&project, "odd").is_empty(), "nothing ran");

    // A file a run would refuse can still be read.
    executable(
        &project.root.join(".zaf/actions"),
        "plain",
        "#!/bin/sh\necho plain\n",
    );
    let output = zaf(&project, &["do", "--show", "plain"]);
    assert_eq!(output.status.code(), Some(0), "{}", said(&output));
    assert_eq!(output.stdout, b"#!/bin/sh\necho plain\n");

    let run = zaf(&project, &["do", "--show", "ghost"]);
    assert_eq!(outcome(&run).0, 125);
    assert_eq!(said(&run), "NOT_RUN ghost: no such action\n");

    let run = zaf(&project, &["do", "--show"]);
    assert_eq!(outcome(&run).0, 2, "{}", said(&run));
}

#[test]
fn do_json_fields() {
    let project = project();
    action(&project, "fine", "", "echo fine");
    action(&project, "boom", "", "exit 1");

    let parse = |output: &Output| -> serde_json::Value {
        let (_, stdout, stderr) = outcome(output);
        assert_eq!(stderr, "", "--json says it all on stdout");
        serde_json::from_str(&stdout).expect(&stdout)
    };
    let keys = |value: &serde_json::Value| -> Vec<String> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    };
    let expected = [
        "exit", "log", "meaning", "name", "result", "seconds", "tier",
    ];

    let run = zaf(&project, &["do", "--json", "fine"]);
    assert_eq!(outcome(&run).0, 0);
    let fine = parse(&run);
    assert_eq!(keys(&fine), expected);
    assert_eq!(fine["result"], "OK");
    assert_eq!(fine["name"], "fine");
    assert_eq!(fine["tier"], "project");
    assert_eq!(fine["exit"], 0);
    assert!(fine["meaning"].is_null());
    assert!(fine["seconds"].is_number());
    assert_eq!(
        PathBuf::from(fine["log"].as_str().expect("a path")),
        log_of(&project, "fine")
    );

    let run = zaf(&project, &["do", "--json", "boom"]);
    assert_eq!(outcome(&run).0, 1);
    let boom = parse(&run);
    assert_eq!(keys(&boom), expected);
    assert_eq!(boom["result"], "FAIL");
    assert_eq!(boom["exit"], 1);
    assert_eq!(boom["meaning"], "it failed");

    let run = zaf(&project, &["do", "--json", "ghost"]);
    assert_eq!(outcome(&run).0, 125);
    let ghost = parse(&run);
    assert_eq!(keys(&ghost), expected);
    assert_eq!(ghost["result"], "NOT_RUN");
    assert_eq!(ghost["exit"], 125);
    assert_eq!(ghost["meaning"], "no such action");
    assert!(ghost["tier"].is_null());
    assert!(ghost["log"].is_null());
}

#[test]
fn do_works_outside_a_project() {
    let project = project();
    let outside = project.temp.path().join("elsewhere");
    fs::create_dir_all(&outside).expect("outside");
    // A stray folder here is no project tier: there is no manifest above it.
    executable(
        &outside.join(".zaf/actions"),
        "stray",
        &action_text("stray", "", "touch ran"),
    );
    machine_action(
        &project,
        "tool",
        "echo \"root=${ZAF_ROOT-unset} home=$ZAF_HOME\"\npwd",
    );

    let (code, stdout, stderr) = outcome(&zaf_in(&project, &outside, &["do"], &[]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.starts_with("tool"), "{stdout}");
    assert!(stdout.contains("machine"), "{stdout}");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");

    // A caller's stale `ZAF_ROOT` does not reach the body.
    let run = zaf_in(
        &project,
        &outside,
        &["do", "tool"],
        &[("ZAF_ROOT", "/stale")],
    );
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    let logs = files_in(&project.home.join("logs"));
    assert_eq!(logs.len(), 1, "{logs:?}");
    let text = read(&logs[0]);
    assert!(text.starts_with("root=unset home="), "{text}");
    assert!(text.contains(&outside.display().to_string()), "{text}");
    assert!(!outside.join(".zaf/logs").exists());

    let run = zaf_in(&project, &outside, &["do", "stray"], &[]);
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert_eq!(said(&run), "NOT_RUN stray: no such action\n");
    assert!(!outside.join("ran").exists());

    let run = zaf_in(&project, &outside, &["do", "--show", "tool"], &[]);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(outcome(&run).1.contains("the tool action"));

    // The machine's own `[actions]` is the bound here, and a bad one starts nothing.
    write(
        &project.home,
        "config.toml",
        "[actions]\ntimeout_minutes = 0\n",
    );
    let run = zaf_in(&project, &outside, &["do", "tool"], &[]);
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert!(said(&run).contains("timeout_minutes"), "{}", said(&run));
    assert_eq!(files_in(&project.home.join("logs")).len(), 1, "no new log");
}

const PROGRAMMER: &str = "programmer instance 1 on #9";
const TEAM: &str = "team instance 1 on #9";
const RELEASE: &str = "release instance 1 on #9";

/// An action named `name` under `metadata` that leaves `name.ran` in the project when it runs.
fn gated(project: &Project, name: &str, metadata: &str) -> PathBuf {
    action(project, name, metadata, &format!("touch {name}.ran"))
}

/// The run of `name` by `instance`, or by hand with none.
fn run_as(project: &Project, name: &str, instance: Option<&str>) -> Output {
    match instance {
        Some(instance) => zaf_in(
            project,
            &project.root,
            &["do", name],
            &[("ZAF_INSTANCE", instance)],
        ),
        None => zaf(project, &["do", name]),
    }
}

/// The body ran: exit 0 and its mark. Clears the mark and the log, for a refusal to be judged by.
fn assert_ran(project: &Project, name: &str, run: &Output) {
    assert_eq!(outcome(run).0, 0, "{name}: {}", said(run));
    let mark = project.root.join(format!("{name}.ran"));
    assert!(mark.exists(), "{name} should have run");
    fs::remove_file(mark).expect("clear the mark");
    for log in logs_of(project, name) {
        fs::remove_file(log).expect("clear the log");
    }
}

/// Nothing started: no mark and no log.
fn assert_nothing_started(project: &Project, name: &str) {
    assert!(
        !project.root.join(format!("{name}.ran")).exists(),
        "{name} ran"
    );
    assert_eq!(logs_of(project, name), Vec::<PathBuf>::new(), "{name}");
}

/// The mode refused `role`: the one line, exit 125 and nothing started.
fn assert_refused(project: &Project, name: &str, run: &Output, role: &str) {
    let (code, stdout, stderr) = outcome(run);
    assert_eq!(code, 125, "{name}: {stdout}{stderr}");
    assert_eq!(stdout, "", "{name}");
    assert_eq!(
        stderr,
        format!("NOT_RUN {name}: not permitted for {role}\n")
    );
    assert_nothing_started(project, name);
}

/// The refusal `[forbidden]` gives: the one line, exit 125 and nothing started.
fn assert_forbidden(project: &Project, name: &str, run: &Output, role: &str) {
    let (code, stdout, stderr) = outcome(run);
    assert_eq!(code, 125, "{name}: {stdout}{stderr}");
    assert_eq!(stdout, "", "{name}");
    assert_eq!(stderr, format!("NOT_RUN {name}: forbidden for {role}\n"));
    assert_nothing_started(project, name);
}

#[test]
fn do_refuses_a_forbidden_name_outside_nesting() {
    let project = Project::new(&format!(
        "{MANIFEST}\n[forbidden]\ncommands = [\"gh pr merge\", \"zaf do land\", \
         \"zaf do --tier machine pr-merge\"]\n"
    ));
    set_up(&project);
    for name in ["land", "pr-merge", "tidy", "ship"] {
        gated(&project, name, "");
    }

    // An instance is refused each forbidden name, with flags or without, and nothing starts.
    for role in [PROGRAMMER, TEAM, RELEASE] {
        for args in [
            vec!["do", "land"],
            vec!["do", "land", "12"],
            vec!["--more", "do", "land", "12"],
            vec!["do", "--tier", "machine", "pr-merge", "12"],
            vec!["do", "pr-merge"],
        ] {
            let name = args
                .iter()
                .find(|word| ["land", "pr-merge"].contains(word))
                .expect("a name");
            let run = zaf_in(&project, &project.root, &args, &[("ZAF_INSTANCE", role)]);
            let shown = role.split(' ').next().expect("a role");
            assert_forbidden(&project, name, &run, shown);
        }
    }

    // A name that is not forbidden still runs for an instance.
    for name in ["tidy", "ship"] {
        let run = run_as(&project, name, Some(RELEASE));
        assert_ran(&project, name, &run);
    }

    // The Engineer's hand run is under no profile.
    let run = run_as(&project, "land", None);
    assert_ran(&project, "land", &run);

    // A call from inside another action's body is nested, and runs: this is how `slice-merge`
    // and `release` reach the merge paths. (`outer` stands for either.)
    action(
        &project,
        "outer",
        "",
        "zaf do land && zaf do --tier machine pr-merge",
    );
    machine_action(&project, "pr-merge", "touch \"$ZAF_ROOT/pr-merge.ran\"");
    let run = run_as(&project, "outer", Some(RELEASE));
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(project.root.join("land.ran").exists(), "nested land ran");
    assert!(
        project.root.join("pr-merge.ran").exists(),
        "nested pr-merge ran"
    );
    // The outer call itself is not forbidden, only the names are.
    let run = run_as(&project, "outer", Some(PROGRAMMER));
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
}

#[test]
fn engineer_digit_applies() {
    let project = project();
    // The owner digit alone opens or closes a hand run, whatever the other two say.
    gated(&project, "owner-open", "# zaf mode: 100\n");
    assert_ran(
        &project,
        "owner-open",
        &run_as(&project, "owner-open", None),
    );
    for mode in ["077", "677", "477", "277"] {
        let name = format!("owner-shut-{mode}");
        gated(&project, &name, &format!("# zaf mode: {mode}\n"));
        assert_refused(
            &project,
            &name,
            &run_as(&project, &name, None),
            "the Engineer",
        );
    }
    // The same files answer a role by another digit: the digit is the caller's own.
    assert_refused(
        &project,
        "owner-open",
        &run_as(&project, "owner-open", Some(PROGRAMMER)),
        "programmer",
    );
    assert_ran(
        &project,
        "owner-shut-077",
        &run_as(&project, "owner-shut-077", Some(PROGRAMMER)),
    );
}

#[test]
fn group_digit_applies() {
    let project = project();
    let named = "# zaf group: team, release\n";
    gated(&project, "landing", &format!("# zaf mode: 010\n{named}"));
    for instance in [TEAM, RELEASE] {
        assert_ran(
            &project,
            "landing",
            &run_as(&project, "landing", Some(instance)),
        );
    }
    assert_refused(
        &project,
        "landing",
        &run_as(&project, "landing", Some(PROGRAMMER)),
        "programmer",
    );
    assert_refused(
        &project,
        "landing",
        &run_as(&project, "landing", None),
        "the Engineer",
    );

    // No falling through to a wider digit: the group is refused where others may run.
    gated(
        &project,
        "not-landing",
        &format!("# zaf mode: 701\n{named}"),
    );
    assert_refused(
        &project,
        "not-landing",
        &run_as(&project, "not-landing", Some(TEAM)),
        "team",
    );
    assert_ran(
        &project,
        "not-landing",
        &run_as(&project, "not-landing", Some(PROGRAMMER)),
    );
}

#[test]
fn role_named_in_group_gets_its_digit() {
    let project = project();
    // There is no fixed list of roles: a project's own name counts as a built-in one does.
    for role in ["auditor", "night-shift", "engineer", "team"] {
        let instance = format!("{role} instance 1 on #9");
        let refused = format!("refused-{role}");
        let permitted = format!("permitted-{role}");
        gated(
            &project,
            &refused,
            &format!("# zaf mode: 701\n# zaf group: {role}\n"),
        );
        gated(
            &project,
            &permitted,
            &format!("# zaf mode: 710\n# zaf group: {role}\n"),
        );
        assert_refused(
            &project,
            &refused,
            &run_as(&project, &refused, Some(&instance)),
            role,
        );
        assert_ran(
            &project,
            &permitted,
            &run_as(&project, &permitted, Some(&instance)),
        );

        // The same files read another digit for a role the group does not name.
        let other = "inspector instance 1 on #9";
        assert_ran(&project, &refused, &run_as(&project, &refused, Some(other)));
        assert_refused(
            &project,
            &permitted,
            &run_as(&project, &permitted, Some(other)),
            "inspector",
        );
    }
}

#[test]
fn others_digit_applies() {
    let project = project();
    // A mode with no group: every role is others.
    gated(&project, "open", "# zaf mode: 701\n");
    gated(&project, "shut", "# zaf mode: 710\n");
    for instance in [TEAM, PROGRAMMER] {
        assert_ran(&project, "open", &run_as(&project, "open", Some(instance)));
    }
    assert_refused(
        &project,
        "shut",
        &run_as(&project, "shut", Some(TEAM)),
        "team",
    );

    // A role not named in the group is others.
    gated(&project, "unnamed", "# zaf mode: 710\n# zaf group: team\n");
    assert_refused(
        &project,
        "unnamed",
        &run_as(&project, "unnamed", Some(PROGRAMMER)),
        "programmer",
    );

    // A name no list of roles holds is still others when the group leaves it out.
    gated(
        &project,
        "others-open",
        "# zaf mode: 701\n# zaf group: team\n",
    );
    let auditor = "auditor instance 1 on #9";
    assert_ran(
        &project,
        "others-open",
        &run_as(&project, "others-open", Some(auditor)),
    );
    assert_refused(
        &project,
        "unnamed",
        &run_as(&project, "unnamed", Some(auditor)),
        "auditor",
    );

    // No role name reaches the Engineer's digit.
    gated(&project, "hand-only", "# zaf mode: 700\n");
    for role in ["engineer", "Engineer", "owner", "the Engineer", "local"] {
        let instance = format!("{role} instance 1 on #9");
        assert_refused(
            &project,
            "hand-only",
            &run_as(&project, "hand-only", Some(&instance)),
            role,
        );
    }
    assert_ran(&project, "hand-only", &run_as(&project, "hand-only", None));
}

#[test]
fn flags_do_not_dodge_a_mode() {
    let project = project();
    gated(&project, "gated", "# zaf mode: 750\n# zaf group: team\n");
    let env = [("ZAF_INSTANCE", PROGRAMMER)];

    for args in [
        &["do", "--more", "gated"][..],
        &["--more", "do", "gated"],
        &["do", "--", "gated"],
        &["do", "--more", "--", "gated"],
        &["do", "gated", "--json"],
        &["do", "gated", "--show"],
        &["do", "gated", "--help"],
    ] {
        let run = zaf_in(&project, &project.root, args, &env);
        assert_refused(&project, "gated", &run, "programmer");
    }

    for args in [&["do", "--json", "gated"][..], &["--json", "do", "gated"]] {
        let run = zaf_in(&project, &project.root, args, &env);
        let (code, stdout, stderr) = outcome(&run);
        assert_eq!(code, 125, "{args:?}: {stdout}{stderr}");
        let report: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON line");
        assert_eq!(report["result"], "NOT_RUN", "{args:?}");
        assert_eq!(report["exit"], 125, "{args:?}");
        assert_eq!(
            report["meaning"], "not permitted for programmer",
            "{args:?}"
        );
        assert_eq!(report["log"], serde_json::Value::Null, "{args:?}");
        assert_nothing_started(&project, "gated");
    }
}

#[test]
fn nesting_cannot_launder_a_role() {
    let project = project();
    gated(&project, "inner", "# zaf mode: 750\n# zaf group: team\n");
    action(&project, "outer", "", "zaf do inner\nexit $?");
    action(&project, "outermost", "", "zaf do outer\nexit $?");

    // The body passes the refusal on: the inner run is NOT_RUN and the outer FAIL.
    let run = run_as(&project, "outer", Some(PROGRAMMER));
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    let log = log_of(&project, "outer");
    assert!(
        said(&run).starts_with(&format!(
            "FAIL outer: exit 125, a nested action did not run, log {}\n",
            log.display()
        )),
        "{}",
        said(&run)
    );
    assert!(
        read(&log).contains("NOT_RUN inner: not permitted for programmer\n"),
        "{}",
        read(&log)
    );
    assert_nothing_started(&project, "inner");

    // Two levels down the role is still the programmer's.
    let run = run_as(&project, "outermost", Some(PROGRAMMER));
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert_nothing_started(&project, "inner");

    // The same chain opens for a caller the inner mode permits, so the role is what closed it.
    for caller in [Some(TEAM), None] {
        let run = run_as(&project, "outermost", caller);
        assert_eq!(outcome(&run).0, 0, "{caller:?}: {}", said(&run));
        let mark = project.root.join("inner.ran");
        assert!(mark.exists(), "{caller:?}: inner should have run");
        fs::remove_file(mark).expect("clear the mark");
    }
}

#[test]
fn bad_instance_is_refused() {
    let project = project();
    // Open to the Engineer alone, and open to everyone: neither is a way in.
    gated(&project, "hand-only", "# zaf mode: 700\n");
    gated(&project, "open", "# zaf mode: 755\n");
    for value in [
        "",
        " ",
        "local",
        "nonsense",
        "team",
        "team instance",
        "team instance x on #9",
        " instance 1 on #9",
        "team instance 1 on #9\nengineer",
    ] {
        for name in ["hand-only", "open"] {
            let run = run_as(&project, name, Some(value));
            let (code, stdout, stderr) = outcome(&run);
            assert_eq!(code, 125, "{value:?}: {stdout}{stderr}");
            assert_eq!(stdout, "", "{value:?}");
            let expected = format!(
                "NOT_RUN {name}: ZAF_INSTANCE is {value:?}, not `<role> instance <n> on #<epic>`\n"
            );
            assert_eq!(stderr, expected, "{value:?}");
            assert_nothing_started(&project, name);
        }
    }
}

/// `args` run by `instance`, or by hand with none.
fn zaf_as(project: &Project, args: &[&str], instance: Option<&str>) -> Output {
    let env: Vec<(&str, &str)> = instance
        .map(|role| ("ZAF_INSTANCE", role))
        .into_iter()
        .collect();
    zaf_in(project, &project.root, args, &env)
}

/// The names `zaf do` lists for `instance`, which the text and the `--json` listing agree on.
fn listed_for(project: &Project, instance: Option<&str>) -> Vec<String> {
    let (code, stdout, stderr) = outcome(&zaf_as(project, &["do"], instance));
    assert_eq!(code, 0, "{instance:?}: {stdout}{stderr}");
    let names: Vec<String> = stdout
        .lines()
        .map(|line| line.split_whitespace().next().expect("a name").to_owned())
        .collect();

    let (code, stdout, stderr) = outcome(&zaf_as(project, &["do", "--json"], instance));
    assert_eq!(code, 0, "{instance:?}: {stdout}{stderr}");
    let rows: serde_json::Value = serde_json::from_str(&stdout).expect(&stdout);
    let json_names: Vec<String> = rows
        .as_array()
        .expect("an array")
        .iter()
        .map(|row| row["name"].as_str().expect("a name").to_owned())
        .collect();
    assert_eq!(names, json_names, "{instance:?}");
    names
}

/// Files whose modes answer each kind of caller differently, and the ones whose mode cannot be
/// told. What a role gets from them is always the digit, never a fallback.
fn modes_to_read(project: &Project) {
    let team = "# zaf group: team\n";
    gated(project, "open", "# zaf mode: 755\n");
    gated(project, "hand-only", "# zaf mode: 700\n");
    gated(project, "team-reads", &format!("# zaf mode: 750\n{team}"));
    gated(project, "team-shut", &format!("# zaf mode: 705\n{team}"));
    gated(project, "run-only", "# zaf mode: 711\n");
    gated(project, "read-only", "# zaf mode: 744\n");
    gated(project, "owner-shut", "# zaf mode: 011\n");
    gated(project, "write-only", "# zaf mode: 722\n");
    // Each of these names its mode, or would, in a file that does not parse.
    let actions = project.root.join(".zaf/actions");
    executable(&actions, "no-metadata", "#!/bin/sh\necho SECRET\n");
    executable(
        &actions,
        "bad-mode",
        "#!/bin/sh\n# zaf description: SECRET\n# zaf 1: no\n# zaf mode: open\n",
    );
    executable(
        &actions,
        "bad-line",
        "#!/bin/sh\n# zaf description: SECRET\n# zaf 1: no\n# zaf mode: 755\n# zaf nonsense: x\n",
    );
    // A byte that is no text would end the block early, and the mode under it would be lost.
    let odd = actions.join("not-text");
    fs::write(
        &odd,
        b"#!/bin/sh\n# zaf description: SECRET\n# zaf 1: no\n# \xff\n# zaf mode: 700\n",
    )
    .expect("not-text");
    fs::set_permissions(&odd, fs::Permissions::from_mode(0o755)).expect("chmod");
}

#[test]
fn listing_hides_the_unreadable() {
    let project = project();
    modes_to_read(&project);

    assert_eq!(
        listed_for(&project, Some(PROGRAMMER)),
        ["open", "read-only", "team-shut"],
        "others: the third digit's read bit"
    );
    assert_eq!(
        listed_for(&project, Some(TEAM)),
        ["open", "read-only", "team-reads"],
        "the group: the second digit's, even where others may read"
    );
    assert_eq!(
        listed_for(&project, None),
        [
            "bad-line",
            "bad-mode",
            "hand-only",
            "no-metadata",
            "not-text",
            "open",
            "read-only",
            "run-only",
            "team-reads",
            "team-shut",
            "write-only",
        ],
        "the Engineer: the first digit's, and what only the Engineer can repair"
    );

    // Hidden is hidden: no row, no width, no reason from the file.
    for args in [&["do"][..], &["do", "--json"], &["do", "--more"]] {
        let (_, stdout, stderr) = outcome(&zaf_as(&project, args, Some(PROGRAMMER)));
        for hidden in [
            "hand-only",
            "no-metadata",
            "bad-mode",
            "invalid metadata",
            "SECRET",
        ] {
            assert!(!stdout.contains(hidden), "{args:?}: {hidden}: {stdout}");
            assert!(!stderr.contains(hidden), "{args:?}: {hidden}: {stderr}");
        }
    }

    // A caller that cannot be told sees nothing at all, in either form.
    for value in ["", " ", "local", "team", "team instance 1 on #9\nengineer"] {
        for args in [&["do"][..], &["do", "--json"]] {
            let (code, stdout, stderr) = outcome(&zaf_as(&project, args, Some(value)));
            assert_eq!(code, 2, "{value:?} {args:?}: {stdout}{stderr}");
            assert_eq!(stdout, "", "{value:?} {args:?}");
            assert!(stderr.contains("ZAF_INSTANCE"), "{value:?}: {stderr}");
        }
    }
}

#[test]
fn listing_of_nothing_readable_is_empty_not_an_error() {
    let project = project();
    gated(&project, "hand-only", "# zaf mode: 700\n");
    assert_eq!(listed_for(&project, Some(PROGRAMMER)), Vec::<String>::new());
}

#[test]
fn machine_tier_needs_read_outside_a_project() {
    let project = project();
    let outside = project.temp.path().join("elsewhere");
    fs::create_dir_all(&outside).expect("outside");
    let machine = project.home.join("actions");
    let text = action_text("tool", "# zaf mode: 700\n", "true");
    executable(&machine, "tool", &text);
    let role = [("ZAF_INSTANCE", PROGRAMMER)];

    let (code, stdout, stderr) = outcome(&zaf_in(&project, &outside, &["do"], &role));
    assert_eq!((code, stdout.as_str()), (0, ""), "{stderr}");
    let (code, stdout, stderr) = outcome(&zaf_in(
        &project,
        &outside,
        &["do", "--show", "tool"],
        &role,
    ));
    assert_eq!((code, stdout.as_str()), (125, ""));
    assert_eq!(stderr, "NOT_RUN tool: not permitted for programmer\n");

    let shown = zaf_in(&project, &outside, &["do", "--show", "tool"], &[]);
    assert_eq!(outcome(&shown).0, 0, "{}", said(&shown));
    assert_eq!(shown.stdout, text.as_bytes());
}

#[test]
fn listing_passes_the_role_through_nesting() {
    let project = project();
    gated(&project, "hand-only", "# zaf mode: 700\n");
    action(
        &project,
        "lists",
        "",
        "zaf do\nzaf do --show hand-only\necho \"show=$?\"",
    );

    let run = zaf_as(&project, &["do", "lists"], Some(PROGRAMMER));
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let text = read(&log_of(&project, "lists"));
    assert!(text.contains("the lists action"), "{text}");
    assert!(!text.contains("the hand-only action"), "{text}");
    assert!(text.contains("show=125"), "{text}");

    for log in logs_of(&project, "lists") {
        fs::remove_file(log).expect("clear the log");
    }
    let run = zaf_as(&project, &["do", "lists"], None);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let text = read(&log_of(&project, "lists"));
    assert!(text.contains("the hand-only action"), "{text}");
    assert!(text.contains("show=0"), "{text}");
}

/// `--show` of `name` printed the file, byte for byte, and ran nothing.
fn assert_shown(project: &Project, name: &str, instance: Option<&str>) {
    let run = zaf_as(project, &["do", "--show", name], instance);
    assert_eq!(outcome(&run).0, 0, "{name} {instance:?}: {}", said(&run));
    let file = project.root.join(".zaf/actions").join(name);
    let bytes = fs::read(file).expect("the file");
    assert_eq!(run.stdout, bytes, "{name} {instance:?}");
    assert_nothing_started(project, name);
}

/// `--show` of `name` was refused the way a run is: one line, exit 125, none of the file.
fn assert_show_refused(project: &Project, name: &str, instance: Option<&str>, role: &str) {
    let run = zaf_as(project, &["do", "--show", name], instance);
    assert_refused(project, name, &run, role);
}

#[test]
fn show_needs_read() {
    let project = project();
    modes_to_read(&project);

    // Others read the third digit.
    let who = Some(PROGRAMMER);
    for name in ["open", "read-only", "team-shut"] {
        assert_shown(&project, name, who);
    }
    for name in [
        "hand-only",
        "team-reads",
        "run-only",
        "owner-shut",
        "write-only",
    ] {
        assert_show_refused(&project, name, who, "programmer");
    }

    // Neither bit stands in for the other: what cannot be read can run, and the reverse.
    assert_ran(
        &project,
        "run-only",
        &run_as(&project, "run-only", Some(PROGRAMMER)),
    );
    assert_refused(
        &project,
        "read-only",
        &run_as(&project, "read-only", Some(PROGRAMMER)),
        "programmer",
    );

    // The group reads the second, and is refused where others may read.
    let who = Some(TEAM);
    for name in ["open", "read-only", "team-reads"] {
        assert_shown(&project, name, who);
    }
    for name in ["team-shut", "hand-only", "run-only", "write-only"] {
        assert_show_refused(&project, name, who, "team");
    }

    // The Engineer reads the first, and a closed digit closes it for the Engineer too.
    for name in ["open", "hand-only", "run-only", "read-only", "write-only"] {
        assert_shown(&project, name, None);
    }
    assert_show_refused(&project, "owner-shut", None, "the Engineer");

    // A file whose mode cannot be told is not read by a role: nothing of it comes out, not
    // even why it does not parse. The Engineer, who repairs it, still can.
    for name in ["no-metadata", "bad-mode", "bad-line", "not-text"] {
        assert_show_refused(&project, name, Some(PROGRAMMER), "programmer");
        assert_show_refused(&project, name, Some(TEAM), "team");
        assert_shown(&project, name, None);
    }

    // The refusal is the same one in `--json`, and the file stays out of it.
    for args in [
        &["do", "--json", "--show", "hand-only"][..],
        &["--json", "do", "--show", "hand-only"],
    ] {
        let run = zaf_as(&project, args, Some(PROGRAMMER));
        let (code, stdout, stderr) = outcome(&run);
        assert_eq!(code, 125, "{args:?}: {stdout}{stderr}");
        let report: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON line");
        assert_eq!(report["result"], "NOT_RUN", "{args:?}");
        assert_eq!(
            report["meaning"], "not permitted for programmer",
            "{args:?}"
        );
        assert!(!stdout.contains("the hand-only action"), "{stdout}");
    }

    // A caller that cannot be told reads nothing, whatever the file's mode says.
    for value in ["", " ", "local", "team", "team instance 1 on #9\nengineer"] {
        for name in ["open", "hand-only"] {
            let run = zaf_as(&project, &["do", "--show", name], Some(value));
            let (code, stdout, stderr) = outcome(&run);
            assert_eq!(code, 125, "{value:?}: {stdout}{stderr}");
            assert_eq!(stdout, "", "{value:?}");
            let expected = format!(
                "NOT_RUN {name}: ZAF_INSTANCE is {value:?}, not `<role> instance <n> on #<epic>`\n"
            );
            assert_eq!(stderr, expected, "{value:?}");
        }
    }

    // A name no tier has reads the same for everyone.
    let run = zaf_as(&project, &["do", "--show", "ghost"], Some(PROGRAMMER));
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert_eq!(said(&run), "NOT_RUN ghost: no such action\n");
}

#[test]
fn show_does_not_fall_through_to_the_shadowed_file() {
    let project = project();
    machine_action(&project, "both", "echo machine body");
    gated(&project, "both", "# zaf mode: 700\n");

    assert_show_refused(&project, "both", Some(PROGRAMMER), "programmer");
    assert_eq!(listed_for(&project, Some(PROGRAMMER)), Vec::<String>::new());
    assert_shown(&project, "both", None);
}

/// A machine-tier action with its own metadata lines; its mark is never its project shadow's.
fn machine_gated(project: &Project, name: &str, metadata: &str) -> PathBuf {
    executable(
        &project.home.join("actions"),
        name,
        &action_text(name, metadata, &format!("touch {name}.machine-ran")),
    )
}

/// `name` is refused as the project's file shadowing the machine's: exit 125, a `NOT_RUN` line
/// naming both, and neither body started.
fn assert_unshadowable(project: &Project, name: &str, instance: Option<&str>, why: &str) {
    let run = run_as(project, name, instance);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 125, "{name}: {stdout}{stderr}");
    assert_eq!(stdout, "", "{name}");
    let file = real(&project.root).join(".zaf/actions").join(name);
    let over = project.home.join("actions").join(name);
    let start = format!(
        "NOT_RUN {name}: {} may not shadow {}: {why}",
        file.display(),
        over.display()
    );
    assert!(stderr.starts_with(&start), "{stderr}\nnot from\n{start}");
    assert_nothing_started(project, name);
    let machine_mark = project.root.join(format!("{name}.machine-ran"));
    assert!(
        !machine_mark.exists(),
        "{name}: fell through to the machine"
    );
}

#[test]
fn unshadowable_name_refuses_the_file() {
    let project = project();
    // Every owner digit without a w, whatever the other two say and the shadow declares.
    for owner in ["0", "1", "4", "5"] {
        for rest in ["00", "55", "77"] {
            let mode = format!("{owner}{rest}");
            let name = format!("locked-{mode}");
            machine_gated(&project, &name, &format!("# zaf mode: {mode}\n"));
            gated(&project, &name, "# zaf mode: 777\n");
            let why = format!("its mode {mode} has no owner w");
            for instance in [None, Some(TEAM), Some(PROGRAMMER)] {
                assert_unshadowable(&project, &name, instance, &why);
            }
        }
    }

    // The name is refused before the shadow is read, so a broken shadow changes nothing.
    machine_gated(&project, "locked-broken", "# zaf mode: 500\n");
    executable(
        &project.root.join(".zaf/actions"),
        "locked-broken",
        "#!/bin/sh\ntouch locked-broken.ran\n",
    );
    assert_unshadowable(
        &project,
        "locked-broken",
        None,
        "its mode 500 has no owner w",
    );

    // A machine file with no mode to read refuses the same way: the guard fails closed.
    executable(
        &project.home.join("actions"),
        "over-broken",
        "#!/bin/sh\ntouch over-broken.machine-ran\n",
    );
    gated(&project, "over-broken", "");
    assert_unshadowable(&project, "over-broken", None, "it has no mode:");

    // `--json` says the same, and names the tier of the refused file.
    let run = zaf(&project, &["do", "--json", "locked-500"]);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 125, "{stdout}{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON line");
    assert_eq!(report["result"], "NOT_RUN");
    assert_eq!(report["exit"], 125);
    assert_eq!(report["tier"], "project");
    let meaning = report["meaning"].as_str().expect("a reason");
    assert!(meaning.contains(".zaf/actions/locked-500"), "{meaning}");
    assert!(
        meaning.ends_with("its mode 500 has no owner w"),
        "{meaning}"
    );

    // A w opens it: the shadow is the file decided on, and the machine's body never runs.
    for owner in ["2", "3", "6", "7"] {
        let name = format!("open-{owner}");
        machine_gated(&project, &name, &format!("# zaf mode: {owner}00\n"));
        gated(&project, &name, "# zaf mode: 777\n");
        let run = run_as(&project, &name, None);
        if owner == "3" || owner == "7" {
            assert_ran(&project, &name, &run);
        } else {
            assert_refused(&project, &name, &run, "the Engineer");
        }
        assert!(!project.root.join(format!("{name}.machine-ran")).exists());
    }

    // No shadow, no refusal: the locked file itself still runs.
    machine_gated(&project, "locked-alone", "# zaf mode: 500\n");
    let run = run_as(&project, "locked-alone", None);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(project.root.join("locked-alone.machine-ran").exists());
}

#[test]
fn a_file_is_not_its_own_shadow() {
    let project = project();
    // A project at the parent of the zaf home has one actions folder for both tiers.
    gated(&project, "one-folder", "# zaf mode: 500\n");
    let home = project.root.join(".zaf");
    let home = home.to_str().expect("a text path");
    let run = zaf_in(
        &project,
        &project.root,
        &["do", "one-folder"],
        &[("ZAF_HOME", home)],
    );
    assert_ran(&project, "one-folder", &run);

    // A link to the machine's file runs that very file.
    let file = machine_gated(&project, "linked", "# zaf mode: 500\n");
    std::os::unix::fs::symlink(&file, project.root.join(".zaf/actions/linked")).expect("link");
    let run = run_as(&project, "linked", None);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(project.root.join("linked.machine-ran").exists());
}

#[test]
fn shadowing_never_widens() {
    let project = project();
    let group = "# zaf group: team\n";
    // Each subject's run bit, in every pairing: it runs only when both files say x.
    for (over, own) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        let cases = [
            (
                "owner",
                format!("{}00", 6 + over),
                format!("{}00", 6 + own),
                None,
                "the Engineer",
            ),
            (
                "group",
                format!("7{over}0"),
                format!("7{own}0"),
                Some(TEAM),
                "team",
            ),
            (
                "others",
                format!("70{over}"),
                format!("70{own}"),
                Some(PROGRAMMER),
                "programmer",
            ),
        ];
        for (subject, over_mode, own_mode, instance, role) in cases {
            let name = format!("{subject}-{over}-{own}");
            machine_gated(
                &project,
                &name,
                &format!("# zaf mode: {over_mode}\n{group}"),
            );
            gated(&project, &name, &format!("# zaf mode: {own_mode}\n{group}"));
            let run = run_as(&project, &name, instance);
            if over == 1 && own == 1 {
                assert_ran(&project, &name, &run);
            } else {
                assert_refused(&project, &name, &run, role);
            }
        }
    }

    // Whatever mode it declares: no `mode` line is 755, and it still gets no wider.
    machine_gated(&project, "undeclared", "# zaf mode: 700\n");
    gated(&project, "undeclared", "");
    let run = run_as(&project, "undeclared", Some(PROGRAMMER));
    assert_refused(&project, "undeclared", &run, "programmer");
    let run = run_as(&project, "undeclared", None);
    assert_ran(&project, "undeclared", &run);

    // A shadow cannot widen by naming a role either: the machine's file sees it as others.
    machine_gated(&project, "added", "# zaf mode: 750\n# zaf group: team\n");
    gated(
        &project,
        "added",
        "# zaf mode: 777\n# zaf group: team, programmer\n",
    );
    let run = run_as(&project, "added", Some(PROGRAMMER));
    assert_refused(&project, "added", &run, "programmer");
    let run = run_as(&project, "added", Some("auditor instance 1 on #9"));
    assert_refused(&project, "added", &run, "auditor");
    let run = run_as(&project, "added", Some(TEAM));
    assert_ran(&project, "added", &run);

    // Swapping the group opens it to neither: each role is others in one of the two files.
    machine_gated(&project, "swapped", "# zaf mode: 750\n# zaf group: team\n");
    gated(
        &project,
        "swapped",
        "# zaf mode: 750\n# zaf group: release\n",
    );
    for (instance, role) in [(TEAM, "team"), ("release instance 1 on #9", "release")] {
        let run = run_as(&project, "swapped", Some(instance));
        assert_refused(&project, "swapped", &run, role);
    }
    let run = run_as(&project, "swapped", None);
    assert_ran(&project, "swapped", &run);

    // Narrowing is the shadow's to do.
    machine_gated(&project, "narrowed", "");
    gated(&project, "narrowed", "# zaf mode: 700\n");
    let run = run_as(&project, "narrowed", Some(PROGRAMMER));
    assert_refused(&project, "narrowed", &run, "programmer");
    let run = run_as(&project, "narrowed", None);
    assert_ran(&project, "narrowed", &run);
}

/// The run of `zaf do --tier machine <name>` by `instance`, or by hand with none.
fn run_machine_as(project: &Project, name: &str, instance: Option<&str>) -> Output {
    let env: Vec<(&str, &str)> = instance.map(|i| ("ZAF_INSTANCE", i)).into_iter().collect();
    zaf_in(
        project,
        &project.root,
        &["do", "--tier", "machine", name],
        &env,
    )
}

/// The machine's body of `name` left its mark; clears it.
fn take_machine_mark(project: &Project, name: &str) -> bool {
    let mark = project.root.join(format!("{name}.machine-ran"));
    let ran = mark.exists();
    if ran {
        fs::remove_file(mark).expect("clear the mark");
    }
    ran
}

#[test]
fn override_reaches_the_machine_file() {
    let project = project();
    // The project's `x` does more than the machine's, and reaches it by name: `zaf do x` inside
    // it would resolve to itself.
    machine_gated_exiting(&project, "x", 1);
    action(
        &project,
        "x",
        "",
        "touch x.project-ran\nzaf do --tier machine x\nexit $?",
    );

    let run = zaf(&project, &["do", "x"]);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(
        code, 1,
        "the machine file's code passes through: {stdout}{stderr}"
    );
    assert!(project.root.join("x.project-ran").exists());
    assert!(take_machine_mark(&project, "x"), "the machine's body ran");

    // The nested run logs as any run does, and its path is in the outer log.
    let logs = logs_of(&project, "x");
    assert_eq!(logs.len(), 2, "{logs:?}");
    let outer = logs
        .iter()
        .find(|log| read(log).contains("zaf do x: log"))
        .expect("the outer log names the inner");
    let inner = logs
        .iter()
        .find(|log| *log != outer)
        .expect("the inner log");
    assert!(
        read(outer).contains(&inner.display().to_string()),
        "{}",
        read(outer)
    );

    // Straight from the command line too: the machine's file, not the project's, and its code.
    fs::remove_file(project.root.join("x.project-ran")).expect("clear");
    let run = zaf(&project, &["do", "--tier", "machine", "x"]);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stderr.starts_with("FAIL x: exit 1, it failed, log "),
        "{stderr}"
    );
    assert!(take_machine_mark(&project, "x"));
    assert!(!project.root.join("x.project-ran").exists());

    // `--json` names the machine's tier; an exit 0 prints nothing.
    machine_gated_exiting(&project, "y", 0);
    action(&project, "y", "", "touch y.project-ran");
    let run = zaf(&project, &["do", "--tier", "machine", "y"]);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
    assert!(take_machine_mark(&project, "y"));
    let run = zaf(&project, &["do", "--json", "--tier", "machine", "y"]);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON line");
    assert_eq!(report["tier"], "machine");
    assert!(take_machine_mark(&project, "y"));
    assert!(!project.root.join("y.project-ran").exists());

    // `--tier=machine` is the same flag, and it comes before the name like every flag.
    let run = zaf(&project, &["do", "--tier=machine", "y"]);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(take_machine_mark(&project, "y"));
    let run = zaf(&project, &["do", "y", "--tier", "machine"]);
    assert_eq!(
        outcome(&run).0,
        0,
        "after the name it is the body's: {}",
        said(&run)
    );
    assert!(!take_machine_mark(&project, "y"));
}

/// A machine-tier action that leaves `name.machine-ran` and exits `code`.
fn machine_gated_exiting(project: &Project, name: &str, code: u8) -> PathBuf {
    executable(
        &project.home.join("actions"),
        name,
        &action_text(name, "", &format!("touch {name}.machine-ran\nexit {code}")),
    )
}

#[test]
fn tier_machine_without_a_machine_file_is_missing() {
    let project = project();
    action(&project, "only-here", "", "touch only-here.ran");

    // As a name no tier provides, though the project has it.
    let run = zaf(&project, &["do", "--tier", "machine", "only-here"]);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 125, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    assert_eq!(stderr, "NOT_RUN only-here: no such action\n");
    let ghost = zaf(&project, &["do", "ghost"]);
    assert_eq!(outcome(&ghost).0, 125);
    assert_eq!(said(&ghost), "NOT_RUN ghost: no such action\n");
    assert_nothing_started(&project, "only-here");

    // The same through `--json`: no tier, no log.
    let run = zaf(
        &project,
        &["do", "--json", "--tier", "machine", "only-here"],
    );
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 125, "{stdout}{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON line");
    assert_eq!(report["result"], "NOT_RUN");
    assert_eq!(report["tier"], serde_json::Value::Null);
    assert_eq!(report["log"], serde_json::Value::Null);

    // A bad name is as bad as it is without the flag.
    let run = zaf(&project, &["do", "--tier", "machine", "../ZAF.toml"]);
    assert_eq!(outcome(&run).0, 125);
    assert!(
        said(&run).starts_with("NOT_RUN ../ZAF.toml: bad action name"),
        "{}",
        said(&run)
    );

    // Called from a body, the refusal is a nested run that did not run.
    action(
        &project,
        "caller",
        "",
        "zaf do --tier machine only-here\nexit $?",
    );
    let run = zaf(&project, &["do", "caller"]);
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert!(
        said(&run).contains("NOT_RUN only-here: no such action"),
        "{}",
        said(&run)
    );
    assert_nothing_started(&project, "only-here");
}

#[test]
fn other_tier_value_is_malformed() {
    let project = project();
    action(&project, "x", "", "touch x.ran");
    machine_gated_exiting(&project, "x", 0);

    for args in [
        &["do", "--tier", "project", "x"][..],
        &["do", "--tier", "Machine", "x"],
        &["do", "--tier", "machines", "x"],
        &["do", "--tier", "", "x"],
        &["do", "--tier=all", "x"],
        &["do", "--tier"],
        &["do", "--tier", "x"],
        &["do", "--tier", "machine"],
        &["do", "--tier", "machine", "--show"],
    ] {
        let run = zaf(&project, args);
        let (code, stdout, stderr) = outcome(&run);
        assert_eq!(code, 2, "{args:?}: {stdout}{stderr}");
        assert_eq!(stdout, "", "{args:?}");
        assert_nothing_started(&project, "x");
        assert!(!take_machine_mark(&project, "x"), "{args:?}");
    }
    assert!(files_in(&real(&project.root).join(".zaf/logs")).is_empty());
}

#[test]
fn tier_flag_counts_depth() {
    let project = project();
    executable(
        &project.home.join("actions"),
        "deep",
        &action_text(
            "deep",
            "",
            "echo \"$ZAF_DEPTH\" >> \"$ZAF_ROOT/depths\"\nzaf do --tier machine deep\nexit $?",
        ),
    );
    // The project's file is the way in; every later call goes past it.
    action(&project, "deep", "", "zaf do --tier machine deep\nexit $?");

    let run = zaf(&project, &["do", "deep"]);
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert_eq!(
        read(&project.root.join("depths")),
        "1\n2\n3\n4\n5\n6\n7\n",
        "the project's call is depth 0, and the flag's calls count on from it"
    );
    let chain = ["deep"; 9].join(" > ");
    let refused = format!("NOT_RUN deep: depth 8 reached: {chain}");
    assert!(
        logs_of(&project, "deep")
            .iter()
            .any(|log| read(log).contains(&refused)),
        "the innermost log names the chain"
    );
    assert_eq!(
        logs_of(&project, "deep").len(),
        8,
        "every run that started logged"
    );

    // Called directly from such a depth, nothing starts and nothing is logged.
    machine_gated_exiting(&project, "plain", 0);
    let run = zaf_in(
        &project,
        &project.root,
        &["do", "--tier", "machine", "plain"],
        &[("ZAF_DEPTH", "8"), ("ZAF_ACTION_CHAIN", "first > second")],
    );
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert_eq!(
        said(&run),
        "NOT_RUN plain: depth 8 reached: first > second > plain\n"
    );
    assert!(!take_machine_mark(&project, "plain"));
    assert!(logs_of(&project, "plain").is_empty());
}

#[test]
fn tier_flag_keeps_the_mode_check() {
    let project = project();
    machine_gated(&project, "gated", "# zaf mode: 750\n# zaf group: team\n");
    gated(&project, "gated", "# zaf mode: 755\n");
    let env = [("ZAF_INSTANCE", PROGRAMMER)];

    // No way of writing the flag, or the other flags around it, gets past the mode.
    for args in [
        &["do", "--tier", "machine", "gated"][..],
        &["do", "--tier=machine", "gated"],
        &["do", "--more", "--tier", "machine", "gated"],
        &["--more", "do", "--tier", "machine", "gated"],
        &["do", "--tier", "machine", "--", "gated"],
        &["do", "--tier", "machine", "gated", "--json"],
        &["do", "--tier", "machine", "gated", "--show"],
    ] {
        let run = zaf_in(&project, &project.root, args, &env);
        assert_refused(&project, "gated", &run, "programmer");
        assert!(!take_machine_mark(&project, "gated"), "{args:?}");
    }

    let run = zaf_in(
        &project,
        &project.root,
        &["do", "--json", "--tier", "machine", "gated"],
        &env,
    );
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 125, "{stdout}{stderr}");
    let report: serde_json::Value = serde_json::from_str(&stdout).expect("one JSON line");
    assert_eq!(report["result"], "NOT_RUN");
    assert_eq!(report["meaning"], "not permitted for programmer");
    assert_eq!(report["tier"], "machine");
    assert_eq!(report["log"], serde_json::Value::Null);

    // `--show` reads the machine's file under its read bit, as it reads any.
    let run = zaf_in(
        &project,
        &project.root,
        &["do", "--show", "--tier", "machine", "gated"],
        &env,
    );
    assert_refused(&project, "gated", &run, "programmer");
    let run = zaf(&project, &["do", "--show", "--tier", "machine", "gated"]);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("touch gated.machine-ran"), "{stdout}");

    // The role is the same through a nested call, and the mode still decides there.
    action(
        &project,
        "relay",
        "",
        "zaf do --tier machine gated\nexit $?",
    );
    let run = zaf_in(&project, &project.root, &["do", "relay"], &env);
    assert_eq!(outcome(&run).0, 125, "{}", said(&run));
    assert!(
        said(&run).contains("NOT_RUN gated: not permitted for programmer"),
        "{}",
        said(&run)
    );
    assert!(!take_machine_mark(&project, "gated"));

    // The same calls open for a caller the mode permits, so the mode is what closed them.
    for instance in [Some(TEAM), None] {
        let run = run_machine_as(&project, "gated", instance);
        assert_eq!(outcome(&run).0, 0, "{instance:?}: {}", said(&run));
        assert!(take_machine_mark(&project, "gated"), "{instance:?}");
    }
}

#[test]
fn tier_machine_obeys_the_mode() {
    let project = project();
    // The project's file is open to all and the machine's is not: the flag reaches past the
    // project's file, and what it reaches is held to its own mode.
    for (name, mode, role, instance) in [
        ("hand-only", "700", "programmer", PROGRAMMER),
        ("hand-only", "700", "team", TEAM),
        ("team-only", "710", "programmer", PROGRAMMER),
        ("closed", "000", "the Engineer", ""),
    ] {
        machine_gated(
            &project,
            name,
            &format!("# zaf mode: {mode}\n# zaf group: team\n"),
        );
        gated(&project, name, "# zaf mode: 777\n");
        let instance = (!instance.is_empty()).then_some(instance);
        let run = run_machine_as(&project, name, instance);
        assert_refused(&project, name, &run, role);
        assert!(!take_machine_mark(&project, name), "{name} for {role}");
        assert!(
            !project.root.join(format!("{name}.ran")).exists(),
            "{name}: the project's file did not run in its place"
        );
    }

    // The one that is permitted runs the machine's file and nothing else.
    let run = run_machine_as(&project, "hand-only", None);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(take_machine_mark(&project, "hand-only"));
    let run = run_machine_as(&project, "team-only", Some(TEAM));
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(take_machine_mark(&project, "team-only"));
    assert!(!project.root.join("hand-only.ran").exists());
    assert!(!project.root.join("team-only.ran").exists());
}

/// Task 2 of epic 1, as `gh issue view` prints it.
const TASK_TWO: &str = r#"{"number":2,"title":"Export the list","body":"","labels":[{"name":"task"},{"name":"ready"}],"parent":{"number":1},"state":"OPEN","comments":[]}"#;

/// What a caller could try to hand a slice action in place of zaf's own values.
const FORGED: [(&str, &str); 4] = [
    ("ZAF_SLICE", "feature/evil"),
    ("ZAF_PR", "99"),
    ("ZAF_TASK_TITLE", "forged title"),
    ("ZAF_BASE", "feature/forged"),
];

/// A project whose fake `gh` knows task 2 (or fails on it), the caller's environment forged, and
/// an action `slice-thing` of the given target that writes its environment.
fn slice_project(target: &str, board_fails: bool) -> Project {
    let mut project = Project::new(MANIFEST);
    if board_fails {
        project.fail_on("issue view 2 *", "HTTP 502");
    } else {
        project.on("issue view 2 *", TASK_TWO);
    }
    for (name, value) in FORGED {
        project.env(name, value);
    }
    project.env("ZAF_VERSION", "v1");
    set_up(&project);
    action(
        &project,
        "slice-thing",
        target,
        "env > \"$ZAF_ROOT/env\"\n: > \"$ZAF_ROOT/ran\"",
    );
    project
}

#[test]
fn do_target_task_replaces_the_callers_slice_env() {
    let project = slice_project("# zaf target: task\n", false);

    // No record: zaf's slice and title replace the forged ones, and the forged pull request goes.
    let run = project.zaf(&["do", "slice-thing", "2"]);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let env = env_lines(&read(&project.root.join("env")));
    assert_eq!(env["ZAF_SLICE"], "feature/v1-1-2");
    assert_eq!(env["ZAF_TASK_TITLE"], "Export the list");
    assert_eq!(env["ZAF_BASE"], "feature/v1");
    assert!(!env.contains_key("ZAF_PR"), "{env:?}");

    // A record: its pull request replaces the forged one.
    write(&project.root, ".zaf/slices/2.pr", "7");
    let run = project.zaf(&["do", "slice-thing", "2"]);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let env = env_lines(&read(&project.root.join("env")));
    assert_eq!(env["ZAF_PR"], "7");
    assert_eq!(env["ZAF_SLICE"], "feature/v1-1-2");
}

#[test]
fn do_targetless_run_gets_only_the_base() {
    let project = slice_project("", false);

    let run = project.zaf(&["do", "slice-thing", "2"]);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    let env = env_lines(&read(&project.root.join("env")));
    for name in ["ZAF_SLICE", "ZAF_PR", "ZAF_TASK_TITLE"] {
        assert!(!env.contains_key(name), "{name} in {env:?}");
    }
    assert_eq!(env["ZAF_BASE"], "feature/v1");
}

#[test]
fn do_board_failure_is_not_run() {
    let project = slice_project("# zaf target: task\n", true);

    let run = project.zaf(&["do", "slice-thing", "2"]);
    let (code, stdout, stderr) = outcome(&run);
    assert_eq!(code, 125, "{stdout}{stderr}");
    assert!(stderr.contains("NOT_RUN"), "{stderr}");
    assert!(!project.root.join("ran").exists());

    // A run with no target does not read the board, so the failing board stops nothing.
    let plain = slice_project("", true);
    let run = plain.zaf(&["do", "slice-thing", "2"]);
    assert_eq!(outcome(&run).0, 0, "{}", said(&run));
    assert!(plain.root.join("ran").exists());
}
