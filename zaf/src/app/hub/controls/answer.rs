//! `POST /api/answer` with `{"project": <root or name>, "issue": <number>, "ruling": <text>}`:
//! records the Engineer's ruling on an open question, as `zaf board ruling` does from that
//! project's root.
//!
//! Answers `200` with what the verb's `--json` says (`question`, `ready`) and, in plain words,
//! `answer`. An issue that is not an open question is `409` and changes nothing; a body that is
//! not that, or a ruling with no words, `400`; a project the list does not hold or holds as
//! missing, `404`; a backend that failed, `500`. The server bounds the body before it gets here.

use std::path::Path;

use super::stop::resolve;
use super::{Answer, Failure, Ruler};
use crate::app::board::{self, BoardError, RulingCommand};
use crate::domain::TicketId;
use crate::ports::{Board, PortError};

/// What a body that is not an answer is told.
const NEEDS: &str = "an answer needs a JSON body with \"project\" (a listed project), \"issue\" \
                     (a number) and \"ruling\" (text)";

pub(super) fn run(home: Option<&Path>, ruler: &Ruler, body: &[u8]) -> Answer {
    let Some((project, question, ruling)) = parse(body) else {
        return Answer::refused("400 Bad Request", NEEDS);
    };
    if ruling.trim().is_empty() {
        return Answer::refused("400 Bad Request", "the ruling is empty");
    }
    let root = match resolve(home, &project) {
        Ok(root) => root,
        Err(why) => return Answer::refused("404 Not Found", &why),
    };
    match ruler(&root, question, &ruling) {
        Ok(ready) => {
            let answer = match ready {
                Some(task) => format!("{question} ruled; {task} ready"),
                None => format!("{question} ruled"),
            };
            Answer::done(
                "200 OK",
                &serde_json::json!({
                    "answer": answer,
                    "question": question.get(),
                    "ready": ready.map(TicketId::get),
                }),
            )
        }
        Err(Failure::Refused(why)) => Answer::refused("409 Conflict", &why),
        Err(Failure::Failed(why)) => Answer::refused("500 Internal Server Error", &why),
    }
}

/// The ruling of `zaf board ruling` on `board`, if `question` is an open question there: the
/// question is closed and the task it held returns to `ready`, which is the task returned. Any
/// other issue is refused and nothing is changed; a refusal of the board is a refusal, and any
/// other failure a failure of a backend.
pub fn rule(
    board: &dyn Board,
    question: TicketId,
    ruling: &str,
) -> Result<Option<TicketId>, Failure> {
    let open = board
        .questions()
        .map_err(|error| Failure::Failed(error.to_string()))?;
    if !open.iter().any(|ticket| ticket.id == question) {
        return Err(Failure::Refused(format!(
            "{question} is not an open question; nothing was changed"
        )));
    }
    let command = RulingCommand {
        question,
        ruling: ruling.to_owned(),
    };
    board::ruling(board, &command).map_err(|error| match error {
        BoardError::Port(
            PortError::Conflict(_) | PortError::Rejected(_) | PortError::NotFound(_),
        ) => Failure::Refused(error.to_string()),
        error => Failure::Failed(error.to_string()),
    })
}

/// The project, the question and the ruling a body names; none for a body that is not an object
/// with a string `project`, a whole, positive `issue` and a string `ruling`.
fn parse(body: &[u8]) -> Option<(String, TicketId, String)> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let project = value.get("project")?.as_str()?.to_owned();
    let issue = value.get("issue")?.as_u64().filter(|issue| *issue > 0)?;
    let ruling = value.get("ruling")?.as_str()?.to_owned();
    Some((project, TicketId::from(issue), ruling))
}
