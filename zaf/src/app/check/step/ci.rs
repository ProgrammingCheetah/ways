//! `ci`: wait until no forge check is pending, then each failed check is a must-fix.

use std::thread;

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
    loop {
        let checks = forge.checks(context.pr)?;
        let pending = checks
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
        let left = context
            .deadline
            .saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Err(PortError::Unavailable(format!(
                "checks on {} still pending",
                context.pr
            )));
        }
        thread::sleep(context.poll.min(left));
    }
}
