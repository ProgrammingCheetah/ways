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
//!   that does not answer, profiles that are stale (one item a project), a folder that is gone;
//! - a release phase that is due: a release's objectives or design, `zaf begin --version <v>`.
//!
//! An item's `blocks` is the number of tasks it holds back: for a question, the waiting tickets
//! that name it among what they wait on, where the core placed them. Items sort by it, most first. Ties
//! break by age, and the board gives no time for a question or a ready task, so the age is the
//! issue's number: a lower one was opened first. Items still equal keep the list's order, the
//! projects as they joined and each project's items as listed above. A project nobody reads
//! (`missing`, or whose board did not answer) shows its problems and nothing else.

use std::path::{Path, PathBuf};

use super::state::{
    MachineState, Problem, ProblemKind, ProjectState, QuestionState, ReleaseDueState, WaitState,
    WatchState,
};
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
    /// A release's objectives or design are due.
    Phase,
}

/// One thing that waits on the Engineer.
///
/// Interface: `app::hub::next::NextItem { kind, project, title, blocks, how, link, command,
/// number, proposal }`.
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
    /// The issue it is about: the question to rule or the epic to sign; none for the others.
    pub number: Option<u64>,
    /// The answer a question proposes ([`proposal_of`]); none when it proposes none.
    pub proposal: Option<String>,
}

/// The longest a question's title is, in characters.
const TITLE_LIMIT: usize = 100;

/// What the item rules read of a project: its name and root, its epics, its open questions, the
/// scheduler's state, its ready queue, its problems and its due releases. A chair's
/// [`ProjectState`] fills it ([`Inbox::from`]) and so does a `zaf status` screen
/// (`Status::needs_you`); a field one of them does not hold stays empty.
///
/// Interface: `app::hub::next::Inbox`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inbox {
    /// The project's name.
    pub project: String,
    /// Where the project lives.
    pub root: PathBuf,
    /// Whether its folder is gone.
    pub missing: bool,
    /// Its open epics.
    pub epics: Vec<InboxEpic>,
    /// Its open questions, in issue order.
    pub questions: Vec<QuestionState>,
    /// Whether its scheduler runs; none when nothing read it.
    pub watch: Option<WatchState>,
    /// The numbers of its ready tasks, the queue's ready ones.
    pub ready: Vec<u64>,
    /// What keeps it from running.
    pub problems: Vec<Problem>,
    /// The releases whose objectives or design are due.
    pub releases_due: Vec<ReleaseDueState>,
}

/// An epic as the item rules read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxEpic {
    /// Its issue number.
    pub number: u64,
    /// Its title.
    pub title: String,
    /// Where it stands.
    pub stage: crate::domain::EpicStage,
    /// The release it is of, when its body names one.
    pub release: Option<String>,
    /// Where a person opens it; none when the board gave no link.
    pub url: Option<String>,
    /// Its tasks done.
    pub done: u32,
    /// Its tasks that count.
    pub total: u32,
    /// Its tasks, each with what it waits on.
    pub tasks: Vec<InboxTask>,
}

/// A task of an epic as the item rules read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InboxTask {
    /// Its issue number.
    pub number: u64,
    /// What it waits on, where the core placed it; empty for a task that does not wait or that
    /// the core did not place.
    pub waits: Vec<WaitState>,
}

impl From<&ProjectState> for Inbox {
    fn from(project: &ProjectState) -> Self {
        Self {
            project: project.project.clone(),
            root: project.root.clone(),
            missing: project.missing,
            epics: project
                .epics
                .iter()
                .map(|epic| InboxEpic {
                    number: epic.number,
                    title: epic.title.clone(),
                    stage: epic.stage,
                    release: epic.release.clone(),
                    url: epic.url.clone(),
                    done: epic.done,
                    total: epic.total,
                    tasks: epic
                        .tasks
                        .iter()
                        .map(|task| InboxTask {
                            number: task.number,
                            waits: task.waits.clone().unwrap_or_default(),
                        })
                        .collect(),
                })
                .collect(),
            questions: project.questions.clone(),
            watch: project.watch.clone(),
            ready: project
                .queue
                .iter()
                .filter(|task| task.state == crate::domain::TicketState::Ready)
                .map(|task| task.number)
                .collect(),
            problems: project.problems.clone(),
            releases_due: project.releases_due.clone(),
        }
    }
}

/// The Next for you list of `state`: every item, most blocking first; empty when nothing waits.
///
/// Interface: `next_for_you(&MachineState) -> Vec<NextItem>`.
pub fn next_for_you(state: &MachineState) -> Vec<NextItem> {
    ranked(
        state
            .projects
            .iter()
            .flat_map(|project| of_project(&Inbox::from(project)))
            .collect(),
    )
}

/// The Next for you list of one project: its items in the order [`next_for_you`] gives them.
pub fn next_for_project(project: &ProjectState) -> Vec<NextItem> {
    items(&Inbox::from(project))
}

/// The items of one project's `inbox`, most blocking first, ties by age.
///
/// Interface: `app::hub::next::items(&Inbox) -> Vec<NextItem>`.
pub fn items(inbox: &Inbox) -> Vec<NextItem> {
    ranked(of_project(inbox))
}

/// The items most blocking first, ties by age; what is equal keeps the order it was built in.
fn ranked(mut items: Vec<(u64, NextItem)>) -> Vec<NextItem> {
    items.sort_by_key(|(age, item)| (std::cmp::Reverse(item.blocks), *age));
    items.into_iter().map(|(_, item)| item).collect()
}

/// A project's items, each with its age.
fn of_project(project: &Inbox) -> Vec<(u64, NextItem)> {
    let mut items = Vec::new();
    signs(project, &mut items);
    questions(project, &mut items);
    unticked(project, &mut items);
    broken(project, &mut items);
    phases(project, &mut items);
    items
}

/// The tasks of the epic `epic` still to finish; none for a number that is not an open epic.
fn open_tasks_of(project: &Inbox, epic: u64) -> Option<u32> {
    project
        .epics
        .iter()
        .find(|candidate| candidate.number == epic)
        .map(|epic| epic.total.saturating_sub(epic.done))
}

fn signs(project: &Inbox, items: &mut Vec<(u64, NextItem)>) {
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
                number: Some(epic.number),
                proposal: None,
            },
        ));
    }
}

fn questions(project: &Inbox, items: &mut Vec<(u64, NextItem)>) {
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

/// The tasks a question holds back: the waiting tickets that name it among what they wait on,
/// where the core placed the project's tickets. Where it placed none, or none names it, a
/// question on an epic holds the epic's tasks still to finish, one on a task holds that task, one
/// that names no issue holds none.
fn blocks_of(project: &Inbox, question: &QuestionState) -> u32 {
    let naming = project
        .epics
        .iter()
        .flat_map(|epic| &epic.tasks)
        .filter(|task| {
            task.waits
                .iter()
                .any(|wait| wait.is_question(question.number))
        })
        .count();
    if naming > 0 {
        return u32::try_from(naming).unwrap_or(u32::MAX);
    }
    match question.raised_on {
        Some(issue) => open_tasks_of(project, issue).unwrap_or(1),
        None => 0,
    }
}

/// A question to rule on, a door when it was raised as one.
fn asked(project: &Inbox, question: &QuestionState, blocks: u32) -> NextItem {
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
        number: Some(question.number),
        proposal,
    }
}

/// A phase that got nowhere three times: its question, which the phase waits on.
fn failed(project: &Inbox, question: &QuestionState, blocks: u32) -> NextItem {
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
        number: Some(question.number),
        proposal: None,
    }
}

/// `cd <project> && zaf board ruling <question> "<ruling>"`, the ruling quoted for the shell.
fn ruling_command(project: &Inbox, question: u64, ruling: &str) -> String {
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
fn unticked(project: &Inbox, items: &mut Vec<(u64, NextItem)>) {
    let Some(watch) = &project.watch else {
        return;
    };
    if watch.running || project.missing {
        return;
    }
    let ready = &project.ready;
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
            number: None,
            proposal: None,
        },
    ));
}

/// What keeps a project from running, one item for each fault.
fn broken(project: &Inbox, items: &mut Vec<(u64, NextItem)>) {
    let held_back: u32 = project
        .epics
        .iter()
        .map(|epic| epic.total.saturating_sub(epic.done))
        .sum();
    // An epic with every task dropped is shown in the problems, not as the project unable to run.
    let faults = project
        .problems
        .iter()
        .filter(|problem| problem.kind != ProblemKind::EveryTaskDropped);
    for problem in faults {
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
                number: None,
                proposal: None,
            },
        ));
    }
}

/// A release whose objectives or design are due: one item each. It holds back the open tasks of
/// the release's epics, none before its epics are filed; its age is its place among the due.
fn phases(project: &Inbox, items: &mut Vec<(u64, NextItem)>) {
    if project.missing {
        return;
    }
    for (index, due) in project.releases_due.iter().enumerate() {
        let blocks: u32 = project
            .epics
            .iter()
            .filter(|epic| epic.release.as_deref() == Some(due.version.as_str()))
            .map(|epic| epic.total.saturating_sub(epic.done))
            .sum();
        items.push((
            u64::try_from(index).unwrap_or(u64::MAX),
            NextItem {
                kind: NextKind::Phase,
                project: project.project.clone(),
                title: format!(
                    "Release {}: {} due",
                    due.version,
                    if due.phase == "objectives" {
                        "its objectives are"
                    } else {
                        "its design is"
                    }
                ),
                blocks,
                how: format!(
                    "Nothing of {} is built before its {} are written and signed: run the session \
                     and sign the document it leaves.",
                    due.version, due.phase
                ),
                link: None,
                command: Some(format!(
                    "cd {} && {}",
                    shell_word(&project.root),
                    due.command
                )),
                number: None,
                proposal: None,
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
/// the objectives skill writes it, with its date in brackets), `Proposed answer` or `Proposal`;
/// else the option marked recommended, in any of the house forms: `(Recommended: <answer>)`,
/// `(a) Recommended: <answer>; (b) …` and `(a) **(Recommended)** <answer>`, the option ending at
/// the next option's `(b)` or at the line's end. None when the text proposes nothing.
pub fn proposal_of(text: &str) -> Option<String> {
    opening_line(text).or_else(|| recommended(text))
}

/// The answer after a line that opens with one of the openings.
fn opening_line(text: &str) -> Option<String> {
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

/// The option a question marks recommended, in the first house form that holds.
fn recommended(text: &str) -> Option<String> {
    const WORD: &str = "recommended";
    // ASCII lowering keeps every byte where it was, so an index into it indexes `text`.
    let lower = text.to_ascii_lowercase();
    lower.match_indices(WORD).find_map(|(at, _)| {
        let before = &text[..at];
        let after = &text[at + WORD.len()..];
        let opened = before.ends_with('(');
        let answer = if opened && after.starts_with(':') {
            // `(Recommended: <answer>)`: to the bracket that closes it.
            closed(&after[1..])
        } else if opened && after.starts_with(')') {
            // `(a) **(Recommended)** <answer>`: the option's text after the mark.
            option_text(after[1..].trim_start_matches(['*', ' ']))
        } else if after.trim_start_matches('*').starts_with(':')
            && ends_with_option(before.trim_end_matches(['*', ' ']))
        {
            // `(a) Recommended: <answer>; (b) …`.
            let rest = after.trim_start_matches('*');
            option_text(rest[1..].trim_start_matches(['*', ' ']))
        } else {
            None
        }?;
        let answer = answer.trim();
        (!answer.is_empty()).then(|| answer.to_owned())
    })
}

/// `text` up to the `)` that closes a bracket opened just before it.
fn closed(text: &str) -> Option<&str> {
    let mut depth = 1_usize;
    for (at, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[..at]);
                }
            }
            _ => {}
        }
    }
    None
}

/// An option's text: up to the next option's marker or the line's end, without the `;`, `,` or
/// `or` that joins it to the next.
fn option_text(text: &str) -> Option<&str> {
    let line = text.lines().next()?;
    let end = (0..line.len())
        .filter(|at| line.is_char_boundary(*at))
        .find(|at| starts_option(line, *at))
        .unwrap_or(line.len());
    let mut answer = line[..end].trim_end_matches([' ', ';', ',']);
    if let Some(cut) = answer.strip_suffix(" or") {
        answer = cut.trim_end_matches([' ', ';', ',']);
    }
    Some(answer)
}

/// Whether an option's marker, `(b)`, starts at `at` in `line`, after a space, a `;` or nothing.
fn starts_option(line: &str, at: usize) -> bool {
    let bytes = line.as_bytes();
    let spaced = at == 0 || matches!(bytes[at - 1], b' ' | b';' | b'\t');
    spaced
        && bytes.get(at) == Some(&b'(')
        && bytes.get(at + 1).is_some_and(u8::is_ascii_lowercase)
        && bytes.get(at + 2) == Some(&b')')
}

/// Whether `text` ends with an option's marker, `(a)`.
fn ends_with_option(text: &str) -> bool {
    text.len() >= 3 && starts_option(text, text.len() - 3)
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
