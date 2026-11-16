//! The trace of where everything was (R24, docs/zaf-actions.md §5.1): one line of JSON per real
//! tick, which [`super::ticks`] appends to `.zaf/ticks.jsonl`. It is output only: zaf writes it
//! and never reads it back to decide anything; it reads it back only to say what moved and to
//! show it (`zaf spawn trace`).
//!
//! One line, its keys in this order:
//!
//! ```json
//! {"at": "2026-11-12T17:15:00Z", "zaf": "0.3.1",
//!  "tickets": [{"ticket": 7, "place": "running", "phase": "implement", "head": null,
//!               "started_at": "<time>", "short": "running(implement)"},
//!              {"ticket": 9, "place": "lane", "next": "review", "since": "<time>",
//!               "weight": 3600, "short": "lane(review)"}],
//!  "unreadable": [{"ticket": 11, "reason": "<why>"}],
//!  "did": {"started": {"task": 9, "action": "review", "units": 2},
//!          "waits": [{"task": 4, "action": "ci"}], "ran": [{"task": 3, "action": "merge", "exit": 0}],
//!          "questions": [{"task": 5, "question": 61,
//!                         "cause": "merge got nowhere three times at d962227"}],
//!          "unblocked": [{"task": 26, "blockers": [25]}],
//!          "relabelled": [{"task": 27, "from": "ready", "to": "question",
//!                          "why": "it waits on question #45"}],
//!          "launched": ["planning instance 3 on #1"],
//!          "released": [{"task": 4, "instance": "team instance 2 on #1"}], "lost": []},
//!  "skipped": ["#2 skipped: the challenger runs on it"],
//!  "moved": [{"ticket": 7, "from": "lane(implement)", "to": "running(implement)"}]}
//! ```
//!
//! `tickets` is the core's placement ([`super::shadow`]) at the tick, before it acted, each with
//! its [`ShortPlace`]; `did` is what the tick then did ([`Did`]); `skipped` what it held back, as
//! its block words it; `moved` each ticket whose short place differs from the last line that had tickets
//! ([`moves`]), `from` null for a new ticket and `to` null for one gone. A tick that could not
//! place the tickets writes `error` in place of `tickets` and `unreadable`, and moves nothing.
//! Each question in `did` carries the issue it was raised on, the question and its cause
//! ([`QuestionStep`]); a line written before causes were kept lists the issue alone, and still
//! reads back.
//!
//! Everything here is pure: the time is handed in and nothing reads a file.

pub mod listing;

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::gather::{Gathered, Unreadable};
use super::labels::{Relabelled, Unblocked};
use super::shadow::{self, Placed};
use crate::app::actions::ActionName;
use crate::app::spawn::outcome::{Asked, Launched};
use crate::app::spawn::{PlannedLaunch, Ran, Released, Started, TickOutcome};
use crate::domain::schedule::{Place, WaitingOn};
use crate::domain::{CommitId, TicketId, TicketState};
use crate::newtype::text_newtype;

/// How long a line is kept: a tick that writes drops the lines older than this.
pub const KEEP_FOR: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// The zaf that writes a line, as its `zaf` key names it.
pub const ZAF_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How many characters of a head a short place shows.
const SHORT_HEAD: usize = 7;

text_newtype! {
    /// A ticket's place in a few words, as a move names it: `running(implement)`,
    /// `waiting(question #45, ticket #5)`, `waiting(check a1b2c3d)`, `waiting(epic #1)`,
    /// `waiting(merge of #12)`, `waiting(base a1b2c3d)`, `lane(implement)`, `done`, or
    /// `unreadable` for a ticket the gather could not read.
    ShortPlace
}

impl ShortPlace {
    /// The short form of `place`. A waiting ticket names each thing it waits on, in the core's
    /// order (epic, question, ticket, check, merge, base), a check and the base by the first
    /// characters of the head.
    pub fn of(place: &Place) -> Self {
        let text = match place {
            Place::Running(run) => format!("running({})", run.phase),
            Place::Waiting(waits) => {
                let on: Vec<String> = waits.iter().map(waiting_short).collect();
                format!("waiting({})", on.join(", "))
            }
            Place::InLane { next, .. } => format!("lane({next})"),
            Place::Done => "done".to_owned(),
        };
        Self::from(text)
    }

    /// The place of a ticket the gather could not read.
    pub fn unreadable() -> Self {
        Self::from("unreadable")
    }
}

/// One thing a ticket waits on, in short.
fn waiting_short(waiting_on: &WaitingOn) -> String {
    match waiting_on {
        WaitingOn::Epic(epic) => format!("epic {epic}"),
        WaitingOn::Question(question) => format!("question {question}"),
        WaitingOn::Ticket(ticket) => format!("ticket {ticket}"),
        WaitingOn::Check(_, head) => format!("check {}", short_head(head)),
        WaitingOn::Merge(ticket) => format!("merge of {ticket}"),
        WaitingOn::Base(head) => format!("base {}", short_head(head)),
    }
}

/// The first [`SHORT_HEAD`] characters of a head.
fn short_head(head: &CommitId) -> String {
    head.as_str().chars().take(SHORT_HEAD).collect()
}

/// Where each ticket sat, in short, by ticket.
pub type Places = BTreeMap<TicketId, ShortPlace>;

/// A ticket whose place differs from the last line's: `from` none for a ticket new since, `to`
/// none for one gone since (done on the board, closed, or its type's list dropped).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    /// The ticket.
    pub ticket: TicketId,
    /// Where it sat on the last line; none when it was on none.
    pub from: Option<ShortPlace>,
    /// Where it sits now; none when it is gone.
    pub to: Option<ShortPlace>,
}

impl Move {
    /// The move in one line: `#7 lane(implement) -> running(implement)`, `#9 new -> lane(brief)`
    /// or `#5 done -> gone`.
    pub fn line(&self) -> String {
        format!(
            "{} {} -> {}",
            self.ticket,
            self.from.as_ref().map_or("new", ShortPlace::as_str),
            self.to.as_ref().map_or("gone", ShortPlace::as_str)
        )
    }
}

/// Each ticket whose place in `now` differs from its place in `previous`, a ticket in one of them
/// only included, in ticket order. A ticket that stayed is no move.
pub fn moves(previous: &Places, now: &Places) -> Vec<Move> {
    let tickets: BTreeSet<TicketId> = previous.keys().chain(now.keys()).copied().collect();
    tickets
        .into_iter()
        .filter_map(|ticket| {
            let from = previous.get(&ticket);
            let to = now.get(&ticket);
            (from != to).then(|| Move {
                ticket,
                from: from.cloned(),
                to: to.cloned(),
            })
        })
        .collect()
}

/// What the core saw at the tick: each gathered ticket in its place, in ticket order, the
/// tickets it could not read, and how many tasks of the open epics are done.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// Each ticket in its place.
    pub placed: Vec<Placed>,
    /// The tickets that could not be read.
    pub unreadable: Vec<Unreadable>,
    /// The done tasks of the open epics that are not among `placed` ([`Gathered::done`]): the
    /// board lists no done ticket as open, so the count takes them from the epics' task lists.
    pub done: usize,
}

impl Seen {
    /// What the core sees of `gathered` at `now` ([`shadow::places`]).
    pub fn of(gathered: &Gathered, now: SystemTime) -> Self {
        let placed = shadow::places(gathered, now);
        let done = gathered
            .done
            .iter()
            .filter(|task| !placed.iter().any(|placed| placed.ticket == **task))
            .count();
        Self {
            placed,
            unreadable: gathered.unreadable.clone(),
            done,
        }
    }

    /// Each ticket's short place, an unreadable ticket's being [`ShortPlace::unreadable`].
    pub fn places(&self) -> Places {
        let mut places: Places = self
            .placed
            .iter()
            .map(|placed| (placed.ticket, ShortPlace::of(&placed.place)))
            .collect();
        for unreadable in &self.unreadable {
            places.insert(unreadable.ticket, ShortPlace::unreadable());
        }
        places
    }

    /// The count line: `26 tickets: 1 running, 3 in the lane, 8 waiting, 14 done`, then `, 2 not
    /// read` when the gather could not read some; none when it saw no ticket at all. Done counts
    /// the tickets the core placed as done and the done tasks of the open epics ([`Seen::done`]).
    pub fn count_line(&self) -> Option<String> {
        let total = self.placed.len() + self.unreadable.len() + self.done;
        if total == 0 {
            return None;
        }
        let count = |wanted: fn(&Place) -> bool| {
            self.placed
                .iter()
                .filter(|placed| wanted(&placed.place))
                .count()
        };
        let noun = if total == 1 { "ticket" } else { "tickets" };
        let mut line = format!(
            "{total} {noun}: {} running, {} in the lane, {} waiting, {} done",
            count(|place| matches!(place, Place::Running(_))),
            count(|place| matches!(place, Place::InLane { .. })),
            count(|place| matches!(place, Place::Waiting(_))),
            count(|place| matches!(place, Place::Done)) + self.done,
        );
        if !self.unreadable.is_empty() {
            line.push_str(&format!(", {} not read", self.unreadable.len()));
        }
        Some(line)
    }

    /// The tickets as a line carries them: each placement's JSON with its `short`.
    fn tickets_json(&self) -> Vec<Value> {
        self.placed
            .iter()
            .map(|placed| {
                let mut entry = shadow::placed_json(placed);
                entry["short"] = json!(ShortPlace::of(&placed.place));
                entry
            })
            .collect()
    }
}

/// The lines the tick's block prints of the places (docs/zaf-actions.md §5.1): each move in ticket
/// order, then the count line. With `more`, every ticket instead, each once and in ticket order: a
/// moved one as its move, any other as `#8 waiting(ticket #7)`. No ticket and no move, no line.
pub fn place_lines(seen: &Seen, moves: &[Move], more: bool) -> Vec<String> {
    let mut lines: Vec<String> = if more {
        let places = seen.places();
        let tickets: BTreeSet<TicketId> = places
            .keys()
            .copied()
            .chain(moves.iter().map(|moved| moved.ticket))
            .collect();
        tickets
            .into_iter()
            .map(
                |ticket| match moves.iter().find(|moved| moved.ticket == ticket) {
                    Some(moved) => moved.line(),
                    None => format!("{ticket} {}", places[&ticket]),
                },
            )
            .collect()
    } else {
        moves.iter().map(Move::line).collect()
    };
    lines.extend(seen.count_line());
    lines
}

/// A model phase a tick started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartedStep {
    /// The task.
    pub task: TicketId,
    /// The action it started.
    pub action: ActionName,
    /// What it holds of the budget.
    pub units: u32,
}

/// An action a tick launched or ran for a task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionStep {
    /// The task.
    pub task: TicketId,
    /// The action.
    pub action: ActionName,
}

/// A `zaf`-kind action a tick ran inline, and the exit code `zaf do` gave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RanStep {
    /// The task.
    pub task: TicketId,
    /// The action.
    pub action: ActionName,
    /// Its exit code.
    pub exit: u8,
}

/// A blocked task a tick set ready, and its blockers, each done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnblockedStep {
    /// The task.
    pub task: TicketId,
    /// Its blockers.
    pub blockers: Vec<TicketId>,
}

/// Any other label a tick set ([`Relabelled`]): `to` in place of `from`, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelabelledStep {
    /// The task.
    pub task: TicketId,
    /// The label it carried; none when it carried none.
    pub from: Option<TicketState>,
    /// The label it got.
    pub to: TicketState,
    /// Why, in words.
    pub why: String,
}

/// A task a tick released from an instance that no longer runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleasedStep {
    /// The task.
    pub task: TicketId,
    /// The instance that held it.
    pub instance: String,
}

/// What a tick did, as its trace line keeps it: its phase steps ([`TickOutcome`]) and its own
/// acts (the tasks it unblocked, relabelled and released, the instances it launched for epics and
/// the ones it found lost).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Did {
    /// The model phase it started, if any.
    pub started: Option<StartedStep>,
    /// The waits it launched.
    pub waits: Vec<ActionStep>,
    /// The `zaf`-kind actions it ran inline, with their exit codes.
    pub ran: Vec<RanStep>,
    /// The questions it raised, each with the issue it was raised on and its cause.
    pub questions: Vec<QuestionStep>,
    /// The blocked tasks it set ready.
    pub unblocked: Vec<UnblockedStep>,
    /// The other labels it set; left out of a line that set none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub relabelled: Vec<RelabelledStep>,
    /// The planning and challenger instances it launched for epics.
    pub launched: Vec<String>,
    /// The tasks it released.
    pub released: Vec<ReleasedStep>,
    /// The launches it found ended with nobody there to record it.
    pub lost: Vec<String>,
}

impl Did {
    /// What a tick did: its phase steps' `outcome`, the tasks it `unblocked`, the planning and
    /// challenger launches it made, the tasks it `released` and the launches it found `lost`.
    pub fn of(
        outcome: &TickOutcome,
        unblocked: &[Unblocked],
        launches: &[PlannedLaunch],
        released: &[Released],
        lost: &[String],
    ) -> Self {
        Self {
            started: outcome.started.as_ref().map(|started| StartedStep {
                task: started.task,
                action: started.action.clone(),
                units: started.units,
            }),
            waits: outcome
                .waits
                .iter()
                .map(|wait| ActionStep {
                    task: wait.task,
                    action: wait.action.clone(),
                })
                .collect(),
            ran: outcome
                .ran
                .iter()
                .map(|ran| RanStep {
                    task: ran.task,
                    action: ran.action.clone(),
                    exit: ran.exit,
                })
                .collect(),
            questions: question_steps(outcome),
            unblocked: unblocked
                .iter()
                .map(|unblocked| UnblockedStep {
                    task: unblocked.task,
                    blockers: unblocked.blockers.clone(),
                })
                .collect(),
            launched: launches
                .iter()
                .map(|launch| launch.instance.to_string())
                .collect(),
            released: released
                .iter()
                .map(|released| ReleasedStep {
                    task: released.task,
                    instance: released.instance.to_string(),
                })
                .collect(),
            lost: lost.to_vec(),
            relabelled: Vec::new(),
        }
    }

    /// The same, with the other labels the tick set ([`Relabelled`]).
    #[must_use]
    pub fn with_relabelled(mut self, relabelled: &[Relabelled]) -> Self {
        self.relabelled = relabelled
            .iter()
            .map(|relabelled| RelabelledStep {
                task: relabelled.task,
                from: relabelled.from,
                to: relabelled.to,
                why: relabelled.why.clone(),
            })
            .collect();
        self
    }

    /// What it did, one line each, worded as the tick's block words it and in its order: `lost`,
    /// `released`, `unblocked`, `labelled`, then `ran`, `started`, `wait` and `question on`, then
    /// `launching`.
    pub fn lines(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .lost
            .iter()
            .map(|instance| format!("lost {instance}"))
            .collect();
        lines.extend(
            self.released
                .iter()
                .map(|released| format!("released {} from {}", released.task, released.instance)),
        );
        lines.extend(self.unblocked.iter().map(|step| {
            Unblocked {
                task: step.task,
                blockers: step.blockers.clone(),
            }
            .line()
        }));
        lines.extend(self.relabelled.iter().map(|step| {
            Relabelled {
                task: step.task,
                from: step.from,
                to: step.to,
                why: step.why.clone(),
            }
            .line()
        }));
        lines.extend(self.outcome().render_text().lines().map(str::to_owned));
        lines.extend(
            self.launched
                .iter()
                .map(|instance| format!("launching {instance}")),
        );
        lines
    }

    /// The phase steps, as the tick's outcome holds them.
    fn outcome(&self) -> TickOutcome {
        TickOutcome {
            started: self.started.as_ref().map(|started| Started {
                task: started.task,
                action: started.action.clone(),
                units: started.units,
            }),
            waits: self
                .waits
                .iter()
                .map(|wait| Launched {
                    task: wait.task,
                    action: wait.action.clone(),
                })
                .collect(),
            ran: self
                .ran
                .iter()
                .map(|ran| Ran {
                    task: ran.task,
                    action: ran.action.clone(),
                    exit: ran.exit,
                })
                .collect(),
            questions: self.questions.iter().map(QuestionStep::task).collect(),
            asked: self
                .questions
                .iter()
                .filter_map(|step| match step {
                    QuestionStep::Asked {
                        task,
                        question,
                        cause,
                    } => Some(Asked {
                        task: *task,
                        question: *question,
                        cause: cause.clone(),
                    }),
                    QuestionStep::Task(_) => None,
                })
                .collect(),
        }
    }
}

/// The questions of `outcome` as a trace line keeps them, in the order raised: each issue with
/// the question raised on it and its cause, or the issue alone when the outcome names no cause
/// for it.
fn question_steps(outcome: &TickOutcome) -> Vec<QuestionStep> {
    let mut asked: Vec<Option<&Asked>> = outcome.asked.iter().map(Some).collect();
    outcome
        .questions
        .iter()
        .map(|task| {
            let found = asked
                .iter_mut()
                .find(|asked| asked.is_some_and(|asked| asked.task == *task))
                .and_then(Option::take);
            found.map_or(QuestionStep::Task(*task), |asked| QuestionStep::Asked {
                task: asked.task,
                question: asked.question,
                cause: asked.cause.clone(),
            })
        })
        .collect()
}

/// A question a tick raised, as its trace line keeps it: `{"task": 7, "question": 61, "cause":
/// "<why>"}`, or, on a line written before causes were kept, the issue alone, `7`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QuestionStep {
    /// The question with its cause.
    Asked {
        /// The issue it was raised on.
        task: TicketId,
        /// The question.
        question: TicketId,
        /// Its cause, in a few words ([`crate::app::schedule::ask::Cause::line`]).
        cause: String,
    },
    /// The issue alone, as an older line keeps it.
    Task(TicketId),
}

impl QuestionStep {
    /// The issue it was raised on.
    pub fn task(&self) -> TicketId {
        match self {
            Self::Asked { task, .. } | Self::Task(task) => *task,
        }
    }
}

/// What a tick hands its trace: when it began, what the core saw or why it saw nothing, what it
/// did, and what it held back.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// When the tick began.
    pub at: SystemTime,
    /// What the core saw, or why the tickets could not be placed.
    pub seen: Result<Seen, String>,
    /// What the tick did.
    pub did: Did,
    /// What it held back and why, as its block words it.
    pub skipped: Vec<String>,
}

impl Entry {
    /// The line of a tick that stopped before it placed anything, and why.
    pub fn stopped(at: SystemTime, why: impl Into<String>) -> Self {
        Self {
            at,
            seen: Err(why.into()),
            did: Did::default(),
            skipped: Vec::new(),
        }
    }

    /// The entry as one line, its moves worked out against `previous`, the places of the last
    /// line that had tickets (empty when no line had any, so every ticket is new). With
    /// `previous` unknown (the trace could not be read), nothing is a move. A line with an
    /// `error` moves nothing.
    pub fn line(&self, previous: Option<&Places>) -> Line {
        let moved = match (&self.seen, previous) {
            (Ok(seen), Some(previous)) => moves(previous, &seen.places()),
            _ => Vec::new(),
        };
        let (error, tickets, unreadable) = match &self.seen {
            Ok(seen) => (
                None,
                Some(seen.tickets_json()),
                Some(
                    seen.unreadable
                        .iter()
                        .map(shadow::unreadable_json)
                        .collect(),
                ),
            ),
            Err(why) => (Some(why.clone()), None, None),
        };
        let written = Written {
            at: shadow::time(self.at),
            zaf: ZAF_VERSION,
            error,
            tickets,
            unreadable,
            did: &self.did,
            skipped: &self.skipped,
            moved: &moved,
        };
        // Written as text from the struct, so the file keeps the module's key order.
        let (json, text) = match (
            serde_json::to_value(&written),
            serde_json::to_string(&written),
        ) {
            (Ok(json), Ok(text)) => (json, text),
            (Err(error), _) | (_, Err(error)) => {
                let json = json!({
                    "at": shadow::time(self.at),
                    "zaf": ZAF_VERSION,
                    "error": error.to_string(),
                });
                let text = json.to_string();
                (json, text)
            }
        };
        Line { json, text, moved }
    }
}

/// One line as it is written, its keys in the module's order.
#[derive(Serialize)]
struct Written<'a> {
    at: String,
    zaf: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tickets: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unreadable: Option<Vec<Value>>,
    did: &'a Did,
    skipped: &'a [String],
    moved: &'a [Move],
}

/// A line made from an [`Entry`]: its JSON, its text and its moves.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The line, as one JSON object.
    pub json: Value,
    /// The line as the file holds it: one JSON object, its keys in the module's order, no newline.
    text: String,
    /// The tickets that moved since the last line that had tickets.
    pub moved: Vec<Move>,
}

impl Line {
    /// The line as the file holds it: one JSON object, its keys in the module's order, no newline.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// One line read back: when its tick began, the places it saw (none on a line with an `error`),
/// its moves and what its tick did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    /// When the tick began.
    pub at: SystemTime,
    /// Where each ticket sat, in short; none on a line that placed nothing.
    pub places: Option<Places>,
    /// The tickets that moved.
    pub moved: Vec<Move>,
    /// What the tick did.
    pub did: Did,
}

/// One line as it is read back: only what is shown or compared.
#[derive(Deserialize)]
struct Read {
    at: String,
    #[serde(default)]
    tickets: Option<Vec<ReadTicket>>,
    #[serde(default)]
    unreadable: Vec<ReadUnreadable>,
    #[serde(default)]
    moved: Vec<Move>,
    #[serde(default)]
    did: Did,
}

/// A ticket of a line read back: its number and its short place.
#[derive(Deserialize)]
struct ReadTicket {
    ticket: TicketId,
    short: ShortPlace,
}

/// An unreadable ticket of a line read back: its number.
#[derive(Deserialize)]
struct ReadUnreadable {
    ticket: TicketId,
}

/// A line read for its time alone.
#[derive(Deserialize)]
struct ReadAt {
    at: String,
}

impl Recorded {
    /// `text`, one line of the file, read back; none when it is no line of the trace (half
    /// written, or with no time).
    pub fn parse(text: &str) -> Option<Self> {
        let read: Read = serde_json::from_str(text).ok()?;
        let at = humantime::parse_rfc3339(&read.at).ok()?;
        let places = read.tickets.map(|tickets| {
            let mut places: Places = tickets
                .into_iter()
                .map(|ticket| (ticket.ticket, ticket.short))
                .collect();
            for unreadable in read.unreadable {
                places.insert(unreadable.ticket, ShortPlace::unreadable());
            }
            places
        });
        Some(Self {
            at,
            places,
            moved: read.moved,
            did: read.did,
        })
    }
}

/// When the line `text` was written; none when it holds no time.
fn at_of(text: &str) -> Option<SystemTime> {
    let read: ReadAt = serde_json::from_str(text).ok()?;
    humantime::parse_rfc3339(&read.at).ok()
}

/// Whether a line of a tick that began at `at` is kept at `now`: it is younger than [`KEEP_FOR`].
/// A time after `now`, a clock that stepped back, is kept.
pub fn kept(at: SystemTime, now: SystemTime) -> bool {
    now.duration_since(at).map_or(true, |age| age < KEEP_FOR)
}

/// The file as a tick finds it before it writes, at `now`: the lines it keeps, whether it drops
/// any, and the places of the last kept line that had tickets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Before<'a> {
    /// The lines kept, in the file's order.
    pub kept: Vec<&'a str>,
    /// Whether a line is dropped: older than [`KEEP_FOR`], with no time, half written or blank.
    /// The file is then written anew.
    pub dropped: bool,
    /// The places of the last kept line that had tickets; empty when none had any.
    pub previous: Places,
}

impl<'a> Before<'a> {
    /// The file's `text` at `now` (see [`Before`]).
    pub fn of(text: &'a str, now: SystemTime) -> Self {
        let mut kept_lines = Vec::new();
        let mut dropped = false;
        for line in text.lines() {
            match at_of(line) {
                Some(at) if kept(at, now) => kept_lines.push(line),
                _ => dropped = true,
            }
        }
        let previous = kept_lines
            .iter()
            .rev()
            .find_map(|line| Recorded::parse(line)?.places)
            .unwrap_or_default();
        Self {
            kept: kept_lines,
            dropped,
            previous,
        }
    }
}

#[cfg(test)]
mod tests;
