//! The questions `zaf setup` asks where a folder has no manifest: write a minimal manifest by asking.
//!
//! Four questions: project name, board, forge and the check command. Each is a [`Question`], a
//! label, one line on what it means and the default a blank answer takes (the folder's name, the
//! `owner/name` of the git remote `origin`, and no check command); [`questions`] lists them and
//! the caller's drawer shows each as it likes. The
//! written manifest also carries the standard values a tick needs: the claude runner, the
//! roles the phase actions run as, the review steps, the phase lists, the pack's tag, the
//! tools the actions need (`[requires]`) and the merge commands no instance may run
//! (`[forbidden]`, so a merge goes through `slice-merge` alone).
//!
//! A GitHub `origin`, or none, writes GitHub's kinds. Any other `origin` is taken for a Forgejo
//! instance: one more question, the forge's address, follows the forge, offering as its default the
//! first of a short list of candidates built from the remote's host that answers
//! `GET /api/v1/version` (the probe is handed in, so this module reaches no network itself); the
//! manifest then names the kinds `forgejo-issues` and `forgejo` with that `url`, the `repo` and
//! `token_env = "FORGEJO_TOKEN"`, and leaves out what only makes sense on GitHub (`gh`).

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::app::profile::ProfileError;
use crate::manifest::{ForgeUrl, Manifest, ManifestError, ProjectFiles, ROOT_FILE, RepoName};

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

/// One question init asks: what it is called, what it means and the answer a blank line takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Question {
    /// The name the question goes by, also what a refusal names.
    pub label: &'static str,
    /// One line on what the answer is for.
    pub meaning: String,
    /// What a blank answer takes.
    pub default: Option<String>,
    /// Whether a blank answer with no default is refused; the check command may be blank.
    pub required: bool,
}

impl Question {
    /// The question as plain text: its label padded to 14 and its meaning on one line, then the
    /// prompt `  › ` with the default in brackets when there is one.
    pub fn plain(&self) -> String {
        let default = self
            .default
            .as_deref()
            .map_or_else(String::new, |default| format!("[{default}] "));
        format!("  {:<14} {}\n  › {default}", self.label, self.meaning)
    }
}

/// How a question is shown before its answer is read: the whole text, prompt included.
pub type Draw<'a> = &'a dyn Fn(&Question) -> String;

/// What the folder and its `origin` say init can offer.
struct Facts {
    folder: Option<String>,
    origin: Origin,
}

impl Facts {
    fn of(dir: &Path) -> Self {
        Self {
            folder: folder_name(dir),
            origin: origin_of(dir),
        }
    }

    /// The `owner/name` the remote names, when it names one.
    fn remote(&self) -> Option<String> {
        match &self.origin {
            Origin::Github(repo) => Some(repo.clone()),
            Origin::Other(other) => Some(other.repo().to_string()),
            Origin::None => None,
        }
    }

    fn list(&self) -> Vec<Question> {
        let remote = self.remote();
        let kind = match self.origin {
            Origin::Other(_) => "Forgejo",
            _ => "GitHub",
        };
        let mut list = vec![
            Question {
                label: "Project name",
                meaning: "what zaf calls this project".to_owned(),
                default: self.folder.clone(),
                required: true,
            },
            Question {
                label: "Board",
                meaning: format!("where your tickets live: {kind} issues of owner/name"),
                default: remote.clone(),
                required: true,
            },
            Question {
                label: "Forge",
                meaning: "where pull requests open: owner/name".to_owned(),
                default: remote,
                required: true,
            },
        ];
        if matches!(self.origin, Origin::Other(_)) {
            list.push(Question {
                label: "Forge address",
                meaning: "where the forge answers: its base address, e.g. https://forge.example"
                    .to_owned(),
                default: None,
                required: true,
            });
        }
        list.push(Question {
            label: "Check command",
            meaning: "the command that proves the code is good, e.g. cargo test (empty: none)"
                .to_owned(),
            default: None,
            required: false,
        });
        list
    }
}

/// The questions `init` asks in `dir`, in order, with the defaults known before any answer: a
/// GitHub origin, or none, gives four; any other origin gives five, the forge's address among
/// them with no default (finding one asks the network, which only [`init_with_home`] does). With
/// no `origin` the forge question has none either: it takes the board's answer.
pub fn questions(dir: &Path) -> Vec<Question> {
    Facts::of(dir).list()
}

/// Shows `question`, reads one line and returns it trimmed, or the default on a blank one; a
/// blank one with no default is refused naming the question when it is required.
fn put(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    draw: Draw<'_>,
    question: &Question,
) -> Result<String, InitError> {
    write!(output, "{}", draw(question))?;
    output.flush()?;
    let mut line = String::new();
    if input.read_line(&mut line)? == 0 {
        return Err(InitError::Closed(question.label));
    }
    let answer = line.trim();
    match (answer.is_empty(), &question.default) {
        (false, _) => Ok(answer.to_owned()),
        (true, Some(default)) => Ok(default.clone()),
        (true, None) if question.required => Err(InitError::Blank(question.label)),
        (true, None) => Ok(String::new()),
    }
}

/// How a Forgejo instance is reached from a remote's address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    /// The remote's host, without user or port.
    host: String,
    /// The scheme and port the remote itself uses when it is an http(s) remote.
    http: Option<(String, Option<u16>)>,
    /// The repository the remote names.
    repo: RepoName,
}

impl Remote {
    /// The `owner/name` the remote names.
    pub fn repo(&self) -> &RepoName {
        &self.repo
    }

    /// The host the remote names.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The base URLs a Forgejo instance on this remote's host may answer at, most likely first:
    /// the remote's own scheme, host and port when it is an http(s) remote, then `https://<host>`,
    /// `http://<host>:3000` and `http://<host>`; none twice.
    pub fn candidates(&self) -> Vec<ForgeUrl> {
        let host = &self.host;
        let mut urls = Vec::new();
        if let Some((scheme, port)) = &self.http {
            urls.push(match port {
                Some(port) => format!("{scheme}://{host}:{port}"),
                None => format!("{scheme}://{host}"),
            });
        }
        urls.push(format!("https://{host}"));
        urls.push(format!("http://{host}:3000"));
        urls.push(format!("http://{host}"));
        let mut seen: Vec<String> = Vec::new();
        for url in urls {
            if !seen.contains(&url) {
                seen.push(url);
            }
        }
        seen.into_iter().map(ForgeUrl::from).collect()
    }
}

/// What the git remote `origin` says about the forge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// No `origin`, or one that names no repository on a host.
    None,
    /// A GitHub repository, as `owner/name`.
    Github(String),
    /// Any other host: taken for a Forgejo instance.
    Other(Remote),
}

/// Reads a remote URL: GitHub ones as before, any other ssh, scp-like or http(s) address as a
/// [`Remote`] naming `owner/name`, with or without `.git`.
pub fn parse_origin(url: &str) -> Origin {
    if let Some(repo) = github_repo(url) {
        return Origin::Github(repo);
    }
    match other_remote(url) {
        Some(remote) if !remote.host.eq_ignore_ascii_case("github.com") => Origin::Other(remote),
        _ => Origin::None,
    }
}

/// `host[:port]` of an authority without its user part: the host, and the port when it is one.
fn host_and_port(authority: &str) -> Option<(String, Option<u16>)> {
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host, Some(port.parse::<u16>().ok()?)),
        None => (authority, None),
    };
    let valid = !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'));
    valid.then(|| (host.to_owned(), port))
}

/// `owner/name` of a path such as `/zuri/ways.git`: exactly two parts.
fn two_part_repo(path: &str) -> Option<RepoName> {
    let path = path.trim().trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    let valid =
        |part: &str| !part.is_empty() && !part.contains('/') && !part.contains(char::is_whitespace);
    (valid(owner) && valid(name)).then(|| RepoName::from(format!("{owner}/{name}")))
}

fn other_remote(url: &str) -> Option<Remote> {
    let url = url.trim();
    if let Some((scheme, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        let (host, port) = host_and_port(authority)?;
        let http = matches!(scheme, "http" | "https").then(|| (scheme.to_owned(), port));
        if http.is_none() && !matches!(scheme, "ssh" | "git" | "git+ssh" | "ssh+git") {
            return None;
        }
        return Some(Remote {
            host,
            http,
            repo: two_part_repo(path)?,
        });
    }
    // scp-like: [user@]host:owner/name
    let (authority, path) = url.split_once(':')?;
    if authority.contains('/') || path.starts_with("//") {
        return None;
    }
    let (host, port) = host_and_port(authority)?;
    if port.is_some() {
        return None;
    }
    Some(Remote {
        host,
        http: None,
        repo: two_part_repo(path)?,
    })
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

/// What the git remote `origin` of `dir` says; [`Origin::None`] without git or a remote.
fn origin_of(dir: &Path) -> Origin {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .output();
    match out {
        Ok(out) if out.status.success() => parse_origin(&String::from_utf8_lossy(&out.stdout)),
        _ => Origin::None,
    }
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

/// The tools and merge denies of a GitHub project: `gh` is needed, and `gh pr merge` is denied.
const GITHUB_TOOLS: &str = r#"
[requires]
jj = "*"
gh = "*"

[forbidden]
commands = [
  "gh pr merge",
  "zaf forge merge",
  "zaf do pr-merge",
  "zaf do --tier machine pr-merge",
]
"#;

/// The tools and merge denies of a Forgejo project, which needs no `gh`.
const FORGEJO_TOOLS: &str = r#"
[requires]
jj = "*"

[forbidden]
commands = [
  "zaf forge merge",
  "zaf do pr-merge",
  "zaf do --tier machine pr-merge",
]
"#;

/// The variable a Forgejo project's token is read from.
const TOKEN_ENV: &str = "FORGEJO_TOKEN";

/// Whether a candidate base URL answers as a Forgejo or Gitea instance; called in the order of
/// [`Remote::candidates`], and it must not wait long on any one.
pub type ProbeForge<'a> = &'a mut dyn FnMut(&ForgeUrl) -> bool;

/// Writes `<dir>/.zaf.toml` from answers read on `input`, prompting on `output`;
/// returns the file's path. Refuses, before asking anything, when `dir` holds any manifest
/// file. `learn_tag` gives `[pack] tag`; no instance is probed, so a remote that is not GitHub's
/// leaves `forge url` with no default. See [`init_with_home`] to leave the zaf home's own
/// `config.toml` out of that check.
pub fn init(
    dir: &Path,
    learn_tag: LearnTag<'_>,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<PathBuf, InitError> {
    init_with_home(
        dir,
        None,
        learn_tag,
        &mut |_| false,
        &Question::plain,
        input,
        output,
    )
}

/// [`init`] knowing the zaf `home`: when `dir/.zaf` is the zaf home, its `config.toml` is the
/// machine's, not a project manifest, and does not make init refuse. Each question is shown as
/// `draw` says.
pub fn init_with_home(
    dir: &Path,
    home: Option<&Path>,
    learn_tag: LearnTag<'_>,
    probe: ProbeForge<'_>,
    draw: Draw<'_>,
    input: &mut dyn BufRead,
    output: &mut dyn Write,
) -> Result<PathBuf, InitError> {
    let path = dir.join(MANIFEST_FILE);
    let found = ProjectFiles::find(dir, home);
    if let Some(existing) = found.root.or(found.folder).or(found.legacy) {
        return Err(InitError::Exists(existing));
    }
    let facts = Facts::of(dir);
    let mut list = facts.list().into_iter();
    let mut next = || list.next().expect("the list holds every question asked");
    let name = put(input, output, draw, &next())?;
    let board = put(input, output, draw, &next())?;
    let mut forge_question = next();
    forge_question.default.get_or_insert_with(|| board.clone());
    let forge = put(input, output, draw, &forge_question)?;
    let url = match &facts.origin {
        Origin::Other(other) => {
            writeln!(output, "looking for a Forgejo instance on {}", other.host())?;
            let found = other
                .candidates()
                .into_iter()
                .find(|candidate| probe(candidate));
            let mut address = next();
            address.default = found.as_ref().map(|found| found.as_str().to_owned());
            let url = put(input, output, draw, &address)?;
            Some(url.trim_end_matches('/').to_owned())
        }
        _ => None,
    };
    let gate = put(input, output, draw, &next())?;

    let mut text = match &url {
        Some(url) => format!(
            "[project]\nname = {}\n\n[board]\nkind = \"forgejo-issues\"\nurl = {}\nrepo = {}\n\
             token_env = \"{TOKEN_ENV}\"\n\n[vcs]\nkind = \"jj\"\n\n[forge]\nkind = \"forgejo\"\n\
             url = {}\nrepo = {}\ntoken_env = \"{TOKEN_ENV}\"\n",
            quoted(&name),
            quoted(url),
            quoted(&board),
            quoted(url),
            quoted(&forge),
        ),
        None => format!(
            "[project]\nname = {}\n\n[board]\nkind = \"github-issues\"\nrepo = {}\n\n\
             [vcs]\nkind = \"jj\"\n\n[forge]\nkind = \"github\"\nrepo = {}\n",
            quoted(&name),
            quoted(&board),
            quoted(&forge),
        ),
    };
    if !gate.is_empty() {
        text.push_str(&format!("\n[gate]\nfull = {}\n", quoted(&gate)));
    }
    text.push_str(STANDARD);
    text.push_str(if url.is_some() {
        FORGEJO_TOOLS
    } else {
        GITHUB_TOOLS
    });
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
