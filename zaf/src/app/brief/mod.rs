//! `zaf brief`: one context pack per task, so a programmer starts from a file instead of a look-around.
//!
//! The pack holds, in order: the ticket body, each design page the ticket names
//! in full, `nodes chain <path>` for each owned path (or a note when no `nodes`
//! binary is on PATH), the project's style file, and the interface help page.
//! It is written to `.zaf/runs/<instance slug>/brief-<task>.md`, the slug being
//! [`Instance::slug_of`] of the instance's name.

use std::fmt::{self, Write as _};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::domain::{Instance, IssueNumber, PageId, TaskTemplate};
use crate::ports::{Board, Design, Page, PortError, Ticket};

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

/// Runs `nodes chain <path>` and returns its output, or why it failed.
pub type NodesChain<'a> = &'a dyn Fn(&str) -> Result<String, String>;

/// What the pack is built for.
#[derive(Debug, Clone, Copy)]
pub struct BriefQuery<'a> {
    /// The task.
    pub task: IssueNumber,
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
    /// The `nodes` binary; `None` when it is not on PATH.
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

/// Builds the task's pack, writes it and returns where, with its size.
pub fn brief(query: &BriefQuery<'_>, ports: &BriefPorts<'_>) -> Result<Brief, BriefError> {
    if !is_path_segment(query.instance) {
        return Err(BriefError::Instance(query.instance.to_owned()));
    }
    let ticket = ports.board.task(query.task).map_err(BriefError::Board)?;
    let template = ticket.body.parse::<TaskTemplate>().ok();
    let pages = match (&template, ports.design) {
        (Some(template), Some(design)) => {
            named_pages(design, &template.design_pages).map_err(BriefError::Design)?
        }
        _ => Vec::new(),
    };
    let text = render(query, ports, &ticket, template.as_ref(), &pages)
        .expect("writing to a String cannot fail");

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

fn is_path_segment(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\'])
}

fn render(
    query: &BriefQuery<'_>,
    ports: &BriefPorts<'_>,
    ticket: &Ticket,
    template: Option<&TaskTemplate>,
    pages: &[Page],
) -> Result<String, fmt::Error> {
    let mut text = String::new();
    writeln!(text, "# Brief: {} {}\n", ticket.number, ticket.title)?;

    writeln!(text, "## Ticket\n\n{}\n", ticket.body.trim_end())?;
    if template.is_none() {
        writeln!(
            text,
            "The body does not follow the task template, so no pages or paths are read from it.\n"
        )?;
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
        (None, Some(_)) => {
            writeln!(text, "{NODES_ABSENT_NOTE}\n")?;
        }
        (Some(chain), Some(paths)) => {
            for path in paths {
                let output = chain(&path).unwrap_or_else(|why| format!("nodes failed: {why}"));
                writeln!(text, "### {path}\n\n```\n{}\n```\n", output.trim_end())?;
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

    writeln!(text, "## Interface\n\n{}", INTERFACE_PAGE.trim_end())?;
    Ok(text)
}

#[cfg(test)]
mod tests;
