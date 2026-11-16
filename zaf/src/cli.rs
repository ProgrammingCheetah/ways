//! The command line: every verb listed, each in its own module, exiting 64 until it is built.
//!
//! Terse by default, `--json` for the spawner, `--more` for depth. A verb module
//! owns the printing and the exit code; the use case it calls lives in
//! [`crate::app`] and sees only ports.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::app::projects::Projects;
use crate::app::setup::ensure_homes;
use crate::composition::Composition;
use crate::domain::{DEFAULT_TASK_TYPE, TicketId};
use crate::manifest::ProjectFiles;
use crate::ports::PrNumber;

pub mod ask;
pub mod asklog;
pub mod board;
pub mod brief;
pub mod changes;
pub mod check;
pub mod do_action;
pub mod doctor;
pub mod epic;
pub mod exit;
pub mod forge;
pub mod guard;
pub mod hub;
pub mod profile;
pub mod progress;
pub mod report;
pub mod scheduler;
pub mod setup;
pub mod slice;
pub mod spawn;
pub mod spawn_phase;
pub mod spawn_stop;
pub mod spawn_trace;
pub mod status;
pub mod update;

pub use exit::NOT_IMPLEMENTED;

/// `zaf`: the verb set of the AI framework.
#[derive(Debug, clap::Parser)]
#[command(
    name = "zaf",
    version,
    about = "The verb set of the AI framework: one project-blind binary."
)]
pub struct Cli {
    /// Structured output for the spawner.
    #[arg(long, global = true)]
    pub json: bool,
    /// More depth than the terse default.
    #[arg(long, global = true)]
    pub more: bool,
    /// The verb to run.
    #[command(subcommand)]
    pub verb: Verb,
}

/// Every verb.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum Verb {
    /// Read and write the board.
    Board {
        /// What to do on the board.
        #[command(subcommand)]
        action: BoardAction,
    },
    /// Ask the keeper up to five questions about the design.
    Ask {
        /// The task whose named pages are searched first.
        #[arg(short, long)]
        task: TicketId,
        /// Each question as `<kind>: <question>`.
        #[arg(required = true, num_args = 1..=5)]
        questions: Vec<String>,
    },
    /// Build a task's context pack: ticket, named pages, owned paths, style, help.
    Brief {
        /// The task.
        task: TicketId,
        /// The phase action the pack is for, one of its task type's `[phases]` list; a task with
        /// a pull request then gets its diff, new findings and hand-off. The call is
        /// `zaf brief <task> --phase <action>`, refused with exit 1 for an unlisted action.
        #[arg(long)]
        phase: Option<String>,
    },
    /// Run the configured check steps for a task, a pull request or a milestone.
    Check {
        /// The target.
        target: String,
        /// Read the last results without running.
        #[arg(long)]
        status: bool,
    },
    /// A task's change: stat first, hunks on request.
    Changes {
        /// The task.
        task: TicketId,
        /// Show the hunks of this file.
        #[arg(long)]
        path: Option<String>,
    },
    /// Check a task's acceptance lines and declared interfaces.
    Verify {
        /// The task.
        task: TicketId,
    },
    /// Open, publish, merge or drop a task's slice.
    Slice {
        /// What to do with the slice.
        #[command(subcommand)]
        action: SliceAction,
    },
    /// Run the spawner's tick, or start one phase action of a task.
    #[command(args_conflicts_with_subcommands = true)]
    Spawn {
        /// What to start; none runs the tick.
        #[command(subcommand)]
        action: Option<SpawnAction>,
        /// One tick, then exit; while a scheduler runs (`--watch`), ask it for the tick instead.
        #[arg(long)]
        once: bool,
        /// The milestone's version the launched instances build (`ZAF_VERSION`).
        #[arg(long)]
        version: Option<String>,
        /// Plan the tick and print each launch's command line; launch nothing, write nothing.
        #[arg(long)]
        dry_run: bool,
        /// With `--once` and no scheduler running: exit 0 at once, doing nothing, when another
        /// tick holds `spawn.lock`.
        #[arg(long, requires = "once")]
        no_wait: bool,
        /// Run the project's scheduler in the foreground until SIGINT or SIGTERM: one tick at a
        /// time, at once, whenever asked and each `[spawn] tick_minutes`; it holds
        /// `.zaf/scheduler.lock` and starts again as a new zaf binary once one is installed.
        #[arg(long, conflicts_with_all = ["once", "dry_run"])]
        watch: bool,
    },
    /// The ask log: top pages, recurring questions, gaps.
    Asklog,
    /// Board, running instances, slots, guard and open questions, on one screen.
    Status,
    /// The weekly table: tokens per role, findings by source, rounds per task, cycle time.
    Report(report::ReportArgs),
    /// The budget state the spawner reads.
    Guard,
    /// Validate the manifest and verify the required tools.
    Doctor,
    /// Make the project's `.zaf/` and the zaf home, run doctor and print what is left to do.
    Setup,
    /// Manage the context profiles.
    Profile {
        /// What to do with the profiles.
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// List the actions, run one, or print the file a name resolves to.
    Do(do_action::DoArgs),
    /// The live view of what is happening; `zaf hub report` is the tool-call hook.
    #[command(args_conflicts_with_subcommands = true)]
    Hub {
        /// What to do; none draws the hub.
        #[command(subcommand)]
        action: Option<HubAction>,
        /// Serve the frame as a web page on every interface at this port, for anyone on the
        /// LAN, and redraw it when a report arrives; 0 takes a free port. Reading needs no login;
        /// a control needs the key the hub makes at start.
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
        /// Serve the page with every control refused, whatever key comes with it, and the state
        /// telling the page there are no controls.
        #[arg(long, requires = "port")]
        read_only: bool,
    },
    /// A model's phases on a task: declare them, then report each change.
    Progress {
        /// What to do with the phases.
        #[command(subcommand)]
        action: ProgressAction,
    },
    /// Replace this zaf with the newest release; up to date prints nothing.
    Update {
        /// Only say whether a newer release is out (exit 1 when one is); change nothing.
        #[arg(long)]
        check: bool,
    },
    /// The open epics and where each stands; the Engineer's signature on one.
    Epic {
        /// What to do with the epics.
        #[command(subcommand)]
        action: EpicAction,
    },
    /// The forge `[forge]` names: open and merge pull requests, read their checks and head.
    Forge {
        /// What to do on the forge.
        #[command(subcommand)]
        action: ForgeAction,
    },
}

/// `zaf forge …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum ForgeAction {
    /// Open a pull request for a branch into a base and print its number; one already open for
    /// the branch is handed back instead, and nothing is opened.
    Open {
        /// The branch the pull request carries.
        head: String,
        /// The branch it merges into.
        base: String,
        /// Its title.
        #[arg(allow_hyphen_values = true)]
        title: String,
        /// Its body; may be empty.
        #[arg(allow_hyphen_values = true)]
        body: String,
    },
    /// Merge a pull request with a merge commit, or squashed; one already merged is quiet. A
    /// merge the forge cannot make yet exits 1 saying `not mergeable yet`.
    Merge {
        /// The pull request, `7` or `#7`.
        pr: PrNumber,
        /// Squash its commits into one.
        #[arg(long)]
        squash: bool,
        /// Merge only while the pull request's head is this commit.
        #[arg(long, value_name = "SHA")]
        match_head_commit: Option<String>,
    },
    /// The checks on a pull request's head, one line each: bucket, name and link, tab-separated.
    Checks {
        /// The pull request, `7` or `#7`.
        pr: PrNumber,
    },
    /// The commit a pull request's head is at; nothing when the forge reports none.
    Head {
        /// The pull request, `7` or `#7`.
        pr: PrNumber,
    },
    /// What a person reads of a failed check's log: the job's failed lines, or the check's link
    /// on a forge with no log API; nothing when the link names no job.
    Log {
        /// The check's name, as `checks` prints it.
        #[arg(allow_hyphen_values = true)]
        name: String,
        /// The check's link, as `checks` prints it.
        link: String,
    },
}

/// `zaf epic …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum EpicAction {
    /// List the open epics, each with its stage and its tasks done.
    List,
    /// Show one open epic and its tasks, closed ones too, each with its state and group; with
    /// `--more`, the epic's body under them.
    Show {
        /// The epic.
        epic: TicketId,
    },
    /// Sign an epic as its body reads now; an edit voids it. Refused inside an instance.
    Sign {
        /// The epic.
        epic: TicketId,
    },
}

/// `zaf progress …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum ProgressAction {
    /// Declare the phases this model goes through on a task, in order.
    Plan {
        /// The task.
        task: TicketId,
        /// The phases, one word each.
        #[arg(required = true, num_args = 1..)]
        phases: Vec<String>,
        /// The role the phases are for; the role `ZAF_INSTANCE` names when not given.
        #[arg(long)]
        role: Option<String>,
    },
    /// Report that this model is now in one of its declared phases.
    At {
        /// The task.
        task: TicketId,
        /// The phase, as declared.
        phase: String,
        /// The role the phase is for; the role `ZAF_INSTANCE` names when not given.
        #[arg(long)]
        role: Option<String>,
    },
}

/// `zaf spawn …`, beside the flags of the tick.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum SpawnAction {
    /// Start a task's current phase action as a process of its own and return at once.
    Phase {
        /// The task.
        task: TicketId,
        /// The phase action: the task's current one.
        action: String,
    },
    /// End the instance running a task, with every process of its group, and release the task's
    /// claim, so a later tick can start it again.
    Stop {
        /// The task.
        task: TicketId,
    },
    /// Print where the tickets moved, tick by tick and oldest first, from the trace each real tick
    /// leaves in `.zaf/ticks.jsonl`; `--more` adds what each tick did. It only reads.
    Trace {
        /// Only this ticket's moves.
        ticket: Option<TicketId>,
        /// Only the ticks from this time on: an RFC 3339 time (`2026-11-12T17:00:00Z`), or a span
        /// back from now such as `2h` or `3d`.
        #[arg(long, value_parser = since_arg)]
        since: Option<std::time::SystemTime>,
    },
    /// The launcher `phase` starts: runs the action of a phase run folder and waits on it.
    #[command(hide = true)]
    PhaseRun {
        /// The phase run folder.
        run_folder: PathBuf,
    },
}

/// A `--since` of `zaf spawn trace`, read now ([`crate::app::schedule::trace::listing::since`]).
fn since_arg(text: &str) -> Result<std::time::SystemTime, String> {
    crate::app::schedule::trace::listing::since(text, std::time::SystemTime::now())
}

/// `zaf hub …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum HubAction {
    /// Leave a report of the tool call on stdin: the profiles' `PostToolUse` hook. Prints
    /// nothing and exits 0 whatever it is given.
    Report,
    /// Print the link that carries the running hub's key, from a terminal outside any instance:
    /// exit 1 inside an instance, or when no hub runs.
    Link,
    /// Run the hub as a systemd user service: it starts with the session on a fixed port and
    /// restarts when it stops.
    Service {
        /// What to do with the service.
        #[command(subcommand)]
        action: ServiceAction,
    },
}

/// `zaf hub service …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum ServiceAction {
    /// Write the unit, enable it and start it; a unit already there is replaced. Linux only.
    Install {
        /// The port the hub serves; 7777 when none is given.
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
    },
    /// Stop the service, disable it and delete the unit; none installed is no failure.
    Remove,
}

/// `zaf board …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum BoardAction {
    /// List the ready tasks.
    Ready,
    /// Claim a task under a lease.
    Claim {
        /// The task.
        task: TicketId,
    },
    /// Write the instance's status comment on a task.
    Report {
        /// The task.
        task: TicketId,
        /// The report.
        body: String,
    },
    // The verb as typed: zaf board handoff <task> "<text>"
    /// Leave the hand-off comment on a task's recorded pull request; a second call replaces it.
    Handoff {
        /// The task.
        task: TicketId,
        /// The hand-off.
        text: String,
    },
    /// Raise a question for the Engineer on an issue.
    Question {
        /// The issue that waits.
        on: TicketId,
        /// The question.
        text: String,
        /// A one-way decision.
        #[arg(long)]
        door: bool,
    },
    // The verb as typed: zaf board ruling <question> '<answer>' [--as-written]
    /// Record the Engineer's ruling on a question; an empty answer, or one with a gap a shell left, is refused.
    ///
    /// A gap is two or more spaces in a row inside a line: what a shell leaves where it ate
    /// text in backticks or after `$` inside double quotes. Such an answer exits 1 and nothing
    /// is written: quote the answer in single quotes, or pass --as-written.
    Ruling {
        /// The question.
        question: TicketId,
        /// The ruling; in single quotes, so the shell leaves it as typed.
        text: String,
        /// Send the ruling exactly as it is, gaps included; an empty one is still refused.
        #[arg(long)]
        as_written: bool,
    },
    /// Mark a task done once its work has landed: in land, or when its recorded pull request has merged.
    Done {
        /// The task.
        task: TicketId,
    },
    /// File a task under an epic, its body in the task template; a title the epic already has is not filed twice.
    File {
        /// The epic.
        epic: TicketId,
        /// The task's title.
        title: String,
        /// The task's body.
        #[arg(
            long,
            conflicts_with = "body_file",
            required_unless_present = "body_file"
        )]
        body: Option<String>,
        /// A file holding the task's body; `-` reads standard input.
        #[arg(long)]
        body_file: Option<PathBuf>,
        /// The task's group (its `group:<name>` label on GitHub).
        #[arg(long)]
        group: Option<String>,
        /// The task's issue type, by the forge's name; the repository must define it.
        #[arg(long = "type", value_name = "NAME", default_value = DEFAULT_TASK_TYPE)]
        kind: String,
        /// Open it blocked, not ready: its `Blockers` are not done yet.
        #[arg(long)]
        blocked: bool,
    },
    /// Name every field of a task's body that does not follow the template.
    Lint {
        /// The task.
        issue: TicketId,
    },
}

/// `zaf slice …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum SliceAction {
    /// Open the task's slice and workspace.
    Open {
        /// The task.
        task: TicketId,
    },
    /// Publish the slice and open or update its pull request.
    Pr {
        /// The task.
        task: TicketId,
    },
    /// Merge the slice's pull request when the merge conditions hold.
    Merge {
        /// The task.
        task: TicketId,
    },
    /// Drop the slice and its workspace.
    Drop {
        /// The task.
        task: TicketId,
    },
}

/// `zaf profile …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum ProfileAction {
    /// Generate the profiles from the manifest.
    Init,
}

impl Verb {
    /// The verb as typed, with its action for verbs that take one (`board claim`).
    pub fn path(&self) -> String {
        let verb: &'static str = self.into();
        if let Self::Hub {
            action: Some(HubAction::Service { action }),
            ..
        } = self
        {
            let action: &'static str = action.into();
            return format!("{verb} service {action}");
        }
        let action: Option<&'static str> = match self {
            Self::Board { action } => Some(action.into()),
            Self::Slice { action } => Some(action.into()),
            Self::Profile { action } => Some(action.into()),
            Self::Hub { action, .. } => action.as_ref().map(Into::into),
            Self::Spawn { action, .. } => action.as_ref().map(Into::into),
            Self::Progress { action } => Some(action.into()),
            Self::Epic { action } => Some(action.into()),
            Self::Forge { action } => Some(action.into()),
            _ => None,
        };
        action.map_or_else(|| verb.to_owned(), |action| format!("{verb} {action}"))
    }
}

/// Parses the arguments and runs the verb; returns the process exit code.
pub fn run<I, T>(args: I) -> u8
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    <Cli as clap::Parser>::try_parse_from(args).map_or_else(
        |error| {
            // Printing help or an error is best effort: a closed stream changes nothing.
            let _ = error.print();
            u8::try_from(error.exit_code()).unwrap_or(2)
        },
        |cli| {
            // The unit tests call `run` in-process from the crate's own folder: they must
            // never make the real zaf home or this repository's `.zaf/`.
            // The hook runs after every tool call and prints nothing: it makes nothing either.
            let hook = matches!(
                cli.verb,
                Verb::Hub {
                    action: Some(HubAction::Report),
                    ..
                }
            );
            // Updating the binary touches no project and makes no home.
            let homeless = matches!(cli.verb, Verb::Setup | Verb::Update { .. });
            if !cfg!(test) && !hook && !homeless {
                first_run();
            }
            dispatch(&cli)
        },
    )
}

/// Makes a missing project `.zaf/` and zaf home before the verb runs, one line on stderr when
/// it made anything and nothing on stdout. Outside a project only the zaf home is made. A
/// failure is reported and the verb still runs: it will say what it needs itself.
pub fn first_run() {
    let Ok(here) = std::env::current_dir() else {
        return;
    };
    first_run_at(&here, crate::composition::zaf_home().as_deref());
}

/// [`first_run`] from `here` with the zaf `home` given instead of read from the environment.
pub fn first_run_at(here: &Path, home: Option<&Path>) {
    let root = project_root(here, home);
    match ensure_homes(root.as_deref(), home) {
        Ok(homes) => {
            if let Some(notice) = homes.notice() {
                eprintln!("{notice}");
            }
        }
        Err(error) => eprintln!("zaf: cannot make the zaf homes: {error}"),
    }
    join_the_list(root.as_deref(), home);
}

/// Puts the project at `root` on the zaf home's list of projects: the first verb to run from
/// its root does it, and a project already on the list is left as it is. Outside a project, or
/// with no zaf home, there is nothing to add. A failure is reported and the verb still runs.
pub fn join_the_list(root: Option<&Path>, home: Option<&Path>) {
    if let (Some(root), Some(home)) = (root, home)
        && let Err(error) = Projects::add(home, root)
    {
        eprintln!("zaf: cannot add the project to the list in the zaf home: {error}");
    }
}

/// The project root for `here`: the one discovery finds, else, when a manifest is broken, the
/// nearest directory up the tree that holds any manifest file; `None` outside a project.
pub fn project_root(here: &Path, home: Option<&Path>) -> Option<PathBuf> {
    if let Ok(composition) = Composition::discover_with_home(here, home) {
        return Some(composition.root().to_owned());
    }
    here.ancestors()
        .find(|dir| !ProjectFiles::find(dir, home).is_empty())
        .map(Path::to_owned)
}

/// Runs a parsed command line: each verb goes to its module; an unbuilt one says so and exits 64.
/// A verb that changed something then leaves its report for the hub. The unit tests run verbs
/// in-process from the crate's own folder and must not write this repository's reports.
pub fn dispatch(cli: &Cli) -> u8 {
    let code = run_verb(cli);
    if !cfg!(test) {
        hub::leave_report(&cli.verb, code);
    }
    code
}

fn run_verb(cli: &Cli) -> u8 {
    match &cli.verb {
        Verb::Board { action } => board::run(cli, action),
        Verb::Ask { task, questions } => ask::run(cli, *task, questions),
        Verb::Brief { task, phase } => brief::run(cli, *task, phase.as_deref()),
        Verb::Check { target, status } => check::run(cli, target, *status),
        Verb::Changes { task, path } => changes::run(cli, *task, path.as_deref()),
        Verb::Verify { task } => check::verify(cli, *task),
        Verb::Slice { action } => slice::run(cli, action),
        Verb::Asklog => asklog::run(cli),
        Verb::Doctor => doctor::run(cli),
        Verb::Setup => setup::run(cli),
        Verb::Status => status::run(cli),
        Verb::Report(args) => report::run(cli, args),
        Verb::Guard => guard::run(cli),
        Verb::Spawn {
            action: Some(SpawnAction::Phase { task, action }),
            ..
        } => spawn_phase::run(cli, *task, action),
        Verb::Spawn {
            action: Some(SpawnAction::Stop { task }),
            ..
        } => spawn_stop::run(cli, *task),
        Verb::Spawn {
            action: Some(SpawnAction::Trace { ticket, since }),
            ..
        } => spawn_trace::run(cli, *ticket, *since),
        Verb::Spawn {
            action: Some(SpawnAction::PhaseRun { run_folder }),
            ..
        } => spawn_phase::launcher(cli, run_folder),
        Verb::Spawn {
            action: None,
            once,
            version,
            dry_run,
            no_wait,
            watch,
        } => {
            if *watch {
                spawn::watch(cli, version.as_deref())
            } else {
                spawn::run(cli, *once, version.as_deref(), *dry_run, *no_wait)
            }
        }
        Verb::Profile { action } => profile::run(cli, action),
        Verb::Do(args) => do_action::run(cli, args),
        Verb::Hub {
            action,
            port,
            read_only,
        } => hub::run(cli, action.as_ref(), *port, *read_only),
        Verb::Progress { action } => progress::run(cli, action),
        Verb::Update { check } => update::run(cli, *check),
        Verb::Epic { action } => epic::run(cli, action),
        Verb::Forge { action } => forge::run(cli, action),
    }
}

#[cfg(test)]
mod tests;
