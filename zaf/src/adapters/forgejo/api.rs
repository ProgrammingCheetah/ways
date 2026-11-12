//! The one door to a Forgejo instance's REST API (`<url>/api/v1`): every call of the Forgejo
//! adapters goes through [`ForgejoApi`], which runs `curl`.
//!
//! The token never reaches a command line, a manifest or a log: curl reads it, with the
//! request's body, from a config handed over on its stdin (`--config -`).

use std::ffi::OsString;
use std::fmt;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde::de::DeserializeOwned;

use super::rows::{ApiSettingsRow, VersionRow};
use crate::manifest::{EnvVarName, ForgeUrl, RepoName};
use crate::ports::PortError;

/// The program run; found on the `PATH` the call sees.
const PROGRAM: &str = "curl";

/// Where the API lives under an instance's base URL.
const API_ROOT: &str = "/api/v1";

/// Seconds curl waits for the instance to accept the connection.
const CONNECT_TIMEOUT_SECONDS: &str = "10";

/// Seconds one call may take in all; past it curl gives up and the call is unavailable.
const MAX_TIME_SECONDS: &str = "60";

/// What curl appends to the body it prints: a line holding the HTTP status.
const STATUS_LINE: &str = "\\n%{http_code}";

/// The most characters of a reply that is not Forgejo's JSON error a message carries.
const MESSAGE_LIMIT: usize = 300;

/// A Forgejo access token. It has no `Display`, and its `Debug` hides it, so no message,
/// log or panic can print it.
#[derive(Clone, PartialEq, Eq)]
pub struct ForgejoToken(String);

impl From<String> for ForgejoToken {
    fn from(token: String) -> Self {
        Self(token)
    }
}

impl From<&str> for ForgejoToken {
    fn from(token: &str) -> Self {
        Self(token.to_owned())
    }
}

impl fmt::Debug for ForgejoToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ForgejoToken(<hidden>)")
    }
}

/// An HTTP method the door sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "UPPERCASE")]
pub enum Method {
    /// Reads.
    Get,
    /// Creates.
    Post,
    /// Changes some fields.
    Patch,
    /// Replaces.
    Put,
    /// Removes.
    Delete,
}

/// What the instance answered, whatever its status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// The HTTP status.
    pub status: u16,
    /// The body, as sent.
    pub body: String,
}

impl Reply {
    /// Whether the status is a success (2xx).
    pub fn success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Forgejo's own words for a failed call: the `message` of its JSON error, else the body's
    /// first characters.
    pub fn message(&self) -> String {
        let parsed: Option<serde_json::Value> = serde_json::from_str(&self.body).ok();
        if let Some(message) = parsed
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(serde_json::Value::as_str)
            .filter(|message| !message.trim().is_empty())
        {
            return message.trim().to_owned();
        }
        let body = self.body.trim();
        if body.is_empty() {
            return "(no message)".to_owned();
        }
        body.chars().take(MESSAGE_LIMIT).collect()
    }
}

/// Talks to one repository on one Forgejo instance through `curl`, with the token a variable
/// held when it was built.
///
/// A call answered 2xx returns its body, decoded where asked. A 404 is a
/// [`PortError::NotFound`]; any other status is a [`PortError::Backend`] carrying the status
/// and Forgejo's `message`. An instance that does not answer within the time bound, refuses
/// the connection or cannot be resolved, and a `curl` that cannot be started, are
/// [`PortError::Unavailable`]; so is a repository call made with no token. A body that is not
/// the expected JSON is a `Backend` error.
#[derive(Debug, Clone)]
pub struct ForgejoApi {
    url: ForgeUrl,
    repo: RepoName,
    token_env: EnvVarName,
    token: Option<ForgejoToken>,
    env: Vec<(OsString, OsString)>,
    limits: Option<Limits>,
    page_size: OnceLock<usize>,
}

/// Shorter time bounds than the defaults, in seconds, for a call that must not hold anything up.
#[derive(Debug, Clone, Copy)]
struct Limits {
    connect: u32,
    total: u32,
}

impl PartialEq for ForgejoApi {
    fn eq(&self, other: &Self) -> bool {
        self.url == other.url
            && self.repo == other.repo
            && self.token_env == other.token_env
            && self.token == other.token
            && self.env == other.env
    }
}

impl Eq for ForgejoApi {}

impl ForgejoApi {
    /// The door to `repo` on the instance at `url`, with `token`, the value of the variable
    /// `token_env` (named in messages; `None` when it is unset or empty).
    pub fn new(
        url: ForgeUrl,
        repo: RepoName,
        token_env: EnvVarName,
        token: Option<ForgejoToken>,
    ) -> Self {
        let url = ForgeUrl::from(url.as_str().trim_end_matches('/'));
        Self {
            url,
            repo,
            token_env,
            token,
            env: Vec::new(),
            limits: None,
            page_size: OnceLock::new(),
        }
    }

    /// The door to `repo` on the instance at `url`, with the token this process's variable
    /// `token_env` holds; an unset or empty variable leaves the door without one.
    pub fn from_env(url: ForgeUrl, repo: RepoName, token_env: EnvVarName) -> Self {
        let token = std::env::var(token_env.as_str())
            .ok()
            .filter(|token| !token.trim().is_empty())
            .map(|token| ForgejoToken::from(token.trim()));
        Self::new(url, repo, token_env, token)
    }

    /// Sets an environment variable on every call, as [`Command::env`] does;
    /// a `PATH` set here also decides which `curl` runs.
    pub fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Bounds every call to `connect` seconds to connect and `total` seconds in all, instead of
    /// the defaults: for a probe that must not hang its caller.
    pub fn time_limits(mut self, connect: u32, total: u32) -> Self {
        self.limits = Some(Limits { connect, total });
        self
    }

    /// The version the instance reports (`GET /api/v1/version`, no token needed); an error when
    /// nothing answers or what answers is not a Forgejo or Gitea version.
    pub fn version(&self) -> Result<String, PortError> {
        let row: VersionRow = self.instance("version")?;
        if row.version.trim().is_empty() {
            return Err(PortError::Backend(
                "the instance reports no version".to_owned(),
            ));
        }
        Ok(row.version)
    }

    /// The repository every repository call addresses.
    pub fn repo(&self) -> &RepoName {
        &self.repo
    }

    /// The instance's base URL, without a trailing `/`.
    pub fn url(&self) -> &ForgeUrl {
        &self.url
    }

    /// The variable the token was read from.
    pub fn token_env(&self) -> &EnvVarName {
        &self.token_env
    }

    /// Whether the door holds a token.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// The repository's owner and name, as an issue reference across the API names them.
    pub fn owner_and_name(&self) -> Result<(&str, &str), PortError> {
        let repo = self.repo.as_str();
        repo.split_once('/')
            .filter(|(owner, name)| !owner.is_empty() && !name.is_empty())
            .ok_or_else(|| PortError::Backend(format!("`{repo}` is not owner/name")))
    }

    /// `GET /repos/<repo>/<endpoint>`, decoded.
    pub fn get<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T, PortError> {
        self.send(Method::Get, endpoint, None)
    }

    /// `<method> /repos/<repo>/<endpoint>` with `body` as JSON, the reply decoded; a reply
    /// with no body decodes as JSON `null`, so `()` reads a `204`.
    pub fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<T, PortError> {
        let path = self.repo_path(endpoint);
        let reply = self.call(method, &path, body, true)?;
        decode(method, &path, &self.checked(method, &path, reply)?)
    }

    /// Every item of a list endpoint under `/repos/<repo>/`, page by page (`page`, `limit`),
    /// until a page comes back short of the instance's page size.
    pub fn list<T: DeserializeOwned>(&self, endpoint: &str) -> Result<Vec<T>, PortError> {
        let limit = self.page_size()?;
        let separator = if endpoint.contains('?') { '&' } else { '?' };
        let mut items = Vec::new();
        for page in 1.. {
            let paged = format!("{endpoint}{separator}page={page}&limit={limit}");
            let found: Vec<T> = self.get(&paged)?;
            let short = found.len() < limit;
            items.extend(found);
            if short {
                break;
            }
        }
        Ok(items)
    }

    /// `GET /<path>` on the instance, outside any repository (`version`, `settings/api`),
    /// decoded; it needs no token, and sends one when the door holds it.
    pub fn instance<T: DeserializeOwned>(&self, path: &str) -> Result<T, PortError> {
        let path = format!("{API_ROOT}/{}", path.trim_start_matches('/'));
        let reply = self.call(Method::Get, &path, None, false)?;
        decode(
            Method::Get,
            &path,
            &self.checked(Method::Get, &path, reply)?,
        )
    }

    /// `<method> /repos/<repo>/<endpoint>` with `body`, returning whatever status came back:
    /// only a call that got no answer at all, or had no token to send, is an error. For a check
    /// that reads the status itself, such as whether the token may write.
    pub fn probe(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<Reply, PortError> {
        self.call(method, &self.repo_path(endpoint), body, true)
    }

    /// The most items the instance puts on one page (its `max_response_items`), read once.
    fn page_size(&self) -> Result<usize, PortError> {
        if let Some(size) = self.page_size.get() {
            return Ok(*size);
        }
        let settings: ApiSettingsRow = self.instance("settings/api")?;
        let size = usize::try_from(settings.max_response_items)
            .unwrap_or(usize::MAX)
            .max(1);
        Ok(*self.page_size.get_or_init(|| size))
    }

    /// `/api/v1/repos/<repo>`, then `/<endpoint>` unless it is empty.
    fn repo_path(&self, endpoint: &str) -> String {
        let endpoint = endpoint.trim_start_matches('/');
        if endpoint.is_empty() {
            return format!("{API_ROOT}/repos/{}", self.repo);
        }
        format!("{API_ROOT}/repos/{}/{endpoint}", self.repo)
    }

    /// The reply when it is a success; a 404 is not found, any other status a backend error.
    fn checked(&self, method: Method, path: &str, reply: Reply) -> Result<String, PortError> {
        if reply.success() {
            return Ok(reply.body);
        }
        let message = format!(
            "Forgejo {method} {path}: {} {}",
            reply.status,
            reply.message()
        );
        if reply.status == 404 {
            return Err(PortError::NotFound(message));
        }
        Err(PortError::Backend(message))
    }

    /// Runs curl once: the token and the body go to its stdin as a config, never to argv.
    fn call(
        &self,
        method: Method,
        path: &str,
        body: Option<&serde_json::Value>,
        needs_token: bool,
    ) -> Result<Reply, PortError> {
        if needs_token && self.token.is_none() {
            return Err(PortError::Unavailable(format!(
                "no Forgejo token: the variable `{}` is not set",
                self.token_env
            )));
        }
        let mut config = String::new();
        if let Some(token) = &self.token {
            config.push_str(&config_line(
                "header",
                &format!("Authorization: token {}", token.0),
            ));
        }
        if let Some(body) = body {
            config.push_str(&config_line("data-raw", &body.to_string()));
        }
        let url = format!("{}{path}", self.url);
        let method_name = method.to_string();
        let (connect, total) = self.limits.map_or(
            (
                CONNECT_TIMEOUT_SECONDS.to_owned(),
                MAX_TIME_SECONDS.to_owned(),
            ),
            |limits| (limits.connect.to_string(), limits.total.to_string()),
        );
        let mut command = Command::new(PROGRAM);
        command.args([
            "--silent",
            "--show-error",
            "--globoff",
            "--config",
            "-",
            "--request",
            method_name.as_str(),
            "--header",
            "Accept: application/json",
            "--connect-timeout",
            connect.as_str(),
            "--max-time",
            total.as_str(),
            "--write-out",
            STATUS_LINE,
        ]);
        if body.is_some() {
            command.args(["--header", "Content-Type: application/json"]);
        }
        command.arg(&url);
        for (key, value) in &self.env {
            command.env(key, value);
        }
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
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
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(PortError::Unavailable(format!(
                "Forgejo at {} does not answer {method} {path}: {stderr}",
                self.url
            )));
        }
        let stdout = String::from_utf8(output.stdout).map_err(|_| {
            PortError::Backend(format!("Forgejo {method} {path}: the reply is not UTF-8"))
        })?;
        let (body, status) = stdout.rsplit_once('\n').unwrap_or(("", stdout.as_str()));
        let status = status.trim().parse::<u16>().map_err(|_| {
            PortError::Backend(format!(
                "Forgejo {method} {path}: {PROGRAM} printed no status: `{}`",
                status.trim()
            ))
        })?;
        Ok(Reply {
            status,
            body: body.to_owned(),
        })
    }
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

fn decode<T: DeserializeOwned>(method: Method, path: &str, body: &str) -> Result<T, PortError> {
    let text = if body.trim().is_empty() { "null" } else { body };
    serde_json::from_str(text).map_err(|error| {
        PortError::Backend(format!("Forgejo {method} {path}: unreadable JSON: {error}"))
    })
}

/// `text` made safe for a URL's query string: every byte but `A-Z a-z 0-9 - . _ ~`
/// percent-encoded.
pub fn query_value(text: &str) -> String {
    let mut encoded = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
