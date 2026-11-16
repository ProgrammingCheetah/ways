//! The Next for you list: everything that waits on the Engineer, across all projects, most
//! blocking first.
//!
//! [`next_for_you`] derives it on every call from a [`MachineState`] and stores nothing. Five
//! things wait on the Engineer, each an item ([`NextKind`]):
//!
//! - an unsigned epic: `cd <project> && zaf epic sign <n>`, with the epic's link;
//! - a question, with the answer it proposes when its text holds one; a door, the one-way kind,
//!   is marked as its own kind;
//! - ready work no scheduler ticks: tasks ready in a project whose `zaf spawn --watch` does not run;
//! - a phase that got nowhere three times, which raised a question of its own;
//! - a project that cannot run: a manifest that does not read, a token that is not set, a board
//!   that does not answer, a profile that is stale, a folder that is gone.
//!
//! An item's `blocks` is the number of tasks it holds back. Items sort by it, most first. Ties
//! break by age, and the board gives no time for a question or a ready task, so the age is the
//! issue's number: a lower one was opened first. Items still equal keep the list's order, the
//! projects as they joined and each project's items as listed above. A project nobody reads
//! (`missing`, or whose board did not answer) shows its problems and nothing else.

use std::path::Path;

use super::state::{MachineState, ProblemKind, ProjectState, QuestionState};
use crate::app::schedule::ask::is_nowhere;

/// What kind of thing waits on the Engineer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextKind {
    /// An epic to sign.
    Sign,
    /// A question to rule on.
    Question,
    /// A question raised as a door, a one-way decision.
    Door,
    /// Ready work no scheduler ticks.
    Unticked,
    /// A phase that got nowhere three times, with its question.
    Failed,
    /// A project that cannot run.
    Broken,
}

/// One thing that waits on the Engineer.
///
/// Interface: `app::hub::next::NextItem { kind, project, title, blocks, how, link, command }`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NextItem {
    /// What it is.
    pub kind: NextKind,
    /// The project it is in, by name.
    pub project: String,
    /// What it is, in a line.
    pub title: String,
    /// The number of tasks it holds back.
    pub blocks: u32,
    /// How to do it, in a sentence or two.
    pub how: String,
    /// Where a person opens it; none when the board gave no link.
    pub link: Option<String>,
    /// The command that does it, run as written; none when no command does.
    pub command: Option<String>,
}

/// The longest a question's title is, in characters.
const TITLE_LIMIT: usize = 100;

/// The Next for you list of `state`: every item, most blocking first; empty when nothing waits.
///
/// Interface: `next_for_you(&MachineState) -> Vec<NextItem>`.
pub fn next_for_you(state: &MachineState) -> Vec<NextItem> {
    let mut ranked: Vec<(u64, NextItem)> = state.projects.iter().flat_map(of_project).collect();
    // Stable: what is equal keeps the order it was built in.
    ranked.sort_by_key(|(age, item)| (std::cmp::Reverse(item.blocks), *age));
    ranked.into_iter().map(|(_, item)| item).collect()
}

/// A project's items, each with its age.
fn of_project(project: &ProjectState) -> Vec<(u64, NextItem)> {
    let mut items = Vec::new();
    signs(project, &mut items);
    questions(project, &mut items);
    unticked(project, &mut items);
    broken(project, &mut items);
    items
}

/// The tasks of the epic `epic` still to finish; none for a number that is not an open epic.
fn open_tasks_of(project: &ProjectState, epic: u64) -> Option<u32> {
    project
        .epics
        .iter()
        .find(|candidate| candidate.number == epic)
        .map(|epic| epic.total.saturating_sub(epic.done))
}

fn signs(project: &ProjectState, items: &mut Vec<(u64, NextItem)>) {
    for epic in project
        .epics
        .iter()
        .filter(|epic| epic.stage == crate::domain::EpicStage::Unsigned)
    {
        items.push((
            epic.number,
            NextItem {
                kind: NextKind::Sign,
                project: project.project.clone(),
                title: format!("Sign epic #{}: {}", epic.number, epic.title),
                blocks: epic.total.saturating_sub(epic.done),
                how: format!(
                    "Read the epic and sign it from a terminal outside any instance; its \
                     {} tasks start once it is signed and challenged.",
                    epic.total.saturating_sub(epic.done)
                ),
                link: epic.url.clone(),
                command: Some(format!(
                    "cd {} && zaf epic sign {}",
                    shell_word(&project.root),
                    epic.number
                )),
            },
        ));
    }
}

fn questions(project: &ProjectState, items: &mut Vec<(u64, NextItem)>) {
    for question in &project.questions {
        let blocks = blocks_of(project, question);
        let item = if is_nowhere(&question.text) {
            failed(project, question, blocks)
        } else {
            asked(project, question, blocks)
        };
        items.push((question.number, item));
    }
}

/// The tasks a question holds back: a question on an epic holds the epic's tasks still to
/// finish, one on a task holds that task, one that names no issue holds none.
fn blocks_of(project: &ProjectState, question: &QuestionState) -> u32 {
    match question.raised_on {
        Some(issue) => open_tasks_of(project, issue).unwrap_or(1),
        None => 0,
    }
}

/// A question to rule on, a door when it was raised as one.
fn asked(project: &ProjectState, question: &QuestionState, blocks: u32) -> NextItem {
    let proposal = proposal_of(&question.text);
    let door = question.door;
    let ruling = proposal.as_deref().unwrap_or("<ruling>");
    let how = match (&proposal, door) {
        (Some(proposal), true) => format!(
            "A door, a one-way decision: read it twice. It proposes: {proposal}. Rule with \
             the proposal, or with your own answer."
        ),
        (Some(proposal), false) => {
            format!("It proposes: {proposal}. Rule with the proposal, or with your own answer.")
        }
        (None, true) => {
            "A door, a one-way decision: read it twice. It proposes no answer; give yours."
                .to_owned()
        }
        (None, false) => "It proposes no answer; give yours.".to_owned(),
    };
    NextItem {
        kind: if door {
            NextKind::Door
        } else {
            NextKind::Question
        },
        project: project.project.clone(),
        title: format!(
            "Question #{}: {}",
            question.number,
            headline(&question.text)
        ),
        blocks,
        how,
        link: question.url.clone(),
        command: Some(ruling_command(project, question.number, ruling)),
    }
}

/// A phase that got nowhere three times: its question, which the phase waits on.
fn failed(project: &ProjectState, question: &QuestionState, blocks: u32) -> NextItem {
    let held = question
        .text
        .split(';')
        .next()
        .unwrap_or(&question.text)
        .trim();
    let on = question
        .raised_on
        .map_or_else(String::new, |issue| format!(" on #{issue}"));
    NextItem {
        kind: NextKind::Failed,
        project: project.project.clone(),
        title: format!("Question #{}{on}: {held}", question.number),
        blocks,
        how: "It is not started again until this is ruled: read the log it names, fix the \
              cause, then rule the question."
            .to_owned(),
        link: question.url.clone(),
        command: Some(ruling_command(project, question.number, "<ruling>")),
    }
}

/// `cd <project> && zaf board ruling <question> "<ruling>"`, the ruling quoted for the shell.
fn ruling_command(project: &ProjectState, question: u64, ruling: &str) -> String {
    let ruling = if ruling == "<ruling>" {
        "'<ruling>'".to_owned()
    } else {
        shell_quote(ruling)
    };
    format!(
        "cd {} && zaf board ruling {question} {ruling}",
        shell_word(&project.root)
    )
}

/// Ready tasks in a project whose scheduler does not run.
fn unticked(project: &ProjectState, items: &mut Vec<(u64, NextItem)>) {
    let Some(watch) = &project.watch else {
        return;
    };
    if watch.running || project.missing {
        return;
    }
    let ready: Vec<u64> = project
        .queue
        .iter()
        .filter(|task| task.state == crate::domain::TicketState::Ready)
        .map(|task| task.number)
        .collect();
    let Some(oldest) = ready.iter().min().copied() else {
        return;
    };
    let blocks = u32::try_from(ready.len()).unwrap_or(u32::MAX);
    let tasks = if blocks == 1 { "task" } else { "tasks" };
    items.push((
        oldest,
        NextItem {
            kind: NextKind::Unticked,
            project: project.project.clone(),
            title: format!(
                "{blocks} ready {tasks} in {}: no scheduler ticks",
                project.project
            ),
            blocks,
            how: "Start the project's scheduler, naming the milestone's version; it picks the \
                  ready work up at its next tick."
                .to_owned(),
            link: None,
            command: Some(format!(
                "cd {} && zaf spawn --watch --version <version>",
                shell_word(&project.root)
            )),
        },
    ));
}

/// What keeps a project from running, one item for each fault.
fn broken(project: &ProjectState, items: &mut Vec<(u64, NextItem)>) {
    let held_back: u32 = project
        .epics
        .iter()
        .map(|epic| epic.total.saturating_sub(epic.done))
        .sum();
    for problem in &project.problems {
        let how = match (&problem.fix, problem.kind) {
            (Some(fix), _) => format!("{}. Fix it with `{fix}`.", sentence(&problem.detail)),
            (None, ProblemKind::Missing) => format!(
                "{}. Restore it, or drop the project from the hub's list.",
                sentence(&problem.detail)
            ),
            (None, _) => format!("{}.", sentence(&problem.detail)),
        };
        items.push((
            u64::MAX,
            NextItem {
                kind: NextKind::Broken,
                project: project.project.clone(),
                title: format!("{} cannot run: {}", project.project, problem.detail),
                blocks: held_back,
                how,
                link: None,
                command: problem.fix.clone(),
            },
        ));
    }
}

/// `text` without the full stop it may end with.
fn sentence(text: &str) -> &str {
    text.trim().trim_end_matches('.')
}

/// The first line of a question's text, cut at [`TITLE_LIMIT`] characters.
fn headline(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("");
    let line = line.trim();
    match line.char_indices().nth(TITLE_LIMIT) {
        Some((cut, _)) => format!("{}…", line[..cut].trim_end()),
        None => line.to_owned(),
    }
}

/// The answer a question proposes: what follows a line that opens with `Claude's answer` (as
/// the objectives skill writes it, with its date in brackets), `Proposed answer` or `Proposal`.
/// None when no such line holds an answer.
pub fn proposal_of(text: &str) -> Option<String> {
    const OPENINGS: [&str; 3] = ["claude's answer", "proposed answer", "proposal"];
    text.lines().find_map(|line| {
        let line = line.trim().trim_start_matches(['*', '-', ' ']);
        let lower = line.to_lowercase();
        let opening = OPENINGS
            .iter()
            .find(|opening| lower.starts_with(**opening))?;
        // `Claude's answer (2026-11-13; the Engineer adjusts):` keeps its colon after the
        // brackets, so a colon inside them does not count.
        let rest = line.get(opening.len()..)?;
        let rest = match rest.trim_start().strip_prefix('(') {
            Some(inner) => inner.split_once(')')?.1,
            None => rest,
        };
        let answer = rest
            .trim_start()
            .strip_prefix(':')?
            .trim_start_matches(['*', ' '])
            .trim();
        (!answer.is_empty()).then(|| answer.to_owned())
    })
}

/// A path as one shell word: as it is when it holds only safe characters, else in single quotes.
fn shell_word(path: &Path) -> String {
    let text = path.to_string_lossy();
    if !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+:@%,".contains(c))
    {
        text.into_owned()
    } else {
        shell_quote(&text)
    }
}

/// `text` in single quotes, a quote inside it closed, escaped and opened again.
fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests;
