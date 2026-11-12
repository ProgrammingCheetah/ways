//! `zaf status`: the board, running instances, slots, guard and open questions on one screen.
//!
//! Everything on it is read live. The board gives the ready tasks, the claims and the open
//! questions. The run folders and the process table give what is running: an instance runs
//! while the process recorded in its run folder does, so one that died frees its slot at
//! once, with no tick in between. A claim held by an instance that no longer runs is shown
//! as orphaned, not claimed: the next tick releases it, unless its task has a recorded pull request
//! or a running phase, which hold it ([`spawn::held`]). The guard line is whatever
//! `zaf guard` last wrote. The machine line counts every live zaf instance on the machine,
//! from any project, as the process table reads it when asked ([`Status::with_machine`]);
//! the budget line sets that use against the zaf home's `[spawn] budget`
//! ([`Status::with_budget`]).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::epic::EpicLine;
use super::guard::{self, GuardError, GuardState};
use super::profile::StaleProfile;
use super::spawn::{self, blockers, blockers::DeadLine};
use crate::domain::{
    Budget, DEFAULT_RESERVE, Mebibytes, SLOTS_DIR, SizeTable, SlotRecord, TaskTemplate, Ticket,
    TicketId, TicketSummary, Units,
};
use crate::ports::{Board, ClaimRecord, Holder, MachineInstance, PortError, live_use};

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
    /// The task it works on; none for an instance launched with no task (planning, the
    /// challenger), which holds its slot all the same.
    #[serde(default)]
    pub task: Option<u64>,
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

/// A build slot a gate run holds now, read from `.zaf/slots/<pid>`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SlotLine {
    /// The instance that runs the gate, or `the Engineer`.
    pub holder: String,
    /// The gate level: `full`, `light` or `tests`.
    pub level: String,
    /// When the run took the slot, RFC 3339.
    pub since: String,
}

/// The whole screen.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Status {
    /// The open epics, each with its stage; none on a board that cannot list them.
    pub epics: Vec<EpicLine>,
    /// Tasks open for a claim whose body follows the template.
    pub ready: Vec<TicketSummary>,
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
    /// The build slots gate runs hold now, one per live record in `.zaf/slots/`.
    pub held_slots: Vec<SlotLine>,
    /// The guard as last computed; none if `zaf guard` never ran.
    pub guard: Option<GuardState>,
    /// Questions still waiting for the Engineer.
    pub questions: Vec<TicketSummary>,
    /// Generated profiles older than the manifest or zaf, or with no stamp; `zaf spawn`
    /// refuses them. [`status`] leaves it empty, [`Status::with_stale`] fills it.
    pub stale: Vec<StaleProfile>,
    /// Blocker cycles among the open tasks reached from the ready ones, each from its
    /// lowest task: the first waits for the second, the last for the first.
    pub cycles: Vec<Vec<u64>>,
    /// Blockers those tasks name that never close.
    pub dead_blockers: Vec<DeadLine>,
    /// Every live zaf instance on the machine, from any project; empty until
    /// [`Status::with_machine`] gives the process table's read.
    pub machine: Machine,
    /// The machine budget against the live use; no machine cap until
    /// [`Status::with_budget`] gives the manifest's.
    pub budget: BudgetLine,
}

/// The machine budget and how much of it the live instances use, read when asked and never
/// stored.
///
/// The text screen prints it as `budget <used> of <units> units, reserve <n>`, or
/// `budget: no machine cap` when the zaf home sets no `[spawn] budget`; the JSON carries the
/// use in units, the budget, the reserve and the size table.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct BudgetLine {
    /// The live use of the machine, the machine line's total, in units.
    #[serde(rename = "in_use_units", serialize_with = "units_as_number")]
    pub in_use: Units,
    /// The machine's `[spawn] budget`, in units; none means no machine cap.
    pub units: Option<u32>,
    /// The machine's `[spawn] reserve`, in units: what an E leaves free of the budget.
    pub reserve: u32,
    /// What each size costs, in units.
    pub sizes: SizeTable,
}

impl Default for BudgetLine {
    /// No machine cap, nothing in use, the default reserve and the default sizes.
    fn default() -> Self {
        Self {
            in_use: Units::default(),
            units: None,
            reserve: DEFAULT_RESERVE,
            sizes: SizeTable::default(),
        }
    }
}

/// Units as a plain number of units, `34.5`, rather than the mebibytes they hold.
fn units_as_number<S: serde::Serializer>(units: &Units, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_f64(units.as_f64())
}

/// The live zaf instances of the whole machine, read from the process table when asked.
///
/// The text screen prints it as `machine: <n> instances in <m> projects, <u> units in use`;
/// the JSON carries the total as `memory_mib`, mebibytes, which a unit is 1024 of.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Machine {
    /// How many projects they come from: their distinct roots.
    pub projects: usize,
    /// The live use of the machine: the memory of every instance, summed.
    #[serde(rename = "memory_mib")]
    pub in_use: Units,
    /// Each instance once.
    pub instances: Vec<MachineLine>,
}

/// One live instance of the machine.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct MachineLine {
    /// The root of the project that launched it.
    pub root: PathBuf,
    /// Who it is, as `<role> instance <n> on #<epic>`, when its environment says.
    pub instance: Option<String>,
    /// Its size name; none for a launch that carries none.
    pub size: Option<String>,
    /// The resident memory of its processes, summed, in mebibytes.
    pub memory_mib: Mebibytes,
}

impl Machine {
    /// The screen's view of the machine read.
    pub fn read(instances: &[MachineInstance]) -> Self {
        let roots: BTreeSet<&Path> = instances.iter().map(|one| one.root.as_path()).collect();
        Self {
            projects: roots.len(),
            in_use: live_use(instances),
            instances: instances
                .iter()
                .map(|one| MachineLine {
                    root: one.root.clone(),
                    instance: one.instance.clone(),
                    size: one.size.map(|size| size.name().to_owned()),
                    memory_mib: one.memory,
                })
                .collect(),
        }
    }
}

/// Reads the running launches through the spawner's own reader, so both count alike.
fn read_spawn_state(
    state_dir: &Path,
    alive: &dyn Fn(&Holder) -> bool,
) -> Result<(spawn::SpawnState, Vec<spawn::runs::Run>), StatusError> {
    spawn::derive(state_dir, alive).map_err(spawn_error)
}

/// A spawner read's failure as the screen's.
fn spawn_error(error: spawn::SpawnError) -> StatusError {
    match error {
        spawn::SpawnError::Read { path, reason } => StatusError::SpawnState { path, reason },
        spawn::SpawnError::Write { path, source } => StatusError::SpawnState {
            path,
            reason: source.to_string(),
        },
        spawn::SpawnError::Port(error) => StatusError::Board(error),
    }
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
    let epics = match board.epics() {
        Ok(epics) => epics.iter().map(EpicLine::from).collect(),
        Err(PortError::Rejected(_)) => Vec::new(),
        Err(error) => return Err(error.into()),
    };
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
    for ticket in &ready_tickets {
        let problems = TaskTemplate::problems(ticket.body.as_str());
        if problems.is_empty() {
            ready.push(ticket.summary());
        } else {
            unparsed.push(UnparsedLine {
                number: ticket.id.get(),
                title: ticket.title.to_string(),
                problems: problems.iter().map(ToString::to_string).collect(),
            });
        }
    }
    let claims = board.claims()?;
    let held = spawn::held(state_dir, alive).map_err(spawn_error)?;
    let ended = spawn::ended_phases(state_dir, alive).map_err(spawn_error)?;
    let orphaned_claims = spawn::orphaned(&claims, &runs, &ended, alive, &held);
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
    let questions = board.questions()?.iter().map(Ticket::summary).collect();
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
        epics,
        ready,
        unparsed,
        claimed,
        orphaned,
        slots: Slots {
            total: slots,
            free: slots.saturating_sub(u32::try_from(in_use).unwrap_or(u32::MAX)),
        },
        held_slots: read_slots(state_dir, alive),
        running_count: in_use,
        instances,
        running: state.launches,
        guard: guard::read(state_dir)?,
        questions,
        stale: Vec::new(),
        cycles,
        dead_blockers,
        machine: Machine::default(),
        budget: BudgetLine::default(),
    })
}

/// The live records of `.zaf/slots/`, oldest first. A record whose process is gone is
/// dropped: its file is removed, best effort, so it never shows again.
fn read_slots(state_dir: &Path, alive: &dyn Fn(&Holder) -> bool) -> Vec<SlotLine> {
    let Ok(entries) = std::fs::read_dir(state_dir.join(SLOTS_DIR)) else {
        return Vec::new();
    };
    let mut live = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let pid = entry.file_name().to_string_lossy().parse::<u32>().ok();
        let record = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<SlotRecord>(&text).ok());
        // Not a record (or one still being written): shown nowhere, removed never.
        if let (Some(pid), Some(record)) = (pid, record) {
            let holder = Holder {
                pid,
                start: record.start.clone(),
            };
            if alive(&holder) {
                live.push(SlotLine {
                    holder: record.holder,
                    level: record.level,
                    since: record.since,
                });
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    live.sort_by(|a, b| a.since.cmp(&b.since).then_with(|| a.holder.cmp(&b.holder)));
    live
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
    /// The screen with the machine's live instances, as the process table reads them now.
    #[must_use]
    pub fn with_machine(mut self, instances: &[MachineInstance]) -> Self {
        self.machine = Machine::read(instances);
        self.budget.in_use = self.machine.in_use;
        self
    }

    /// The screen with the machine budget and size table, measured against the machine's
    /// live use.
    #[must_use]
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = BudgetLine {
            in_use: self.machine.in_use,
            units: budget.units,
            reserve: budget.reserve,
            sizes: budget.sizes,
        };
        self
    }

    /// The screen with the profiles `profile::stale` names.
    #[must_use]
    pub fn with_stale(mut self, stale: Vec<StaleProfile>) -> Self {
        self.stale = stale;
        self
    }

    /// The screen as text.
    pub fn render_text(&self) -> String {
        let mut out = String::new();
        let mut line = |text: String| {
            out.push_str(&text);
            out.push('\n');
        };
        for epic in &self.epics {
            line(format!("epic      {}", epic.render()));
        }
        line(format!(
            "ready     {}{}",
            self.ready.len(),
            numbers(self.ready.iter().map(|t| t.number.get()))
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
            numbers(self.questions.iter().map(|q| q.number.get()))
        ));
        line(format!("running   {}", self.running_count));
        for instance in &self.instances {
            let tasks: Vec<String> = instance
                .tasks
                .iter()
                .map(|task| format!("#{task}"))
                .collect();
            if tasks.is_empty() {
                line(format!("  {}", instance.instance));
            } else {
                line(format!("  {}: {}", instance.instance, tasks.join(" ")));
            }
        }
        line(format!(
            "slots     {} free of {}",
            self.slots.free, self.slots.total
        ));
        for held in &self.held_slots {
            line(format!(
                "slot: {} {} since {}",
                held.holder, held.level, held.since
            ));
        }
        line(format!(
            "machine: {} instances in {} projects, {} units in use",
            self.machine.instances.len(),
            self.machine.projects,
            self.machine.in_use
        ));
        line(match self.budget.units {
            Some(units) => format!(
                "budget {} of {units} units, reserve {}",
                self.budget.in_use, self.budget.reserve
            ),
            None => "budget: no machine cap".to_owned(),
        });
        line(format!(
            "guard     {}",
            self.guard.as_ref().map_or(
                "not computed (run `zaf guard`)".to_owned(),
                GuardState::line
            )
        ));
        for stale in &self.stale {
            line(stale.line());
        }
        for cycle in &self.cycles {
            let tasks: Vec<TicketId> = cycle.iter().copied().map(TicketId::from).collect();
            line(format!("blocker cycle: {}", blockers::render_cycle(&tasks)));
        }
        for dead in &self.dead_blockers {
            line(format!(
                "dead blocker #{} on #{}: {}",
                dead.blocker, dead.on, dead.reason
            ));
        }
        for question in &self.questions {
            line(format!("question  {} {}", question.number, question.title));
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
