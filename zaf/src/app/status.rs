//! `zaf status`: the board, running instances, slots, guard and open questions on one screen.
//!
//! Everything on it is read live. The board gives the ready tasks, the claims and the open
//! questions. The run folders and the process table give what is running: an instance runs
//! while the process recorded in its run folder does, so one that died frees its slot at
//! once, with no tick in between. A claim held by an instance that no longer runs is shown
//! as orphaned, not claimed: the next tick releases it. The guard line is whatever
//! `zaf guard` last wrote.

use std::path::{Path, PathBuf};

use super::guard::{self, GuardError, GuardState};
use super::spawn::{self, blockers, blockers::DeadLine};
use crate::domain::{IssueNumber, TaskTemplate};
use crate::ports::{Board, ClaimRecord, Holder, PortError, Ticket};

/// The spawner's state file under the state folder.
pub const SPAWN_STATE_FILE: &str = "spawn.state";

/// Why the status could not be read.
#[derive(Debug, thiserror::Error)]
pub enum StatusError {
    /// The board failed.
    #[error(transparent)]
    Board(#[from] PortError),
    /// `.zaf/spawn.state` is there but unreadable.
    #[error("cannot read {path}: {reason}")]
    SpawnState {
        /// The file.
        path: PathBuf,
        /// Why.
        reason: String,
    },
    /// `.zaf/guard.json` is there but unreadable.
    #[error(transparent)]
    Guard(#[from] GuardError),
}

/// A ticket as a line of the screen.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct TicketLine {
    /// Its issue number.
    pub number: u64,
    /// Its title.
    pub title: String,
}

/// A ready task whose body does not follow the template, and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UnparsedLine {
    /// Its issue number.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// One line per problem, as `zaf board lint` prints them.
    pub problems: Vec<String>,
}

/// A live claim.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ClaimLine {
    /// The task.
    pub task: u64,
    /// The instance holding it.
    pub instance: String,
}

/// One task of a launch that runs now, read from its run folder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Launch {
    /// The task it works on.
    pub task: u64,
    /// The instance, as `<role> instance <n> on #<epic>`.
    pub instance: String,
    /// The process holding it, when known: the tick until the instance starts, then the instance.
    #[serde(default)]
    pub pid: Option<u32>,
    /// When it started, RFC 3339, when known.
    #[serde(default)]
    pub started_at: Option<String>,
}

/// A running instance and the tasks it carries.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct InstanceLine {
    /// The instance, as `<role> instance <n> on #<epic>`.
    pub instance: String,
    /// Its tasks.
    pub tasks: Vec<u64>,
}

/// The slots: how many there are and how many are free (a slot is one distinct running instance).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Slots {
    /// The manifest's `[spawn] slots`.
    pub total: u32,
    /// Slots without a launch.
    pub free: u32,
}

/// The whole screen.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Status {
    /// Tasks open for a claim whose body follows the template.
    pub ready: Vec<TicketLine>,
    /// Ready tasks whose body does not, which the spawner skips.
    pub unparsed: Vec<UnparsedLine>,
    /// Claims held now by an instance that runs, or by one this machine did not launch.
    pub claimed: Vec<ClaimLine>,
    /// Claims held by an instance this machine launched that no longer runs: free, and
    /// released by the next tick.
    pub orphaned: Vec<ClaimLine>,
    /// Running instances, read from the run folders: one slot each, however many tasks.
    pub running_count: usize,
    /// The running instances, each once with the tasks it carries.
    pub instances: Vec<InstanceLine>,
    /// Every running launch, one per task.
    pub running: Vec<Launch>,
    /// The slots.
    pub slots: Slots,
    /// The guard as last computed; none if `zaf guard` never ran.
    pub guard: Option<GuardState>,
    /// Questions still waiting for the Engineer.
    pub questions: Vec<TicketLine>,
    /// Blocker cycles among the open tasks reached from the ready ones, each from its
    /// lowest task: the first waits for the second, the last for the first.
    pub cycles: Vec<Vec<u64>>,
    /// Blockers those tasks name that never close.
    pub dead_blockers: Vec<DeadLine>,
}

/// Reads the running launches through the spawner's own reader, so both count alike.
fn read_spawn_state(
    state_dir: &Path,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<(spawn::SpawnState, Vec<spawn::runs::Run>), StatusError> {
    spawn::derive(state_dir, alive).map_err(|error| match error {
        spawn::SpawnError::Read { path, reason } => StatusError::SpawnState { path, reason },
        spawn::SpawnError::Write { path, source } => StatusError::SpawnState {
            path,
            reason: source.to_string(),
        },
        spawn::SpawnError::Port(error) => StatusError::Board(error),
    })
}

/// Reads the board, the run folders and `guard.json` into one screen; `alive` says whether a
/// recorded process still runs.
pub fn status(
    board: &dyn Board,
    state_dir: &Path,
    slots: u32,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<Status, StatusError> {
    let (state, runs) = read_spawn_state(state_dir, alive)?;
    let ready_tickets = board.ready()?;
    let found = blockers::read(board, &ready_tickets)?;
    let cycles = blockers::cycles(&found.graph)
        .iter()
        .map(|cycle| blockers::cycle_numbers(cycle))
        .collect();
    let dead_blockers = blockers::dead(&found.graph, &found.dead)
        .iter()
        .map(blockers::DeadLine::from)
        .collect();
    let mut ready = Vec::new();
    let mut unparsed = Vec::new();
    for ticket in ready_tickets {
        let problems = TaskTemplate::problems(&ticket.body);
        if problems.is_empty() {
            ready.push(ticket_line(ticket));
        } else {
            unparsed.push(UnparsedLine {
                number: ticket.number.get(),
                title: ticket.title,
                problems: problems.iter().map(ToString::to_string).collect(),
            });
        }
    }
    let claims = board.claims()?;
    let orphaned_claims = spawn::orphaned(&claims, &runs, alive);
    let claim_line = |claim: &ClaimRecord| ClaimLine {
        task: claim.task.get(),
        instance: claim.instance.to_string(),
    };
    let claimed = claims
        .iter()
        .filter(|claim| !orphaned_claims.contains(claim))
        .map(claim_line)
        .collect();
    let orphaned = orphaned_claims.into_iter().map(claim_line).collect();
    let questions = board.questions()?.into_iter().map(ticket_line).collect();
    // A batch of tasks shares one instance and one slot; the state counts them.
    let in_use = state.running();
    let instances = state
        .running_instances()
        .into_iter()
        .map(|instance| InstanceLine {
            tasks: state.tasks_of(&instance),
            instance: instance.to_string(),
        })
        .collect();
    Ok(Status {
        ready,
        unparsed,
        claimed,
        orphaned,
        slots: Slots {
            total: slots,
            free: slots.saturating_sub(u32::try_from(in_use).unwrap_or(u32::MAX)),
        },
        running_count: in_use,
        instances,
        running: state.launches,
        guard: guard::read(state_dir)?,
        questions,
        cycles,
        dead_blockers,
    })
}

fn ticket_line(ticket: Ticket) -> TicketLine {
    TicketLine {
        number: ticket.number.get(),
        title: ticket.title,
    }
}

fn numbers(lines: impl Iterator<Item = u64>) -> String {
    let list: Vec<String> = lines.map(|number| format!("#{number}")).collect();
    if list.is_empty() {
        String::new()
    } else {
        format!(" ({})", list.join(" "))
    }
}

impl Status {
    /// The screen as text.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        let mut line = |text: String| {
            out.push_str(&text);
            out.push('\n');
        };
        line(format!(
            "ready     {}{}",
            self.ready.len(),
            numbers(self.ready.iter().map(|t| t.number))
        ));
        if !self.unparsed.is_empty() {
            line("Ready, does not parse:".to_owned());
            for task in &self.unparsed {
                line(format!("  #{} {}", task.number, task.title));
                for problem in &task.problems {
                    line(format!("    {problem}"));
                }
            }
        }
        line(format!(
            "claimed   {}{}",
            self.claimed.len(),
            numbers(self.claimed.iter().map(|c| c.task))
        ));
        if !self.orphaned.is_empty() {
            line(format!(
                "orphaned  {}{}",
                self.orphaned.len(),
                numbers(self.orphaned.iter().map(|c| c.task))
            ));
        }
        line(format!(
            "questions {}{}",
            self.questions.len(),
            numbers(self.questions.iter().map(|q| q.number))
        ));
        line(format!("running   {}", self.running_count));
        for instance in &self.instances {
            let tasks: Vec<String> = instance
                .tasks
                .iter()
                .map(|task| format!("#{task}"))
                .collect();
            line(format!("  {}: {}", instance.instance, tasks.join(" ")));
        }
        line(format!(
            "slots     {} free of {}",
            self.slots.free, self.slots.total
        ));
        line(format!(
            "guard     {}",
            self.guard.as_ref().map_or(
                "not computed (run `zaf guard`)".to_owned(),
                GuardState::line
            )
        ));
        for cycle in &self.cycles {
            let tasks: Vec<IssueNumber> = cycle.iter().copied().map(IssueNumber::from).collect();
            line(format!("blocker cycle: {}", blockers::render_cycle(&tasks)));
        }
        for dead in &self.dead_blockers {
            line(format!(
                "dead blocker #{} on #{}: {}",
                dead.blocker, dead.on, dead.reason
            ));
        }
        for question in &self.questions {
            line(format!("question  #{} {}", question.number, question.title));
        }
        out
    }

    /// The whole screen as JSON for the spawner.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }
}

#[cfg(test)]
mod tests;
