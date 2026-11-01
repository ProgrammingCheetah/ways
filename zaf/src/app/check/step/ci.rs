//! `ci`: wait until no forge check is pending, then each failed check is a must-fix.
//!
//! Right after a push the forge lists no checks for a while, so an empty list counts
//! as pending until the grace window, counted from the step's start, has passed; after
//! it an empty list is clean (a pull request that triggers no workflow stays mergeable).

use std::thread;
use std::time::Instant;

use super::StepContext;
use crate::app::check::StepPorts;
use crate::domain::{Finding, FindingSource, Severity};
use crate::ports::{CheckState, PortError};

pub(super) fn run(
    source: &FindingSource,
    ports: &dyn StepPorts,
    context: &StepContext,
) -> Result<Vec<Finding>, PortError> {
    let forge = ports.forge()?;
    let started = Instant::now();
    loop {
        let checks = forge.checks(context.pr)?;
        let not_listed_yet = checks.is_empty() && started.elapsed() < context.grace;
        let pending = not_listed_yet
            || checks
                .iter()
                .any(|check| check.state == CheckState::Pending);
        if !pending {
            let findings = checks
                .iter()
                .filter(|check| check.state == CheckState::Failed)
                .map(|check| Finding {
                    source: source.clone(),
                    severity: Severity::MustFix,
                    message: format!("check `{}` failed", check.name),
                    location: None,
                })
                .collect();
            return Ok(findings);
        }
        let left = context.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(PortError::Unavailable(format!(
                "checks on {} still pending",
                context.pr
            )));
        }
        thread::sleep(context.poll.min(left));
    }
}

#[cfg(test)]
mod tests;
