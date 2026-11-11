//! `zaf spawn --once --version <v>`: one tick of the spawner.
//!
//! With `--no-wait` it does nothing, with exit `0`, while another tick holds `spawn.lock`; a phase's
//! launcher asks for a tick that way once the phase has ended.
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
use std::process::{Command, Stdio};

use super::Cli;
use super::board::INSTANCE_VAR;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::slice::VERSION_VAR;
use crate::adapters::process;
use crate::app::actions::{
    ActionName, Caller, HOOKS_DIR, MINUTE, Nesting, RunRequest as ActionRequest, RunResult, hook,
    last_lines, run_hook_with_input,
};
use crate::app::profile;
use crate::app::spawn::{
    self, DryLaunch, LaunchContext, LaunchPorts, Launched, PlannedPhase, Prepared, SpawnError,
    TEAM_ROLE, TickOutcome,
};
use crate::composition::{Composition, STATE_DIR, zaf_home};
use crate::domain::{CHALLENGER_ROLE, Instance, RoleName, TicketId};
use crate::ports::{PortError, RunRequest};

/// Runs one tick; refuses without `--once` or `--version`. With `dry_run`, launches
/// nothing and writes nothing. With `no_wait`, a tick that finds `spawn.lock` held by another
/// does nothing and exits `0`: that tick reads the same forge (#421 ruling 25).
pub fn run(cli: &Cli, once: bool, version: Option<&str>, dry_run: bool, no_wait: bool) -> u8 {
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
    // Before the hook and any forge read: a tick that runs already does what this one would.
    if no_wait
        && !dry_run
        && let Some(code) = skip_if_held(cli, &path, &composition)
    {
        return code;
    }
    // First of all, before any forge read: a hook that fails fails the tick, with nothing started.
    // A dry run is no tick, starts nothing and writes nothing, so it runs no hook either.
    if !dry_run && let Some(code) = pre_tick(&path, &composition, version) {
        return code;
    }
    let code = tick(cli, &path, &composition, version, dry_run);
    // Last of all, idle ticks and failed ones included; the hook's failure changes nothing, so the
    // code is the tick's own. A dry run is no tick and runs no hook.
    if !dry_run {
        post_tick(&path, &composition, version);
    }
    code
}

/// What this tick did, for the `post-tick` hook. The steps that fill a [`TickOutcome`] (the model
/// phase started, the waits launched, the `zaf` actions run, the questions raised) are not wired
/// into this tick yet, so none is reported and the outcome says idle.
fn outcome_of_tick() -> TickOutcome {
    TickOutcome::default()
}

/// The tick itself, everything between the `pre-tick` hook and the `post-tick` one.
fn tick(cli: &Cli, path: &str, composition: &Composition, version: &str, dry_run: bool) -> u8 {
    let path = path.to_owned();
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
        return dry(cli, &path, composition, version, &project_env);
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
    let jobs = jobs(composition, &prepared, version, &project_env);
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
    if let Err(message) = super::report::ingest_ledger(composition) {
        eprintln!("zaf {path}: warning: {message}");
    }
    code
}

/// `--no-wait`: `Some(exit)` ends the verb, `None` lets the tick go on. A held lock exits `0`
/// with nothing done, and a lock that cannot be read is a backend failure, never a guess.
fn skip_if_held(cli: &Cli, path: &str, composition: &Composition) -> Option<u8> {
    match spawn::lock_is_held(&composition.state_dir()) {
        Ok(false) => None,
        Ok(true) => {
            if cli.json {
                println!("{}", serde_json::json!({"skipped": "spawn.lock is held"}));
            } else {
                println!("skipped: another tick holds spawn.lock");
            }
            Some(OK)
        }
        Err(error) => Some(failed(path, &error)),
    }
}

/// Runs `zaf do <action> <task>` in this process: the runner the tick hands
/// [`spawn::run_zaf_actions`] for a `zaf`-kind action. A failure prints its headline on stderr
/// and the code is passed on: 0 passed, 125 never ran, any other the action's own.
pub fn run_zaf_action(task: TicketId, action: &ActionName) -> u8 {
    let outcome = super::do_action::run_named(
        action.as_str(),
        &[std::ffi::OsString::from(task.get().to_string())],
        None,
    );
    if let Some(headline) = outcome.headline() {
        eprintln!("{headline}");
    }
    outcome.exit
}

/// Starts the tick's one model phase with `zaf spawn phase <task> <action>`: the `start` the tick
/// hands [`spawn::start_planned`]. It waits only for `zaf spawn phase`, which returns once the
/// phase's launcher is started as its own process, never for the phase itself (#421 ruling 19).
/// Its refusal or failure is on stderr; a non-zero exit is `Err` naming the code.
pub fn start_phase(planned: &PlannedPhase) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("cannot find the zaf binary to start a phase: {error}"))?;
    spawn_phase_with(Command::new(exe), planned)
}

/// Runs `<zaf> spawn phase <task> <action>`, `zaf` the command that stands for the binary, and
/// waits for that command alone. Its stdout, the `started` line, is dropped so a `--json` tick
/// prints one document; its stderr is the tick's.
fn spawn_phase_with(mut zaf: Command, planned: &PlannedPhase) -> Result<(), String> {
    let status = zaf
        .args(["spawn", "phase"])
        .arg(planned.task.get().to_string())
        .arg(planned.action.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .map_err(|error| format!("cannot run zaf spawn phase: {error}"))?;
    if status.success() {
        return Ok(());
    }
    let code = status
        .code()
        .map_or_else(|| "a signal".to_owned(), |code| format!("exit {code}"));
    Err(format!(
        "zaf spawn phase {} {} ended with {code}",
        planned.task.get(),
        planned.action
    ))
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
    run_tick_hook(path, composition, version, "pre-tick", None)
}

/// The `post-tick` hook (docs/zaf-actions.md §5.1, #421 ruling 12): runs after every tick, idle
/// ones too, with the tick's outcome as JSON on its stdin. A hook that fails is reported on stderr
/// as `pre-tick`'s is and changes nothing: the tick's exit code is not read from it.
fn post_tick(path: &str, composition: &Composition, version: &str) {
    let outcome = outcome_of_tick();
    spawn::post_tick::run(
        &outcome,
        &mut |input| match run_tick_hook(path, composition, version, "post-tick", Some(input)) {
            None => Ok(()),
            Some(code) => Err(format!("exit {code}")),
        },
        &mut |line| eprintln!("zaf {path}: {line}"),
    );
}

/// Runs the hook of one tick event from the project root, `input` on its stdin when it has any:
/// `None` when there is no hook or it exited 0, and `Some(BACKEND)` after printing why when it did
/// not. The caller decides what a failure means.
fn run_tick_hook(
    path: &str,
    composition: &Composition,
    version: &str,
    event: &str,
    input: Option<&str>,
) -> Option<u8> {
    let root = composition.root();
    let manifest = composition.manifest();
    let name = format!("{HOOKS_DIR}/{event}");
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        // With no home there is no machine tier, so the project's hook alone is looked for.
        _ => root.join(STATE_DIR),
    };
    hook(root, &home, event)?;
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
    let request = ActionRequest {
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
    };
    let outcome = run_hook_with_input(&request, input);
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

    /// A stand-in for the zaf binary: `sh` on a script that records its arguments in `args`, then
    /// runs `body`. `sh` reads the script, so no freshly written file is executed.
    fn fake_zaf(dir: &std::path::Path, body: &str) -> Command {
        let script = dir.join("zaf.sh");
        let args = dir.join("args");
        let text = format!("echo \"$@\" > '{}'\n{body}\n", args.display());
        std::fs::write(&script, text).expect("writes the fake zaf");
        let mut zaf = Command::new("sh");
        zaf.arg(script);
        zaf
    }

    fn planned() -> PlannedPhase {
        PlannedPhase {
            task: TicketId::from(42),
            action: crate::app::actions::ActionName::from("implement"),
            units: 4,
        }
    }

    #[test]
    fn the_tick_starts_its_phase_with_zaf_spawn_phase_and_does_not_wait() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        // `zaf spawn phase` returns once its launcher is started; the phase runs on, detached.
        let zaf = fake_zaf(dir.path(), "sleep 30 >/dev/null 2>&1 &\nexit 0");
        let began = std::time::Instant::now();
        assert_eq!(spawn_phase_with(zaf, &planned()), Ok(()));
        assert!(began.elapsed() < std::time::Duration::from_secs(10));
        let args = std::fs::read_to_string(dir.path().join("args")).expect("the args");
        assert_eq!(args.trim(), "spawn phase 42 implement");
    }

    #[test]
    fn a_refused_phase_start_is_an_error_naming_its_code() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let zaf = fake_zaf(dir.path(), "exit 1");
        assert_eq!(
            spawn_phase_with(zaf, &planned()),
            Err("zaf spawn phase 42 implement ended with exit 1".to_owned())
        );
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
