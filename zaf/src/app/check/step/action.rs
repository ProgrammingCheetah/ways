//! `action:<name>`: run an action through the one action runner (docs/zaf-actions.md §4, §5).
//!
//! Exit 0 is a clean step. A `FAIL` is one must-fix finding carrying the declared meaning of
//! the code and the path of the action's own log. A `NOT_RUN` never becomes a finding: the
//! body did not run, so there is no declared code to read, and the step fails. An action no
//! tier provides fails it as `missing action <name>`; any other refusal fails it with the
//! reason the runner gave.

use crate::app::actions::{NO_SUCH_ACTION, RunOutcome, RunResult};
use crate::app::check::StepPorts;
use crate::domain::{Finding, FindingSource, Severity};
use crate::ports::PortError;

pub(super) fn run(
    source: &FindingSource,
    name: &str,
    ports: &dyn StepPorts,
) -> Result<Vec<Finding>, PortError> {
    judge(source, name, ports.action(name))
}

/// The step's findings for how the run ended, or the failure of the step.
fn judge(
    source: &FindingSource,
    name: &str,
    outcome: RunOutcome,
) -> Result<Vec<Finding>, PortError> {
    match outcome.result {
        RunResult::Ok => Ok(Vec::new()),
        RunResult::Fail => Ok(vec![Finding {
            source: source.clone(),
            severity: Severity::MustFix,
            message: outcome.headline().unwrap_or_default(),
            location: None,
        }]),
        RunResult::NotRun => {
            let reason = outcome.meaning.unwrap_or_default();
            if outcome.tier.is_none() && reason == NO_SUCH_ACTION {
                Err(PortError::Unavailable(format!("missing action {name}")))
            } else {
                Err(PortError::Rejected(format!(
                    "action {name} did not run: {reason}"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests;
