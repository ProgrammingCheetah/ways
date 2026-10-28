//! `zaf spawn --once --version <v>`: one tick of the spawner.
//!
//! The verb never loops: a timer runs it. It prints the plan, then runs each
//! planned launch to its exit, all in parallel (one thread each, joined before
//! the tick returns), and exits `2` when a launch
//! could not be recorded or its runner could not run. Each launch runs the
//! `team` role through [`Composition::runner_for_role`], inside its profile.
//! With `--dry-run` it prints the plan and, per launch, the instance, the epic,
//! the tasks, the profile, the environment and the command line, and stops there.

use std::collections::BTreeMap;
use std::process::{Command, Stdio};

use super::Cli;
use super::board::INSTANCE_VAR;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::slice::VERSION_VAR;
use crate::app::spawn::{
    self, DryLaunch, LaunchContext, LaunchExit, LaunchPorts, Launched, Prepared, SpawnError,
    TEAM_ROLE,
};
use crate::composition::Composition;
use crate::domain::{Instance, RoleName};
use crate::ports::{PortError, RunRequest};

/// Runs one tick; refuses without `--once` or `--version`. With `dry_run`, launches
/// nothing and writes nothing.
pub fn run(cli: &Cli, once: bool, version: Option<&str>, dry_run: bool) -> u8 {
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
    let team = RoleName::from(TEAM_ROLE);
    // Each launch thread builds its own runner; a missing one stops the tick here, before planning.
    if let Err(error) = composition.runner_for_role(&team) {
        return exit::fail_composition(&path, &error);
    }
    let manifest = composition.manifest();
    let state_dir = composition.state_dir();
    if dry_run {
        return dry(cli, &path, &composition, version);
    }
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
    let jobs = jobs(&composition, &prepared, version);
    let ports = || -> Result<LaunchPorts, PortError> {
        let board = composition
            .board()
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
        let runner = composition
            .runner_for_role(&team)
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
        Ok((board, runner))
    };
    let mut code = OK;
    let mut launched: Vec<Launched> = Vec::new();
    for result in spawn::launch_all(&jobs, &ports) {
        match result {
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

/// Each planned launch with the context it runs under.
fn jobs<'a>(
    composition: &Composition,
    prepared: &'a Prepared,
    version: &str,
) -> Vec<(&'a spawn::PlannedLaunch, LaunchContext)> {
    let manifest = composition.manifest();
    let profile = spawn::team_profile(manifest);
    prepared
        .plan
        .launches
        .iter()
        .map(|planned| {
            let context = LaunchContext {
                state_dir: composition.state_dir(),
                project: manifest.project.name.to_string(),
                profile,
                env: launch_env(&planned.instance, version),
            };
            (planned, context)
        })
        .collect()
}

/// The dry run: plans the tick without reserving it and prints what each launch would run.
fn dry(cli: &Cli, path: &str, composition: &Composition, version: &str) -> u8 {
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(path, &error),
    };
    let manifest = composition.manifest();
    let prepared =
        match spawn::plan_only(board.as_ref(), &composition.state_dir(), manifest, &alive) {
            Ok(prepared) => prepared,
            Err(error) => return failed(path, &error),
        };
    let jobs = jobs(composition, &prepared, version);
    let describe = |request: &RunRequest| {
        composition
            .role_launch(request)
            .map_err(|error| PortError::Unavailable(error.to_string()))
    };
    let profile_dir = composition.profile_dir(spawn::team_profile(manifest));
    let launches: Vec<DryLaunch> = match spawn::dry_run(&jobs, &profile_dir, &describe) {
        Ok(launches) => launches,
        Err(error) => return exit::fail(path, &error),
    };
    if cli.json {
        let mut json = prepared.plan.to_json(&[], &prepared.lost);
        json["dry_run"] = serde_json::json!(launches);
        println!("{json}");
        return OK;
    }
    for instance in &prepared.lost {
        println!("lost {instance} (dry run: not recorded)");
    }
    print!("{}", prepared.plan.render_text());
    for launch in &launches {
        print!("{}", launch.render_text());
    }
    OK
}

/// The environment a launch carries for the verbs its instance runs.
pub fn launch_env(instance: &Instance, version: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (INSTANCE_VAR.to_owned(), instance.to_string()),
        (VERSION_VAR.to_owned(), version.to_owned()),
    ])
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
            BTreeMap::from([
                (
                    "ZAF_INSTANCE".to_owned(),
                    "team instance 12 on #16".to_owned()
                ),
                ("ZAF_VERSION".to_owned(), "0.3.0".to_owned()),
            ])
        );
    }
}
