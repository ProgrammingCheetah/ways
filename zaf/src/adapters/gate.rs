//! The shell adapter for [`Gate`]: the manifest's gate command lines, run by `sh -c`.
//!
//! Each run writes stdout and stderr, interleaved as printed, to a fresh file under
//! `.zaf/runs/` in the directory the gate runs in. That directory carries a
//! `.gitignore` of `*`, so a gate run never shows up as uncommitted work.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::manifest::{CommandLine, GateSection};
use crate::ports::{Gate, GateLevel, GateRun, GateVerdict, PortError};

/// Where gate output goes, relative to the directory the gate runs in.
pub const RUNS_DIR: &str = ".zaf/runs";

/// A finished gate run as the shell saw it: the exit code and the captured output's file.
///
/// A custom shape beside [`GateRun`]: the port carries the verdict and the text,
/// while a caller that keeps the log (the check verb) needs the code and the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateLog {
    /// The exit code; `None` when a signal ended the command.
    pub exit_code: Option<i32>,
    /// The file holding stdout and stderr.
    pub path: PathBuf,
}

/// The project's gate as shell command lines, run in one directory (a slice's workspace).
#[derive(Debug, Clone)]
pub struct ShellGate {
    dir: PathBuf,
    gate: GateSection,
}

impl ShellGate {
    /// The gate `[gate]` declares, run in `dir`.
    ///
    /// An inherent constructor: it joins a directory and a manifest section,
    /// which no std conversion takes.
    pub fn new(dir: &Path, gate: &GateSection) -> Self {
        Self {
            dir: dir.to_owned(),
            gate: gate.clone(),
        }
    }

    /// Runs a level and keeps its output in a file; an undeclared level is [`PortError::Unavailable`].
    pub fn run_logged(&self, level: GateLevel) -> Result<GateLog, PortError> {
        let command_line = self.command_line(level)?;
        let path = self.new_log(level)?;
        let stdout = OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|error| file_error(&path, &error))?;
        let stderr = stdout
            .try_clone()
            .map_err(|error| file_error(&path, &error))?;
        let status = Command::new("sh")
            .arg("-c")
            .arg(command_line.as_ref())
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .status()
            .map_err(|error| PortError::Unavailable(format!("sh: {error}")))?;
        let gate_log = GateLog {
            exit_code: status.code(),
            path,
        };
        Ok(gate_log)
    }

    fn command_line(&self, level: GateLevel) -> Result<&CommandLine, PortError> {
        let declared = match level {
            GateLevel::Full => Some(&self.gate.full),
            GateLevel::Light => self.gate.light.as_ref(),
        };
        declared.ok_or_else(|| PortError::Unavailable(format!("the {level} gate is not declared")))
    }

    /// Creates an empty, never-before-used log file for one run.
    fn new_log(&self, level: GateLevel) -> Result<PathBuf, PortError> {
        let runs = self.dir.join(RUNS_DIR);
        std::fs::create_dir_all(&runs).map_err(|error| file_error(&runs, &error))?;
        let ignore = runs.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n").map_err(|error| file_error(&ignore, &error))?;
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let mut attempt = 0_u64;
        loop {
            let path = runs.join(format!("{level}-{stamp}-{attempt}.log"));
            match File::create_new(&path) {
                Ok(_) => return Ok(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
                Err(error) => return Err(file_error(&path, &error)),
            }
        }
    }
}

impl Gate for ShellGate {
    fn run(&self, level: GateLevel) -> Result<GateRun, PortError> {
        let gate_log = self.run_logged(level)?;
        let output = std::fs::read_to_string(&gate_log.path)
            .map_err(|error| file_error(&gate_log.path, &error))?;
        let verdict = match gate_log.exit_code {
            Some(0) => GateVerdict::Passed,
            _ => GateVerdict::Failed,
        };
        Ok(GateRun { verdict, output })
    }
}

fn file_error(path: &Path, error: &std::io::Error) -> PortError {
    PortError::Backend(format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests;
