//! `zaf spawn --once --version <v>`: one tick of the spawner.
//!
//! The verb never loops: a timer runs it. It prints the plan, then runs each
//! planned launch to its exit, one after another, and exits `2` when a launch
//! could not be recorded or its runner could not run.

use std::process::{Command, Stdio};

use super::Cli;
use super::board::INSTANCE_VAR;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::slice::VERSION_VAR;
use crate::app::spawn::{self, LaunchContext, LaunchExit, Launched, SpawnError};
use crate::composition::Composition;
use crate::domain::Instance;
use crate::manifest::RunnerName;

/// The runner every launch goes through.
pub const RUNNER: &str = "claude";

/// Runs one tick; refuses without `--once` or `--version`.
pub fn run(cli: &Cli, once: bool, version: Option<&str>) -> u8 {
    let path = cli.verb.path();
    if !once {
        eprintln!("zaf {path}: only one tick at a time; pass --once and let a timer repeat it");
        return REFUSED;
    }
    let Some(version) = version else {
        eprintln!("zaf {path}: pass --version <v>, the milestone the instances build");
        return REFUSED;
    };
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let runner = match composition.runner(&RunnerName::from(RUNNER)) {
        Ok(runner) => runner,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let manifest = composition.manifest();
    let state_dir = composition.state_dir();
    let prepared = match spawn::prepare(
        board.as_ref(),
        &state_dir,
        manifest,
        &alive,
        std::process::id(),
    ) {
        Ok(prepared) => prepared,
        Err(error) => return failed(&path, &error),
    };
    if !cli.json {
        for instance in &prepared.lost {
            println!("lost {instance}");
        }
        print!("{}", prepared.plan.render_text());
    }
    let context = LaunchContext {
        state_dir,
        project: manifest.project.name.to_string(),
        profile: spawn::team_profile(manifest),
    };
    let mut code = OK;
    let mut launched: Vec<Launched> = Vec::new();
    for planned in &prepared.plan.launches {
        set_launch_env(&planned.instance, version);
        match spawn::launch(board.as_ref(), runner.as_ref(), &context, planned) {
            Ok(done) => {
                if let LaunchExit::Error(error) = &done.exit {
                    eprintln!("zaf {path}: {}: {error}", done.instance);
                    code = BACKEND;
                }
                if !cli.json {
                    println!(
                        "launched {}: exit {} ({})",
                        done.instance,
                        done.exit.status(),
                        done.run_dir.join(spawn::REPORT_FILE).display()
                    );
                }
                launched.push(done);
            }
            Err(error) => code = code.max(failed(&path, &error)),
        }
    }
    if cli.json {
        println!("{}", prepared.plan.to_json(&launched, &prepared.lost));
    }
    code
}

/// The environment a launch carries for the verbs its instance runs.
pub fn launch_env(instance: &Instance, version: &str) -> [(&'static str, String); 2] {
    [
        (INSTANCE_VAR, instance.to_string()),
        (VERSION_VAR, version.to_owned()),
    ]
}

/// Sets the launch's environment on this process, which the runner's child inherits.
///
/// The Runner port carries no environment, so the spawner sets its own before each run.
fn set_launch_env(instance: &Instance, version: &str) {
    for (name, value) in launch_env(instance, version) {
        // SAFETY: the spawner runs its launches one after another on the main
        // thread; no other thread is alive while the variable is set (the
        // runner's watchdog thread is joined before `run` returns).
        unsafe { std::env::set_var(name, value) };
    }
}

/// Whether a recorded process still runs: this one does; another is asked with `kill -0`.
fn alive(pid: u32) -> bool {
    if pid == std::process::id() {
        return true;
    }
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn failed(path: &str, error: &SpawnError) -> u8 {
    match error {
        SpawnError::Port(error) => exit::fail(path, error),
        SpawnError::Read { .. } | SpawnError::Write { .. } => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli;
    use crate::domain::{InstanceNumber, IssueNumber, RoleName};

    #[test]
    fn without_once_the_verb_refuses() {
        assert_eq!(cli::run(["zaf", "spawn"]), REFUSED);
        assert_eq!(cli::run(["zaf", "spawn", "--version", "0.3.0"]), REFUSED);
    }

    #[test]
    fn without_a_version_the_tick_refuses() {
        assert_eq!(cli::run(["zaf", "spawn", "--once"]), REFUSED);
    }

    #[test]
    fn a_launch_carries_its_instance_and_version() {
        let instance = Instance {
            role: RoleName::from("team"),
            epic: IssueNumber::from(16),
            number: InstanceNumber::from(12),
        };
        assert_eq!(
            launch_env(&instance, "0.3.0"),
            [
                ("ZAF_INSTANCE", "team instance 12 on #16".to_owned()),
                ("ZAF_VERSION", "0.3.0".to_owned()),
            ]
        );
    }
}
