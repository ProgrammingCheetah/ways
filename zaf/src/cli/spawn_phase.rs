//! `zaf spawn phase <task> <action>` and the internal `zaf spawn phase-run <run-folder>`.
//!
//! `phase` checks that the action is the task's current phase and that none of its phases runs,
//! writes the phase run folder and starts a detached `zaf spawn phase-run` on it, then returns at
//! once: exit `0` once the launcher is started, `1` when the state refuses it, `2` when it cannot
//! start. `phase-run` is the launcher: not in the help, it runs the action as its child through
//! `zaf do` from the project root with `ZAF_RUN` (the run folder), `ZAF_SIZE` (the task's size)
//! and `ZAF_INSTANCE` (`<action> instance <n> on #<epic>`) set, records the child's process in
//! the run folder's `pid`, waits on it, records its exit code in the run folder's `exit` file (a
//! signal `n` records `128 + n`) as its first step once it has ended, and exits with that code.
//! It serves a model action and a wait alike. The post-phase hook and the tick are later steps.

use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Cli;
use super::exit::{self, BACKEND, OK, REFUSED};
use crate::adapters::process;
use crate::app::actions::{ACTION_VAR, CHAIN_VAR, DEPTH_VAR, LOG_VAR};
use crate::app::spawn::phase::{self, PhaseError, Ports, Started};
use crate::composition::{Composition, STATE_DIR, zaf_home};
use crate::domain::TicketId;
use crate::ports::{Holder, INSTANCE_VAR, RUN_VAR, SIZE_VAR};

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
    let ports = Ports {
        board: board.as_ref(),
        forge: forge.as_ref(),
        manifest: composition.manifest(),
        root,
        home: &home,
        state_dir: &state_dir,
        alive: &process::alive,
    };
    let launch = |started: &Started| detach(root, &started.run_dir);
    match phase::start(&ports, task, action, &launch) {
        Ok(started) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "task": task.get(),
                        "action": action,
                        "instance": started.instance,
                        "run": started.run_dir,
                    })
                );
            } else {
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
    let exe = std::env::current_exe()
        .map_err(|error| format!("cannot find the zaf binary to start the launcher: {error}"))?;
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
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            eprintln!("zaf {path}: cannot find the zaf binary: {error}");
            return BACKEND;
        }
    };
    let mut command = Command::new(exe);
    command
        .arg("do")
        .arg(launch.action.as_str())
        .arg(launch.task.get().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env(RUN_VAR, &run_dir)
        .env(SIZE_VAR, &launch.size)
        .env(INSTANCE_VAR, &launch.instance);
    clear_action_state(&mut command);
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
            // The first step once the child has ended, before post-phase (#439) or a tick (#440).
            if let Err(reason) = phase::end(&run_dir, code, &|| {}) {
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
