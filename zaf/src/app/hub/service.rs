//! The hub as a systemd user service (`zaf hub service install | remove`, docs/zaf-actions.md §5).
//!
//! Install writes the unit [`UNIT_NAME`] under the user's systemd folder, running
//! `zaf hub --port <n>` with `Restart=on-failure`, and enables and starts it; remove stops and
//! disables it and deletes the file. The unit's name and path are what the doctor's hub line
//! reads: [`UNIT_NAME`] and [`unit_path`]. The hub's machine lock (`lock`) keeps a second hub
//! from serving beside the service's.
//!
//! `systemctl` is the caller's to run, behind [`Systemctl`], so a test stands a fake in for it.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The unit's file name, and the name `systemctl --user` knows it by.
pub const UNIT_NAME: &str = "zaf-hub.service";

/// The port the service serves when the command line names none.
pub const DEFAULT_PORT: u16 = 7777;

/// The environment variable that moves the user's config folder.
pub const XDG_CONFIG_HOME: &str = "XDG_CONFIG_HOME";

/// What `systemctl --user <args>` does, so a test can stand in for it.
pub trait Systemctl {
    /// Runs it; the reason it failed, or could not run, as the error.
    fn user(&self, args: &[&str]) -> Result<(), String>;
}

/// Why install or remove stopped.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// The unit's folder cannot be found: neither `XDG_CONFIG_HOME` nor `HOME` is set.
    #[error("no place for a systemd user unit: neither XDG_CONFIG_HOME nor HOME is set")]
    NoHome,
    /// The unit file could not be written or removed.
    #[error("cannot use {}: {source}", path.display())]
    Io {
        /// The unit file.
        path: PathBuf,
        /// What the system said.
        source: io::Error,
    },
    /// `systemctl --user` failed, or is not there.
    #[error("systemctl --user {args}: {reason}")]
    Systemctl {
        /// The arguments it was given.
        args: String,
        /// What went wrong.
        reason: String,
    },
}

/// The folder user units live in: under `XDG_CONFIG_HOME`, else `~/.config`. An empty value
/// counts as unset.
pub fn unit_dir(config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    let set = |value: Option<OsString>| value.filter(|value| !value.is_empty());
    set(config_home)
        .map(PathBuf::from)
        .or_else(|| set(home).map(|home| PathBuf::from(home).join(".config")))
        .map(|dir| dir.join("systemd").join("user"))
}

/// The unit file in `dir`, which is [`unit_dir`]'s.
pub fn unit_path(dir: &Path) -> PathBuf {
    dir.join(UNIT_NAME)
}

/// What the unit runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The installed binary.
    pub exe: PathBuf,
    /// The port the hub serves.
    pub port: u16,
    /// The project's root: the hub reads the board from the project it is started in.
    pub project: PathBuf,
    /// The zaf home, when the environment moved it; the service gets the same one.
    pub zaf_home: Option<PathBuf>,
}

impl Unit {
    /// The unit file's text.
    pub fn render(&self) -> String {
        let mut text = String::from(
            "[Unit]\nDescription=zaf hub: the live view of the work, served on the LAN\n\n[Service]\n",
        );
        text.push_str(&format!(
            "ExecStart={} hub --port {}\n",
            quote(&self.exe),
            self.port
        ));
        text.push_str(&format!("WorkingDirectory={}\n", setting(&self.project)));
        if let Some(home) = &self.zaf_home {
            text.push_str(&format!(
                "Environment={}\n",
                quote_assignment("ZAF_HOME", home)
            ));
        }
        text.push_str("Restart=on-failure\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n");
        text
    }
}

/// A path as systemd reads it as a whole setting (`WorkingDirectory=`): the rest of the line,
/// which it neither unquotes nor reads a `$` in, so only `%` is doubled. Quoted, it would not be
/// an absolute path and the unit would be refused.
fn setting(path: &Path) -> String {
    path.to_string_lossy().replace('%', "%%")
}

/// A path as systemd reads it in a command line (`ExecStart=`): quoted when it holds a space or
/// a character systemd gives a meaning, with `%` and `$` doubled.
fn quote(path: &Path) -> String {
    let text = path.to_string_lossy().replace('%', "%%").replace('$', "$$");
    let plain = text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-+=:,@%$".contains(c));
    if plain {
        text
    } else {
        format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

/// `NAME=value` as one quoted `Environment=` word.
fn quote_assignment(name: &str, value: &Path) -> String {
    let text = value.to_string_lossy().replace('%', "%%");
    format!(
        "\"{name}={}\"",
        text.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// What install did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The unit file written.
    pub path: PathBuf,
    /// Whether it replaced one.
    pub replaced: bool,
}

/// Writes `unit` in `dir` and enables and starts it. A unit already there is replaced and its
/// running hub stopped first, so the new one starts with the new port; a stop that fails is the
/// error, since the old hub may still serve the old port.
pub fn install(
    dir: &Path,
    unit: &Unit,
    systemctl: &dyn Systemctl,
) -> Result<Installed, ServiceError> {
    let path = unit_path(dir);
    let io = |source| ServiceError::Io {
        path: path.clone(),
        source,
    };
    let replaced = path.exists();
    fs::create_dir_all(dir).map_err(io)?;
    fs::write(&path, unit.render()).map_err(io)?;
    run(systemctl, &["daemon-reload"])?;
    if replaced {
        // The unit is loaded by now, so stopping one that was not running succeeds: a failure
        // here is a hub that did not stop.
        run(systemctl, &["stop", UNIT_NAME])?;
    }
    run(systemctl, &["enable", UNIT_NAME])?;
    run(systemctl, &["start", UNIT_NAME])?;
    Ok(Installed { path, replaced })
}

/// What remove did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Removed {
    /// There was no unit.
    Absent(PathBuf),
    /// The unit was stopped, disabled and deleted.
    Gone(PathBuf),
}

/// Stops and disables the unit in `dir` and deletes it; none installed is no failure. A stop or
/// a disable that fails still leaves the file deleted, since a unit that cannot be stopped is
/// no reason to keep it, and the failure is the error.
///
/// A unit file already gone (deleted by hand) may have left its hub running and its unit
/// enabled, so the manager is still asked to stop and disable it. It refuses both for a unit it
/// does not know, which is [`Removed::Absent`]; a stop it carries out is [`Removed::Gone`].
pub fn remove(dir: &Path, systemctl: &dyn Systemctl) -> Result<Removed, ServiceError> {
    let path = unit_path(dir);
    if !path.exists() {
        let stopped = systemctl.user(&["stop", UNIT_NAME]).is_ok();
        let _ = systemctl.user(&["disable", UNIT_NAME]);
        let _ = systemctl.user(&["daemon-reload"]);
        return Ok(if stopped {
            Removed::Gone(path)
        } else {
            Removed::Absent(path)
        });
    }
    let stopped = run(systemctl, &["stop", UNIT_NAME]);
    let disabled = run(systemctl, &["disable", UNIT_NAME]);
    fs::remove_file(&path).map_err(|source| ServiceError::Io {
        path: path.clone(),
        source,
    })?;
    // A unit already gone from disk is forgotten by the manager on its next reload.
    let _ = systemctl.user(&["daemon-reload"]);
    stopped.and(disabled).map(|()| Removed::Gone(path))
}

fn run(systemctl: &dyn Systemctl, args: &[&str]) -> Result<(), ServiceError> {
    systemctl
        .user(args)
        .map_err(|reason| ServiceError::Systemctl {
            args: args.join(" "),
            reason,
        })
}

#[cfg(test)]
mod tests;
