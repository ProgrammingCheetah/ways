//! Starting and stopping the build from the chair, and the controls beside it (F9, F15; D11).
//!
//! `b` starts the project's scheduler, `zaf spawn --watch --version <v>`, as a process of its own
//! that outlives the chair and the terminal (a new session, stdin from `/dev/null`, its output in a
//! log under `.zaf/logs/`); `<v>` is the version whose epics are signed and open, and the status
//! line asks which when there are several. It never goes through the hub's in-process watch
//! control, which ends with its process. Whether a scheduler runs is read from its lock
//! (`WatchState` in `ProjectState`, `NowView::watch` here). `B` stops it after its tick: SIGTERM
//! to the pid its lock names, which the scheduler takes as its stop. `x` stops the instance
//! running a ticket and frees its claim, as the hub's stop control does (`zaf spawn stop`); `t`
//! asks the running scheduler for a tick, as `zaf spawn --once` does while one runs. `q` stops
//! none of it.
//!
//! Each control is asked about on the status line first ([`Ask`]), and runs only on a yes. What
//! is pure is here: which version, what is asked, what a key answers, the scheduler's command
//! line and log. What runs is behind [`Controls`], the port the runtime calls on a thread of its
//! own; [`machine::Machine`] is this machine's.

pub mod machine;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::text::Span;

use super::keys::Key;
use super::panes::{Hint, hint};
use super::theme::Theme;
use crate::app::chair::{ChairState, ReleaseVersion, SquareState, WatchView};
use crate::app::hub::controls::{Failure, valid_version};
use crate::domain::screen::Meaning;
use crate::domain::{EpicStage, TicketId};

/// The keys that ask for a control: start and stop the build, tick now, stop a task.
pub const KEYS: [char; 4] = ['b', 'B', 't', 'x'];

/// A control the chair runs once the Engineer said yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Control {
    /// Start the project's scheduler for `version`, detached.
    StartBuild {
        /// The project's root.
        root: PathBuf,
        /// The version it builds.
        version: ReleaseVersion,
    },
    /// Stop the scheduler whose lock names `pid`, after its tick.
    StopBuild {
        /// The project's root.
        root: PathBuf,
        /// The pid the lock named when the Engineer was asked.
        pid: u32,
    },
    /// End the instance running `task` and free its claim.
    StopTask {
        /// The project's root.
        root: PathBuf,
        /// The task.
        task: TicketId,
    },
    /// Ask the running scheduler for a tick now.
    Tick {
        /// The project's root.
        root: PathBuf,
    },
}

impl Control {
    /// The project it acts on.
    pub fn root(&self) -> &Path {
        match self {
            Self::StartBuild { root, .. }
            | Self::StopBuild { root, .. }
            | Self::StopTask { root, .. }
            | Self::Tick { root } => root,
        }
    }

    /// What is said while it runs.
    pub fn doing_words(&self) -> String {
        match self {
            Self::StartBuild { version, .. } => format!("starting the build of {version}…"),
            Self::StopBuild { pid, .. } => format!("stopping the scheduler (pid {pid})…"),
            Self::StopTask { task, .. } => format!("stopping {task}…"),
            Self::Tick { .. } => "asking the scheduler for a tick…".to_owned(),
        }
    }

    /// What is said when the Engineer says no.
    pub fn not_words(&self) -> String {
        match self {
            Self::StartBuild { .. } => "the build was not started".to_owned(),
            Self::StopBuild { .. } => "the scheduler was not stopped".to_owned(),
            Self::StopTask { task, .. } => format!("{task} was not stopped"),
            Self::Tick { .. } => "no tick was asked for".to_owned(),
        }
    }

    /// What is said, before the reason, when it failed.
    fn failed_words(&self) -> String {
        match self {
            Self::StartBuild { version, .. } => format!("the build of {version} did not start"),
            Self::StopBuild { .. } => "the scheduler was not stopped".to_owned(),
            Self::StopTask { task, .. } => format!("{task} was not stopped"),
            Self::Tick { .. } => "no tick was asked for".to_owned(),
        }
    }
}

/// What the chair asks on the status line before a control runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Yes or no on `control`.
    Confirm {
        /// The question.
        words: String,
        /// What a yes runs.
        control: Control,
        /// What Enter says: yes for a start or a tick, no for a stop.
        yes: bool,
    },
    /// Which version to build, a digit each, lowest first.
    Version {
        /// The project's root.
        root: PathBuf,
        /// The versions whose epics are signed and open.
        versions: Vec<ReleaseVersion>,
    },
}

/// What a key does while the chair asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answered {
    /// Yes: run it.
    Run(Control),
    /// Ask this next.
    Ask(Ask),
    /// No: say this.
    Cancelled(String),
    /// Close the chair.
    Quit,
    /// Not an answer: keep asking.
    Waiting,
}

/// What `key` answers to `ask`.
pub fn answer(ask: &Ask, key: Key) -> Answered {
    match (ask, key) {
        (_, Key::CtrlC) => Answered::Quit,
        (Ask::Confirm { control, .. }, Key::Char('y' | 'Y'))
        | (
            Ask::Confirm {
                control, yes: true, ..
            },
            Key::Enter,
        ) => Answered::Run(control.clone()),
        (Ask::Confirm { control, .. }, Key::Char('n' | 'N') | Key::Esc | Key::Enter) => {
            Answered::Cancelled(control.not_words())
        }
        (Ask::Version { root, versions }, Key::Char(digit @ '1'..='9')) => {
            let index = digit as usize - '1' as usize;
            versions.get(index).map_or(Answered::Waiting, |version| {
                Answered::Ask(confirm_start(root, version))
            })
        }
        (Ask::Version { .. }, Key::Esc | Key::Char('n' | 'N')) => {
            Answered::Cancelled("the build was not started".to_owned())
        }
        _ => Answered::Waiting,
    }
}

/// The versions whose epics are signed and open (signed, planned or challenged, a `Release:`
/// line naming a version a command line may take), lowest first.
pub fn versions(state: &ChairState) -> Vec<ReleaseVersion> {
    let mut versions: Vec<ReleaseVersion> = state
        .epics
        .iter()
        .filter(|epic| {
            matches!(
                epic.stage,
                EpicStage::Signed | EpicStage::Planned | EpicStage::Challenged
            )
        })
        .filter_map(|epic| epic.release.clone())
        .filter(|version| valid_version(version.as_str()))
        .collect();
    versions.sort_by_key(|version| order(version.as_str()));
    versions.dedup();
    versions
}

/// A version's order: its dotted parts, numbers by value and words after them.
fn order(version: &str) -> Vec<(u64, String)> {
    version
        .split(['.', '-', '+'])
        .map(|part| {
            part.parse::<u64>()
                .map_or_else(|_| (u64::MAX, part.to_owned()), |n| (n, String::new()))
        })
        .collect()
}

/// The pid `B` sends SIGTERM to: the one the scheduler's lock names, while one runs.
pub fn stop_target(watch: Option<&WatchView>) -> Option<u32> {
    watch
        .filter(|watch| watch.running)
        .and_then(|watch| watch.pid)
}

const UNREAD: &str = "the scheduler could not be read; r reads again";

fn confirm_start(root: &Path, version: &ReleaseVersion) -> Ask {
    Ask::Confirm {
        words: format!("Start the build of {version}? It runs on after zaf closes."),
        control: Control::StartBuild {
            root: root.to_owned(),
            version: version.clone(),
        },
        yes: true,
    }
}

/// What `b` asks on `state`, or why there is nothing to ask.
pub fn start(state: &ChairState) -> Result<Ask, String> {
    let watch = state.now.watch.as_ref().ok_or_else(|| UNREAD.to_owned())?;
    if watch.running {
        let version = watch
            .version
            .as_ref()
            .map_or_else(|| "a version".to_owned(), ToString::to_string);
        return Err(format!(
            "a scheduler already builds {version}; B stops it after its tick"
        ));
    }
    let versions = versions(state);
    match versions.as_slice() {
        [] => Err("no signed epic is open: sign one, then b builds its version".to_owned()),
        [version] => Ok(confirm_start(&state.root, version)),
        _ => Ok(Ask::Version {
            root: state.root.clone(),
            versions,
        }),
    }
}

/// What `B` asks on `state`, or why there is nothing to ask.
pub fn stop(state: &ChairState) -> Result<Ask, String> {
    let watch = state.now.watch.as_ref().ok_or_else(|| UNREAD.to_owned())?;
    if !watch.running {
        return Err("no scheduler runs; b starts one".to_owned());
    }
    let pid = stop_target(Some(watch))
        .ok_or_else(|| "the scheduler's lock names no process; r reads again".to_owned())?;
    let version = watch
        .version
        .as_ref()
        .map_or_else(String::new, |version| format!(" of {version}"));
    Ok(Ask::Confirm {
        words: format!("Stop the scheduler{version} (pid {pid}) after its tick?"),
        control: Control::StopBuild {
            root: state.root.clone(),
            pid,
        },
        yes: false,
    })
}

/// What `t` asks on `state`, or why there is nothing to ask.
pub fn tick(state: &ChairState) -> Result<Ask, String> {
    let watch = state.now.watch.as_ref().ok_or_else(|| UNREAD.to_owned())?;
    if !watch.running {
        return Err("no scheduler runs to tick; b starts one, and it ticks at once".to_owned());
    }
    let version = watch
        .version
        .as_ref()
        .map_or_else(String::new, |version| format!(" of {version}"));
    Ok(Ask::Confirm {
        words: format!("Ask the scheduler{version} for a tick now?"),
        control: Control::Tick {
            root: state.root.clone(),
        },
        yes: true,
    })
}

/// What `x` asks about `ticket` on `state`, or why there is nothing to ask: only a ticket an
/// instance runs can be stopped.
pub fn stop_task(state: &ChairState, ticket: Option<TicketId>) -> Result<Ask, String> {
    let task = ticket.ok_or_else(|| "no ticket is selected".to_owned())?;
    let listed = state
        .now
        .running
        .iter()
        .any(|line| line.ticket == Some(task));
    let square = state
        .epics
        .iter()
        .flat_map(|epic| &epic.squares)
        .find(|square| square.ticket == task);
    if !listed && square.is_none_or(|square| square.state != SquareState::Running) {
        let word = square.map_or("not on an open epic", |square| square.state.word());
        return Err(format!("{task} is not running ({word}): nothing to stop"));
    }
    Ok(Ask::Confirm {
        words: format!("Stop {task}? Its instance ends and its claim is freed."),
        control: Control::StopTask {
            root: state.root.clone(),
            task,
        },
        yes: false,
    })
}

/// What `key`, one of [`KEYS`], asks on `state`, `ticket` being the one selected; none for
/// another key.
pub fn asked(
    key: char,
    state: &ChairState,
    ticket: Option<TicketId>,
) -> Option<Result<Ask, String>> {
    Some(match key {
        'b' => start(state),
        'B' => stop(state),
        't' => tick(state),
        'x' => stop_task(state, ticket),
        _ => return None,
    })
}

/// The keys the key bar shows while the chair asks.
pub fn hints(ask: &Ask) -> Vec<Hint> {
    match ask {
        Ask::Confirm { yes: true, .. } => vec![hint("Enter/y", "yes"), hint("n/Esc", "no")],
        Ask::Confirm { yes: false, .. } => vec![hint("y", "yes"), hint("Enter/n/Esc", "no")],
        Ask::Version { .. } => vec![hint("1-9", "build that version"), hint("Esc", "cancel")],
    }
}

/// The status line while the chair asks: the question, then `[Y/n]` with the answer Enter gives
/// in capitals and colour, or the versions to pick from.
pub fn spans(ask: &Ask, theme: Theme) -> Vec<Span<'static>> {
    let paint = |meaning: Meaning, text: String| Span::styled(text, theme.style(meaning));
    let mut spans = vec![paint(Meaning::Waiting, "? ".to_owned())];
    match ask {
        Ask::Confirm { words, yes, .. } => {
            spans.push(Span::styled(words.clone(), theme.bold()));
            spans.push(Span::raw(" ["));
            if *yes {
                spans.push(paint(Meaning::Done, "Y".to_owned()));
                spans.push(Span::raw("/n]"));
            } else {
                spans.push(Span::raw("y/"));
                spans.push(paint(Meaning::Done, "N".to_owned()));
                spans.push(Span::raw("]"));
            }
        }
        Ask::Version { versions, .. } => {
            spans.push(Span::styled(
                "Build which version?".to_owned(),
                theme.bold(),
            ));
            for (index, version) in versions.iter().take(9).enumerate() {
                spans.push(Span::raw("  "));
                spans.push(paint(Meaning::Verb, (index + 1).to_string()));
                spans.push(Span::raw(" "));
                spans.push(paint(Meaning::Noun, version.to_string()));
            }
            spans.push(paint(Meaning::Dim, "  · Esc cancels".to_owned()));
        }
    }
    spans
}

/// What runs the controls: this machine's processes, or a fake in a test. A refusal of the state
/// and a failure of the machine are told apart, as for the hub's controls.
pub trait Controls: Send + Sync {
    /// Starts the scheduler of the project at `root` for `version`, detached; what it says.
    fn start(&self, root: &Path, version: &ReleaseVersion) -> Result<String, Failure>;
    /// Sends SIGTERM to the scheduler of the project at `root` when its lock still names `pid`.
    fn stop(&self, root: &Path, pid: u32) -> Result<String, Failure>;
    /// Ends the instance running `task` in the project at `root` and frees its claim.
    fn stop_task(&self, root: &Path, task: TicketId) -> Result<String, Failure>;
    /// Asks the running scheduler of the project at `root` for a tick.
    fn tick(&self, root: &Path) -> Result<String, Failure>;
}

/// Runs `control` through `controls`: what it did, or why it did not, in a line.
pub fn carry_out(controls: &dyn Controls, control: &Control) -> Result<String, String> {
    let done = match control {
        Control::StartBuild { root, version } => controls.start(root, version),
        Control::StopBuild { root, pid } => controls.stop(root, *pid),
        Control::StopTask { root, task } => controls.stop_task(root, *task),
        Control::Tick { root } => controls.tick(root),
    };
    done.map_err(|(Failure::Refused(why) | Failure::Failed(why))| {
        format!("{}: {why}", control.failed_words())
    })
}

/// The scheduler's arguments: `spawn --watch --version <version>`.
pub fn watch_args(version: &ReleaseVersion) -> Vec<String> {
    vec![
        "spawn".to_owned(),
        "--watch".to_owned(),
        "--version".to_owned(),
        version.to_string(),
    ]
}

/// The scheduler's log under `.zaf/logs/`, stamped as a step's log is: the time in UTC to the
/// second, then `spawn-watch-<version>`.
pub fn log_name(at: SystemTime, version: &ReleaseVersion) -> String {
    let stamp: String = humantime::format_rfc3339_seconds(at)
        .to_string()
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect();
    format!("{stamp}_spawn-watch-{version}.log")
}

#[cfg(test)]
mod tests;
