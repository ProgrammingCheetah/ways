//! `zaf progress`: a model declares its phases on a task and reports each change of phase
//! (docs/zaf-actions.md §5 Model phases). Each call leaves one hub report, so the frame redraws.

use std::ffi::OsStr;
use std::str::FromStr;
use std::time::SystemTime;

use super::exit::{BACKEND, OK, REFUSED, fail_composition};
use super::{Cli, ProgressAction};
use crate::app::actions::{Caller, INSTANCE_VAR};
use crate::app::hub::{PhaseError, PhasePlan, Report, declare, enter, record};
use crate::composition::Composition;
use crate::domain::{Instance, TicketId};

/// What a plan leaves as its report: the frame's word for a task whose model has just planned.
const PLANNED: &str = "planned";

/// Runs `zaf progress plan` or `zaf progress at`.
pub fn run(cli: &Cli, action: &ProgressAction) -> u8 {
    let path = cli.verb.path();
    let refuse = |reason: String| {
        crate::eprintln_zaf!("zaf {path}: {reason}");
        REFUSED
    };
    let (split, role) = match action {
        ProgressAction::Plan { words, role } => (split_plan(words), role.as_deref()),
        ProgressAction::At { words, role } => (split_at(words), role.as_deref()),
    };
    let (given, rest) = match split {
        Ok(split) => split,
        Err(reason) => return refuse(reason),
    };
    let role = match role_of(role, Caller::from_env()) {
        Ok(role) => role,
        Err(reason) => return refuse(reason),
    };
    let task = match ticket_of(given, std::env::var_os(INSTANCE_VAR).as_deref()) {
        Ok(task) => task,
        Err(reason) => return refuse(reason),
    };
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return fail_composition(&path, &error),
    };
    let state_dir = composition.state_dir();
    let (stored, what) = match action {
        ProgressAction::Plan { .. } => {
            (declare(&state_dir, task, &role, &rest), PLANNED.to_owned())
        }
        ProgressAction::At { .. } => (enter(&state_dir, task, &role, &rest[0]), rest[0].clone()),
    };
    let plan = match stored {
        Ok(plan) => plan,
        Err(error) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
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

/// What `split_plan` and `split_at` hand back: the task when one was given, and the phases.
type Split = (Option<TicketId>, Vec<String>);

/// `plan [<task>] <phase>…`: a first word that parses as a ticket number is the task and the
/// rest are the phases; no phase left is refused.
fn split_plan(words: &[String]) -> Result<Split, String> {
    let (task, phases) = match words.split_first() {
        Some((first, rest)) => match first.parse::<TicketId>() {
            Ok(task) => (Some(task), rest),
            Err(_) => (None, words),
        },
        None => (None, words),
    };
    if phases.is_empty() {
        return Err("no phase given: progress plan [<task>] <phase>…".to_owned());
    }
    Ok((task, phases.to_vec()))
}

/// `at [<task>] <phase>`: one word is the phase, two are the task and the phase.
fn split_at(words: &[String]) -> Result<Split, String> {
    match words {
        [phase] => Ok((None, vec![phase.clone()])),
        [task, phase] => task
            .parse::<TicketId>()
            .map(|task| (Some(task), vec![phase.clone()]))
            .map_err(|_| format!("`{task}` is not a task number: progress at [<task>] <phase>")),
        _ => Err("progress at takes [<task>] <phase>".to_owned()),
    }
}

/// The ticket a call reports on: the one given, else the `#<id>` `ZAF_INSTANCE` names, an epic
/// or a task alike. With neither, or a `ZAF_INSTANCE` that does not parse, it is refused.
fn ticket_of(given: Option<TicketId>, instance: Option<&OsStr>) -> Result<TicketId, String> {
    if let Some(task) = given {
        return Ok(task);
    }
    let Some(value) = instance else {
        return Err(format!(
            "no task: give one or set {INSTANCE_VAR} (`<role> instance <n> on #<id>`)"
        ));
    };
    value
        .to_str()
        .and_then(|text| Instance::from_str(text).ok())
        .map(|instance| instance.epic)
        .ok_or_else(|| {
            format!("no task, and {INSTANCE_VAR} is {value:?}, not `<role> instance <n> on #<id>`")
        })
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
fn print_plan(json: bool, task: TicketId, plan: &PhasePlan) {
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

    fn ticket(value: &str) -> TicketId {
        value.parse().unwrap()
    }

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_planning_instance_reports_on_its_epic_with_no_task() {
        let instance = OsStr::new("planning instance 1 on #900");
        assert_eq!(ticket_of(None, Some(instance)), Ok(ticket("900")));
        assert_eq!(
            split_plan(&words("read file")),
            Ok((None, words("read file")))
        );
        assert_eq!(split_at(&words("read")), Ok((None, words("read"))));
    }

    #[test]
    fn a_programmer_reports_on_the_task_it_gives() {
        assert_eq!(
            split_at(&words("330 write")),
            Ok((Some(ticket("330")), words("write")))
        );
        assert_eq!(
            split_plan(&words("330 read write")),
            Ok((Some(ticket("330")), words("read write")))
        );
        assert!(split_plan(&words("330")).is_err());
        assert!(split_at(&words("soon write")).is_err());
    }

    #[test]
    fn the_given_task_wins_over_the_instance() {
        let instance = OsStr::new("programmer instance 1 on #103");
        assert_eq!(
            ticket_of(Some(ticket("330")), Some(instance)),
            Ok(ticket("330"))
        );
    }

    #[test]
    fn a_hand_run_with_no_task_and_no_instance_is_refused() {
        assert!(ticket_of(None, None).unwrap_err().contains("ZAF_INSTANCE"));
        let bad = Some(OsStr::new("nonsense"));
        assert!(ticket_of(None, bad).unwrap_err().contains("ZAF_INSTANCE"));
    }

    #[test]
    fn at_with_one_word_is_the_phase() {
        assert_eq!(split_at(&words("write")), Ok((None, words("write"))));
        assert_eq!(split_at(&words("330")), Ok((None, words("330"))));
    }

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
