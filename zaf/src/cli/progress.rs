//! `zaf progress`: a model declares its phases on a task and reports each change of phase
//! (docs/zaf-actions.md §5 Model phases). Each call leaves one hub report, so the frame redraws.

use std::time::SystemTime;

use super::exit::{BACKEND, OK, REFUSED, fail_composition};
use super::{Cli, ProgressAction};
use crate::app::actions::Caller;
use crate::app::hub::{PhaseError, PhasePlan, Report, declare, enter, record};
use crate::composition::Composition;
use crate::domain::IssueNumber;

/// What a plan leaves as its report: the frame's word for a task whose model has just planned.
const PLANNED: &str = "planned";

/// Runs `zaf progress plan` or `zaf progress at`.
pub fn run(cli: &Cli, action: &ProgressAction) -> u8 {
    let path = cli.verb.path();
    let (task, role) = match action {
        ProgressAction::Plan { task, role, .. } | ProgressAction::At { task, role, .. } => {
            (*task, role.as_deref())
        }
    };
    let role = match role_of(role, Caller::from_env()) {
        Ok(role) => role,
        Err(reason) => {
            eprintln!("zaf {path}: {reason}");
            return REFUSED;
        }
    };
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return fail_composition(&path, &error),
    };
    let state_dir = composition.state_dir();
    let (stored, what) = match action {
        ProgressAction::Plan { phases, .. } => {
            (declare(&state_dir, task, &role, phases), PLANNED.to_owned())
        }
        ProgressAction::At { phase, .. } => (enter(&state_dir, task, &role, phase), phase.clone()),
    };
    let plan = match stored {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return match error {
                PhaseError::Store(_) => BACKEND,
                _ => REFUSED,
            };
        }
    };
    record(
        &state_dir,
        &Report {
            at: SystemTime::now(),
            task: Some(task),
            worker: role,
            what,
        },
    );
    print_plan(cli.json, task, &plan);
    OK
}

/// The role a call reports as: the one given, else the one `ZAF_INSTANCE` names. A hand run with
/// neither is refused, and so is a `ZAF_INSTANCE` that does not parse.
fn role_of(given: Option<&str>, caller: Result<Caller, String>) -> Result<String, String> {
    if let Some(role) = given {
        return Ok(role.to_owned());
    }
    match caller? {
        Caller::Role(role) => Ok(role.as_ref().to_owned()),
        Caller::Engineer => Err("no role: set ZAF_INSTANCE or give --role <role>".to_owned()),
    }
}

/// One line, or the plan as JSON: `#330 programmer: write ×2 (3 of 5)`.
fn print_plan(json: bool, task: IssueNumber, plan: &PhasePlan) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "task": task.get(),
                "role": plan.role,
                "phases": plan.phases,
                "current": plan.current(),
                "iteration": plan.iteration(),
            })
        );
        return;
    }
    let place = match plan.current() {
        None => format!("planned {}", plan.phases.join(" ")),
        Some(current) if plan.iteration() > 1 => format!("{current} ×{}", plan.iteration()),
        Some(current) => current.to_owned(),
    };
    println!("#{} {}: {place}", task.get(), plan.role);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_given_role_wins_over_the_instance() {
        let caller = Caller::from_value(Some("programmer instance 1 on #103".as_ref()));
        assert_eq!(role_of(Some("keeper"), caller), Ok("keeper".to_owned()));
    }

    #[test]
    fn the_instance_names_the_role_when_none_is_given() {
        let caller = Caller::from_value(Some("programmer instance 1 on #103".as_ref()));
        assert_eq!(role_of(None, caller), Ok("programmer".to_owned()));
    }

    #[test]
    fn no_role_and_no_instance_is_refused() {
        assert!(role_of(None, Caller::from_value(None)).is_err());
        assert!(role_of(None, Caller::from_value(Some("".as_ref()))).is_err());
    }
}
