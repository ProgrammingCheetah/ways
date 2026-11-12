//! `zaf init`: write a minimal manifest by asking.
//!
//! Four questions on a line each: project name, board repo, forge repo (default:
//! the board repo) and the gate's full command (blank for no gate). The written manifest
//! also carries the standard values a tick needs: the claude runner, the roles the phase
//! actions run as, the review steps, the phase lists and the pack's tag.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::app::profile::ProfileError;
use crate::manifest::{Manifest, ManifestError, ProjectFiles, ROOT_FILE};

/// The file `init` writes.
pub const MANIFEST_FILE: &str = ROOT_FILE;

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
    /// The marketplace's current pack tag could not be learned; nothing was written.
    #[error("the pack's current tag: {0}")]
    Pack(#[from] ProfileError),
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

/// Learns the marketplace's current pack tag, such as `teams-v2.0.0`; called once, after the
/// answers are in and before anything is written.
pub type LearnTag<'a> = &'a mut dyn FnMut() -> Result<String, ProfileError>;

/// The standard values every new project's manifest carries after its own sections: ways'
/// runner command and timeout, the roles the phase actions run as, the review steps and the
/// phase lists. `[pack] tag` follows them.
const STANDARD: &str = r#"
[runners.claude]
command = "claude -p {prompt} --model {model} --add-dir {root}"
timeout_minutes = 45

[roles.programmer]
model = "sonnet"
effort = "high"
profile = "lean"
turns = 150

[roles.implement]
model = "sonnet"
effort = "high"
profile = "lean"
turns = 150

[roles.fix]
model = "sonnet"
effort = "high"
profile = "lean"
turns = 150

[roles.merge]
model = "sonnet"
effort = "high"
profile = "lean"
turns = 1

[roles.release]
model = "opus"
effort = "xhigh"
profile = "build"
turns = 200

[check.pr]
steps = ["ci"]

[phases]
Task = ["implement", "ci", "review", "merge"]
Bug = ["implement", "ci", "review", "merge"]
"#;

/// Writes `<dir>/.zaf.toml` from answers read on `input`, prompting on `output`;
/// returns the file's path. Refuses, before asking anything, when `dir` holds any manifest
/// file. `learn_tag` gives `[pack] tag`. See [`init_with_home`] to leave the zaf home's own
/// `config.toml` out of that check.
pub fn init(
    dir: &Path,
    learn_tag: LearnTag<'_>,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<PathBuf, InitError> {
    init_with_home(dir, None, learn_tag, input, output)
}

/// [`init`] knowing the zaf `home`: when `dir/.zaf` is the zaf home, its `config.toml` is the
/// machine's, not a project manifest, and does not make init refuse.
pub fn init_with_home(
    dir: &Path,
    home: Option<&Path>,
    learn_tag: LearnTag<'_>,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<PathBuf, InitError> {
    let path = dir.join(MANIFEST_FILE);
    let found = ProjectFiles::find(dir, home);
    if let Some(existing) = found.root.or(found.folder).or(found.legacy) {
        return Err(InitError::Exists(existing));
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
    text.push_str(STANDARD);
    text.push_str(&format!("\n[pack]\ntag = {}\n", quoted(&learn_tag()?)));
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
