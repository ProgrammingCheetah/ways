//! `runner:<name>:<role>`: write the review brief, run the role on the runner,
//! and parse its report into findings.

use crate::app::check::{StepPorts, parse_report};
use crate::domain::{Finding, FindingSource, Profile};
use crate::ports::{PortError, RunRequest};

use super::{RunnerStep, StepContext};

/// The default brief for a critique role.
pub const CRITIQUE_BRIEF: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/check/critique.md"
));

/// The default brief for a role whose name holds `security`.
pub const SECURITY_BRIEF: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/check/security.md"
));

/// The brief's file name inside the step's directory.
pub const BRIEF_FILE: &str = "brief.md";

pub(super) fn run(
    source: &FindingSource,
    step: &RunnerStep,
    ports: &dyn StepPorts,
    context: &StepContext,
) -> Result<Vec<Finding>, PortError> {
    let runner = ports.runner(&step.runner)?;
    let template = if step.role.as_ref().contains("security") {
        SECURITY_BRIEF
    } else {
        CRITIQUE_BRIEF
    };
    let brief = template
        .replace("{pr}", &context.pr.to_string())
        .replace("{root}", &context.root.display().to_string())
        .replace("{role}", step.role.as_ref());
    std::fs::create_dir_all(&context.dir).map_err(|error| file_error(&context.dir, &error))?;
    let brief_path = context.dir.join(BRIEF_FILE);
    std::fs::write(&brief_path, brief).map_err(|error| file_error(&brief_path, &error))?;
    let request = RunRequest {
        role: step.role.clone(),
        profile: Profile::Review,
        prompt: format!("Follow the brief at {}.", brief_path.display()),
        dir: context.dir.clone(),
        env: std::collections::BTreeMap::new(),
    };
    let output = runner.run(&request)?;
    if output.exit_code != 0 {
        return Err(PortError::Backend(format!(
            "{source} exited {}",
            output.exit_code
        )));
    }
    Ok(parse_report(source, &output.transcript))
}

fn file_error(path: &std::path::Path, error: &std::io::Error) -> PortError {
    PortError::Backend(format!("{}: {error}", path.display()))
}
