//! zaf's releases on GitHub, read over `curl` from GitHub's public API, with no `gh` on the
//! machine: `GET https://api.github.com/repos/<repo>/releases` lists them with their notes and
//! each file's download link.
//!
//! `GITHUB_TOKEN`, when set and not empty, is sent to the API as a bearer header to lift the
//! anonymous rate limit. It goes to curl in a config on its stdin, never on a command line
//! (as `forgejo/api.rs` does), and never to a download, which is a link on another host; no
//! message, log or `Debug` holds it. No network is [`PortError::Unavailable`]; a refusal (403,
//! 429: the rate limit) is a [`PortError::Backend`] naming the limit. Either leaves whatever
//! the caller was about to replace as it was, since nothing is written before a reply is read.
//!
//! What the network sends is not trusted: a download link must point into the repository's own
//! releases, a file name must be a plain name, and notes shown to a terminal lose their control
//! characters.

use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, PoisonError};

use semver::Version;
use serde::Deserialize;

use crate::manifest::RepoName;
use crate::ports::{PortError, Releases};

/// The program run; found on the `PATH` the call sees.
const PROGRAM: &str = "curl";

/// The variable the token is read from.
const TOKEN_ENV: &str = "GITHUB_TOKEN";

/// Where the API lives.
const API_ROOT: &str = "https://api.github.com";

/// Where the repository's files are served raw from, for the teams plugin's `plugin.json`.
const RAW_ROOT: &str = "https://raw.githubusercontent.com";

/// The teams plugin's manifest, under the repository's `main`.
const PLUGIN_JSON: &str = "plugins/teams/.claude-plugin/plugin.json";

/// The most bytes of `plugin.json` read; a real one is a few hundred.
const PLUGIN_JSON_LIMIT: usize = 64 * 1024;

/// Where a release's files are served from, under the repository.
const DOWNLOAD_ROOT: &str = "https://github.com";

/// What curl appends to the body it prints: a line holding the HTTP status.
const STATUS_LINE: &str = "\\n%{http_code}";

/// Seconds curl waits to connect.
const CONNECT_TIMEOUT_SECONDS: &str = "10";

/// Seconds a call to the API may take in all.
const API_MAX_TIME_SECONDS: &str = "60";

/// Seconds a download may take in all.
const DOWNLOAD_MAX_TIME_SECONDS: &str = "300";

/// The most characters of GitHub's own message an error carries.
const MESSAGE_LIMIT: usize = 200;

/// A GitHub token. No `Display`, and its `Debug` hides it.
#[derive(Clone, PartialEq, Eq)]
struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Token(<hidden>)")
    }
}

/// A release as the API gives it; the fields zaf reads.
#[derive(Debug, Clone, Deserialize)]
struct ReleaseRow {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<AssetRow>,
}

/// A file of a release.
#[derive(Debug, Clone, Deserialize)]
struct AssetRow {
    name: String,
    browser_download_url: String,
}

/// What a call to GitHub answered, whatever its status.
struct Reply {
    status: u16,
    body: Vec<u8>,
}

/// The published releases of one repository, read over `curl` from GitHub's public API.
///
/// The release list is read once and kept (a clone shares it): GitHub allows 60 unauthenticated
/// calls an hour, and one `zaf update` asks for the list several times. A failed listing is not
/// kept, so a later call lists again.
#[derive(Debug, Clone)]
pub struct PublicReleases {
    repo: RepoName,
    token: Option<Token>,
    env: Vec<(OsString, OsString)>,
    kept: Arc<Mutex<Option<Arc<Vec<ReleaseRow>>>>>,
}

impl PublicReleases {
    /// The releases of `repo` (`owner/name`), with the token `GITHUB_TOKEN` holds in this
    /// process; unset or empty, they are read anonymously.
    pub fn from_env(repo: RepoName) -> Self {
        let token = std::env::var(TOKEN_ENV)
            .ok()
            .map(|token| token.trim().to_owned())
            .filter(|token| !token.is_empty())
            .map(Token);
        Self {
            repo,
            token,
            env: Vec::new(),
            kept: Arc::default(),
        }
    }

    /// The releases of `repo`, read with `token` (as `GITHUB_TOKEN` would give it).
    pub fn with_token(repo: RepoName, token: Option<&str>) -> Self {
        Self {
            repo,
            token: token.map(|token| Token(token.to_owned())),
            env: Vec::new(),
            kept: Arc::default(),
        }
    }

    /// Sets an environment variable on every call, as [`Command::env`] does;
    /// a `PATH` set here also decides which `curl` runs.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// The published releases, drafts and pre-releases left out. One page of 100 is enough:
    /// zaf has published far fewer releases than that, and the newest come first. Listed on the
    /// first call and kept; a failed listing is returned and not kept.
    fn published(&self) -> Result<Arc<Vec<ReleaseRow>>, PortError> {
        // Held across the listing, so two callers at once still make one.
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(rows) = kept.as_ref() {
            return Ok(Arc::clone(rows));
        }
        let rows = Arc::new(self.list()?);
        *kept = Some(Arc::clone(&rows));
        Ok(rows)
    }

    /// One call to GitHub's list of releases.
    fn list(&self) -> Result<Vec<ReleaseRow>, PortError> {
        let url = format!("{API_ROOT}/repos/{}/releases?per_page=100", self.repo);
        let reply = self.get(&url, true)?;
        let body = self.checked(&url, reply)?;
        let rows: Vec<ReleaseRow> = serde_json::from_slice(&body).map_err(|error| {
            PortError::Backend(format!("GitHub's release list is unreadable JSON: {error}"))
        })?;
        Ok(rows
            .into_iter()
            .filter(|row| !row.draft && !row.prerelease)
            .collect())
    }

    fn release(&self, tag: &str) -> Result<ReleaseRow, PortError> {
        self.published()?
            .iter()
            .find(|row| row.tag_name == tag)
            .cloned()
            .ok_or_else(|| PortError::NotFound(format!("release {tag}")))
    }

    /// The body of a reply that is a success; a 404 is not found, a refusal names the limit,
    /// any other status is a backend error.
    fn checked(&self, url: &str, reply: Reply) -> Result<Vec<u8>, PortError> {
        match reply.status {
            200..=299 => Ok(reply.body),
            404 => Err(PortError::NotFound(format!("GitHub has nothing at {url}"))),
            status @ (403 | 429) => Err(PortError::Backend(format!(
                "GitHub refused the request (HTTP {status}): the API rate limit is used up{}; {}",
                message(&reply.body),
                if self.token.is_some() {
                    "wait for the limit to reset"
                } else {
                    "set GITHUB_TOKEN to raise it, or wait for it to reset"
                }
            ))),
            status => Err(PortError::Backend(format!(
                "GitHub answered {status} for {url}{}",
                message(&reply.body)
            ))),
        }
    }

    /// Runs curl once on `url`. The token, when `api` and one is held, goes to curl's stdin
    /// as a config; the arguments hold none of it. A download follows redirects, over https
    /// alone; an API call does not need to.
    fn get(&self, url: &str, api: bool) -> Result<Reply, PortError> {
        let mut config = String::new();
        if api && let Some(token) = &self.token {
            config.push_str(&config_line(
                "header",
                &format!("Authorization: Bearer {}", token.0),
            ));
        }
        let mut command = Command::new(PROGRAM);
        command.args([
            // First, as curl requires: the user's curlrc is not read, so nothing in it (a
            // `trace`, a `proxy`) sees the token's header.
            "--disable",
            "--silent",
            "--show-error",
            "--globoff",
            "--config",
            "-",
            "--request",
            "GET",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            CONNECT_TIMEOUT_SECONDS,
            "--max-time",
            if api {
                API_MAX_TIME_SECONDS
            } else {
                DOWNLOAD_MAX_TIME_SECONDS
            },
            "--write-out",
            STATUS_LINE,
        ]);
        if api {
            command.args([
                "--header",
                "Accept: application/vnd.github+json",
                "--header",
                "X-GitHub-Api-Version: 2022-11-28",
            ]);
        } else {
            command.arg("--location");
        }
        command.arg(url);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // A process group of its own, so a Ctrl-C meant for zaf does not cut a read short.
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|error| PortError::Unavailable(format!("cannot run {PROGRAM}: {error}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            // A curl that stops before reading its config says why on stderr, read below.
            let _ = stdin.write_all(config.as_bytes());
        }
        let output = child.wait_with_output().map_err(|error| {
            PortError::Unavailable(format!("{PROGRAM} did not finish: {error}"))
        })?;
        if !output.status.success() {
            let stderr = clean(&String::from_utf8_lossy(&output.stderr));
            return Err(PortError::Unavailable(format!(
                "no network to GitHub: {PROGRAM} could not read {}: {}",
                url.split('?').next().unwrap_or(url),
                stderr.trim()
            )));
        }
        let at = output
            .stdout
            .iter()
            .rposition(|byte| *byte == b'\n')
            .ok_or_else(|| PortError::Backend(format!("{PROGRAM} printed no status")))?;
        let status = String::from_utf8_lossy(&output.stdout[at + 1..])
            .trim()
            .parse::<u16>()
            .map_err(|_| PortError::Backend(format!("{PROGRAM} printed no status")))?;
        let mut body = output.stdout;
        body.truncate(at);
        Ok(Reply { status, body })
    }
}

impl Releases for PublicReleases {
    fn plugin_version(&self) -> Result<Version, PortError> {
        // Another host than the API's: the token is not sent, as for a download.
        let url = format!("{RAW_ROOT}/{}/main/{PLUGIN_JSON}", self.repo);
        let reply = self.get(&url, false)?;
        let body = self.checked(&url, reply)?;
        let unreadable =
            |why: &str| PortError::Backend(format!("plugin.json on main (teams) {why}"));
        if body.len() > PLUGIN_JSON_LIMIT {
            return Err(unreadable("is too large to be a plugin.json"));
        }
        let value: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|error| unreadable(&format!("is not JSON: {error}")))?;
        let text = value
            .get("version")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| unreadable("has no `version`"))?;
        Version::parse(text.trim()).map_err(|_| {
            let shown: String = clean(text).chars().take(MESSAGE_LIMIT).collect();
            unreadable(&format!("has a `version` that is not semver: `{shown}`"))
        })
    }

    fn tags(&self) -> Result<Vec<String>, PortError> {
        Ok(self
            .published()?
            .iter()
            .map(|row| row.tag_name.clone())
            .collect())
    }

    fn notes(&self, tag: &str) -> Result<Option<String>, PortError> {
        let body = self.release(tag)?.body.unwrap_or_default();
        let body = clean(&body);
        Ok((!body.trim().is_empty()).then_some(body))
    }

    fn download(&self, tag: &str, assets: &[String], dir: &Path) -> Result<(), PortError> {
        let release = self.release(tag)?;
        let prefix = format!("{DOWNLOAD_ROOT}/{}/releases/download/", self.repo);
        for name in assets {
            if !plain_name(name) {
                return Err(PortError::Backend(format!(
                    "`{name}` is not a plain file name"
                )));
            }
            let asset = release
                .assets
                .iter()
                .find(|asset| &asset.name == name)
                .ok_or_else(|| PortError::NotFound(format!("{name} in release {tag}")))?;
            // A link GitHub gave is followed only into the repository's own releases.
            if !asset.browser_download_url.starts_with(&prefix) {
                return Err(PortError::Backend(format!(
                    "the link GitHub gives for {name} in release {tag} does not point into {}",
                    self.repo
                )));
            }
            let reply = self.get(&asset.browser_download_url, false)?;
            let bytes = self.checked(&asset.browser_download_url, reply)?;
            std::fs::write(dir.join(name), bytes).map_err(|error| {
                PortError::Backend(format!("cannot write {name} in {}: {error}", dir.display()))
            })?;
        }
        Ok(())
    }
}

/// Whether `name` is one file name, not a path.
fn plain_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}

/// GitHub's own words of an error reply (`, "<message>"`), cleaned and cut short; empty when the
/// reply carries none.
fn message(body: &[u8]) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_slice(body).ok();
    let Some(text) = parsed
        .as_ref()
        .and_then(|value| value.get("message"))
        .and_then(serde_json::Value::as_str)
    else {
        return String::new();
    };
    let text: String = clean(text).trim().chars().take(MESSAGE_LIMIT).collect();
    if text.is_empty() {
        String::new()
    } else {
        format!(" (\"{text}\")")
    }
}

/// `text` with every control character but a line break and a tab dropped, and `\r\n` read as
/// `\n`: nothing the network sent can drive a terminal.
fn clean(text: &str) -> String {
    text.replace("\r\n", "\n")
        .chars()
        .filter(|ch| !ch.is_control() || matches!(ch, '\n' | '\t'))
        .collect()
}

/// One curl config line, `name = "value"`, the value quoted as curl's config reads it back.
fn config_line(name: &str, value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    for ch in value.chars() {
        match ch {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            '\r' => quoted.push_str("\\r"),
            '\t' => quoted.push_str("\\t"),
            '\u{0b}' => quoted.push_str("\\v"),
            other => quoted.push(other),
        }
    }
    format!("{name} = \"{quoted}\"\n")
}

#[cfg(test)]
mod tests;
