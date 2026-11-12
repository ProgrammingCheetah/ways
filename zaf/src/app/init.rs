//! `zaf init`: write a minimal manifest by asking.
//!
//! Four questions on a line each: project name, board repo, forge repo and the gate's
//! full command. Each offers the value init can work out as the default a blank answer
//! takes: the folder's name, the `owner/name` of the git remote `origin` (a GitHub one),
//! and no gate. The written manifest also carries the standard values a tick needs: the
//! claude runner, the roles the phase actions run as, the review steps, the phase lists
//! and the pack's tag.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

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
    shown: &str,
) -> Result<String, InitError> {
    write!(output, "{shown}: ")?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err(InitError::Closed(question));
    }
    Ok(line.trim().to_owned())
}

/// Asks `question`, showing `default`; a blank answer takes it, and with none it is
/// refused naming the question.
fn required(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    question: &'static str,
    default: Option<&str>,
) -> Result<String, InitError> {
    let shown = match default {
        Some(default) => format!("{question} [{default}]"),
        None => question.to_owned(),
    };
    let answer = ask(input, output, question, &shown)?;
    match (answer.is_empty(), default) {
        (false, _) => Ok(answer),
        (true, Some(default)) => Ok(default.to_owned()),
        (true, None) => Err(InitError::Blank(question)),
    }
}

/// The `owner/name` of a GitHub remote URL, ssh or https; none for any other.
fn github_repo(url: &str) -> Option<String> {
    let url = url.trim();
    let path = [
        "git@github.com:",
        "ssh://git@github.com/",
        "git://github.com/",
    ]
    .iter()
    .find_map(|prefix| url.strip_prefix(prefix))
    .or_else(|| {
        let rest = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;
        let rest = rest.split_once('@').map_or(rest, |(_, host)| host);
        rest.strip_prefix("github.com/")
    })?;
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    let valid =
        |part: &str| !part.is_empty() && !part.contains('/') && !part.contains(char::is_whitespace);
    (valid(owner) && valid(name)).then(|| format!("{owner}/{name}"))
}

/// The GitHub repo the git remote `origin` of `dir` names; none without git, a remote or
/// a GitHub one.
fn origin_repo(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    github_repo(&String::from_utf8_lossy(&out.stdout))
}

/// The folder's name, when it has one that is text.
fn folder_name(dir: &Path) -> Option<String> {
    dir.file_name()?.to_str().map(str::to_owned)
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
    let remote = origin_repo(dir);
    let name = required(input, output, "project name", folder_name(dir).as_deref())?;
    let board = required(input, output, "board repo (owner/name)", remote.as_deref())?;
    let forge = required(
        input,
        output,
        "forge repo (owner/name)",
        Some(remote.as_deref().unwrap_or(&board)),
    )?;
    let gate = ask(
        input,
        output,
        "gate full command",
        "gate full command [blank: none]",
    )?;

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
