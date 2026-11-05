//! The command line: every verb listed, each in its own module, exiting 64 until it is built.
//!
//! Terse by default, `--json` for the spawner, `--more` for depth. A verb module
//! owns the printing and the exit code; the use case it calls lives in
//! [`crate::app`] and sees only ports.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::app::setup::ensure_homes;
use crate::composition::Composition;
use crate::domain::IssueNumber;
use crate::manifest::ProjectFiles;

pub mod ask;
pub mod asklog;
pub mod board;
pub mod brief;
pub mod changes;
pub mod check;
pub mod do_action;
pub mod doctor;
pub mod exit;
pub mod guard;
pub mod hub;
pub mod init;
pub mod profile;
pub mod report;
pub mod setup;
pub mod slice;
pub mod spawn;
pub mod status;

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
        task: IssueNumber,
        /// Each question as `<kind>: <question>`.
        #[arg(required = true, num_args = 1..=5)]
        questions: Vec<String>,
    },
    /// Build a task's context pack: ticket, named pages, owned paths, style, help.
    Brief {
        /// The task.
        task: IssueNumber,
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
        task: IssueNumber,
        /// Show the hunks of this file.
        #[arg(long)]
        path: Option<String>,
    },
    /// Check a task's acceptance lines and declared interfaces.
    Verify {
        /// The task.
        task: IssueNumber,
    },
    /// Open, publish, merge or drop a task's slice.
    Slice {
        /// What to do with the slice.
        #[command(subcommand)]
        action: SliceAction,
    },
    /// Run the spawner's tick.
    Spawn {
        /// One tick, then exit.
        #[arg(long)]
        once: bool,
        /// The milestone's version the launched instances build (`ZAF_VERSION`).
        #[arg(long)]
        version: Option<String>,
        /// Plan the tick and print each launch's command line; launch nothing, write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// The ask log: top pages, recurring questions, gaps.
    Asklog,
    /// Board, running instances, slots, guard and open questions, on one screen.
    Status,
    /// The weekly table: tokens per role, findings by source, rounds per task, cycle time.
    Report(report::ReportArgs),
    /// The budget state the spawner reads.
    Guard,
    /// Write a minimal manifest by asking.
    Init {
        /// Read an earlier configuration file instead of asking.
        #[arg(long)]
        from_teams: bool,
    },
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
        /// LAN, and redraw it when a report arrives; 0 takes a free port. GET only, no login.
        #[arg(long, value_name = "PORT")]
        port: Option<u16>,
    },
}

/// `zaf hub …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum HubAction {
    /// Leave a report of the tool call on stdin: the profiles' `PostToolUse` hook. Prints
    /// nothing and exits 0 whatever it is given.
    Report,
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
        task: IssueNumber,
    },
    /// Write the instance's status comment on a task.
    Report {
        /// The task.
        task: IssueNumber,
        /// The report.
        body: String,
    },
    /// Raise a question for the Engineer on an issue.
    Question {
        /// The issue that waits.
        on: IssueNumber,
        /// The question.
        text: String,
        /// A one-way decision.
        #[arg(long)]
        door: bool,
    },
    /// Record the Engineer's ruling on a question.
    Ruling {
        /// The question.
        question: IssueNumber,
        /// The ruling.
        text: String,
    },
    /// Mark a task done once its work has landed: in land, or when its recorded pull request has merged.
    Done {
        /// The task.
        task: IssueNumber,
    },
}

/// `zaf slice …`.
#[derive(Debug, clap::Subcommand, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum SliceAction {
    /// Open the task's slice and workspace.
    Open {
        /// The task.
        task: IssueNumber,
    },
    /// Publish the slice and open or update its pull request.
    Pr {
        /// The task.
        task: IssueNumber,
    },
    /// Merge the slice's pull request when the merge conditions hold.
    Merge {
        /// The task.
        task: IssueNumber,
    },
    /// Drop the slice and its workspace.
    Drop {
        /// The task.
        task: IssueNumber,
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
        let action: Option<&'static str> = match self {
            Self::Board { action } => Some(action.into()),
            Self::Slice { action } => Some(action.into()),
            Self::Profile { action } => Some(action.into()),
            Self::Hub { action, .. } => action.as_ref().map(Into::into),
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
            if !cfg!(test) && !hook && !matches!(cli.verb, Verb::Setup) {
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
        Verb::Brief { task } => brief::run(cli, *task),
        Verb::Check { target, status } => check::run(cli, target, *status),
        Verb::Changes { task, path } => changes::run(cli, *task, path.as_deref()),
        Verb::Verify { task } => check::verify(cli, *task),
        Verb::Slice { action } => slice::run(cli, action),
        Verb::Asklog => asklog::run(cli),
        Verb::Init { from_teams } => init::run(cli, *from_teams),
        Verb::Doctor => doctor::run(cli),
        Verb::Setup => setup::run(cli),
        Verb::Status => status::run(cli),
        Verb::Report(args) => report::run(cli, args),
        Verb::Guard => guard::run(cli),
        Verb::Spawn {
            once,
            version,
            dry_run,
        } => spawn::run(cli, *once, version.as_deref(), *dry_run),
        Verb::Profile { action } => profile::run(cli, action),
        Verb::Do(args) => do_action::run(cli, args),
        Verb::Hub { action, port } => hub::run(cli, action.as_ref(), *port),
    }
}

#[cfg(test)]
mod tests;
