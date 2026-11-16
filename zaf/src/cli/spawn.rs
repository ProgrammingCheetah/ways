//! `zaf spawn --once --version <v>`: one tick of the spawner.
//!
//! With `--no-wait` it does nothing, with exit `0`, while another tick holds `spawn.lock`; a phase's
//! launcher asks for a tick that way once the phase has ended, when no scheduler runs.
//! `--once` never loops. While the project's scheduler runs (`zaf spawn --watch`, [`watch`] and
//! [`super::scheduler`]) it ticks nothing itself: it asks the scheduler for a tick and exits `0`
//! (`defer_to_scheduler`); the scheduler runs the same tick ([`one_tick`]). With another
//! `--version` than the scheduler's it is refused with exit `1`, and nothing is asked or run; a
//! request that cannot be written exits `2`, saying no tick was asked for. Every process a tick
//! starts runs in a process group of its own, so a Ctrl-C meant for the scheduler reaches it
//! alone. It prints the plan,
//! then runs each
//! planned launch to its exit, all in parallel (one thread each, joined before
//! the tick returns), and exits `2` when a launch
//! could not be recorded or its runner could not run. Each launch (planning or the challenger)
//! runs its role through [`Composition::runner_for_role`], inside its profile; no launch builds
//! a task, the phase steps do.
//! When the manifest has `[phases]`, the phase steps run first, before any launch: every open
//! ticket whose type has a list is placed by the scheduling core from the forge and its phase run
//! folders, and steps 3 to 6 of docs/zaf-actions.md §5.1 run on the lane through the one gate
//! ([`crate::app::schedule::tick`]), the waits and the one model phase started with `zaf spawn
//! phase`; what they did prints after the plan (under `tick` with `--json`), and each ticket its
//! epic or a question holds back is one `skipped` line (`held_tasks` with `--json`). A `zaf`-kind
//! action run inline leaves a phase run folder, as a launched phase does, and runs with `ZAF_RUN`
//! naming it ([`run_zaf_action`]). A ticket that cannot be read is named and skipped. A model phase
//! that fails to start, a question that cannot be raised or a forge that cannot be reached exits
//! `2`.
//! Before the phase steps, once the core has placed the tickets, each open ticket's state label is
//! set from its place ([`crate::app::schedule::labels`], R5): a blocked task that waits on no
//! ticket is set to `ready`, one `#26 unblocked: #25 is done` line each (`unblocked` with
//! `--json`), and any other label set is one `#27 labelled question: it waits on question #45`
//! line (`relabelled`). With no `[phases]` every open task is labelled so, from its questions and
//! its blockers. A board that fails there is named on stderr and exits `2`, and the rest of the
//! tick still runs.
//! The tick ends by filling the ledger; a ledger failure warns and leaves the exit code.
//! A generated profile older than the manifest or zaf, or with no stamp, refuses the tick (and
//! the dry run) before it plans anything: it exits `1` and names the profile.
//! With `--dry-run` it prints the labels it would set, the plan and, per launch, the instance,
//! the epic, the profile, the environment and the command line, and stops there, writing nothing.
//! With `--json` as well it carries `placement`: where the scheduling core places each open ticket
//! with a phase list, shown beside the plan ([`crate::app::schedule::shadow`]), and `held_tasks`
//! from that placement; the plain dry run reads the same tickets for the labels, and prints no
//! placement and no held task.
//!
//! Every real tick leaves one line in the trace, `.zaf/ticks.jsonl` ([`crate::app::schedule::trace`],
//! R24): where the core places each open ticket with a phase list before the tick acts (the same
//! read the phase steps act on), what the tick did and held back, and what moved since the last
//! line.
//! It is written once the phase steps have run and before the launches do, so its moves head the
//! block, after the slots: one line per ticket that moved, every ticket with `--more`, then a count
//! line; `--json` carries `moved` and `tickets`. A tick that stops before it places the tickets
//! (the `pre-tick` hook, a board or a forge that fails) still leaves a line, with its `error`. A
//! trace that cannot be written is one warning on stderr and leaves the exit code; the dry run
//! writes none.

use std::collections::BTreeMap;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Cli;
use super::board::INSTANCE_VAR;
use super::exit::{self, BACKEND, OK, REFUSED};
use super::slice::VERSION_VAR;
use crate::adapters::{clock, process};
use crate::app::actions::{
    ActionName, Caller, HOOKS_DIR, MINUTE, NOT_RUN_EXIT, Nesting, RunOutcome,
    RunRequest as ActionRequest, RunResult, RunStep, Terminal, hook, last_lines,
    run_hook_with_input,
};
use crate::app::profile;
use crate::app::schedule::gather::Gathered;
use crate::app::schedule::labels::{self, Labelling};
use crate::app::schedule::scheduler::{Asked, Memory};
use crate::app::schedule::tick::{self as schedule_tick, Held, Machine, PhaseTick, Steps};
use crate::app::schedule::trace::{self, Did, Entry, Seen};
use crate::app::schedule::{binary, lock, request};
use crate::app::schedule::{gather as schedule_gather, record, ticks};
use crate::app::spawn::phase::{PhaseError, Ports};
use crate::app::spawn::watch::{SCHEDULED_BY_VAR, is_scheduled_by};
use crate::app::spawn::{
    self, DryLaunch, LaunchContext, LaunchPorts, Launched, PlannedModel, Prepared, SpawnError,
    TickOutcome,
};
use crate::composition::{Composition, STATE_DIR, zaf_home};
use crate::domain::schedule::Placement;
use crate::domain::{Budget, CHALLENGER_ROLE, Instance, RoleName, TicketId};
use crate::ports::{Board, Holder, PortError, RunRequest, e_is_live, live_use};

/// Runs one tick; refuses without `--once` or `--version`. With `dry_run`, launches
/// nothing and writes nothing. With `no_wait`, a tick that finds `spawn.lock` held by another
/// does nothing and exits `0`: that tick reads the same forge (#421 ruling 25). While a scheduler
/// runs for the project (`zaf spawn --watch`, which holds `.zaf/scheduler.lock`), a real tick is
/// not run here: the scheduler is asked for one (`defer_to_scheduler`), since it alone ticks
/// while it runs. The dry run always computes and prints here, writing nothing.
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
    // Before anything else: the scheduler that runs does what this tick would.
    if !dry_run && let Some(code) = defer_to_scheduler(cli, &path, &composition, version) {
        return code;
    }
    // Before the hook and any forge read: a tick that runs already does what this one would.
    if no_wait
        && !dry_run
        && let Some(code) = skip_if_held(cli, &path, &composition)
    {
        return code;
    }
    one_tick(cli, &path, &composition, version, dry_run, None)
}

/// A tick the scheduler runs: who asked for it, and what the scheduler keeps between ticks, which
/// the tick replaces with the placement it acted on.
pub struct Scheduled<'a> {
    /// Who asked.
    pub asked: &'a Asked,
    /// What the scheduler keeps.
    pub memory: &'a mut Memory,
}

/// One tick, from the `pre-tick` hook to the `post-tick` one, for `zaf spawn --once` (`scheduled`
/// none) or for the scheduler. A scheduled tick's block names who asked; one that read the facts
/// leaves its placement in the scheduler's memory, and one that could not read them keeps the last
/// placement, starts nothing and says so in its block and in its trace line.
pub fn one_tick(
    cli: &Cli,
    path: &str,
    composition: &Composition,
    version: &str,
    dry_run: bool,
    scheduled: Option<Scheduled<'_>>,
) -> u8 {
    let began = std::time::SystemTime::now();
    // First of all, before any forge read: a hook that fails fails the tick, with nothing started.
    // A dry run is no tick, starts nothing and writes nothing, so it runs no hook either.
    if !dry_run && let Some(code) = pre_tick(path, composition, version) {
        write_trace(
            path,
            composition,
            &Entry::stopped(began, "the pre-tick hook failed"),
        );
        return code;
    }
    // What the tick's own steps did (the model phase started, the waits launched, the `zaf` actions
    // run, the questions raised): `tick` fills it, and it stays idle when no step ran.
    let mut outcome = TickOutcome::default();
    let mut traced = Traced::default();
    let kept = scheduled
        .as_ref()
        .and_then(|scheduled| scheduled.memory.gathered_at());
    let when = When {
        began,
        asked: scheduled.as_ref().map(|scheduled| scheduled.asked),
        kept,
    };
    let code = tick(
        cli,
        path,
        composition,
        version,
        dry_run,
        &when,
        &mut outcome,
        &mut traced,
    );
    // Last of all, idle ticks and failed ones included; the hook's failure changes nothing, so the
    // code is the tick's own. A dry run is no tick, runs no hook and leaves no trace.
    if !dry_run {
        if !traced.written {
            let mut why = traced.stopped.clone().unwrap_or_else(|| {
                format!("the tick ended with exit {code} before it placed the tickets")
            });
            if traced.unread.is_some() {
                why.push_str(&kept_words(kept.map(shadow_time)));
            }
            write_trace(path, composition, &Entry::stopped(began, why));
        }
        post_tick(path, composition, version, &outcome);
    }
    if let Some(scheduled) = scheduled {
        match (traced.placement.take(), &traced.unread) {
            (Some(placement), _) => scheduled.memory.keep(placement, began),
            // A tick that stopped before its block says so here; one that wrote its trace line
            // said so in its own block.
            (None, Some(why)) if !cli.json && !traced.written => {
                let mut block = spawn::digest::Block::new(began, clock::utc_offset(began));
                block.asked = Some(scheduled.asked.line());
                block.did.push(format!(
                    "could not read the facts: {why}{}",
                    kept_words(kept.map(|at| spawn::digest::clock(at, clock::utc_offset(at))))
                ));
                print!("{}", block.head());
            }
            (None, _) => {}
        }
    }
    code
}

/// What a tick that could not read the facts did instead, for its block and its trace line: it
/// kept the placement of `kept`, when there was one, and started no phase.
fn kept_words(kept: Option<String>) -> String {
    match kept {
        Some(at) => format!("; kept the placement of {at} and started no phase"),
        None => "; started no phase".to_owned(),
    }
}

/// When a tick began, who asked for it when the scheduler runs it, and when the placement the
/// scheduler keeps was gathered.
struct When<'a> {
    /// When the tick began.
    began: std::time::SystemTime,
    /// Who asked, for a tick the scheduler runs.
    asked: Option<&'a Asked>,
    /// When the placement the scheduler keeps was gathered; none before the first.
    kept: Option<std::time::SystemTime>,
}

/// A time as the trace writes it, RFC 3339 in UTC.
fn shadow_time(at: std::time::SystemTime) -> String {
    crate::app::schedule::shadow::time(at)
}

/// When a scheduler runs for the project, asks it for a tick, as `zaf spawn --once`, and gives the
/// verb's exit, `0`, having said so in one line: `asked the running scheduler for a tick: pid
/// 4242, since … (zaf 0.3.1, --version 0.4.0)`. `None` when no scheduler runs, and the tick runs
/// here. A scheduler that builds another milestone than `version` is not asked: the verb is
/// refused, naming it, and no tick is asked for or run, since its tick would build its own. A
/// request that cannot be written is a backend failure, which says no tick was asked for.
fn defer_to_scheduler(
    cli: &Cli,
    path: &str,
    composition: &Composition,
    version: &str,
) -> Option<u8> {
    // The tick of the scheduler the hub runs, which the hub started this process for: it is that
    // scheduler's own tick, so nothing is asked of it.
    let scheduled_by = std::env::var(SCHEDULED_BY_VAR).ok();
    if let Some(holding) = lock::running(&composition.state_dir(), &process::alive)
        && is_scheduled_by(scheduled_by.as_deref(), holding.pid)
    {
        return None;
    }
    if let Some(holding) = lock::running(&composition.state_dir(), &process::alive)
        && holding.version != version
    {
        eprintln!(
            "zaf {path}: the running scheduler ({}) builds another milestone than --version \
             {version}; no tick was asked for or run: pass --version {}, or stop it first",
            holding.whereabouts(),
            holding.version
        );
        return Some(REFUSED);
    }
    let asked = request::ask(
        &composition.state_dir(),
        "zaf spawn --once",
        std::time::SystemTime::now(),
        &process::alive,
    );
    match asked {
        Ok(None) => None,
        Ok(Some(holding)) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "asked": {
                            "pid": holding.pid,
                            "since": holding.since,
                            "zaf": holding.zaf,
                            "version": holding.version,
                        }
                    })
                );
            } else {
                println!(
                    "asked the running scheduler for a tick: {}",
                    holding.whereabouts()
                );
            }
            Some(OK)
        }
        Err(why) => {
            eprintln!("zaf {path}: {why}");
            Some(BACKEND)
        }
    }
}

/// Whether a tick has left its trace line, and why it stopped when it ended before it could; the
/// placement it acted on, and why it could not read the facts when it could not.
#[derive(Debug, Default)]
struct Traced {
    /// The tick wrote its line.
    written: bool,
    /// Why it stopped early, when it says.
    stopped: Option<String>,
    /// Where it placed every ticket before it acted: none when it stopped before.
    placement: Option<Placement>,
    /// Why the facts could not be read (the board, the forge or the run folders): the tick then
    /// started nothing.
    unread: Option<String>,
}

/// Appends `entry`'s line to the trace (docs/zaf-actions.md §5.1). A line that cannot be written
/// is one warning on stderr and fails nothing. Gives the line, with its moves.
fn write_trace(path: &str, composition: &Composition, entry: &Entry) -> trace::Line {
    let appended = ticks::append(
        &composition.state_dir(),
        entry,
        std::time::SystemTime::now(),
    );
    if let Some(why) = &appended.failed {
        eprintln!("zaf {path}: warning: the trace was not written: {why}");
    }
    appended.line
}

/// What a tick saw for its trace: the core's placement, or why it could not place.
struct Observed {
    /// The core's placement, or why there is none.
    seen: Result<Seen, String>,
}

impl Observed {
    /// Nothing seen, and why.
    fn failed(why: String) -> Self {
        Self { seen: Err(why) }
    }
}

/// `zaf spawn --watch --version <v>`: the scheduler that stays up ([`super::scheduler`]).
pub fn watch(cli: &Cli, version: Option<&str>) -> u8 {
    super::scheduler::run(cli, version)
}

/// The tick itself, everything between the `pre-tick` hook and the `post-tick` one, begun at
/// `when`. What its phase steps did is written to `outcome`; whether it wrote its trace line, and
/// why it stopped when it did not, the placement it acted on and why it could not read the facts,
/// to `traced`. A tick that cannot read the facts starts no phase, says so in its block and its
/// trace line, and names the placement the scheduler keeps (`when.kept`).
#[allow(
    clippy::too_many_arguments,
    reason = "the tick's two outputs ride beside its inputs"
)]
fn tick(
    cli: &Cli,
    path: &str,
    composition: &Composition,
    version: &str,
    dry_run: bool,
    when: &When<'_>,
    outcome: &mut TickOutcome,
    traced: &mut Traced,
) -> u8 {
    let began = when.began;
    let path = path.to_owned();
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => {
            traced.stopped = Some(error.to_string());
            return exit::fail_composition(&path, &error);
        }
    };
    // Each launch thread builds its own runner; a missing one stops the tick here, before planning.
    for role in [spawn::PLANNING_ROLE, CHALLENGER_ROLE] {
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
        if !dry_run {
            let lines: Vec<String> = stale.iter().map(|profile| profile.line()).collect();
            traced.stopped = Some(lines.join("; "));
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
        Err(error) => {
            traced.stopped = Some(error.to_string());
            if let SpawnError::Port(_) = error {
                traced.unread = Some(error.to_string());
            }
            return failed(&path, &error);
        }
    };
    // What a person reads of the tick (`spawn::digest`): its head before the launches run, its
    // tail after. `--json` prints none of it.
    let mut block = spawn::digest::Block::new(began, clock::utc_offset(began));
    block.asked = when.asked.map(Asked::line);
    block.did.extend(
        prepared
            .lost
            .iter()
            .map(|instance| format!("lost {instance}")),
    );
    block.did.extend(
        prepared
            .released
            .iter()
            .map(|released| format!("released {} from {}", released.task, released.instance)),
    );
    block.slots = prepared
        .plan
        .render_text()
        .lines()
        .find(|line| line.starts_with("slots "))
        .map(str::to_owned);
    block.skipped = prepared.plan.skipped_lines();
    let mut code = OK;
    // Steps 3 to 6 run before the launches, which run to their exit. With no `[phases]` there is
    // no ticket to place, and nothing is read for the trace; the labels are set all the same.
    let mut held: Vec<Held> = Vec::new();
    let (phases, mut observed, labelling) = if manifest.phases.is_empty() {
        let none = Observed {
            seen: Ok(Seen::default()),
        };
        let labelling = unplaced_labels(board.as_ref(), &prepared, labels::sync);
        traced.placement = Some(Placement::default());
        (None, none, labelling)
    } else {
        let (tick, phase_code, observed, labelling) =
            phase_tick(&path, composition, board.as_ref(), &prepared, version);
        code = code.max(phase_code);
        match &observed.seen {
            Ok(_) => traced.placement = Some(tick.placement.clone()),
            // The facts could not be read: no phase started, and the placement kept stands.
            Err(why) => {
                traced.unread = Some(why.clone());
                let kept = when.kept;
                block.did.push(format!(
                    "could not read the facts: {why}{}",
                    kept_words(kept.map(|at| spawn::digest::clock(at, clock::utc_offset(at))))
                ));
            }
        }
        (Some(tick), observed, labelling)
    };
    // A board that failed while labelling fails the tick, whose other steps still ran.
    block.did.extend(labelling.lines());
    for failure in &labelling.failed {
        eprintln!("zaf {path}: {failure}");
        code = BACKEND;
    }
    if let Some(steps) = &phases {
        block
            .did
            .extend(steps.outcome.render_text().lines().map(str::to_owned));
        // The tickets their epic or a question holds back, as the old gate worded them.
        block.skipped.extend(steps.held.iter().map(Held::line));
        held.clone_from(&steps.held);
        // What the `post-tick` hook gets: the tick's own steps, not an empty outcome.
        outcome.clone_from(&steps.outcome);
    }
    // A launch of planning or the challenger is a line of its own: the outcome above holds only
    // the phase steps, so a tick that launched one is no idle tick.
    block.did.extend(
        prepared
            .plan
            .launches
            .iter()
            .map(|launch| format!("launching {}", launch.instance)),
    );
    // The trace line, once the phase steps have run and before the launches do: its moves head
    // the block. One whose facts could not be read says what the tick did instead.
    if let Err(why) = &mut observed.seen {
        why.push_str(&kept_words(when.kept.map(shadow_time)));
    }
    let entry = Entry {
        at: began,
        did: Did::of(
            outcome,
            &labelling.unblocked,
            &prepared.plan.launches,
            &prepared.released,
            &prepared.lost,
        )
        .with_relabelled(&labelling.relabelled),
        skipped: block.skipped.clone(),
        seen: observed.seen,
    };
    let line = write_trace(&path, composition, &entry);
    traced.written = true;
    if let Ok(seen) = &entry.seen {
        block.places = trace::place_lines(seen, &line.moved, cli.more);
    }
    if !cli.json {
        print!("{}", block.head());
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
    let mut launched: Vec<Launched> = Vec::new();
    for result in spawn::launch_all(&jobs, &ports) {
        match result {
            Ok(done) => {
                if let Some(error) = done.exit.problem() {
                    eprintln!("zaf {path}: {}: {error}", done.instance);
                    code = BACKEND;
                }
                block.launched.push(format!(
                    "launched {}: exit {} ({})",
                    done.instance,
                    done.exit.status(),
                    done.run_dir.join(spawn::REPORT_FILE).display()
                ));
                launched.push(done);
            }
            Err(error) => code = code.max(failed(&path, &error)),
        }
    }
    if !cli.json {
        read_block(composition, board.as_ref(), &mut block);
        print!("{}", block.tail());
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
        json["held_tasks"] = held_json(&held);
        json["unblocked"] = labelling.to_json();
        json["relabelled"] = labelling.relabelled_json();
        if let Some(steps) = &phases {
            json["tick"] = steps.outcome.to_json();
        }
        if let Some(asked) = when.asked {
            json["asked_by"] = asked.who().into();
        }
        json["moved"] = line.json["moved"].clone();
        json["tickets"] = line.json["tickets"].clone();
        println!("{json}");
    }
    if let Err(message) = super::report::ingest_ledger(composition) {
        eprintln!("zaf {path}: warning: {message}");
    }
    code
}

/// The tickets their epic or a question holds back, as `--json` carries them.
fn held_json(held: &[Held]) -> serde_json::Value {
    held.iter()
        .map(|hold| serde_json::json!({ "task": hold.ticket.get(), "reason": hold.reason }))
        .collect()
}

/// Fills the block's reads: what runs now, the open questions and the unsigned epics. These are
/// the reads `zaf status` makes; one that fails becomes a `could not read` line and never fails
/// the tick.
fn read_block(composition: &Composition, board: &dyn Board, block: &mut spawn::digest::Block) {
    let now = std::time::SystemTime::now();
    let state = spawn::derive(&composition.state_dir(), &process::alive)
        .map(|(state, _)| state)
        .map_err(|error| error.to_string());
    block.read_running(state.as_ref().map_err(Clone::clone), now);
    let phases = spawn::live_phases(&composition.state_dir(), &process::alive)
        .map_err(|error| error.to_string());
    block.read_phases(phases.as_deref().map_err(Clone::clone), now);
    let questions = board
        .questions()
        .map(|questions| {
            questions
                .iter()
                .map(|question| question.id)
                .collect::<Vec<_>>()
        })
        .map_err(|error| error.to_string());
    // As `zaf status` reads them: a board that rejects the listing has no epics to show.
    let unsigned = match board.epics() {
        Ok(epics) => Ok(spawn::digest::unsigned_epics(&epics)),
        Err(PortError::Rejected(_)) => Ok(Vec::new()),
        Err(error) => Err(error.to_string()),
    };
    block.read_waiting(
        questions.as_deref().map_err(Clone::clone),
        unsigned.as_deref().map_err(Clone::clone),
    );
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
                let now = std::time::SystemTime::now();
                println!(
                    "{}",
                    spawn::digest::skipped_line(now, clock::utc_offset(now))
                );
            }
            Some(OK)
        }
        Err(error) => Some(failed(path, &error)),
    }
}

/// What an inline run's record is written from: the ports the gate reads the ticket through again
/// (the board, the forge, the run folders) and the tick's own process, which the run folder names
/// while the action runs.
pub struct InlineRecord<'a> {
    /// What the gate reads.
    pub ports: &'a Ports<'a>,
    /// This process; none when its start time cannot be read, and no folder is opened.
    pub this: Option<&'a Holder>,
}

/// Why an inline run's folder was not opened.
enum NotOpened {
    /// The gate refused it now: the action is not run.
    Refused(String),
    /// The folder could not be written: the action runs all the same.
    Failed(String),
}

impl InlineRecord<'_> {
    /// Opens the run folder of `action` on `task` once the gate, asked again under the lock, lets
    /// it run (see [`spawn::phase::open_inline`]).
    fn open(&self, task: TicketId, action: &ActionName) -> Result<PathBuf, NotOpened> {
        let Some(this) = self.this else {
            return Err(NotOpened::Failed(
                "cannot read this process's start time".to_owned(),
            ));
        };
        spawn::phase::open_inline(self.ports, task, action, this).map_err(|error| match error {
            PhaseError::Refused(reason) => NotOpened::Refused(reason),
            PhaseError::Failed(reason) => NotOpened::Failed(reason),
        })
    }

    /// Ends the run folder `run_dir` of `task` once its action has run to `outcome`: what `zaf do`
    /// would have printed as its `output` (the headline, and the log's last lines on a failure),
    /// then the head it left and the exit, last ([`record::end`]).
    fn end(&self, run_dir: &Path, task: TicketId, outcome: &RunOutcome) -> Result<(), String> {
        let mut text = String::new();
        if let Some(headline) = outcome.headline() {
            text.push_str(&headline);
            text.push('\n');
        }
        if outcome.result == RunResult::Fail
            && !outcome.on_terminal
            && let Some(log) = &outcome.log
            && let Ok(lines) = last_lines(log, super::do_action::TAIL_LINES)
        {
            for line in lines {
                text.push_str(&line);
                text.push('\n');
            }
        }
        let output = record::record_output(run_dir, &text);
        let head_end = record::head_now(Some(self.ports.forge), self.ports.root, task);
        let ended = record::end(run_dir, i32::from(outcome.exit), &head_end, &|| {});
        output.and(ended)
    }
}

/// Runs `zaf do <action> <task>` in this process: the runner the tick hands
/// [`spawn::run_zaf_actions`] for a `zaf`-kind action. It runs under the tick's `version`, whatever
/// this process's environment holds: a `target: task` action names its slice by it, `{version}` in
/// `[env]` is filled with it and the body gets it as `ZAF_VERSION`, for the `zaf do` calls it
/// makes. A failure prints its headline on stderr and the code is passed on: 0 passed, 125 never
/// ran, any other the action's own.
///
/// The run leaves a phase run folder, as a launched phase does ([`InlineRecord`]): opened once the
/// gate, asked again under `spawn.lock`, lets the action run, with no `pid` and an `inline` mark
/// naming this process, so the run reads as running until it ends; ended with its `output`, the
/// head it left and its exit once it has. The action runs with `ZAF_RUN` naming the folder, so a
/// question it raises is recorded in its run. A gate that refuses now (another tick's run of the
/// same action is there, or the ticket moved on) runs nothing: one `NOT_RUN` line on stderr and
/// the code 125, which never counts toward getting nowhere. A record that cannot be written is a
/// warning on stderr; the action runs all the same.
pub fn run_zaf_action(
    record: &InlineRecord<'_>,
    task: TicketId,
    action: &ActionName,
    version: &str,
) -> u8 {
    let not_recorded = |reason: &str| {
        eprintln!("zaf spawn: warning: the run of {action} on {task} is not recorded: {reason}")
    };
    let run_dir = match record.open(task, action) {
        Ok(run_dir) => Some(run_dir),
        Err(NotOpened::Refused(reason)) => {
            eprintln!("zaf spawn: NOT_RUN {action} on {task}: {reason}");
            return NOT_RUN_EXIT;
        }
        Err(NotOpened::Failed(reason)) => {
            not_recorded(&reason);
            None
        }
    };
    // The body's process, recorded as soon as it starts: the run reads as running while it lives,
    // even once this tick has gone.
    let started = |pid: u32| {
        if let Some(run_dir) = &run_dir
            && let Some(holder) = process::holder_of(pid)
            && let Err(reason) = record::mark_inline_body(run_dir, &holder)
        {
            not_recorded(&reason);
        }
    };
    let outcome = super::do_action::run_named_in(
        action.as_str(),
        &[std::ffi::OsString::from(task.get().to_string())],
        None,
        Terminal::Absent,
        super::do_action::VersionFrom::Given(version),
        run_dir.as_deref().map(|dir| RunStep {
            dir,
            started: &started,
        }),
    );
    if let Some(headline) = outcome.headline() {
        eprintln!("{headline}");
    }
    if let Some(run_dir) = &run_dir
        && let Err(reason) = record.end(run_dir, task, &outcome)
    {
        not_recorded(&reason);
        // A folder whose exit could not be written must not read as running for ever.
        if let Err(reason) = record::clear_inline(run_dir) {
            not_recorded(&reason);
        }
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

/// The command that runs this zaf binary, from the one path to start it with
/// ([`binary::path_to_start`]): a binary replaced on disk since this one started is started as the
/// file now there, never as the ` (deleted)` path no file has.
fn this_zaf() -> Result<Command, String> {
    let exe =
        binary::path_to_start().map_err(|why| format!("{why}, so no phase can be started"))?;
    Ok(Command::new(exe))
}

/// The phase steps of the tick (docs/zaf-actions.md §5.1, steps 3 to 6) on every open ticket whose
/// type has a `[phases]` list, and the exit code they leave: `2` when the forge cannot be reached,
/// the board cannot be read, a question cannot be raised or the model phase fails to start, `0`
/// otherwise, idle or not. A ticket that cannot be read is named on stderr and skipped.
///
/// The budget is the machine's: its live use and E across every project, read from the process
/// table (every live process carrying `ZAF_RUN`, an orphan of an ended run included), and against
/// `[spawn] slots` this project's planning and challenger launches (running, and reserved this
/// tick) with the live model runs of its run folders ([`Machine`], `Gathered::model_runs`).
///
/// What the trace keeps ([`Observed`]) is the placement of the same read the steps act on, and
/// the labels are set from it ([`labels::sync`]) before any step runs, so a phase that claims its
/// task finds it `ready`; what the sync did is given beside, its failures for the caller to name.
/// A tick that cannot gather sets no label.
fn phase_tick(
    path: &str,
    composition: &Composition,
    board: &dyn Board,
    prepared: &Prepared,
    version: &str,
) -> (PhaseTick, u8, Observed, Labelling) {
    let forge = match composition.forge() {
        Ok(forge) => forge,
        Err(error) => {
            eprintln!("zaf {path}: the phase steps did not run: {error}");
            let observed = Observed::failed(error.to_string());
            return (
                PhaseTick::default(),
                BACKEND,
                observed,
                Labelling::default(),
            );
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
    let gathered = match schedule_gather::gather(&ports) {
        Ok(gathered) => gathered,
        Err(error) => {
            eprintln!("zaf {path}: the phase steps did not run: {error}");
            let observed = Observed::failed(error.to_string());
            return (
                PhaseTick::default(),
                BACKEND,
                observed,
                Labelling::default(),
            );
        }
    };
    let observed = Observed {
        seen: Ok(Seen::of(&gathered, now)),
    };
    for skipped in &gathered.unreadable {
        eprintln!("zaf {path}: skipped {}: {}", skipped.ticket, skipped.reason);
    }
    for warning in &gathered.warnings {
        eprintln!("zaf {path}: warning: {warning}");
    }
    // After the placement, before the model start (and every other step).
    let labelling = labels::sync(board, &gathered, now);
    let machine = process::machine();
    let launches_here = usize::try_from(prepared.plan.slots.running).unwrap_or(usize::MAX)
        + prepared.plan.launches.len();
    let machine = Machine {
        units: live_use(&machine),
        e_live: e_is_live(&machine),
        launches_here,
        slots: usize::try_from(manifest.spawn.slots).unwrap_or(usize::MAX),
    };
    // An inline run is a step of this process, which its run folder names while it runs.
    let this = process::this();
    let record = InlineRecord {
        ports: &ports,
        this: this.as_ref(),
    };
    let run_inline = |task: TicketId, action: &ActionName, version: &str| {
        run_zaf_action(&record, task, action, version)
    };
    let (tick, code) = phase_steps(
        board,
        &state_dir,
        &gathered,
        now,
        &machine,
        &manifest.spawn.budget(),
        version,
        &this_zaf,
        &run_inline,
    );
    for failure in &tick.failed {
        eprintln!("zaf {path}: {failure}");
    }
    (tick, code, observed, labelling)
}

/// Runs steps 3 to 6 on `gathered` at `now` ([`schedule_tick::tick`]): each `zaf`-kind action
/// with `run_inline(task, action, version)` in this process ([`run_zaf_action`]), each wait and
/// the model phase started with `<zaf> spawn phase`, each question raised under the spawner's lock
/// of `state_dir`. Gives what they did with the exit code: `2`
/// when any step failed, `0` otherwise. All three run under the tick's `version`, whatever the
/// tick's own environment holds: `run_inline` is handed it, and each `zaf spawn phase` gets it as
/// `ZAF_VERSION`, so the actions and the phases can name their branches.
#[allow(
    clippy::too_many_arguments,
    reason = "the steps' three runners ride beside what they act on"
)]
fn phase_steps(
    board: &dyn Board,
    state_dir: &Path,
    gathered: &Gathered,
    now: std::time::SystemTime,
    machine: &Machine,
    budget: &Budget,
    version: &str,
    zaf: &dyn Fn() -> Result<Command, String>,
    run_inline: &dyn Fn(TicketId, &ActionName, &str) -> u8,
) -> (PhaseTick, u8) {
    let versioned = || -> Result<Command, String> {
        let mut command = zaf()?;
        command.env(VERSION_VAR, version);
        Ok(command)
    };
    let mut run = |task: TicketId, action: &ActionName| run_inline(task, action, version);
    let mut launch =
        |task: TicketId, action: &ActionName| spawn_phase_with(versioned()?, task, action);
    let mut start = |planned: &PlannedModel| start_model(versioned()?, planned);
    let mut log = |line: &str| eprintln!("{line}");
    let tick = schedule_tick::tick(
        board,
        state_dir,
        gathered,
        now,
        machine,
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
/// prints one document; its stderr is the tick's, passed on once it has ended. It runs in a
/// process group of its own, so a Ctrl-C meant for the scheduler does not stop a phase half
/// started; it never touches the terminal.
fn spawn_phase_with(mut zaf: Command, task: TicketId, action: &ActionName) -> Result<(), String> {
    let output = zaf
        .args(["spawn", "phase"])
        .arg(task.get().to_string())
        .arg(action.as_str())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .process_group(0)
        .output()
        .map_err(|error| format!("cannot run zaf spawn phase: {error}"))?;
    let _ = std::io::stderr().write_all(&output.stderr);
    let status = output.status;
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
    // The labels the tick would set, read as the tick reads them and written nowhere. A board
    // that failed while reading them fails the dry run, as it fails the tick; the rest is still
    // printed. A gather that fails sets no label in the tick either.
    let now = std::time::SystemTime::now();
    let gathered = dry_gather(composition, board.as_ref());
    let labelling = match &gathered {
        Ok(Some(gathered)) => labels::read(board.as_ref(), gathered, now),
        Ok(None) => unplaced_labels(board.as_ref(), &prepared, labels::read),
        Err(error) => {
            if !cli.json {
                eprintln!("zaf {path}: the labels were not read: {error}");
            }
            Labelling::default()
        }
    };
    for failure in &labelling.failed {
        eprintln!("zaf {path}: {failure}");
    }
    let code = if labelling.failed.is_empty() {
        OK
    } else {
        BACKEND
    };
    if cli.json {
        let mut json = prepared.plan.to_json(&[], &prepared.lost);
        json["dry_run"] = serde_json::json!(launches);
        let none = Gathered::default();
        let (placement, held) = match &gathered {
            Ok(gathered) => {
                let gathered = gathered.as_ref().unwrap_or(&none);
                (
                    crate::app::schedule::shadow::to_json(gathered, now),
                    schedule_tick::held(gathered, now),
                )
            }
            Err(error) => (serde_json::json!({ "error": error }), Vec::new()),
        };
        json["held_tasks"] = held_json(&held);
        json["unblocked"] = labelling.to_json();
        json["relabelled"] = labelling.relabelled_json();
        json["placement"] = placement;
        println!("{json}");
        return code;
    }
    for instance in &prepared.lost {
        println!("lost {instance} (dry run: not recorded)");
    }
    // The tick's own lines: the dry run sets no label.
    for line in labelling.lines() {
        println!("{line}");
    }
    print!("{}", prepared.plan.render_text());
    for launch in &launches {
        print!("{}", launch.render_text());
    }
    code
}

/// What the dry run reads the labels from and the JSON dry run places beside the plan
/// ([`crate::app::schedule::shadow`]) and names as held: every open ticket with a phase list,
/// gathered as the tick gathers them. With no `[phases]` there is nothing to place and nothing is
/// gathered: none. A forge that cannot be built, or a read every ticket needs that fails, is the
/// placement's `error`, and fails nothing else.
fn dry_gather(composition: &Composition, board: &dyn Board) -> Result<Option<Gathered>, String> {
    schedule_gather::gather_for(composition, board)
}

/// The labels of a project with no `[phases]`, whose tick places nothing: every open task, from
/// the open questions the plan read and the blockers each names, read or written by `labelling`
/// ([`labels::read`] or [`labels::sync`]). A board that cannot list the open tasks is the one
/// failure.
fn unplaced_labels(
    board: &dyn Board,
    prepared: &Prepared,
    labelling: fn(&dyn Board, &Gathered, std::time::SystemTime) -> Labelling,
) -> Labelling {
    match board.open_tickets() {
        Ok(open) => {
            let gathered = Gathered {
                open: crate::domain::OpenTicket::tickets(open),
                questions: prepared.questions.clone(),
                ..Gathered::default()
            };
            labelling(board, &gathered, std::time::SystemTime::now())
        }
        Err(error) => Labelling {
            failed: vec![format!("cannot list the open tasks: {error}")],
            ..Labelling::default()
        },
    }
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
        terminal: Terminal::Absent,
        run: None,
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

    /// The tasks of `tasks`, each with the one-action list `listed`, nothing run yet, size S,
    /// ready since 1: each is in the lane at its one action.
    fn gathered(tasks: &[u64], listed: &crate::app::actions::ListedPhase) -> Gathered {
        use crate::app::schedule::gather::{Beside, phases_of};
        let mut gathered = Gathered::default();
        for task in tasks {
            let list = vec![listed.clone()];
            gathered.tickets.push(crate::domain::schedule::TicketFacts {
                ticket: TicketId::from(*task),
                epic: None,
                open_questions: std::collections::BTreeSet::new(),
                dependencies: Vec::new(),
                pull: None,
                runs: Vec::new(),
                phases: phases_of(&list),
                size: crate::domain::TaskSize::S,
                priority: None,
                ruled: Vec::new(),
                ready_since: Some(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)),
            });
            gathered.beside.insert(
                TicketId::from(*task),
                Beside {
                    list,
                    epic: TicketId::from(1),
                    on_epic: std::collections::BTreeSet::new(),
                },
            );
        }
        gathered
    }

    /// `tasks`, each ready at `implement`, a model phase ending at an open pull request.
    fn ready_at_implement(tasks: &[u64]) -> Gathered {
        use crate::app::actions::{ActionKind, ListedPhase, PhaseEnd};
        gathered(
            tasks,
            &ListedPhase {
                action: ActionName::from("implement"),
                kind: ActionKind::Model,
                ends: PhaseEnd::PrOpen,
                on_fail: None,
            },
        )
    }

    /// `tasks`, each ready at `merge`, a `zaf`-kind action ending at the merge.
    fn ready_at_merge(tasks: &[u64]) -> Gathered {
        use crate::app::actions::{ActionKind, ListedPhase, PhaseEnd};
        gathered(
            tasks,
            &ListedPhase {
                action: ActionName::from("merge"),
                kind: ActionKind::Zaf,
                ends: PhaseEnd::Merged,
                on_fail: None,
            },
        )
    }

    /// An idle machine with two slots.
    fn idle() -> Machine {
        Machine {
            units: crate::domain::Units::default(),
            e_live: false,
            launches_here: 0,
            slots: 2,
        }
    }

    /// The tick's clock in these tests.
    fn now() -> std::time::SystemTime {
        std::time::UNIX_EPOCH + std::time::Duration::from_secs(100)
    }

    /// The in-process runner of a test whose tasks have no `zaf`-kind action ready.
    fn no_inline(task: TicketId, action: &ActionName, _: &str) -> u8 {
        panic!("no zaf-kind action is ready, yet {action} ran for {task}");
    }

    #[test]
    fn a_failed_launch_exits_two() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let calls = std::cell::Cell::new(0);
        let zaf = || {
            calls.set(calls.get() + 1);
            Ok(fake_zaf(dir.path(), "exit 1"))
        };
        let board = crate::fakes::FakeBoard::default();
        let (tick, code) = phase_steps(
            &board,
            dir.path(),
            &ready_at_implement(&[3, 5]),
            now(),
            &idle(),
            &Budget::default(),
            "0.3.0",
            &zaf,
            &no_inline,
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
    fn tick_hands_its_version_to_the_phase() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let seen = dir.path().join("version");
        let body = format!("printf %s \"$ZAF_VERSION\" > '{}'", seen.display());
        // The tick's own environment has no `ZAF_VERSION`, as in the bug.
        let zaf = || {
            let mut zaf = fake_zaf(dir.path(), &body);
            zaf.env_remove("ZAF_VERSION");
            Ok(zaf)
        };
        let board = crate::fakes::FakeBoard::default();
        let (_, code) = phase_steps(
            &board,
            dir.path(),
            &ready_at_implement(&[3]),
            now(),
            &idle(),
            &Budget::default(),
            "0.3.0",
            &zaf,
            &no_inline,
        );
        assert_eq!(code, OK);
        assert_eq!(std::fs::read_to_string(seen).expect("the version"), "0.3.0");
    }

    #[test]
    fn tick_hands_its_version_to_the_zaf_action() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        // No `zaf spawn phase` is started: the action runs in this process.
        let zaf = || Ok(fake_zaf(dir.path(), "exit 1"));
        let ran = std::cell::RefCell::new(Vec::new());
        let run_inline = |task: TicketId, action: &ActionName, version: &str| {
            ran.borrow_mut()
                .push((task, action.to_string(), version.to_owned()));
            0
        };
        let board = crate::fakes::FakeBoard::default();
        let (tick, code) = phase_steps(
            &board,
            dir.path(),
            &ready_at_merge(&[7]),
            now(),
            &idle(),
            &Budget::default(),
            "0.1",
            &zaf,
            &run_inline,
        );
        assert_eq!(code, OK, "{:?}", tick.failed);
        assert_eq!(
            ran.into_inner(),
            [(TicketId::from(7), "merge".to_owned(), "0.1".to_owned())]
        );
        assert_eq!(tick.outcome.ran.len(), 1, "{:?}", tick.outcome);
        assert!(!dir.path().join("args").exists(), "no phase was started");
    }

    #[test]
    fn a_started_phase_exits_zero() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let zaf = || Ok(fake_zaf(dir.path(), "exit 0"));
        let board = crate::fakes::FakeBoard::default();
        let (tick, code) = phase_steps(
            &board,
            dir.path(),
            &ready_at_implement(&[3]),
            now(),
            &idle(),
            &Budget::default(),
            "0.3.0",
            &zaf,
            &no_inline,
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
