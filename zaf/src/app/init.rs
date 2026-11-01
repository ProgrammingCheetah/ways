//! `zaf init`: write a minimal manifest by asking.
//!
//! Four questions on a line each: project name, board repo, forge repo (default:
//! the board repo) and the gate's full command (blank for no gate). The rest of
//! the manifest is left to the pack's defaults.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::composition::MANIFEST_FILE;
use crate::manifest::{Manifest, ManifestError};

/// Why no manifest was written.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// A manifest is already there; init never overwrites one.
    #[error("{0} already exists; edit it instead")]
    Exists(PathBuf),
    /// A required answer was left blank.
    #[error("{0} is required")]
    Blank(&'static str),
    /// The input ended before every question was answered.
    #[error("input ended before `{0}` was answered")]
    Closed(&'static str),
    /// The answers make a manifest that does not load; nothing was written.
    #[error(transparent)]
    Invalid(#[from] ManifestError),
    /// Reading the answers or writing the file failed.
    #[error(transparent)]
    Io(#[from] io::Error),
}

fn ask(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    question: &'static str,
) -> Result<String, InitError> {
    write!(output, "{question}: ")?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err(InitError::Closed(question));
    }
    Ok(line.trim().to_owned())
}

fn required(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    question: &'static str,
) -> Result<String, InitError> {
    let answer = ask(input, output, question)?;
    if answer.is_empty() {
        return Err(InitError::Blank(question));
    }
    Ok(answer)
}

fn quoted(text: &str) -> String {
    toml::Value::String(text.to_owned()).to_string()
}

/// Writes `<dir>/ZAF.toml` from answers read on `input`, prompting on `output`;
/// returns the file's path. Refuses, before asking anything, when one exists.
pub fn init(
    dir: &Path,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<PathBuf, InitError> {
    let path = dir.join(MANIFEST_FILE);
    if path.exists() {
        return Err(InitError::Exists(path));
    }
    let name = required(input, output, "project name")?;
    let board = required(input, output, "board repo (owner/name)")?;
    let forge = ask(input, output, "forge repo (blank: the board repo)")?;
    let forge = if forge.is_empty() {
        board.clone()
    } else {
        forge
    };
    let gate = ask(input, output, "gate full command (blank: none)")?;

    let mut text = format!(
        "[project]\nname = {}\n\n[board]\nkind = \"github-issues\"\nrepo = {}\n\n\
         [vcs]\nkind = \"jj\"\n\n[forge]\nkind = \"github\"\nrepo = {}\n",
        quoted(&name),
        quoted(&board),
        quoted(&forge),
    );
    if !gate.is_empty() {
        text.push_str(&format!("\n[gate]\nfull = {}\n", quoted(&gate)));
    }
    text.parse::<Manifest>()?;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| match error.kind() {
            io::ErrorKind::AlreadyExists => InitError::Exists(path.clone()),
            _ => InitError::Io(error),
        })?;
    file.write_all(text.as_bytes())?;
    Ok(path)
}

#[cfg(test)]
mod tests;
