//! `zaf brief`: one context pack per task, so a programmer starts from a file instead of a look-around.
//!
//! The pack holds, in order: the ticket body, the questions raised on the task
//! with their rulings (`## Rulings`, newest last, so a settled question is not asked again),
//! each design page the ticket names
//! in full, `nodes chain <path>` for each owned path (or, when the cli passes no
//! chain, a note and the plain list of owned paths), the project's style file,
//! the actions the reader may read (`## Actions`: name, tier and description, left out when
//! there are none) and the interface help page. Each place gets one map, and a node that two
//! maps share is printed once, under the first place that reaches it.
//! A resumed phase's pack ([`brief_for_phase`] with a [`PhaseView`]) gains, after the rulings,
//! `## Diff` (the recorded pull request's change against its base), `## Findings` (the review
//! comments made at its current head and its failed checks) and `## Hand-off` (the comment the
//! exiting model left); a task with no recorded pull request gets today's pack.
//! It is written to `.zaf/runs/<instance slug>/brief-<task>.md`, the slug being
//! [`Instance::slug_of`] of the instance's name.

use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::actions::{Caller, ListedAction, ResolveError, list_actions};
use crate::composition::STATE_DIR;
use crate::domain::{Instance, PageId, TaskTemplate, Ticket, TicketId};
use crate::ports::{Board, Design, NewQuestion, Page, PortError};

mod phase;

pub use phase::PhaseView;

/// The interface help page every brief ends with.
pub const INTERFACE_PAGE: &str = include_str!("../../../assets/interface.md");

/// The runs folder under the state folder, shared with the gate's logs.
pub const RUNS_DIR: &str = "runs";

/// What the runs folder's `.gitignore` holds: everything in it is local.
const IGNORE_ALL: &str = "*\n";

/// Characters per token, for the estimate printed beside the pack.
const CHARS_PER_TOKEN: usize = 4;

/// The note written in place of the node chains when `nodes` is not on PATH.
pub const NODES_ABSENT_NOTE: &str = "No `nodes` binary on PATH; the node chains are skipped.";

/// The note written in place of the node chains when the project root has no node tree.
///
/// The cli passes no chain in that case, so a missing `nodes` binary reads the same.
pub const NO_NODE_TREE_NOTE: &str =
    "No node tree at the project root; the node chains are skipped.";

/// The note written under a place whose map holds only nodes an earlier map printed.
pub const ALL_PRINTED_NOTE: &str = "Every node of this place's map is printed above.";

/// Runs `nodes chain <path>` and returns its output, or why it failed.
pub type NodesChain<'a> = &'a dyn Fn(&str) -> Result<String, String>;

/// Where the actions the pack lists are read, and who reads them.
#[derive(Debug, Clone, Copy)]
pub struct ActionsView<'a> {
    /// The project root, whose `.zaf/actions/` is the project tier.
    pub project_root: &'a Path,
    /// The machine's zaf home, whose `actions/` is the machine tier; `None` when the machine has
    /// none, and then only the project's actions are listed.
    pub home: Option<&'a Path>,
    /// Who reads the pack: an action their mode hides from them is not listed.
    pub caller: &'a Caller,
}

/// What the pack is built for.
#[derive(Debug, Clone, Copy)]
pub struct BriefQuery<'a> {
    /// The task.
    pub task: TicketId,
    /// The instance the pack is for; one path segment, its folder under `runs`.
    pub instance: &'a str,
    /// The project's state folder, `.zaf`.
    pub state_dir: &'a Path,
    /// The project's style file, when it declares one.
    pub style: Option<&'a Path>,
}

/// What the use case reads through.
#[derive(Clone, Copy)]
pub struct BriefPorts<'a> {
    /// The board holding the ticket.
    pub board: &'a dyn Board,
    /// The design source; `None` when the project has none.
    pub design: Option<&'a dyn Design>,
    /// The `nodes` chain; `None` when the project has no node tree or `nodes` is not on PATH.
    pub nodes: Option<NodesChain<'a>>,
}

/// The written pack.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Brief {
    /// Where it was written.
    pub path: PathBuf,
    /// Its length in characters.
    pub chars: usize,
    /// The token estimate: characters over four.
    pub tokens: usize,
}

/// Why no pack was written.
#[derive(Debug, thiserror::Error)]
pub enum BriefError {
    /// The board could not give the ticket.
    #[error("board: {0}")]
    Board(PortError),
    /// The design source failed while reading a named page.
    #[error("design: {0}")]
    Design(PortError),
    /// The forge failed while reading the pull request's findings or hand-off.
    #[error("forge: {0}")]
    Forge(PortError),
    /// The VCS failed while reading the pull request's diff.
    #[error("vcs: {0}")]
    Vcs(PortError),
    /// The instance is not a single path segment.
    #[error("`{0}` is not an instance name: one path segment, no `/`")]
    Instance(String),
    /// The pack could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// The file or folder.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
}

/// Why a pack with actions was not written.
#[derive(Debug, thiserror::Error)]
pub enum BriefActionsError {
    /// An actions folder or a file in it could not be read.
    #[error("actions: {0}")]
    Actions(ResolveError),
    /// The pack itself could not be built.
    #[error(transparent)]
    Brief(#[from] BriefError),
}

/// Builds the task's pack with no `## Actions` section, writes it and returns where, with its size.
pub fn brief(query: &BriefQuery<'_>, ports: &BriefPorts<'_>) -> Result<Brief, BriefError> {
    build(query, ports, &[], None)
}

/// Builds the task's pack, writes it and returns where, with its size.
///
/// The pack gains an `## Actions` section listing each project and machine action the view's
/// caller may read, unless there is none: then it is the pack [`brief`] writes.
pub fn brief_with_actions(
    query: &BriefQuery<'_>,
    ports: &BriefPorts<'_>,
    view: ActionsView<'_>,
) -> Result<Brief, BriefActionsError> {
    brief_for_phase(query, ports, view, None)
}

/// [`brief_with_actions`] for a resumed phase.
///
/// With a [`PhaseView`] (the task has a recorded pull request) the pack gains `## Diff`,
/// `## Findings` (those at the pull request's current head) and `## Hand-off`; with `None` it is
/// today's pack, byte for byte.
pub fn brief_for_phase(
    query: &BriefQuery<'_>,
    ports: &BriefPorts<'_>,
    view: ActionsView<'_>,
    phase: Option<&PhaseView<'_>>,
) -> Result<Brief, BriefActionsError> {
    // No machine home: the machine tier is pointed at the project's own state folder, whose
    // `actions/` is the project tier. Every name then resolves to the project, so none is listed
    // from the machine.
    let project_state = view.project_root.join(STATE_DIR);
    let home = view.home.unwrap_or(&project_state);
    let actions = list_actions(Some(view.project_root), home, view.caller)
        .map_err(BriefActionsError::Actions)?;
    Ok(build(query, ports, &actions, phase)?)
}

fn build(
    query: &BriefQuery<'_>,
    ports: &BriefPorts<'_>,
    actions: &[ListedAction],
    phase: Option<&PhaseView<'_>>,
) -> Result<Brief, BriefError> {
    if !is_path_segment(query.instance) {
        return Err(BriefError::Instance(query.instance.to_owned()));
    }
    let ticket = ports.board.task(query.task).map_err(BriefError::Board)?;
    let template = ticket.template().ok();
    let pages = match (&template, ports.design) {
        (Some(template), Some(design)) => {
            named_pages(design, &template.design_pages).map_err(BriefError::Design)?
        }
        _ => Vec::new(),
    };
    let questions = ports
        .board
        .questions_on(query.task)
        .map_err(BriefError::Board)?;
    // Read before anything is written, so a forge that fails leaves no half pack.
    let resumed = phase.map(phase::sections).transpose()?;
    let parts = Parts {
        ticket: &ticket,
        questions: &questions,
        template: template.as_ref(),
        pages: &pages,
        actions,
        resumed: resumed.as_deref(),
    };
    let text = render(query, ports, &parts).expect("writing to a String cannot fail");

    let runs = ensure_runs_dir(query.state_dir)
        .map_err(|source| write_error(&query.state_dir.join(RUNS_DIR), source))?;
    let dir = runs.join(Instance::slug_of(query.instance));
    fs::create_dir_all(&dir).map_err(|source| write_error(&dir, source))?;
    let path = dir.join(format!("brief-{}.md", query.task.get()));
    fs::write(&path, &text).map_err(|source| write_error(&path, source))?;
    let chars = text.chars().count();
    Ok(Brief {
        path,
        chars,
        tokens: chars.div_ceil(CHARS_PER_TOKEN),
    })
}

/// The pages a ticket's `Design pages` field names that the design source holds, in the order named.
///
/// The field is prose: every token that reads as a page id is tried, and a
/// token the source does not hold is prose, not an error.
pub(crate) fn named_pages(design: &dyn Design, field: &str) -> Result<Vec<Page>, PortError> {
    let mut seen: Vec<PageId> = Vec::new();
    let mut pages = Vec::new();
    for token in tokens(field) {
        let Ok(id) = token.parse::<PageId>() else {
            continue;
        };
        if seen.contains(&id) {
            continue;
        }
        seen.push(id.clone());
        match design.page(&id) {
            Ok(page) => pages.push(page),
            Err(PortError::NotFound(_)) => {}
            Err(other) => return Err(other),
        }
    }
    Ok(pages)
}

/// The paths an `Owned paths` field names, with trailing globs dropped, in the order named.
pub(crate) fn owned_paths(field: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for token in tokens(field).filter(|token| token.contains('/')) {
        let mut path = token;
        while let Some(stripped) = path
            .strip_suffix("/**")
            .or_else(|| path.strip_suffix("/*"))
            .or_else(|| path.strip_suffix('/'))
        {
            path = stripped;
        }
        if !path.is_empty() && !paths.iter().any(|known| known == path) {
            paths.push(path.to_owned());
        }
    }
    paths
}

/// Creates `<state>/runs` with its ignore-everything `.gitignore`, as the gate does.
pub(crate) fn ensure_runs_dir(state_dir: &Path) -> io::Result<PathBuf> {
    let runs = state_dir.join(RUNS_DIR);
    fs::create_dir_all(&runs)?;
    let ignore = runs.join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, IGNORE_ALL)?;
    }
    Ok(runs)
}

fn write_error(path: &Path, source: io::Error) -> BriefError {
    BriefError::Write {
        path: path.to_owned(),
        source,
    }
}

/// Whitespace-, comma- and semicolon-separated tokens, stripped of quoting and closing punctuation.
fn tokens(field: &str) -> impl Iterator<Item = &str> {
    field
        .split(|character: char| character.is_whitespace() || matches!(character, ',' | ';'))
        .map(|token| {
            token.trim_matches(|character: char| {
                matches!(
                    character,
                    '`' | '"' | '\'' | '(' | ')' | '[' | ']' | '.' | ':'
                )
            })
        })
        .filter(|token| !token.is_empty())
}

/// The nodes of one place's map that no earlier map printed, as one text, and records them in `printed`.
///
/// A map is the `nodes chain` output: node blocks split by blank lines, each
/// opening with a line that starts with the node's path. A node is the same
/// node when its path is, so two places under one folder share the folder's
/// nodes and the pack prints them once, under the first place.
fn unprinted_nodes(map: &str, printed: &mut Vec<String>) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for block in map.trim().split("\n\n") {
        let block = block.trim_end();
        let Some(key) = block.split_whitespace().next() else {
            continue;
        };
        if printed.iter().any(|known| known == key) {
            continue;
        }
        printed.push(key.to_owned());
        kept.push(block);
    }
    kept.join("\n\n")
}

fn is_path_segment(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

/// What the pack is written from, read before it is rendered.
struct Parts<'a> {
    ticket: &'a Ticket,
    questions: &'a [(Ticket, Option<String>)],
    template: Option<&'a TaskTemplate>,
    pages: &'a [Page],
    actions: &'a [ListedAction],
    /// The resumed phase's sections, when the task has a recorded pull request.
    resumed: Option<&'a str>,
}

fn render(
    query: &BriefQuery<'_>,
    ports: &BriefPorts<'_>,
    parts: &Parts<'_>,
) -> Result<String, fmt::Error> {
    let Parts {
        ticket,
        questions,
        template,
        pages,
        actions,
        resumed,
    } = *parts;
    let mut text = String::new();
    writeln!(text, "# Brief: {} {}\n", ticket.id, ticket.title)?;

    writeln!(text, "## Ticket\n\n{}\n", ticket.body.as_str().trim_end())?;
    if template.is_none() {
        writeln!(
            text,
            "The body does not follow the task template, so no pages or paths are read from it.\n"
        )?;
    }

    writeln!(text, "## Rulings\n")?;
    if questions.is_empty() {
        writeln!(text, "— none\n")?;
    }
    for (question, ruling) in questions {
        writeln!(text, "{} {}", question.id, NewQuestion::text_of(question))?;
        match ruling {
            Some(ruling) => writeln!(text, "ruling: {ruling}\n")?,
            None => writeln!(text, "open\n")?,
        }
    }

    if let Some(resumed) = resumed {
        write!(text, "{resumed}")?;
    }

    writeln!(text, "## Design pages\n")?;
    if ports.design.is_none() {
        writeln!(
            text,
            "The project declares no design source; the ticket is the design.\n"
        )?;
    } else if pages.is_empty() {
        writeln!(text, "The ticket names no page the design source holds.\n")?;
    }
    for page in pages {
        writeln!(
            text,
            "### {} {} ({})\n\n{}\n",
            page.id,
            page.title,
            page.status,
            page.text.trim_end()
        )?;
    }

    writeln!(text, "## Owned paths\n")?;
    let paths = template.map(|template| owned_paths(&template.owned_paths));
    match (ports.nodes, paths) {
        (_, None) => {}
        (None, Some(paths)) => {
            writeln!(text, "{NO_NODE_TREE_NOTE}\n")?;
            for path in paths {
                writeln!(text, "- {path}")?;
            }
            writeln!(text)?;
        }
        (Some(chain), Some(paths)) => {
            let mut printed: Vec<String> = Vec::new();
            for path in paths {
                match chain(&path) {
                    Ok(output) => {
                        let nodes = unprinted_nodes(&output, &mut printed);
                        if nodes.is_empty() {
                            writeln!(text, "### {path}\n\n{ALL_PRINTED_NOTE}\n")?;
                        } else {
                            writeln!(text, "### {path}\n\n```\n{nodes}\n```\n")?;
                        }
                    }
                    Err(why) => {
                        writeln!(text, "### {path}\n\n```\nnodes failed: {why}\n```\n")?;
                    }
                }
            }
        }
    }

    writeln!(text, "## Style\n")?;
    match query.style {
        None => {
            writeln!(text, "The project declares no style file.\n")?;
        }
        Some(path) => match fs::read_to_string(path) {
            Ok(style) => {
                writeln!(text, "{}\n", style.trim_end())?;
            }
            Err(error) => {
                writeln!(
                    text,
                    "The style file {} could not be read: {error}\n",
                    path.display()
                )?;
            }
        },
    }

    if !actions.is_empty() {
        writeln!(text, "## Actions\n")?;
        for action in actions {
            writeln!(
                text,
                "- {} ({}): {}",
                action.name, action.tier, action.description
            )?;
        }
        writeln!(text)?;
    }

    writeln!(text, "## Interface\n\n{}", INTERFACE_PAGE.trim_end())?;
    Ok(text)
}

#[cfg(test)]
mod tests;
