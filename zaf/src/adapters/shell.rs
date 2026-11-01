//! The system shell: a command line run by `sh -c` in one directory, under a time bound.
//!
//! The line's stdout and stderr come back interleaved as printed. The shell owns a
//! process group of its own, so a run past the bound takes its children down with it.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::manifest::CommandLine;
use crate::ports::{GateRun, GateVerdict, PortError, Shell};

/// How often the run looks at the child.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Runs command lines through `sh -c` in `dir`, killing a run past `timeout`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemShell {
    dir: PathBuf,
    timeout: Duration,
}

impl SystemShell {
    /// A shell that runs in `dir` and gives each line `timeout`.
    ///
    /// An inherent constructor: it pairs a directory with a bound, which no std conversion takes.
    pub fn new(dir: &Path, timeout: Duration) -> Self {
        Self {
            dir: dir.to_owned(),
            timeout,
        }
    }
}

impl Shell for SystemShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        // `exec 2>&1` first, so even a syntax error in the line is reported on the captured stream.
        let script = format!("exec 2>&1\n{}", line.as_ref());
        let mut command = Command::new("sh");
        command
            .arg("-c")
            .arg(&script)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|error| PortError::Unavailable(format!("sh: {error}")))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| PortError::Backend("sh: no output pipe".to_owned()))?;
        let reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stdout.read_to_end(&mut bytes);
            bytes
        });
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            let waited = child
                .try_wait()
                .map_err(|error| PortError::Backend(format!("cannot wait for sh: {error}")))?;
            if let Some(status) = waited {
                break Some(status);
            }
            if Instant::now() >= deadline {
                kill_group(child.id());
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            thread::sleep(POLL_INTERVAL);
        };
        let bytes = reader
            .join()
            .map_err(|_| PortError::Backend("the output thread panicked".to_owned()))?;
        let Some(status) = status else {
            return Err(PortError::Unavailable(format!(
                "timed out after {}s: {}",
                self.timeout.as_secs_f64(),
                line.as_ref()
            )));
        };
        let verdict = match status.code() {
            Some(0) => GateVerdict::Passed,
            _ => GateVerdict::Failed,
        };
        Ok(GateRun {
            verdict,
            output: String::from_utf8_lossy(&bytes).into_owned(),
        })
    }
}

/// Best effort: kills the whole process group, so `sh -c 'sleep 5'` leaves no orphan.
fn kill_group(pid: u32) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg("--")
        .arg(format!("-{pid}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests;
