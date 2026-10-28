//! `codeql`: each open code scanning alert on the pull request's head is a
//! must-fix titled with its rule id.

use super::StepContext;
use crate::app::check::StepPorts;
use crate::domain::{Finding, FindingSource, Severity};
use crate::ports::PortError;

pub(super) fn run(
    source: &FindingSource,
    ports: &dyn StepPorts,
    context: &StepContext,
) -> Result<Vec<Finding>, PortError> {
    let alerts = ports.forge()?.scanning_alerts(context.pr)?;
    let findings = alerts
        .into_iter()
        .map(|alert| Finding {
            source: source.clone(),
            severity: Severity::MustFix,
            message: alert.rule.to_string(),
            location: alert.location,
        })
        .collect();
    Ok(findings)
}

#[cfg(test)]
mod tests;
