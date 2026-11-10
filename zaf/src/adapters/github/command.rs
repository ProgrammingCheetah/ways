//! The one door to the `gh` CLI: every call of both adapters goes through [`GhCommand`].

use std::ffi::OsString;
use std::process::Command;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::manifest::RepoName;
use crate::ports::PortError;

/// The program run; found on the `PATH` the call sees.
const PROGRAM: &str = "gh";

/// What gh's stderr says, in any case, when the issue addressed does not exist: older gh
/// prints `Could not resolve to an Issue with the number of …`, newer `Could not resolve to an
/// issue or pull request with the number of …`.
const MISSING_ISSUE: &str = "could not resolve to an issue";

/// What GitHub's GraphQL errors say, in any case, when the same call may succeed if sent
/// again: two writes to one Project at once (`… created a temporary conflict. Please try
/// again.`), or a query the server dropped (`Something went wrong while executing your
/// query …`).
const TRANSIENT: &[&str] = &["try again", "something went wrong"];

/// How long a GraphQL call waits before each new try after a transient error.
#[cfg(not(test))]
const RETRY_PAUSES: [Duration; 3] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];

/// Unit tests try again at once.
#[cfg(test)]
const RETRY_PAUSES: [Duration; 3] = [Duration::ZERO; 3];

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

    /// Runs one GraphQL document through `gh api graphql`, each variable passed as a string
    /// (`-f`) or a number (`-F`), and parses the `data` it returns. A GraphQL error exits
    /// non-zero, so it is an error here too; one GitHub calls transient (see [`TRANSIENT`]) is
    /// sent again, up to three more times, before it is.
    pub fn graphql<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: &[(&str, GraphqlValue)],
    ) -> Result<T, PortError> {
        let query = format!("query={query}");
        let mut pairs = Vec::new();
        for (name, value) in variables {
            match value {
                GraphqlValue::Text(text) => pairs.push(("-f", format!("{name}={text}"))),
                GraphqlValue::Number(number) => pairs.push(("-F", format!("{name}={number}"))),
            }
        }
        let mut argv = vec!["api", "graphql", "-f", query.as_str()];
        for (flag, pair) in &pairs {
            argv.extend([*flag, pair.as_str()]);
        }
        let mut pauses = RETRY_PAUSES.iter();
        let stdout = loop {
            match self.output(&argv) {
                Err(PortError::Backend(message)) if is_transient(&message) => {
                    let Some(pause) = pauses.next() else {
                        return Err(PortError::Backend(message));
                    };
                    std::thread::sleep(*pause);
                }
                done => break done?,
            }
        };
        let reply: GraphqlReply<T> = parse_json(&stdout)?;
        Ok(reply.data)
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
            let failed_call_error = if stderr.to_lowercase().contains(MISSING_ISSUE) {
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

/// Whether a failed call's message is one GitHub says to send again.
fn is_transient(message: &str) -> bool {
    let message = message.to_lowercase();
    TRANSIENT
        .iter()
        .any(|transient| message.contains(transient))
}

/// A GraphQL variable as `gh api graphql` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphqlValue {
    /// A string, passed with `-f`.
    Text(String),
    /// A number, passed with `-F`.
    Number(u64),
}

/// What `gh api graphql` prints on success.
#[derive(serde::Deserialize)]
struct GraphqlReply<T> {
    data: T,
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
