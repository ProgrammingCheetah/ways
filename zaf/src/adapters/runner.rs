//! The process runner: a role run as an external command.
//!
//! The manifest's command template gets `{prompt}` and `{dir}` substituted,
//! each single-quoted for the shell, and runs under `sh -c` in the run's
//! directory. A request's role picks the template (and any environment of its
//! own) and its profile adds environment, on top of the runner's defaults; the
//! request's own environment goes on last. stdout is captured to `report.md` and stderr to `errors.log`
//! there. A watchdog thread owns the child and kills it past the timeout.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::domain::{Profile, RoleName};
use crate::manifest::CommandLine;
use crate::ports::{PortError, RunOutput, RunRequest, Runner};

/// What the command's stdout is captured to, inside the run's directory.
pub const REPORT_FILE: &str = "report.md";

/// What the command's stderr is captured to, inside the run's directory.
pub const ERRORS_FILE: &str = "errors.log";

/// How often the watchdog looks at the child.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// What a finished process left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessReport {
    /// How the process exited.
    pub status: ExitStatus,
    /// Where its stdout was captured.
    pub report: PathBuf,
    /// Where its stderr was captured.
    pub errors: PathBuf,
    /// How long it ran.
    pub elapsed: Duration,
}

/// The command template and environment one role runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRun {
    command: CommandLine,
    env: BTreeMap<String, String>,
}

impl RoleRun {
    /// A role run by a command template, with no environment of its own.
    pub fn new(command: CommandLine) -> Self {
        Self {
            command,
            env: BTreeMap::new(),
        }
    }

    /// Adds an environment variable for this role's runs.
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }
}

/// A runner that spawns the manifest's command for a role.
///
/// [`Runner::run`] looks the request's role up among the roles added with
/// [`with_role`](Self::with_role): that role's command template and
/// environment apply, and a role left out runs the runner's own command. The
/// request's profile adds the environment set with
/// [`with_profile_env`](Self::with_profile_env). Later layers win: the runner's
/// environment, then the profile's, then the role's, then the request's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRunner {
    command: CommandLine,
    env: BTreeMap<String, String>,
    roles: BTreeMap<RoleName, RoleRun>,
    profiles: BTreeMap<Profile, BTreeMap<String, String>>,
    timeout: Duration,
}

impl ProcessRunner {
    /// A runner for a command template, killed after `timeout`.
    pub fn new(command: CommandLine, timeout: Duration) -> Self {
        Self {
            command,
            env: BTreeMap::new(),
            roles: BTreeMap::new(),
            profiles: BTreeMap::new(),
            timeout,
        }
    }

    /// Adds an environment variable for the process, on top of the caller's own
    /// (the spawner sets `CLAUDE_CONFIG_DIR` this way).
    #[must_use]
    pub fn with_env(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.insert(name.into(), value.into());
        self
    }

    /// Runs a role with its own command template and environment instead of the runner's own.
    #[must_use]
    pub fn with_role(mut self, role: RoleName, run: RoleRun) -> Self {
        self.roles.insert(role, run);
        self
    }

    /// Adds an environment variable for every run with `profile`.
    #[must_use]
    pub fn with_profile_env(
        mut self,
        profile: Profile,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.profiles
            .entry(profile)
            .or_default()
            .insert(name.into(), value.into());
        self
    }

    /// Runs the runner's own command in `dir` and waits; a timeout is [`PortError::Unavailable`].
    pub fn execute(&self, prompt: &str, dir: &Path) -> Result<ProcessReport, PortError> {
        self.launch(&self.command, &self.env, prompt, dir)
    }

    fn launch(
        &self,
        template: &CommandLine,
        env: &BTreeMap<String, String>,
        prompt: &str,
        dir: &Path,
    ) -> Result<ProcessReport, PortError> {
        let line = expand(template, prompt, dir);
        let report = dir.join(REPORT_FILE);
        let errors = dir.join(ERRORS_FILE);
        let stdout = create(&report)?;
        let stderr = create(&errors)?;
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(&line)
            .current_dir(dir)
            .envs(env)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        // Its own process group, so a timeout can take the command's children down too.
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let started = Instant::now();
        let child = command
            .spawn()
            .map_err(|error| PortError::Backend(format!("cannot start `sh`: {error}")))?;
        let timeout = self.timeout;
        let watchdog = thread::spawn(move || watch(child, timeout));
        let outcome = watchdog
            .join()
            .map_err(|_| PortError::Backend("the watchdog thread panicked".to_owned()))?;
        match outcome? {
            Some(status) => Ok(ProcessReport {
                status,
                report,
                errors,
                elapsed: started.elapsed(),
            }),
            None => Err(PortError::Unavailable(format!(
                "run timed out after {}s: {line}",
                timeout.as_secs_f64()
            ))),
        }
    }
}

impl Runner for ProcessRunner {
    /// The transcript is `report.md`; a process ended by a signal has exit code `-1`.
    /// The request's role selects the command template; the profile and the request add environment.
    fn run(&self, request: &RunRequest) -> Result<RunOutput, PortError> {
        let role = self.roles.get(&request.role);
        let template = role.map_or(&self.command, |run| &run.command);
        let mut env = self.env.clone();
        env.extend(
            self.profiles
                .get(&request.profile)
                .cloned()
                .unwrap_or_default(),
        );
        env.extend(role.map(|run| run.env.clone()).unwrap_or_default());
        env.extend(request.env.clone());
        let report = self.launch(template, &env, &request.prompt, &request.dir)?;
        let transcript = fs::read_to_string(&report.report).map_err(|error| {
            PortError::Backend(format!("cannot read {}: {error}", report.report.display()))
        })?;
        Ok(RunOutput {
            exit_code: report.status.code().unwrap_or(-1),
            transcript,
        })
    }
}

/// The template with `{prompt}` and `{dir}` substituted.
fn expand(template: &CommandLine, prompt: &str, dir: &Path) -> String {
    let dir = dir.to_string_lossy();
    let template: &str = template.as_ref();
    template
        .replace("{prompt}", &single_quote(prompt))
        .replace("{dir}", &single_quote(&dir))
}

/// Wraps text in single quotes for a POSIX shell.
fn single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

fn create(path: &Path) -> Result<File, PortError> {
    File::create(path)
        .map_err(|error| PortError::Backend(format!("cannot create {}: {error}", path.display())))
}

/// Waits for the child; `None` means it outlived the timeout and was killed.
fn watch(mut child: Child, timeout: Duration) -> Result<Option<ExitStatus>, PortError> {
    let deadline = Instant::now() + timeout;
    loop {
        let waited = child
            .try_wait()
            .map_err(|error| PortError::Backend(format!("cannot wait for the process: {error}")))?;
        if let Some(status) = waited {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            kill_group(&child);
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Best effort: kills the child's whole process group, so `sh -c 'sleep 5'` leaves no orphan.
fn kill_group(child: &Child) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg("--")
        .arg(format!("-{}", child.id()))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests;
