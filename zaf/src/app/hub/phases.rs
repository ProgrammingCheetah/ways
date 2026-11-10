//! The phases a model declares and reports (docs/zaf-actions.md §5 Model phases, §8 ruling 44).
//!
//! A model declares its phases once, `zaf progress plan`, and reports each change, `zaf progress
//! at`. The store is one file per task and role under `.zaf/hub/plans`, `<task>.<role>`: a first
//! line `plan <phase> <phase> …` and then one `at <phase>` line per change, oldest first. A phase
//! reported again after another counts an iteration, so the row can say `write ×2`. The hub reads
//! the files; only the verb writes them.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::domain::TicketId;

/// The folder holding the plans, under the project's state folder.
pub fn plans_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("hub").join("plans")
}

/// Why a plan or a change of phase was not stored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PhaseError {
    /// The role is empty or holds whitespace, a control character or a path separator.
    #[error("the role {0:?} is not one word")]
    Role(String),
    /// A phase is empty or holds whitespace or a control character.
    #[error("the phase {0:?} is not one word")]
    Word(String),
    /// A plan with no phase.
    #[error("a plan needs at least one phase")]
    Empty,
    /// A phase named twice in one plan.
    #[error("the phase `{0}` is declared twice")]
    Duplicate(String),
    /// A change of phase on a task and role with no plan.
    #[error("no plan for #{task} as {role}: declare one with `zaf progress plan`")]
    NoPlan {
        /// The task.
        task: u64,
        /// The role.
        role: String,
    },
    /// A phase the plan did not declare.
    #[error("the phase `{phase}` is not in the plan of #{task} as {role}: {declared}")]
    Undeclared {
        /// The phase reported.
        phase: String,
        /// The task.
        task: u64,
        /// The role.
        role: String,
        /// The phases the plan declares, space separated.
        declared: String,
    },
    /// The store could not be read or written.
    #[error("the plan store failed: {0}")]
    Store(String),
}

/// One role's declared phases on a task and where it has been.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhasePlan {
    /// The role that declared it.
    pub role: String,
    /// The declared phases, in order.
    pub phases: Vec<String>,
    /// The phases reported, oldest first, a phase reported twice running counted once.
    pub trail: Vec<String>,
}

impl PhasePlan {
    /// The phase last reported; none before the first report.
    pub fn current(&self) -> Option<&str> {
        self.trail.last().map(String::as_str)
    }

    /// How many times the current phase has been entered: 1 the first time, 2 when it was
    /// reported again after another phase, and so on. Zero before the first report.
    pub fn iteration(&self) -> u32 {
        let Some(current) = self.current() else {
            return 0;
        };
        let entered = self.trail.iter().filter(|phase| *phase == current).count();
        u32::try_from(entered).unwrap_or(u32::MAX)
    }

    /// The current phase's place among the declared ones, from 1; zero before the first report.
    pub fn step(&self) -> usize {
        self.current()
            .and_then(|current| self.phases.iter().position(|phase| phase == current))
            .map_or(0, |index| index + 1)
    }

    fn text(&self) -> String {
        let mut text = format!("plan {}\n", self.phases.join(" "));
        for phase in &self.trail {
            text.push_str(&format!("at {phase}\n"));
        }
        text
    }

    fn read(role: &str, text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let phases: Vec<String> = lines
            .next()?
            .strip_prefix("plan ")?
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if phases.is_empty() {
            return None;
        }
        let trail = lines
            .filter_map(|line| line.strip_prefix("at "))
            .map(|phase| phase.trim().to_owned())
            .filter(|phase| !phase.is_empty())
            .collect();
        Some(Self {
            role: role.to_owned(),
            phases,
            trail,
        })
    }
}

fn plan_file(state_dir: &Path, task: TicketId, role: &str) -> PathBuf {
    plans_dir(state_dir).join(format!("{}.{role}", task.get()))
}

fn check_role(role: &str) -> Result<(), PhaseError> {
    let bad = |c: char| c.is_whitespace() || c.is_control() || matches!(c, '/' | '\\');
    if role.is_empty() || role.chars().any(bad) {
        return Err(PhaseError::Role(role.to_owned()));
    }
    Ok(())
}

fn check_word(phase: &str) -> Result<(), PhaseError> {
    if phase.is_empty() || phase.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(PhaseError::Word(phase.to_owned()));
    }
    Ok(())
}

fn store(state_dir: &Path, task: TicketId, plan: &PhasePlan) -> Result<(), PhaseError> {
    let failed = |error: std::io::Error| PhaseError::Store(error.to_string());
    fs::create_dir_all(plans_dir(state_dir)).map_err(failed)?;
    fs::write(plan_file(state_dir, task, &plan.role), plan.text()).map_err(failed)
}

/// Stores the phases `role` declares for `task`, replacing an earlier plan and its trail. Nothing
/// is stored when the role or a phase is not one word, the plan is empty or names a phase twice.
pub fn declare(
    state_dir: &Path,
    task: TicketId,
    role: &str,
    phases: &[String],
) -> Result<PhasePlan, PhaseError> {
    check_role(role)?;
    if phases.is_empty() {
        return Err(PhaseError::Empty);
    }
    for (index, phase) in phases.iter().enumerate() {
        check_word(phase)?;
        if phases[..index].contains(phase) {
            return Err(PhaseError::Duplicate(phase.clone()));
        }
    }
    let plan = PhasePlan {
        role: role.to_owned(),
        phases: phases.to_vec(),
        trail: Vec::new(),
    };
    store(state_dir, task, &plan)?;
    Ok(plan)
}

/// Stores that `role` is now in `phase` on `task`. A phase the plan did not declare, or no plan,
/// stores nothing. The same phase reported twice running is one entry; the same phase after
/// another one counts an iteration.
pub fn enter(
    state_dir: &Path,
    task: TicketId,
    role: &str,
    phase: &str,
) -> Result<PhasePlan, PhaseError> {
    check_role(role)?;
    check_word(phase)?;
    let mut plan = read_one(state_dir, task, role)?.ok_or_else(|| PhaseError::NoPlan {
        task: task.get(),
        role: role.to_owned(),
    })?;
    if !plan.phases.iter().any(|declared| declared == phase) {
        return Err(PhaseError::Undeclared {
            phase: phase.to_owned(),
            task: task.get(),
            role: role.to_owned(),
            declared: plan.phases.join(" "),
        });
    }
    if plan.current() != Some(phase) {
        plan.trail.push(phase.to_owned());
    }
    // Written even when nothing changed, so the file's time is the last report's.
    store(state_dir, task, &plan)?;
    Ok(plan)
}

fn read_one(state_dir: &Path, task: TicketId, role: &str) -> Result<Option<PhasePlan>, PhaseError> {
    match fs::read_to_string(plan_file(state_dir, task, role)) {
        Ok(text) => Ok(PhasePlan::read(role, &text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(PhaseError::Store(error.to_string())),
    }
}

/// Every plan declared on `task`, by role, with when its last report was made. Unreadable files
/// are skipped: the hub only reads.
pub fn plans_of(state_dir: &Path, task: TicketId) -> Vec<(PhasePlan, SystemTime)> {
    let Ok(entries) = fs::read_dir(plans_dir(state_dir)) else {
        return Vec::new();
    };
    let prefix = format!("{}.", task.get());
    let mut plans: Vec<(PhasePlan, SystemTime)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let role = name.strip_prefix(&prefix)?;
            let text = fs::read_to_string(entry.path()).ok()?;
            let made = entry.metadata().and_then(|meta| meta.modified()).ok()?;
            Some((PhasePlan::read(role, &text)?, made))
        })
        .collect();
    plans.sort_by(|a, b| a.0.role.cmp(&b.0.role));
    plans
}
