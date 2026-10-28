//! `command:<cmd>`: run a line in the project; its report lines count, and a
//! non-zero exit with no must-fix line of its own is one must-fix.

use crate::app::check::{StepPorts, parse_report};
use crate::domain::{Finding, FindingSource, Severity};
use crate::manifest::CommandLine;
use crate::ports::{GateVerdict, PortError};

pub(super) fn run(
    source: &FindingSource,
    line: &CommandLine,
    ports: &dyn StepPorts,
) -> Result<Vec<Finding>, PortError> {
    let gate_run = ports.run(line)?;
    let mut findings = parse_report(source, &gate_run.output);
    let has_must_fix = findings
        .iter()
        .any(|finding| finding.severity == Severity::MustFix);
    if gate_run.verdict == GateVerdict::Failed && !has_must_fix {
        findings.push(Finding {
            source: source.clone(),
            severity: Severity::MustFix,
            message: format!("`{line}` exited non-zero"),
            location: None,
        });
    }
    Ok(findings)
}
