//! A step's log: everything a `runner:` or `command:` step printed, kept in
//! `.zaf/logs/<YYYYMMDDTHHMMSSZ>_<step>.log` as an action's run is (the actions
//! design, §3 and ruling 30). The step returns a short reason and the path, never
//! the output.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::composition::STATE_DIR;
use crate::domain::FindingSource;
use crate::ports::PortError;

/// The folder of the logs inside the state folder.
const LOGS_DIR: &str = "logs";

/// The longest a step's name runs in a log's file name.
const NAME_LIMIT: usize = 80;

/// Writes `output` whole to a new log of the project's `.zaf/logs` and returns its path.
///
/// The file is named for the step as written (`command:cargo test` is
/// `command-cargo-test`), stamped in UTC to the second; a log of the same name in the
/// same second gets `.2`, `.3` and so on, so two steps never share a file.
pub(super) fn keep(
    root: &Path,
    source: &FindingSource,
    output: &str,
) -> Result<PathBuf, PortError> {
    let dir = root.join(STATE_DIR).join(LOGS_DIR);
    let (path, mut file) = create(&dir, source.as_ref()).map_err(|error| failed(&dir, &error))?;
    file.write_all(output.as_bytes())
        .map_err(|error| failed(&path, &error))?;
    Ok(path)
}

fn create(dir: &Path, step: &str) -> io::Result<(PathBuf, fs::File)> {
    fs::create_dir_all(dir)?;
    let stamp: String = humantime::format_rfc3339_seconds(SystemTime::now())
        .to_string()
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect();
    let name = file_name_part(step);
    let mut attempt = 1_u32;
    loop {
        let file_name = if attempt == 1 {
            format!("{stamp}_{name}.log")
        } else {
            format!("{stamp}_{name}.{attempt}.log")
        };
        let path = dir.join(file_name);
        match OpenOptions::new()
            .append(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => attempt += 1,
            Err(error) => return Err(error),
        }
    }
}

/// The step as a file name part: every run of characters outside letters, digits, `_`
/// and `.` is one `-`, and the part is cut to [`NAME_LIMIT`].
fn file_name_part(step: &str) -> String {
    let mut part = String::new();
    for c in step.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '_' | '.') {
            part.push(c);
        } else if !part.ends_with('-') {
            part.push('-');
        }
    }
    let part = part.trim_matches(|c| matches!(c, '-' | '.'));
    let cut: String = part.chars().take(NAME_LIMIT).collect();
    if cut.is_empty() {
        "step".to_owned()
    } else {
        cut
    }
}

fn failed(path: &Path, error: &io::Error) -> PortError {
    PortError::Backend(format!("cannot write the log {}: {error}", path.display()))
}
