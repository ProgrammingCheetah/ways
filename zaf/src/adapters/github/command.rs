//! The one door to the `gh` CLI: every call of both adapters goes through [`GhCommand`].

use std::ffi::OsString;
use std::process::Command;

use serde::de::DeserializeOwned;

use crate::manifest::RepoName;
use crate::ports::PortError;

/// The program run; found on the `PATH` the call sees.
const PROGRAM: &str = "gh";

/// What gh's stderr says when the issue addressed does not exist.
const MISSING_ISSUE: &str = "Could not resolve to an Issue";

/// Runs `gh` against one repository and reads its output.
///
/// A call that exits non-zero is a [`PortError::Backend`] carrying gh's stderr,
/// or a [`PortError::NotFound`] when gh could not resolve the issue addressed;
/// output that is not the expected JSON is a `Backend` error too. A `gh` that cannot be
/// started at all is [`PortError::Unavailable`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhCommand {
    repo: RepoName,
    env: Vec<(OsString, OsString)>,
}

impl From<RepoName> for GhCommand {
    fn from(repo: RepoName) -> Self {
        Self {
            repo,
            env: Vec::new(),
        }
    }
}

impl GhCommand {
    /// Sets an environment variable on every call, as [`Command::env`] does;
    /// a `PATH` set here also decides which `gh` runs.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The repository every call addresses.
    pub fn repo(&self) -> &RepoName {
        &self.repo
    }

    /// Runs a repository subcommand (`issue …`, `pr …`) with `--repo` appended; returns stdout.
    pub fn text(&self, args: &[&str]) -> Result<String, PortError> {
        let mut argv: Vec<&str> = args.to_vec();
        argv.extend(["--repo", self.repo.as_ref()]);
        self.output(&argv)
    }

    /// Runs a repository subcommand and parses its stdout as JSON.
    pub fn json<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T, PortError> {
        parse_json(&self.text(args)?)
    }

    /// Runs `gh api repos/<repo>/<endpoint>` with the extra arguments; returns stdout.
    pub fn api_text(&self, endpoint: &str, args: &[&str]) -> Result<String, PortError> {
        let path = format!("repos/{}/{endpoint}", self.repo);
        let mut argv = vec!["api", path.as_str()];
        argv.extend_from_slice(args);
        self.output(&argv)
    }

    /// Reads every page of a list endpoint, oldest page first, as one list.
    pub fn api_list<T: DeserializeOwned>(&self, endpoint: &str) -> Result<Vec<T>, PortError> {
        let text = self.api_text(endpoint, &["--paginate", "--slurp"])?;
        let pages: Vec<Vec<T>> = parse_json(&text)?;
        Ok(pages.into_iter().flatten().collect())
    }

    fn output(&self, argv: &[&str]) -> Result<String, PortError> {
        let mut command = Command::new(PROGRAM);
        command.args(argv);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let output = command
            .output()
            .map_err(|error| PortError::Unavailable(format!("cannot run {PROGRAM}: {error}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let subcommand = argv.iter().take(2).copied().collect::<Vec<_>>().join(" ");
            let message = format!("{PROGRAM} {subcommand}: {stderr}");
            let failed_call_error = if stderr.contains(MISSING_ISSUE) {
                PortError::NotFound(message)
            } else {
                PortError::Backend(message)
            };
            return Err(failed_call_error);
        }
        String::from_utf8(output.stdout)
            .map_err(|_| PortError::Backend(format!("{PROGRAM} wrote output that is not UTF-8")))
    }
}

fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, PortError> {
    serde_json::from_str(text)
        .map_err(|error| PortError::Backend(format!("{PROGRAM} returned unreadable JSON: {error}")))
}

/// The number at the end of an issue or pull request URL, as `gh … create` prints it.
pub(super) fn number_from_url(stdout: &str) -> Result<u64, PortError> {
    let url = stdout.trim();
    url.rsplit('/')
        .next()
        .and_then(|last| last.parse::<u64>().ok())
        .ok_or_else(|| PortError::Backend(format!("{PROGRAM} printed no number: `{url}`")))
}
