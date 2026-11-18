//! `zaf spawn phase <task> <action>` and the internal `zaf spawn phase-run <run-folder>`.
//!
//! `phase` checks, through the tick's own gate, that the task may start the action now (it is the
//! task's current phase, none of its phases runs, nothing holds it waiting, and it is not getting
//! nowhere), releases the task's claim when the instance holding it has ended (one `released #4
//! from <instance>` line), writes the phase run folder and starts a detached `zaf spawn phase-run`
//! on it, then returns at once: exit `0` once the launcher is started, `1` when the state refuses
//! it, `2` when it cannot start. `phase-run` is the launcher: not in the help, it runs the action as its child through
//! `zaf do` from the project root with `ZAF_RUN` (the run folder), `ZAF_SIZE` (the task's size)
//! and `ZAF_INSTANCE` (`<action> instance <n> on #<epic>`) set, records the child's process in
//! the run folder's `pid`, waits on it, and as its first step once it has ended reads the head of
//! the task's pull request and records it in the run folder's `head-end` (empty when the task has
//! no pull request, no file when the head cannot be read), then its exit code in the `exit` file,
//! last (a signal `n` records `128 + n`), then runs the `post-phase`
//! hook if there is one, told `{"task", "action", "exit"}` on stdin (a hook that fails is logged in
//! the run folder's `post-phase.log` and changes nothing), then asks for one tick: a line in the
//! running scheduler's request file, or, with none running, `zaf spawn --once --no-wait` (a tick
//! that fails changes nothing; a scheduler that cannot be asked is named in `post-phase.log`, and
//! no tick is asked for), and exits with the child's code. It serves a model action and a wait
//! alike. Every zaf it starts, the launcher and `zaf do` and the tick, is started from the one
//! path to start zaf with ([`binary::path_to_start`]).

use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Cli;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::slice::VERSION_VAR;
use crate::adapters::process;
use crate::app::actions::{
    ACTION_VAR, CHAIN_VAR, Caller, DEPTH_VAR, HOOKS_DIR, LOG_VAR, MINUTE, Nesting,
    RunRequest as ActionRequest, RunResult, Terminal, hook, last_lines, run_hook_with_input,
};
use crate::app::schedule::{binary, record, request};
use crate::app::spawn::phase::{self, PhaseError, Ports, Started};
use crate::composition::{Composition, STATE_DIR, zaf_home};
use crate::domain::TicketId;
use crate::domain::schedule::HeadEnd;
use crate::ports::{Holder, INSTANCE_VAR, RUN_VAR, SIZE_VAR};

/// The lines of a failed hook's log that its run-folder log keeps.
const HOOK_TAIL_LINES: usize = 10;

/// Starts `action` as `task`'s phase and returns once its launcher is started.
pub fn run(cli: &Cli, task: TicketId, action: &str) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let forge = match composition.forge() {
        Ok(forge) => forge,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let root = composition.root();
    let state_dir = composition.state_dir();
    // With no home there is no machine tier, so the project's actions alone are looked for.
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        _ => root.join(STATE_DIR),
    };
    let version = std::env::var(VERSION_VAR).ok();
    let ports = Ports {
        board: board.as_ref(),
        forge: forge.as_ref(),
        manifest: composition.manifest(),
        root,
        home: &home,
        state_dir: &state_dir,
        alive: &process::alive,
        version: version.as_deref(),
    };
    let launch = |started: &Started| detach(root, &started.run_dir);
    match phase::start(&ports, task, action, &launch) {
        Ok(started) => {
            if cli.json {
                let released: Vec<serde_json::Value> = started
                    .released
                    .iter()
                    .map(|released| {
                        serde_json::json!({
                            "task": released.task.get(),
                            "instance": released.instance.to_string(),
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::json!({
                        "task": task.get(),
                        "action": action,
                        "instance": started.instance,
                        "run": started.run_dir,
                        "released": released,
                    })
                );
            } else {
                for released in &started.released {
                    println!("released {} from {}", released.task, released.instance);
                }
                println!(
                    "started {} ({})",
                    started.instance,
                    started.run_dir.display()
                );
            }
            OK
        }
        Err(PhaseError::Refused(reason)) => {
            eprintln!("zaf {path}: {reason}");
            REFUSED
        }
        Err(PhaseError::Failed(reason)) => {
            eprintln!("zaf {path}: {reason}");
            BACKEND
        }
    }
}

/// Starts `zaf spawn phase-run <run_dir>` as a process of its own: in its own process group, with
/// no terminal, outliving this one. Returns it as a holder, without waiting on it.
fn detach(root: &Path, run_dir: &Path) -> Result<Holder, String> {
    let exe = binary::path_to_start().map_err(|why| format!("{why}, to start the launcher"))?;
    let mut command = Command::new(exe);
    command
        .args(["spawn", "phase-run"])
        .arg(run_dir)
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env(RUN_VAR, run_dir)
        .process_group(0);
    clear_action_state(&mut command);
    #[allow(
        clippy::zombie_processes,
        reason = "the launcher outlives this verb on purpose"
    )]
    let child = command
        .spawn()
        .map_err(|error| format!("cannot start the launcher: {error}"))?;
    process::holder_of(child.id()).ok_or_else(|| "cannot read the launcher's start time".to_owned())
}

/// A phase is not nested in the action that started it: it carries none of that action's state.
fn clear_action_state(command: &mut Command) {
    for var in [ACTION_VAR, DEPTH_VAR, CHAIN_VAR, LOG_VAR] {
        command.env_remove(var);
    }
}

/// Runs the `post-phase` hook, if there is one, once the exit is recorded (ruling 21): it is told
/// the task, the action and the exit code on stdin. A hook that fails is logged in the run folder
/// and changes nothing else; no hook file, nothing runs. The launcher's exit stays the child's.
fn post_phase(path: &str, run_dir: &Path, launch: &phase::Launch, code: i32) {
    let Ok(composition) = Composition::discover_here() else {
        return;
    };
    let root = composition.root();
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        // With no home there is no machine tier, so the project's hook alone is looked for.
        _ => root.join(STATE_DIR),
    };
    if hook(root, &home, "post-phase").is_none() {
        return;
    }
    let name = format!("{HOOKS_DIR}/post-phase");
    let failure = |text: String| {
        if let Err(reason) = phase::record_hook_failure(run_dir, &text) {
            eprintln!("zaf {path}: {reason}");
        }
    };
    let nesting = match Nesting::from_env() {
        Ok(nesting) => nesting,
        Err(reason) => return failure(format!("NOT_RUN {name}: {reason}\n")),
    };
    let caller = match Caller::from_env() {
        Ok(caller) => caller,
        Err(reason) => return failure(format!("NOT_RUN {name}: {reason}\n")),
    };
    let version = std::env::var(VERSION_VAR).ok();
    let env = match composition.manifest().env.resolve(root, version.as_deref()) {
        Ok(env) => env,
        Err(error) => return failure(format!("NOT_RUN {name}: {error}\n")),
    };
    let input = phase::post_phase_input(launch.task, &launch.action, code);
    let outcome = run_hook_with_input(
        &ActionRequest {
            name: &name,
            args: &[],
            cwd: root,
            project_root: Some(root),
            home: &home,
            timeout_minutes: composition.manifest().actions.timeout_minutes,
            minute: MINUTE,
            nesting,
            caller,
            only: None,
            slice: None,
            env,
            terminal: Terminal::Absent,
            run: None,
            phase: None,
        },
        Some(&input),
    );
    if outcome.result == RunResult::Ok {
        return;
    }
    let mut text = outcome
        .headline()
        .unwrap_or_else(|| format!("{name} failed"));
    text.push('\n');
    if let Some(log) = &outcome.log
        && let Ok(lines) = last_lines(log, HOOK_TAIL_LINES)
    {
        for line in lines {
            text.push_str(&line);
            text.push('\n');
        }
    }
    failure(text);
}

/// Asks for one tick once the phase has ended and `post-phase` has run (#421 ruling 25), so budget
/// the phase freed is reused at once. While a scheduler runs for the project (`zaf spawn --watch`),
/// the ask is one line in its request file, `phase-end <action> <task>`, and nothing waits; a
/// request file that cannot be written asks for no tick, and the run folder's `post-phase.log`
/// says so, naming the scheduler (the scheduler alone ticks while it runs, and its timer still
/// does). With no scheduler it runs `<zaf> spawn --once --no-wait` from the project root and waits
/// for it: that does nothing, with exit 0, when another tick holds `spawn.lock`. The milestone is
/// passed on as `--version` from `ZAF_VERSION`, which the tick needs. A tick that fails is on
/// stderr and changes nothing else: the launcher's exit stays the child's.
fn ask_for_tick(path: &str, exe: &Path, launch: &phase::Launch, run_dir: &Path) {
    if let Ok(composition) = Composition::discover_here() {
        let who = format!("phase-end {} {}", launch.action, launch.task);
        match request::ask(
            &composition.state_dir(),
            &who,
            std::time::SystemTime::now(),
            &process::alive,
        ) {
            Ok(Some(_)) => return,
            Ok(None) => {}
            // A tick here would only ask the scheduler again: none is asked for, and the run
            // folder, which outlives this launcher, says so.
            Err(not_asked) => {
                if let Err(reason) = phase::log_after_phase(run_dir, &format!("{not_asked}\n")) {
                    eprintln!("zaf {path}: {reason}");
                }
                return;
            }
        }
    }
    // The launcher was started from the project root (`detach`), and the tick runs from there too.
    let mut command = Command::new(exe);
    command.args(["spawn", "--once", "--no-wait"]);
    if let Ok(version) = std::env::var(VERSION_VAR) {
        command.args(["--version", &version]);
    }
    command.stdin(Stdio::null()).stdout(Stdio::null());
    clear_action_state(&mut command);
    // The tick is no part of the run that asked for it: it carries no `ZAF_RUN`, so the machine
    // read never counts it as the ended run's instance.
    command.env_remove(RUN_VAR);
    match command.status() {
        Ok(status) if status.success() => {}
        Ok(status) => {
            let code = status
                .code()
                .map_or_else(|| "a signal".to_owned(), |code| format!("exit {code}"));
            eprintln!("zaf {path}: the tick ended with {code}");
        }
        Err(error) => eprintln!("zaf {path}: cannot run the tick: {error}"),
    }
}

/// The run folder's `output` file, opened twice for the child's stdout and stderr; none when it
/// cannot be made, and the output is then dropped.
fn open_output(run_dir: &Path) -> Option<(Stdio, Stdio)> {
    let file = std::fs::File::create(run_dir.join(phase::OUTPUT_FILE)).ok()?;
    let twin = file.try_clone().ok()?;
    Some((Stdio::from(file), Stdio::from(twin)))
}

/// The head of `task`'s pull request now, read by the launcher once its child has ended: the head
/// the phase left ([`record::head_now`]). No pull request recorded is none; a project or a forge
/// that cannot be built, or a forge that cannot say, leaves it unknown.
fn head_left(task: TicketId) -> HeadEnd {
    let Ok(composition) = Composition::discover_here() else {
        return HeadEnd::Unknown;
    };
    let forge = composition.forge().ok();
    record::head_now(forge.as_deref(), composition.root(), task)
}

/// The launcher: runs the action of the phase run folder `run_dir` and waits for it.
pub fn launcher(cli: &Cli, run_dir: &Path) -> u8 {
    let path = cli.verb.path();
    let run_dir: PathBuf = match std::path::absolute(run_dir) {
        Ok(run_dir) => run_dir,
        Err(error) => {
            eprintln!("zaf {path}: {}: {error}", run_dir.display());
            return BACKEND;
        }
    };
    let launch = match phase::launch_of(&run_dir) {
        Ok(launch) => launch,
        Err(reason) => {
            eprintln!("zaf {path}: {reason}");
            return BACKEND;
        }
    };
    // The one path to start zaf with: a binary replaced on disk is started as the file now there.
    let exe = match binary::path_to_start() {
        Ok(exe) => exe,
        Err(why) => {
            eprintln!("zaf {path}: {why}");
            return BACKEND;
        }
    };
    let mut command = Command::new(&exe);
    command
        .arg("do")
        .arg(launch.action.as_str())
        .arg(launch.task.get().to_string())
        .stdin(Stdio::null())
        .env(RUN_VAR, &run_dir)
        .env(SIZE_VAR, &launch.size)
        .env(INSTANCE_VAR, &launch.instance);
    clear_action_state(&mut command);
    // What `zaf do` prints, a failure's headline and the tail of its log, goes to the run folder's
    // `output`, so the tick can quote the last line when the phase keeps failing (#550).
    match open_output(&run_dir) {
        Some((out, err)) => command.stdout(out).stderr(err),
        None => command.stdout(Stdio::null()).stderr(Stdio::null()),
    };
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            eprintln!("zaf {path}: cannot start {}: {error}", launch.action);
            return BACKEND;
        }
    };
    // The child may already be gone: then there is no process to record, and nothing to wait for.
    if let Some(holder) = process::holder_of(child.id())
        && let Err(reason) = phase::record_pid(&run_dir, &holder)
    {
        eprintln!("zaf {path}: {reason}");
    }
    match child.wait() {
        Ok(status) => {
            let code = phase::exit_code(status.code(), status.signal());
            // Read once the child has ended, so it is the head the phase left.
            let head_end = head_left(launch.task);
            // The head and the exit first, then the post-phase hook, then the ask for a tick
            // (ruling 25).
            let after = || {
                post_phase(&path, &run_dir, &launch, code);
                ask_for_tick(&path, &exe, &launch, &run_dir);
            };
            if let Err(reason) = phase::end(&run_dir, code, &head_end, &after) {
                eprintln!("zaf {path}: {reason}");
            }
            u8::try_from(code).unwrap_or(BACKEND)
        }
        Err(error) => {
            eprintln!("zaf {path}: cannot wait on {}: {error}", launch.action);
            BACKEND
        }
    }
}
