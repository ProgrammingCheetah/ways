//! [`build`]: a [`ChairState`] from the hub's state of a project and its Next for you items.
//! Pure: it reads no port, no file and no clock.
//!
//! The inbox is the Next for you list as it comes, most blocking first, each question with its
//! whole text and the tickets that wait on it; with no release due it ends with the offer to begin
//! the next version (F16). The problems lead with what keeps the project from running, then the
//! orphaned claims, the failed phases and the tasks the spawner skips or can never start, most
//! work held first. Every string is made printable: a board, a file or a process may hand back
//! any byte.

use std::collections::BTreeSet;

use std::collections::HashMap;

use super::{
    ChairState, EpicView, GuardView, InboxItem, NowView, ProblemItem, QuestionView, ReleaseDue,
    ReleaseVersion, RunningLine, SlotsView, Square, SquareState, TaskTrouble, TroubleKind,
    WatchView,
};
use crate::app::guard::Level;
use crate::app::hub::LineState;
use crate::app::hub::next::{NextItem, NextKind};
use crate::app::hub::state::{
    EpicState, LineTask, OrphanedClaim, ProjectState, QuestionState, StatusState, WaitKind,
    WaitState,
};
use crate::app::schedule::ask::is_nowhere;
use crate::domain::{EpicStage, TicketId, TicketState, printable};
use crate::manifest::ProjectName;

/// The chair's state of `project`, its Next for you items being `next`.
pub fn build(project: &ProjectState, next: &[NextItem]) -> ChairState {
    let tasks: Vec<&LineTask> = project.epics.iter().flat_map(|epic| &epic.tasks).collect();
    let orphaned: BTreeSet<TicketId> = project
        .orphaned
        .iter()
        .map(|claim| TicketId::from(claim.task))
        .collect();
    let mut problems = problems(project, &tasks);
    problems.extend(task_troubles(&project.status, &tasks));
    problems.sort_by_key(rank);
    let pulls: HashMap<u64, (u64, Option<String>)> = project
        .queue
        .iter()
        .filter_map(|task| {
            task.pull_request.map(|number| {
                (
                    task.number,
                    (number, task.pull_request_url.as_deref().map(printable)),
                )
            })
        })
        .collect();
    let mut inbox: Vec<InboxItem> = next
        .iter()
        .map(|item| inbox_item(item, &project.questions, &tasks))
        .collect();
    inbox.extend(begin_offer(project));
    ChairState {
        project: ProjectName::from(printable(&project.project)),
        root: project.root.clone(),
        gathered_at: humantime::parse_rfc3339(&project.gathered_at)
            .unwrap_or(std::time::UNIX_EPOCH),
        reason: project.reason.as_deref().map(printable),
        inbox,
        problems,
        now: now_view(project),
        epics: project
            .epics
            .iter()
            .map(|epic| epic_view(epic, &orphaned, &pulls))
            .collect(),
    }
}

/// The offer to begin the next version, when the board was read and no release is due (F16): a
/// phase item for `zaf begin --version <next>`, the next version pre-filled, that holds nothing.
fn begin_offer(project: &ProjectState) -> Option<InboxItem> {
    if project.missing || project.reason.is_some() || !project.releases_due.is_empty() {
        return None;
    }
    let version = ReleaseVersion::from(printable(project.status.next_version.as_deref()?));
    Some(InboxItem {
        kind: NextKind::Phase,
        title: format!("Begin {version}: its objectives come first"),
        blocks: 0,
        how: format!(
            "No release is due. Beginning {version} opens a Claude session that writes its \
             objectives with you; you sign them when they say what you mean."
        ),
        command: Some(format!("zaf begin --version {version}")),
        link: None,
        number: None,
        question: None,
        begins: Some(version),
    })
}

/// The tasks the spawner skips or can never start, from what the read found: a ready task whose
/// body does not follow the template, a blocker cycle, a blocker that never closes.
fn task_troubles(status: &StatusState, tasks: &[&LineTask]) -> Vec<ProblemItem> {
    let held = |task: u64| holding(tasks, WaitKind::Ticket, task).len();
    let mut troubles: Vec<ProblemItem> = status
        .unparsed
        .iter()
        .map(|task| {
            let id = TicketId::from(task.number);
            let first = task.problems.first().map_or("", String::as_str);
            ProblemItem {
                kind: TroubleKind::Task(TaskTrouble::Unparsed),
                ticket: Some(id),
                question: None,
                headline: printable(&format!("not to the template: {first}")),
                detail: printable(&format!(
                    "{id} {} is labelled ready, but its body does not follow the task \
                     template, so the spawner skips it:\n{}\n\nFix its body on the board; \
                     zaf board lint {} says what is wrong.",
                    task.title,
                    task.problems.join("\n"),
                    task.number
                )),
                fix: None,
                verb: None,
                holds: held(task.number),
            }
        })
        .collect();
    troubles.extend(
        status
            .cycles
            .iter()
            .filter(|cycle| !cycle.is_empty())
            .map(|cycle| {
                let ids: Vec<String> = cycle
                    .iter()
                    .chain(cycle.first())
                    .map(|task| TicketId::from(*task).to_string())
                    .collect();
                let path = ids.join(" → ");
                ProblemItem {
                    kind: TroubleKind::Task(TaskTrouble::Cycle),
                    ticket: cycle.first().copied().map(TicketId::from),
                    question: None,
                    headline: format!("cycle {path}"),
                    detail: format!(
                        "A blocker cycle: {path}. Each waits on the next, so none of them can ever \
                 start. Take one blocker off a Blocked by line on the board."
                    ),
                    fix: None,
                    verb: None,
                    holds: cycle.iter().map(|task| held(*task)).sum(),
                }
            }),
    );
    troubles.extend(status.dead_blockers.iter().map(|dead| {
        let (blocker, on) = (TicketId::from(dead.blocker), TicketId::from(dead.on));
        ProblemItem {
            kind: TroubleKind::Task(TaskTrouble::DeadBlocker),
            ticket: Some(on),
            question: None,
            headline: printable(&format!("dead blocker {blocker}: {}", dead.reason)),
            detail: printable(&format!(
                "{on} is blocked by {blocker}, which is {}: it never closes, so {on} never \
                 starts. Take {blocker} off its Blocked by line, or name the task that replaced \
                 it.",
                dead.reason
            )),
            fix: None,
            verb: None,
            holds: held(dead.on),
        }
    }));
    troubles
}

/// The tasks whose waits hold a `kind` naming `number`, lowest first.
fn holding(tasks: &[&LineTask], kind: WaitKind, number: u64) -> Vec<TicketId> {
    let mut held: Vec<TicketId> = tasks
        .iter()
        .filter(|task| {
            task.waits.as_ref().is_some_and(|waits| {
                waits
                    .iter()
                    .any(|wait| wait.on == kind && wait.number == Some(number))
            })
        })
        .map(|task| TicketId::from(task.number))
        .collect();
    held.sort();
    held.dedup();
    held
}

fn inbox_item(item: &NextItem, questions: &[QuestionState], tasks: &[&LineTask]) -> InboxItem {
    let asks = matches!(
        item.kind,
        NextKind::Question | NextKind::Door | NextKind::Failed
    );
    let question = item
        .number
        .filter(|_| asks)
        .and_then(|number| questions.iter().find(|question| question.number == number))
        .map(|question| QuestionView {
            text: printable(&question.text),
            raised_on: question.raised_on.map(TicketId::from),
            door: question.door,
            proposal: item.proposal.as_deref().map(printable),
            holds: holding(tasks, WaitKind::Question, question.number),
        });
    InboxItem {
        kind: item.kind,
        title: printable(&item.title),
        blocks: item.blocks,
        how: printable(&item.how),
        command: item.command.as_deref().map(printable),
        link: item.link.as_deref().map(printable),
        number: item.number.map(TicketId::from),
        question,
        begins: None,
    }
}

/// The order problems are listed in: by kind, then most work held first, then by ticket.
fn rank(problem: &ProblemItem) -> (u8, std::cmp::Reverse<usize>, Option<TicketId>) {
    let kind = match problem.kind {
        TroubleKind::Project(_) => 0,
        TroubleKind::Orphaned => 1,
        TroubleKind::FailedPhase => 2,
        TroubleKind::Task(_) => 3,
    };
    (kind, std::cmp::Reverse(problem.holds), problem.ticket)
}

fn problems(project: &ProjectState, tasks: &[&LineTask]) -> Vec<ProblemItem> {
    let mut problems: Vec<ProblemItem> = project
        .problems
        .iter()
        .map(|problem| ProblemItem {
            kind: TroubleKind::Project(problem.kind),
            ticket: None,
            question: None,
            headline: printable(&problem.detail),
            detail: printable(&problem.detail),
            fix: problem.fix.as_deref().map(printable),
            verb: problem.verb,
            holds: 0,
        })
        .collect();
    problems.extend(project.orphaned.iter().map(|claim| orphaned(claim, tasks)));
    problems.extend(
        project
            .questions
            .iter()
            .filter(|question| is_nowhere(&question.text))
            .map(|question| failed_phase(question, tasks)),
    );
    problems
}

fn orphaned(claim: &OrphanedClaim, tasks: &[&LineTask]) -> ProblemItem {
    let task = TicketId::from(claim.task);
    ProblemItem {
        kind: TroubleKind::Orphaned,
        ticket: Some(task),
        question: None,
        headline: format!("{task} orphaned"),
        detail: format!(
            "{task} is still claimed by {}, which no longer runs. The next tick releases the \
             claim and starts it again, unless its pull request or a running phase holds it.",
            printable(&claim.instance)
        ),
        fix: None,
        verb: None,
        holds: holding(tasks, WaitKind::Ticket, claim.task).len(),
    }
}

fn failed_phase(question: &QuestionState, tasks: &[&LineTask]) -> ProblemItem {
    let id = TicketId::from(question.number);
    let text = printable(&question.text);
    let held = text.split(';').next().unwrap_or(&text).trim().to_owned();
    ProblemItem {
        kind: TroubleKind::FailedPhase,
        ticket: question.raised_on.map(TicketId::from),
        question: Some(id),
        headline: held,
        detail: format!(
            "{text}\n\nIt is not started again until question {id} is ruled: read the log it \
             names, fix the cause, then rule it."
        ),
        fix: None,
        verb: None,
        holds: holding(tasks, WaitKind::Question, question.number).len(),
    }
}

fn now_view(project: &ProjectState) -> NowView {
    let count = |states: &[TicketState]| {
        project
            .queue
            .iter()
            .filter(|task| states.contains(&task.state))
            .count()
    };
    NowView {
        running: project
            .instances
            .iter()
            .map(|instance| RunningLine {
                ticket: instance.number.map(TicketId::from),
                title: instance.title.as_deref().map(printable),
                instance: printable(&instance.instance),
                phase: instance.phase.as_deref().map(printable),
                run_seconds: instance.run_seconds,
                doing: instance.last_tool_call.as_deref().map(printable),
            })
            .collect(),
        ready: count(&[TicketState::Ready]),
        waiting: count(&[TicketState::Blocked, TicketState::Question]),
        due: project
            .releases_due
            .iter()
            .map(|due| ReleaseDue {
                version: ReleaseVersion::from(printable(&due.version)),
                phase: printable(&due.phase),
                command: printable(&due.command),
            })
            .collect(),
        watch: project.watch.as_ref().map(|watch| WatchView {
            running: watch.running,
            pid: watch.pid,
            version: watch
                .version
                .as_deref()
                .map(|version| ReleaseVersion::from(printable(version))),
        }),
        slots: project.status.slots.map(|slots| SlotsView {
            total: slots.total,
            in_use: slots.in_use,
        }),
        guard: project.status.guard.as_ref().map(|guard| GuardView {
            line: printable(&guard.line()),
            tight: guard.level != Level::Green,
        }),
        budget: None,
    }
}

fn epic_view(
    epic: &EpicState,
    orphaned: &BTreeSet<TicketId>,
    pulls: &HashMap<u64, (u64, Option<String>)>,
) -> EpicView {
    EpicView {
        number: TicketId::from(epic.number),
        title: printable(&epic.title),
        stage: epic.stage,
        // An unsigned epic is not planned yet, whatever its records say.
        half_filed: epic.half_filed && epic.stage != EpicStage::Unsigned,
        release: epic
            .release
            .as_deref()
            .map(|release| ReleaseVersion::from(printable(release))),
        done: epic.done,
        total: epic.total,
        question: epic
            .line_question
            .as_ref()
            .map(|question| TicketId::from(question.number)),
        squares: epic
            .tasks
            .iter()
            .map(|task| square(task, orphaned, pulls))
            .collect(),
    }
}

fn square(
    task: &LineTask,
    orphaned: &BTreeSet<TicketId>,
    pulls: &HashMap<u64, (u64, Option<String>)>,
) -> Square {
    let ticket = TicketId::from(task.number);
    let waits: Vec<WaitState> = task.waits.clone().unwrap_or_default();
    Square {
        ticket,
        title: printable(&task.title),
        // A wait the graph names that is not done yet marks the square waiting, whether or not the
        // core has placed the ticket (D19, D23).
        state: square_state(
            task.state,
            orphaned.contains(&ticket),
            !waits.is_empty()
                || task
                    .blocked_by
                    .iter()
                    .any(|blocker| blocker.state != LineState::Done),
        ),
        phase: task.phase.as_deref().map(printable),
        blocked_by: task
            .blocked_by
            .iter()
            .map(|blocker| TicketId::from(blocker.number))
            .collect(),
        question: task
            .question
            .as_ref()
            .map(|question| TicketId::from(question.number)),
        waits,
        phases_done: task.phases_done,
        phases_total: task.phases_total,
        pull_request: pulls.get(&task.number).cloned(),
        cost: task.cost,
    }
}

/// A square's state from the hub's line state: a claim of an ended instance shows as failed,
/// and a ticket not started that the core says waits shows as waiting.
fn square_state(line: LineState, orphaned: bool, waits: bool) -> SquareState {
    match line {
        LineState::Done => SquareState::Done,
        LineState::Question => SquareState::Question,
        _ if orphaned => SquareState::Failed,
        LineState::Working => SquareState::Running,
        LineState::Blocked => SquareState::Waiting,
        LineState::WaitingTick | LineState::NotStarted if waits => SquareState::Waiting,
        LineState::WaitingTick | LineState::NotStarted => SquareState::Ready,
    }
}
