//! Each open epic's line: its own colour and the tasks that make it up (epic #115 §2).
//!
//! Everything here is worked out on read from what the state gather already reads: the board's
//! epic trees, the questions, where the core places each ticket and the facts that placement read
//! (`StateInputs::places`, `StateInputs::gathered`). Nothing is stored. A task's body is the one
//! thing read per task, for its size, and it comes from the epic's own task row (`EpicTask::body`,
//! `EpicTask::url`), so no task is asked of the board. What a task waits on is read from the wait
//! source ([`Waits::of`], D19, D23): a wait on an epic shows as a wait on that epic, and a task of
//! the older template reads from its `Blocked by:` line as it did. The wait source is read only
//! when an epic holds a planned task; should the board not list its epics, the line reads each
//! task's `Blocked by:` line. A blocker that is
//! no task of a read epic is asked once, for where it stands.

use std::collections::{BTreeSet, HashMap};

use super::{
    LineBlocker, LineQuestion, LineTask, QuestionState, StateInputs, TicketCost, WaitState,
};
use crate::app::hub::gather::{LineState, cost_by_task, has_begun, line_state};
use crate::app::schedule::shadow::current_phase_name;
use crate::app::spawn::blockers::waits::{Wait, Waits};
use crate::app::spawn::blockers::{DeadLine, DeadReason, blockers, finished_epic_standing};
use crate::domain::schedule::{Place, TicketFacts, end_fact_holds};
use crate::domain::{DEFAULT_TASK_TYPE, Epic, EpicTask, Standing, TaskBody, Ticket, TicketId};
use crate::ports::Board;

/// One epic's line.
pub(super) struct EpicLine {
    /// The epic's own colour.
    pub state: LineState,
    /// The first question open on the epic itself.
    pub question: Option<LineQuestion>,
    /// Its tasks that count, in id order.
    pub tasks: Vec<LineTask>,
}

/// What the line holds besides the epics' task rows: the tickets already read.
struct Tickets<'a> {
    /// The open epics, whose finished ones clear the blockers they are (see
    /// [`finished_epic_standing`]).
    epics: &'a [Epic],
    read: HashMap<TicketId, Option<Ticket>>,
    /// Where each task of the open epics stands, by number.
    standings: HashMap<u64, Standing>,
    /// Where each blocker outside every open epic stands, by number, asked of the board once.
    outside: HashMap<u64, Option<Standing>>,
    /// What the ledger's runs cost each task, by number.
    costs: HashMap<u64, TicketCost>,
    /// The blockers open tasks name that never close, as the line's reads found them.
    dead: Vec<DeadLine>,
    /// What each planned task waits on, read from the graph; none when no epic holds a planned
    /// task or the board did not list its epics.
    waits: Option<Waits>,
}

impl Tickets<'_> {
    /// A ticket already read (a ready one, one the gather found open); none for any other, which
    /// the board is not asked for.
    fn get(&self, id: TicketId) -> Option<&Ticket> {
        self.read.get(&id).and_then(Option::as_ref)
    }
}

/// The first of `questions` raised on `issue`, the lowest number.
fn first_on(questions: &[QuestionState], issue: u64) -> Option<LineQuestion> {
    questions
        .iter()
        .filter(|question| question.raised_on == Some(issue))
        .min_by_key(|question| question.number)
        .map(|question| LineQuestion {
            number: question.number,
            text: question.text.clone(),
        })
}

/// The line of every epic of `trees`, by the epic's number, and the blockers its open tasks name
/// that never close. `ready` are the tickets the board listed ready, whose bodies are already
/// read; `titles` is every epic's and task's title.
pub(super) fn read(
    inputs: &StateInputs,
    board: &dyn Board,
    trees: &[Epic],
    questions: &[QuestionState],
    titles: &HashMap<u64, String>,
    ready: Vec<Ticket>,
) -> (HashMap<u64, EpicLine>, Vec<DeadLine>) {
    let facts: HashMap<TicketId, &TicketFacts> = inputs
        .gathered
        .into_iter()
        .flat_map(|gathered| gathered.tickets.iter())
        .map(|facts| (facts.ticket, facts))
        .collect();
    let mut tickets = Tickets {
        epics: trees,
        read: ready
            .into_iter()
            .chain(
                inputs
                    .gathered
                    .into_iter()
                    .flat_map(|gathered| gathered.open.iter().cloned()),
            )
            .map(|ticket| (ticket.id, Some(ticket)))
            .collect(),
        standings: trees
            .iter()
            .flat_map(|tree| &tree.tasks)
            .map(|task| (task.number.get(), task.standing))
            .collect(),
        outside: HashMap::new(),
        costs: cost_by_task(inputs.ledger, inputs.project, inputs.root),
        dead: Vec::new(),
        waits: read_waits(board, trees),
    };
    // Every task's colour first: a blocker's is read from its own epic's tasks.
    let mut states: HashMap<u64, LineState> = HashMap::new();
    for tree in trees {
        let on_epic: BTreeSet<TicketId> = questions
            .iter()
            .filter(|question| question.raised_on == Some(tree.id.get()))
            .map(|question| TicketId::from(question.number))
            .collect();
        for task in &tree.tasks {
            let state = line_state(
                task,
                inputs.places.and_then(|places| places.get(&task.number)),
                facts.get(&task.number).copied(),
                &on_epic,
            );
            states.insert(task.number.get(), state);
        }
    }
    let lines = trees
        .iter()
        .map(|tree| {
            let question = first_on(questions, tree.id.get());
            let counted: Vec<&EpicTask> = tree.tasks.iter().filter(|task| task.counts()).collect();
            let state = epic_state(question.is_some(), &counted, &states, &facts);
            let mut tasks: Vec<LineTask> = counted
                .iter()
                .map(|task| {
                    line_task(
                        inputs,
                        board,
                        task,
                        &states,
                        &facts,
                        questions,
                        titles,
                        &mut tickets,
                    )
                })
                .collect();
            tasks.sort_by_key(|task| task.number);
            (
                tree.id.get(),
                EpicLine {
                    state,
                    question,
                    tasks,
                },
            )
        })
        .collect();
    (lines, tickets.dead)
}

/// An epic's own colour: `question` while one is open on the epic itself, else `done` when every
/// task that counts is done (an epic with none is not), else `working` when any task is, else
/// `waiting_tick` when any has started, else `not_started`. A question on the epic leaves its
/// tasks' colours as they are.
fn epic_state(
    asked: bool,
    counted: &[&EpicTask],
    states: &HashMap<u64, LineState>,
    facts: &HashMap<TicketId, &TicketFacts>,
) -> LineState {
    let of = |task: &&EpicTask| {
        states
            .get(&task.number.get())
            .copied()
            .unwrap_or(LineState::NotStarted)
    };
    if asked {
        LineState::Question
    } else if !counted.is_empty() && counted.iter().all(|task| of(task) == LineState::Done) {
        LineState::Done
    } else if counted.iter().any(|task| of(task) == LineState::Working) {
        LineState::Working
    } else if counted.iter().any(|task| {
        matches!(of(task), LineState::Done | LineState::WaitingTick)
            || facts
                .get(&task.number)
                .is_some_and(|facts| has_begun(facts))
    }) {
        LineState::WaitingTick
    } else {
        LineState::NotStarted
    }
}

/// Whether a task's body is a planned ticket's, whose waits the graph gives.
fn is_planned(task: &EpicTask) -> bool {
    matches!(
        TaskBody::parse(task.body.as_str()),
        Ok(TaskBody::Planned(_))
    )
}

/// The wait source over `trees`, read only when an epic holds a planned task: an older epic reads
/// from its `Blocked by:` lines and asks the board nothing more.
fn read_waits(board: &dyn Board, trees: &[Epic]) -> Option<Waits> {
    if !trees.iter().flat_map(|tree| &tree.tasks).any(is_planned) {
        return None;
    }
    Waits::read(board, &trees.iter().map(|tree| tree.id).collect::<Vec<_>>()).ok()
}

#[allow(clippy::too_many_arguments)]
fn line_task(
    inputs: &StateInputs,
    board: &dyn Board,
    task: &EpicTask,
    states: &HashMap<u64, LineState>,
    facts: &HashMap<TicketId, &TicketFacts>,
    questions: &[QuestionState],
    titles: &HashMap<u64, String>,
    tickets: &mut Tickets,
) -> LineTask {
    let place = inputs.places.and_then(|places| places.get(&task.number));
    let (phase, phases_done, phases_total) = phases(inputs, task, place, facts.get(&task.number));
    // The epic's own row holds the body and the link: no read of the task.
    let size = TaskBody::parse(task.body.as_str())
        .and_then(|body| body.size())
        .ok()
        .map(|size| size.name());
    let planned = is_planned(task);
    let blocked_by: Vec<TicketId> = match &tickets.waits {
        Some(waits) if planned => waits
            .of(task.number)
            .iter()
            .map(|wait| match wait {
                Wait::Ticket(ticket) => *ticket,
                Wait::Epic { epic, .. } => *epic,
            })
            .collect(),
        _ => blockers(task.body.as_str()),
    };
    let issue_url = Some(task.url.to_string()).filter(|url| !url.is_empty());
    let mut named = Vec::new();
    for blocker in blocked_by {
        if named
            .iter()
            .any(|seen: &LineBlocker| seen.number == blocker.get())
        {
            continue;
        }
        let title = titles
            .get(&blocker.get())
            .cloned()
            .or_else(|| tickets.get(blocker).map(|ticket| ticket.title.to_string()));
        // A task of an open epic stands as its epic listed it; any other blocker is asked once.
        let (state, standing) = match states.get(&blocker.get()) {
            Some(state) => (*state, tickets.standings.get(&blocker.get()).copied()),
            None => {
                let epics = tickets.epics;
                let standing = *tickets.outside.entry(blocker.get()).or_insert_with(|| {
                    board.standing(blocker).ok().map(|standing| {
                        finished_epic_standing(blocker, standing.into(), epics).standing
                    })
                });
                let state = if standing == Some(Standing::Done) {
                    LineState::Done
                } else {
                    LineState::NotStarted
                };
                (state, standing)
            }
        };
        let reason = match standing {
            Some(Standing::ClosedNotPlanned) => Some(DeadReason::ClosedNotPlanned),
            Some(Standing::Missing) => Some(DeadReason::NotAnIssue),
            _ => None,
        };
        if let Some(reason) = reason.filter(|_| task.standing == Standing::Open) {
            tickets.dead.push(DeadLine {
                blocker: blocker.get(),
                on: task.number.get(),
                reason: reason.to_string(),
            });
        }
        named.push(LineBlocker {
            number: blocker.get(),
            title,
            state,
        });
    }
    LineTask {
        number: task.number.get(),
        title: task.title.to_string(),
        size,
        state: states
            .get(&task.number.get())
            .copied()
            .unwrap_or(LineState::NotStarted),
        phase,
        phases_done,
        phases_total,
        blocked_by: named,
        question: first_on(questions, task.number.get()),
        issue_url,
        waits: place.map(|place| match place {
            Place::Waiting(waits) => waits.iter().map(WaitState::from).collect(),
            Place::Running(_) | Place::InLane { .. } | Place::Done => Vec::new(),
        }),
        cost: tickets.costs.get(&task.number.get()).copied(),
    }
}

/// The phase a task is in, the phases of its type that have ended and all of them. A done task
/// has all ended; one the core does not place has `0` of `0`.
fn phases(
    inputs: &StateInputs,
    task: &EpicTask,
    place: Option<&Place>,
    facts: Option<&&TicketFacts>,
) -> (Option<String>, u32, u32) {
    let count = |number: usize| u32::try_from(number).unwrap_or(u32::MAX);
    if let Some(facts) = facts.copied() {
        let total = count(facts.phases.len());
        if task.done() || matches!(place, Some(Place::Done)) {
            return (None, total, total);
        }
        let ended = facts
            .phases
            .iter()
            .filter(|phase| end_fact_holds(phase, facts))
            .count();
        return (current_phase_name(facts), count(ended), total);
    }
    if task.done() {
        let kind = task
            .kind
            .as_ref()
            .map_or(DEFAULT_TASK_TYPE, |kind| kind.as_str());
        let total = inputs
            .gathered
            .and_then(|gathered| gathered.phase_totals.get(kind))
            .copied()
            .unwrap_or(0);
        return (None, total, total);
    }
    (None, 0, 0)
}
