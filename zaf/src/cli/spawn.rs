//! `zaf spawn --once --version <v>`: one tick of the spawner.
//!
//! The verb never loops: a timer runs it. It prints the plan, then runs each
//! planned launch to its exit, all in parallel (one thread each, joined before
//! the tick returns), and exits `2` when a launch
//! could not be recorded or its runner could not run. Each launch runs the
//! `team` role through [`Composition::runner_for_role`], inside its profile.
//! The tick ends by filling the ledger; a ledger failure warns and leaves the exit code.
//! A generated profile older than the manifest or zaf, or with no stamp, refuses the tick (and
//! the dry run) before it plans anything: it exits `1` and names the profile.
//! With `--dry-run` it prints the plan and, per launch, the instance, the epic,
//! the tasks, the profile, the environment and the command line, and stops there.

use std::collections::BTreeMap;

use super::Cli;
use super::board::INSTANCE_VAR;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::slice::VERSION_VAR;
use crate::adapters::process;
use crate::app::actions::{
    Caller, HOOKS_DIR, MINUTE, Nesting, RunRequest as ActionRequest, RunResult, hook, last_lines,
    run_hook,
};
use crate::app::profile;
use crate::app::spawn::{
    self, DryLaunch, LaunchContext, LaunchPorts, Launched, Prepared, SpawnError, TEAM_ROLE,
};
use crate::composition::{Composition, STATE_DIR, zaf_home};
use crate::domain::{CHALLENGER_ROLE, Instance, RoleName};
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
    // First of all, before any forge read: a hook that fails fails the tick, with nothing started.
    // A dry run is no tick, starts nothing and writes nothing, so it runs no hook either.
    if !dry_run && let Some(code) = pre_tick(&path, &composition, version) {
        return code;
    }
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    // Each launch thread builds its own runner; a missing one stops the tick here, before planning.
    for role in [TEAM_ROLE, spawn::PLANNING_ROLE, CHALLENGER_ROLE] {
        if let Err(error) = composition.runner_for_role(&RoleName::from(role)) {
            return exit::fail_composition(&path, &error);
        }
    }
    let manifest = composition.manifest();
    let state_dir = composition.state_dir();
    // Before anything is planned or reserved, dry run included: a profile made from an older
    // manifest runs its instances with yesterday's deny list.
    let stale = profile::stale(&state_dir, manifest);
    if !stale.is_empty() {
        for profile in &stale {
            eprintln!("zaf {path}: {}", profile.line());
        }
        return REFUSED;
    }
    // Resolved before anything is reserved, so a bad table holds no slot.
    let project_env = match spawn::manifest_env(manifest, composition.root(), version) {
        Ok(env) => env,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    if dry_run {
        return dry(cli, &path, &composition, version, &project_env);
    }
    // This tick holds each launch it reserves until the runner starts the instance.
    let Some(holder) = process::this() else {
        eprintln!("zaf {path}: cannot read this process's start time, so no launch could be held");
        return BACKEND;
    };
    let prepared = match spawn::prepare(
        board.as_ref(),
        &state_dir,
        manifest,
        &process::alive,
        &holder,
    ) {
        Ok(prepared) => prepared,
        Err(error) => return failed(&path, &error),
    };
    if !cli.json {
        for instance in &prepared.lost {
            println!("lost {instance}");
        }
        for released in &prepared.released {
            println!("released {} from {}", released.task, released.instance);
        }
        print!("{}", prepared.plan.render_text());
    }
    let jobs = jobs(&composition, &prepared, version, &project_env);
    let ports = |role: &RoleName| -> Result<LaunchPorts, PortError> {
        let board = composition
            .board()
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
        let runner = composition
            .runner_for_role(role)
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
        Ok((board, runner))
    };
    let mut code = OK;
    let mut launched: Vec<Launched> = Vec::new();
    for result in spawn::launch_all(&jobs, &ports) {
        match result {
            Ok(done) => {
                if let Some(error) = done.exit.problem() {
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
        let mut json = prepared.plan.to_json(&launched, &prepared.lost);
        json["released"] = prepared
            .released
            .iter()
            .map(|released| {
                serde_json::json!({
                    "task": released.task.get(),
                    "instance": released.instance.to_string(),
                })
            })
            .collect();
        println!("{json}");
    }
    if let Err(message) = super::report::ingest_ledger(&composition) {
        eprintln!("zaf {path}: warning: {message}");
    }
    code
}

/// Each planned launch with the context it runs under.
fn jobs<'a>(
    composition: &Composition,
    prepared: &'a Prepared,
    version: &str,
    project_env: &BTreeMap<String, String>,
) -> Vec<(&'a spawn::PlannedLaunch, LaunchContext)> {
    let manifest = composition.manifest();
    prepared
        .plan
        .launches
        .iter()
        .map(|planned| {
            let context = LaunchContext {
                state_dir: composition.state_dir(),
                project: manifest.project.name.to_string(),
                profile: spawn::role_profile(manifest, &planned.instance.role),
                env: with_project_env(project_env, launch_env(&planned.instance, version)),
            };
            (planned, context)
        })
        .collect()
}

/// The dry run: plans the tick without reserving it and prints what each launch would run.
fn dry(
    cli: &Cli,
    path: &str,
    composition: &Composition,
    version: &str,
    project_env: &BTreeMap<String, String>,
) -> u8 {
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(path, &error),
    };
    let manifest = composition.manifest();
    let prepared = match spawn::plan_only(
        board.as_ref(),
        &composition.state_dir(),
        manifest,
        &process::alive,
    ) {
        Ok(prepared) => prepared,
        Err(error) => return failed(path, &error),
    };
    let jobs = jobs(composition, &prepared, version, project_env);
    let describe = |request: &RunRequest| {
        composition
            .role_launch(request)
            .map_err(|error| PortError::Unavailable(error.to_string()))
    };
    let profile_dir = |profile| composition.profile_dir(profile);
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

/// How many lines of a failed hook's log the tick shows.
const HOOK_TAIL_LINES: usize = 10;

/// The `pre-tick` hook (docs/zaf-actions.md §5.1): `None` when the tick goes on, because there is
/// no hook or it exited 0, and the tick's exit code when it does not. The hook runs from the
/// project root, its output and exit code in its log. A hook that exits non-zero, or that cannot
/// run (a file not executable, a mode or a manifest `[env]` that refuses it), fails the tick: the
/// tick does not guess that a hook that did not run did not matter.
fn pre_tick(path: &str, composition: &Composition, version: &str) -> Option<u8> {
    let root = composition.root();
    let manifest = composition.manifest();
    let name = format!("{HOOKS_DIR}/pre-tick");
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        // With no home there is no machine tier, so the project's hook alone is looked for.
        _ => root.join(STATE_DIR),
    };
    hook(root, &home, "pre-tick")?;
    let failed = |reason: String| {
        eprintln!("zaf {path}: NOT_RUN {name}: {reason}");
        Some(BACKEND)
    };
    let nesting = match Nesting::from_env() {
        Ok(nesting) => nesting,
        Err(reason) => return failed(reason),
    };
    let caller = match Caller::from_env() {
        Ok(caller) => caller,
        Err(reason) => return failed(reason),
    };
    let env = match manifest.env.resolve(root, Some(version)) {
        Ok(env) => env,
        Err(error) => return failed(error.to_string()),
    };
    let outcome = run_hook(&ActionRequest {
        name: &name,
        args: &[],
        cwd: root,
        project_root: Some(root),
        home: &home,
        timeout_minutes: manifest.actions.timeout_minutes,
        minute: MINUTE,
        nesting,
        caller,
        only: None,
        slice: None,
        env,
    });
    if outcome.result == RunResult::Ok {
        return None;
    }
    if let Some(headline) = outcome.headline() {
        eprintln!("zaf {path}: {headline}");
    }
    // The last lines of the log, under the line they explain, as `zaf do` shows them.
    if let Some(log) = &outcome.log
        && let Ok(lines) = last_lines(log, HOOK_TAIL_LINES)
    {
        for line in lines {
            eprintln!("{line}");
        }
    }
    Some(BACKEND)
}

/// The manifest's `[env]` first, then zaf's own variables over it (no `[env]` key may
/// start with `ZAF_`, so none is shadowed).
fn with_project_env(
    project_env: &BTreeMap<String, String>,
    own: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env = project_env.clone();
    env.extend(own);
    env
}

/// The environment a launch carries for the verbs its instance runs.
pub fn launch_env(instance: &Instance, version: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        (INSTANCE_VAR.to_owned(), instance.to_string()),
        (VERSION_VAR.to_owned(), version.to_owned()),
    ])
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
    use crate::domain::{InstanceNumber, RoleName, TicketId};

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
    fn a_launch_carries_the_manifests_env_beside_zafs_own() {
        let instance = Instance {
            role: RoleName::from("team"),
            epic: TicketId::from(16),
            number: InstanceNumber::from(12),
        };
        let project = BTreeMap::from([("GATE_BASE".to_owned(), "feature/release-0-3".to_owned())]);
        let env = with_project_env(&project, launch_env(&instance, "0-3"));
        assert_eq!(env["GATE_BASE"], "feature/release-0-3");
        assert_eq!(env["ZAF_VERSION"], "0-3");
        assert_eq!(env["ZAF_INSTANCE"], "team instance 12 on #16");
        assert_eq!(env.len(), 3);
    }

    #[test]
    fn a_launch_carries_its_instance_and_version() {
        let instance = Instance {
            role: RoleName::from("team"),
            epic: TicketId::from(16),
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
