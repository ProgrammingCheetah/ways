//! `zaf spawn --once --version <v>`: one tick of the spawner.
//!
//! With `--no-wait` it does nothing, with exit `0`, while another tick holds `spawn.lock`; a phase's
//! launcher asks for a tick that way once the phase has ended.
//! `--once` never loops: a timer runs it, or `zaf spawn --watch` ([`watch`]), the foreground loop
//! that runs it each `[spawn] tick_minutes`. It prints the plan, then runs each
//! planned launch to its exit, all in parallel (one thread each, joined before
//! the tick returns), and exits `2` when a launch
//! could not be recorded or its runner could not run. Each launch runs the
//! `team` role through [`Composition::runner_for_role`], inside its profile.
//! When the manifest has `[phases]`, the phase steps run first, before any team launch: every
//! claimed or ready task whose type has a list has its standing derived from the forge and its
//! phase run folders, and steps 3 to 6 of docs/zaf-actions.md §5.1 run on them, the waits and the
//! one model phase started with `zaf spawn phase`; what they did prints after the plan (under
//! `tick` with `--json`). A task that cannot be read is named and skipped. A model phase that
//! fails to start, a question that cannot be raised or a forge that cannot be reached exits `2`.
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
use crate::app::spawn::phase::Ports;
use crate::app::spawn::{
    self, DryLaunch, InUse, LaunchContext, LaunchPorts, Launched, PhaseTask, PhaseTick,
    PlannedModel, Prepared, SpawnError, Steps, TEAM_ROLE, TickOutcome,
};
use crate::composition::{Composition, STATE_DIR, zaf_home};
use crate::domain::{Budget, CHALLENGER_ROLE, Instance, RoleName, TicketId};
use crate::ports::{Board, PortError, RunRequest, e_is_live, live_use};

/// Runs one tick; refuses without `--once` or `--version`. With `dry_run`, launches
/// nothing and writes nothing. With `no_wait`, a tick that finds `spawn.lock` held by another
/// does nothing and exits `0`: that tick reads the same forge (#421 ruling 25).
pub fn run(cli: &Cli, once: bool, version: Option<&str>, dry_run: bool, no_wait: bool) -> u8 {
    let path = cli.verb.path();
    if !once {
        eprintln!(
            "zaf {path}: only one tick at a time; pass --once and let a timer repeat it, or --watch"
        );
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
    // What the tick's own steps did (the model phase started, the waits launched, the `zaf` actions
    // run, the questions raised): `tick` fills it, and it stays idle when no step ran.
    let mut outcome = TickOutcome::default();
    let code = tick(cli, &path, &composition, version, dry_run, &mut outcome);
    // Last of all, idle ticks and failed ones included; the hook's failure changes nothing, so the
    // code is the tick's own. A dry run is no tick and runs no hook.
    if !dry_run {
        post_tick(&path, &composition, version, &outcome);
    }
    code
}

/// `zaf spawn --watch --version <v>`: one tick each `[spawn] tick_minutes`, the first at once,
/// in the foreground until the process is stopped. Each tick is the one of `--once --no-wait`, so
/// a held `spawn.lock` skips it, and a tick that fails is logged on stderr while the loop goes on
/// (#421 ruling 25). Only what stops the loop from starting ends the verb: a missing version, or
/// a manifest that does not load, `tick_minutes = 0` among its failures.
pub fn watch(cli: &Cli, version: Option<&str>) -> u8 {
    let path = cli.verb.path();
    let Some(version) = version else {
        eprintln!("zaf {path}: pass --version <v>, the milestone the instances build");
        return REFUSED;
    };
    let minutes = match Composition::discover_here() {
        Ok(composition) => composition.manifest().spawn.tick_minutes,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    if !cli.json {
        println!("watching: a tick every {minutes} minutes, until stopped");
    }
    spawn::watch::watch(
        MINUTE * minutes,
        &mut || run(cli, true, Some(version), false, true),
        &mut |interval| {
            std::thread::sleep(interval);
            true
        },
        &mut |line| eprintln!("zaf {path}: {line}"),
    );
    OK
}

/// The tick itself, everything between the `pre-tick` hook and the `post-tick` one. What its phase
/// steps did is written to `outcome`.
fn tick(
    cli: &Cli,
    path: &str,
    composition: &Composition,
    version: &str,
    dry_run: bool,
    outcome: &mut TickOutcome,
) -> u8 {
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
    let mut code = OK;
    // Steps 3 to 6 run before the team launches, which run to their exit.
    let phases = if manifest.phases.is_empty() {
        None
    } else {
        let (tick, phase_code) = phase_tick(&path, composition, board.as_ref(), &prepared);
        code = code.max(phase_code);
        if !cli.json {
            print!("{}", tick.render_text());
        }
        // What the `post-tick` hook gets: the tick's own steps, not an empty outcome.
        *outcome = tick.clone();
        Some(tick)
    };
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
        if let Some(tick) = &phases {
            json["tick"] = tick.to_json();
        }
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
pub fn start_phase(planned: &PlannedModel) -> Result<(), String> {
    start_model(this_zaf()?, planned)
}

/// The command that runs this zaf binary.
fn this_zaf() -> Result<Command, String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("cannot find the zaf binary to start a phase: {error}"))?;
    Ok(Command::new(exe))
}

/// The phase steps of the tick (docs/zaf-actions.md §5.1, steps 3 to 6) on every claimed or ready
/// task whose type has a `[phases]` list, and the exit code they leave: `2` when the forge cannot
/// be reached, the board cannot be read, a question cannot be raised or the model phase fails to
/// start, `0` otherwise, idle or not. A task that cannot be read is named on stderr and skipped.
///
/// The budget is the machine's: its live use and E across every project, and this project's
/// running instances, with the team launches this tick reserved, against `[spawn] slots`.
fn phase_tick(
    path: &str,
    composition: &Composition,
    board: &dyn Board,
    prepared: &Prepared,
) -> (spawn::TickOutcome, u8) {
    let forge = match composition.forge() {
        Ok(forge) => forge,
        Err(error) => {
            eprintln!("zaf {path}: the phase steps did not run: {error}");
            return (spawn::TickOutcome::default(), BACKEND);
        }
    };
    let root = composition.root();
    let state_dir = composition.state_dir();
    // With no home there is no machine tier, so the project's actions alone are looked for.
    let home = match zaf_home().map(std::path::absolute) {
        Some(Ok(home)) => home,
        _ => root.join(STATE_DIR),
    };
    let manifest = composition.manifest();
    let ports = Ports {
        board,
        forge: forge.as_ref(),
        manifest,
        root,
        home: &home,
        state_dir: &state_dir,
        alive: &process::alive,
    };
    let now = std::time::SystemTime::now();
    let gathered = match spawn::gather(&ports, now) {
        Ok(gathered) => gathered,
        Err(error) => {
            eprintln!("zaf {path}: the phase steps did not run: {error}");
            return (spawn::TickOutcome::default(), BACKEND);
        }
    };
    for skipped in &gathered.unreadable {
        eprintln!("zaf {path}: skipped {}: {}", skipped.task, skipped.reason);
    }
    let machine = process::machine();
    let in_use = InUse {
        now,
        units: live_use(&machine),
        e_live: e_is_live(&machine),
        running_here: machine
            .iter()
            .filter(|instance| instance.root == root)
            .count()
            + prepared.plan.launches.len(),
        slots: usize::try_from(manifest.spawn.slots).unwrap_or(usize::MAX),
    };
    let (tick, code) = phase_steps(
        board,
        &gathered.tasks,
        &in_use,
        &manifest.spawn.budget(),
        &this_zaf,
    );
    for failure in &tick.failed {
        eprintln!("zaf {path}: {failure}");
    }
    (tick.outcome, code)
}

/// Runs steps 3 to 6 on `tasks`, each wait and the model phase started with `<zaf> spawn phase`,
/// and gives what they did with the exit code: `2` when any step failed, `0` otherwise.
fn phase_steps(
    board: &dyn Board,
    tasks: &[PhaseTask],
    in_use: &InUse,
    budget: &Budget,
    zaf: &dyn Fn() -> Result<Command, String>,
) -> (PhaseTick, u8) {
    let mut run = run_zaf_action;
    let mut launch = |task: TicketId, action: &ActionName| spawn_phase_with(zaf()?, task, action);
    let mut start = |planned: &PlannedModel| start_model(zaf()?, planned);
    let mut log = |line: &str| eprintln!("{line}");
    let tick = spawn::run_steps(
        board,
        tasks,
        in_use,
        budget,
        &mut Steps {
            run: &mut run,
            launch: &mut launch,
            start: &mut start,
            log: &mut log,
        },
    );
    let code = if tick.failed.is_empty() { OK } else { BACKEND };
    (tick, code)
}

/// Starts `planned` with `<zaf> spawn phase <task> <action>`. An epic's launch is not a phase:
/// `zaf spawn phase` has no verb for it yet, so it is an error until the tick's launcher is wired
/// to it.
fn start_model(zaf: Command, planned: &PlannedModel) -> Result<(), String> {
    let PlannedModel::Phase { task, action, .. } = planned else {
        return Err("an epic launch is not started by zaf spawn phase".to_owned());
    };
    spawn_phase_with(zaf, *task, action)
}

/// Runs `<zaf> spawn phase <task> <action>`, `zaf` the command that stands for the binary, and
/// waits for that command alone. Its stdout, the `started` line, is dropped so a `--json` tick
/// prints one document; its stderr is the tick's.
fn spawn_phase_with(mut zaf: Command, task: TicketId, action: &ActionName) -> Result<(), String> {
    let status = zaf
        .args(["spawn", "phase"])
        .arg(task.get().to_string())
        .arg(action.as_str())
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
        "zaf spawn phase {} {action} ended with {code}",
        task.get()
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
fn post_tick(path: &str, composition: &Composition, version: &str, outcome: &TickOutcome) {
    spawn::post_tick::run(
        outcome,
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

    fn planned() -> PlannedModel {
        PlannedModel::Phase {
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
        assert_eq!(start_model(zaf, &planned()), Ok(()));
        assert!(began.elapsed() < std::time::Duration::from_secs(10));
        let args = std::fs::read_to_string(dir.path().join("args")).expect("the args");
        assert_eq!(args.trim(), "spawn phase 42 implement");
    }

    #[test]
    fn an_epic_launch_is_not_started_as_a_phase() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let zaf = fake_zaf(dir.path(), "exit 0");
        let epic = PlannedModel::Epic {
            epic: TicketId::from(7),
            role: RoleName::from("planning"),
            units: 1,
        };
        assert!(start_model(zaf, &epic).is_err());
        assert!(!dir.path().join("args").exists());
    }

    #[test]
    fn a_refused_phase_start_is_an_error_naming_its_code() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let zaf = fake_zaf(dir.path(), "exit 1");
        assert_eq!(
            start_model(zaf, &planned()),
            Err("zaf spawn phase 42 implement ended with exit 1".to_owned())
        );
    }

    /// `task`, ready at `implement` (a model phase ending at an open pull request) since 1, size S.
    fn ready_at_implement(task: u64) -> PhaseTask {
        use crate::app::actions::{ActionKind, ListedPhase, PhaseEnd, StandingFacts};
        use crate::app::spawn::TickTask;
        PhaseTask {
            tick: TickTask {
                task: TicketId::from(task),
                list: vec![ListedPhase {
                    action: ActionName::from("implement"),
                    kind: ActionKind::Model,
                    ends: PhaseEnd::PrOpen,
                    on_fail: None,
                }],
                facts: StandingFacts {
                    ready_since: std::time::UNIX_EPOCH + std::time::Duration::from_secs(1),
                    pull: None,
                    runs: Vec::new(),
                },
            },
            size: crate::domain::TaskSize::S,
        }
    }

    #[test]
    fn a_failed_launch_exits_two() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let calls = std::cell::Cell::new(0);
        let zaf = || {
            calls.set(calls.get() + 1);
            Ok(fake_zaf(dir.path(), "exit 1"))
        };
        let in_use = InUse {
            now: std::time::UNIX_EPOCH,
            units: crate::domain::Units::default(),
            e_live: false,
            running_here: 0,
            slots: 2,
        };
        let board = crate::fakes::FakeBoard::default();
        let (tick, code) = phase_steps(
            &board,
            &[ready_at_implement(3), ready_at_implement(5)],
            &in_use,
            &Budget::default(),
            &zaf,
        );
        assert_eq!(code, BACKEND);
        assert_eq!(tick.outcome.started, None, "nothing is recorded as started");
        assert_eq!(calls.get(), 1, "no other item is tried");
        let args = std::fs::read_to_string(dir.path().join("args")).expect("the args");
        assert_eq!(args.trim(), "spawn phase 3 implement");
        assert_eq!(
            tick.failed,
            ["zaf spawn phase 3 implement ended with exit 1"]
        );
    }

    #[test]
    fn a_started_phase_exits_zero() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let zaf = || Ok(fake_zaf(dir.path(), "exit 0"));
        let in_use = InUse {
            now: std::time::UNIX_EPOCH,
            units: crate::domain::Units::default(),
            e_live: false,
            running_here: 0,
            slots: 2,
        };
        let board = crate::fakes::FakeBoard::default();
        let (tick, code) = phase_steps(
            &board,
            &[ready_at_implement(3)],
            &in_use,
            &Budget::default(),
            &zaf,
        );
        assert_eq!(code, OK);
        assert_eq!(
            tick.outcome.started.map(|started| started.task),
            Some(TicketId::from(3))
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
