//! Each open epic's line: its own colour and the tasks that make it up (epic #115 §2).
//!
//! Everything here is worked out on read from what the state gather already reads: the board's
//! epic trees, the questions, where the core places each ticket and the facts that placement read
//! (`StateInputs::places`, `StateInputs::gathered`). Nothing is stored. A task's body is the one
//! thing read per task, for its size and its `Blocked by:` line: from the tickets already read
//! (the ready ones, the gather's open ones), and from the board for each other task, a done one
//! included.

use std::collections::{BTreeSet, HashMap};

use super::{LineBlocker, LineQuestion, LineTask, QuestionState, StateInputs};
use crate::app::hub::gather::{LineState, has_begun, line_state};
use crate::app::schedule::shadow::current_phase_name;
use crate::app::spawn::blockers::blockers;
use crate::domain::schedule::{Place, TicketFacts, end_fact_holds};
use crate::domain::{DEFAULT_TASK_TYPE, Epic, EpicTask, Standing, Ticket, TicketId};
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

/// The tickets already read, and the board for each other one asked once.
struct Tickets<'a> {
    board: &'a dyn Board,
    read: HashMap<TicketId, Option<Ticket>>,
}

impl Tickets<'_> {
    fn get(&mut self, id: TicketId) -> Option<&Ticket> {
        self.read
            .entry(id)
            .or_insert_with(|| self.board.task(id).ok())
            .as_ref()
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

/// The line of every epic of `trees`, by the epic's number. `ready` are the tickets the board
/// listed ready, whose bodies are already read; `titles` is every epic's and task's title.
pub(super) fn read(
    inputs: &StateInputs,
    board: &dyn Board,
    trees: &[Epic],
    questions: &[QuestionState],
    titles: &HashMap<u64, String>,
    ready: Vec<Ticket>,
) -> HashMap<u64, EpicLine> {
    let facts: HashMap<TicketId, &TicketFacts> = inputs
        .gathered
        .into_iter()
        .flat_map(|gathered| gathered.tickets.iter())
        .map(|facts| (facts.ticket, facts))
        .collect();
    let mut tickets = Tickets {
        board,
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
    trees
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
        .collect()
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
    let (size, blocked_by, issue_url) = match tickets.get(task.number) {
        Some(ticket) => (
            ticket.template().ok().map(|template| template.size.name()),
            blockers(ticket.body.as_str()),
            Some(ticket.url.to_string()).filter(|url| !url.is_empty()),
        ),
        None => (None, Vec::new(), None),
    };
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
        let state =
            states
                .get(&blocker.get())
                .copied()
                .unwrap_or_else(|| match board.standing(blocker) {
                    Ok(Standing::Done) => LineState::Done,
                    _ => LineState::NotStarted,
                });
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
