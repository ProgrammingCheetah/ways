//! The command line: every verb listed, each in its own module, exiting 64 until it is built.
//!
//! Terse by default, `--json` for the spawner, `--more` for depth. A verb module
//! owns the printing and the exit code; the use case it calls lives in
//! [`crate::app`] and sees only ports.

use std::ffi::OsString;

use crate::domain::IssueNumber;

pub mod ask;
pub mod asklog;
pub mod board;
pub mod brief;
pub mod changes;
pub mod check;
pub mod doctor;
pub mod exit;
pub mod guard;
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
    /// Install what the manifest requires and generate the profiles.
    Setup,
    /// Manage the context profiles.
    Profile {
        /// What to do with the profiles.
        #[command(subcommand)]
        action: ProfileAction,
    },
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
        |cli| dispatch(&cli),
    )
}

/// Runs a parsed command line: each verb goes to its module; an unbuilt one says so and exits 64.
pub fn dispatch(cli: &Cli) -> u8 {
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
    }
}

#[cfg(test)]
mod tests;
