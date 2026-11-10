//! `zaf board ready|claim|report|question|ruling|done|file`.
//!
//! The verbs that act as an instance (`claim`, `report`) read who it is from
//! [`INSTANCE_VAR`], which the spawner sets, in the form the board prints it:
//! `<role> instance <number> on #<epic>`.

use serde_json::json;

use super::exit::{BACKEND, REFUSED, code_for, fail_composition};
use super::slice::print;
use super::{BoardAction, Cli};
use crate::app::actions::SliceRecords;
use crate::app::board::{
    self, BoardError, ClaimCommand, DoneOutcome, FileOutcome, ReportCommand, RulingCommand,
};
use crate::composition::Composition;
use crate::domain::{Instance, InstanceNumber, NewTask, RoleName, Ticket, TicketId, TicketState};
use crate::ports::{ClaimOutcome, NewQuestion};

/// The environment variable naming the instance a verb acts as.
pub const INSTANCE_VAR: &str = "ZAF_INSTANCE";

/// How many claims a role without a batch size holds at once.
const UNBATCHED: u32 = 1;

/// Runs a board action.
pub fn run(cli: &Cli, action: &BoardAction) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return fail_composition(&path, &error),
    };
    let board = board.as_ref();
    let done = match action {
        BoardAction::Ready => board::ready(board).map(|tickets| {
            let json: Vec<_> = tickets.iter().map(Ticket::summary).collect();
            let lines: Vec<String> = tickets
                .iter()
                .map(|ticket| format!("{} {}", ticket.id, ticket.title))
                .collect();
            let terse = if lines.is_empty() {
                "no ready tasks".to_owned()
            } else {
                lines.join("\n")
            };
            print(cli, &json!(json), terse)
        }),
        BoardAction::Claim { task } => {
            let instance = match instance(&path) {
                Ok(instance) => instance,
                Err(code) => return code,
            };
            let batch = composition
                .manifest()
                .roles
                .get(&instance.role)
                .and_then(|role| role.batch)
                .unwrap_or(UNBATCHED);
            let command = ClaimCommand {
                task: *task,
                instance,
                batch,
            };
            board::claim(board, &command).map(|outcome| {
                let (word, terse) = match outcome {
                    ClaimOutcome::Claimed => ("claimed", "claimed"),
                    ClaimOutcome::AlreadyHeld => ("already_held", "already held"),
                };
                let json = json!({ "task": task.get(), "outcome": word });
                print(cli, &json, format!("{task} {terse}"))
            })
        }
        BoardAction::Report { task, body } => {
            let instance = match instance(&path) {
                Ok(instance) => instance,
                Err(code) => return code,
            };
            let command = ReportCommand {
                task: *task,
                instance,
                body: body.clone(),
            };
            board::report(board, &command).map(|outcome| {
                let json = json!({ "task": task.get(), "heartbeat": outcome.heartbeat });
                let lease = if outcome.heartbeat {
                    "; lease touched"
                } else {
                    ""
                };
                print(cli, &json, format!("{task} reported{lease}"))
            })
        }
        BoardAction::Question { on, text, door } => {
            let command = NewQuestion {
                on: *on,
                text: text.clone(),
                door: *door,
            };
            board::question(board, &command).map(|number| {
                let json = json!({ "question": number.get(), "on": on.get(), "door": door });
                let kind = if *door { "door question" } else { "question" };
                print(cli, &json, format!("{kind} {number} on {on}"))
            })
        }
        BoardAction::Done { task } => {
            let forge = match composition.forge() {
                Ok(forge) => forge,
                Err(error) => return fail_composition(&path, &error),
            };
            let vcs = match composition.vcs() {
                Ok(vcs) => vcs,
                Err(error) => return fail_composition(&path, &error),
            };
            let records = SliceRecords::new(composition.root());
            board::done(board, forge.as_ref(), vcs.as_ref(), &records, *task).map(|outcome| {
                let (word, terse) = match outcome {
                    DoneOutcome::Marked => ("marked", "done"),
                    DoneOutcome::AlreadyDone => ("already_done", "already done"),
                };
                let json = json!({ "task": task.get(), "outcome": word });
                print(cli, &json, format!("{task} {terse}"))
            })
        }
        BoardAction::File {
            epic,
            title,
            body,
            body_file,
            group,
            blocked,
        } => {
            let body = match read_body(&path, body.as_deref(), body_file.as_deref()) {
                Ok(body) => body,
                Err(code) => return code,
            };
            let task = NewTask {
                epic: *epic,
                title: title.as_str().into(),
                body: body.into(),
                state: if *blocked {
                    TicketState::Blocked
                } else {
                    TicketState::Ready
                },
                group: group.as_deref().map(Into::into),
            };
            board::file(board, &task).map(|outcome| {
                let (word, terse) = match outcome {
                    FileOutcome::Filed(_) => ("filed", "filed"),
                    FileOutcome::AlreadyFiled(_) => ("already_filed", "already filed"),
                };
                let number = outcome.task();
                let json = json!({ "task": number.get(), "epic": epic.get(), "outcome": word });
                print(cli, &json, format!("{number} {terse} under {epic}"))
            })
        }
        BoardAction::Ruling { question, text } => {
            let command = RulingCommand {
                question: *question,
                ruling: text.clone(),
            };
            board::ruling(board, &command).map(|returned| {
                let json = json!({
                    "question": question.get(),
                    "ready": returned.map(TicketId::get),
                });
                let terse = match returned {
                    Some(task) => format!("{question} ruled; {task} ready"),
                    None => format!("{question} ruled"),
                };
                print(cli, &json, terse)
            })
        }
    };
    done.unwrap_or_else(|error| {
        eprintln!("zaf {path}: {error}");
        exit_code(&error)
    })
}

/// The body `board file` was given: the `--body` text, or the `--body-file` contents (`-` is
/// standard input). A file that does not read exits 2.
fn read_body(path: &str, body: Option<&str>, file: Option<&std::path::Path>) -> Result<String, u8> {
    if let Some(body) = body {
        return Ok(body.to_owned());
    }
    let Some(file) = file else {
        eprintln!("zaf {path}: give --body or --body-file");
        return Err(BACKEND);
    };
    let read = if file.as_os_str() == "-" {
        std::io::read_to_string(std::io::stdin())
    } else {
        std::fs::read_to_string(file)
    };
    read.map_err(|error| {
        eprintln!("zaf {path}: reading {}: {error}", file.display());
        BACKEND
    })
}

/// The exit code a board error comes to.
pub(super) fn exit_code(error: &BoardError) -> u8 {
    match error {
        BoardError::BatchFull { .. }
        | BoardError::NotInLand { .. }
        | BoardError::NotATask(_)
        | BoardError::NotAnOpeningState(_) => REFUSED,
        // A read that failed is no answer about the task: exit 2, never a refusal.
        BoardError::Unreadable(_) => BACKEND,
        BoardError::Port(error) => code_for(error),
    }
}

/// The instance this process acts as, from [`INSTANCE_VAR`]; a missing or unreadable one exits 2.
fn instance(path: &str) -> Result<Instance, u8> {
    let text = std::env::var(INSTANCE_VAR).unwrap_or_default();
    parse_instance(&text).ok_or_else(|| {
        eprintln!(
            "zaf {path}: set {INSTANCE_VAR} to `<role> instance <number> on #<epic>`, got `{text}`"
        );
        BACKEND
    })
}

/// Reads an instance in the form it displays as: `team instance 3 on #12`.
fn parse_instance(text: &str) -> Option<Instance> {
    let (role, rest) = text.trim().split_once(" instance ")?;
    let (number, epic) = rest.split_once(" on ")?;
    if role.is_empty() || role.contains(char::is_whitespace) {
        return None;
    }
    let instance = Instance {
        role: RoleName::from(role),
        epic: epic.parse::<TicketId>().ok()?,
        number: InstanceNumber::from(number.parse::<u32>().ok()?),
    };
    Some(instance)
}
