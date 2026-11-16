//! `zaf hub service install | remove`: the hub as a systemd user service (docs/zaf-actions.md §5).
//!
//! The unit and what is done to it are `app::hub::service`'s; this is where the machine is read
//! and the outcome printed. macOS has no service yet: each verb says so and exits 64.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use super::super::ServiceAction;
use super::super::exit::{BACKEND, NOT_IMPLEMENTED, OK};
use crate::app::hub::service::{
    DEFAULT_PORT, Installed, Removed, ServiceError, Systemctl, Unit, XDG_CONFIG_HOME, install,
    remove, unit_dir,
};
use crate::composition::{ZAF_HOME_VAR, zaf_home};

/// What the verb reads of the machine.
pub struct Host {
    /// The operating system, as `std::env::consts::OS` names it.
    pub os: &'static str,
    /// The folder user units live in; none when the environment names no home.
    pub unit_dir: Option<PathBuf>,
    /// The running binary, which the unit runs.
    pub exe: PathBuf,
    /// The directory the verb runs in: the project root.
    pub project: PathBuf,
    /// The zaf home, when the environment moved it.
    pub zaf_home: Option<PathBuf>,
}

impl Host {
    /// This machine and this process.
    fn this() -> std::io::Result<Self> {
        let var = |name: &str| std::env::var_os(name);
        Ok(Self {
            os: std::env::consts::OS,
            unit_dir: unit_dir(var(XDG_CONFIG_HOME), var("HOME")),
            exe: std::env::current_exe()?,
            project: std::env::current_dir()?,
            zaf_home: var(ZAF_HOME_VAR)
                .filter(|value| !value.is_empty())
                .and_then(|_| zaf_home()),
        })
    }
}

/// `systemctl --user`, run for real.
struct RealSystemctl;

impl Systemctl for RealSystemctl {
    fn user(&self, args: &[&str]) -> Result<(), String> {
        let output = Command::new("systemctl")
            .arg("--user")
            .args(args)
            .output()
            .map_err(|error| format!("cannot run systemctl: {error}"))?;
        if output.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(&output.stderr);
        let said = said.trim();
        Err(if said.is_empty() {
            format!("{}", output.status)
        } else {
            said.to_owned()
        })
    }
}

/// Runs `zaf hub service <action>` on this machine.
pub fn run(action: &ServiceAction, path: &str) -> u8 {
    let host = match Host::this() {
        Ok(host) => host,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return BACKEND;
        }
    };
    run_with(
        action,
        path,
        &host,
        &RealSystemctl,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    )
}

/// Runs the verb against `host`, with `systemctl` and the two streams given.
pub fn run_with(
    action: &ServiceAction,
    path: &str,
    host: &Host,
    systemctl: &dyn Systemctl,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    if host.os != "linux" {
        let _ = writeln!(err, "zaf {path}: not built on {}", host.os);
        return NOT_IMPLEMENTED;
    }
    let Some(dir) = host.unit_dir.as_deref() else {
        let _ = writeln!(err, "zaf {path}: {}", ServiceError::NoHome);
        return BACKEND;
    };
    let outcome = match action {
        ServiceAction::Install { port } => {
            let port = port.unwrap_or(DEFAULT_PORT);
            let unit = Unit {
                exe: host.exe.clone(),
                port,
                project: host.project.clone(),
                zaf_home: host.zaf_home.clone(),
            };
            install(dir, &unit, systemctl).map(|Installed { path, replaced }| {
                let verb = if replaced { "replaced" } else { "installed" };
                format!("{verb} {} on port {port}", path.display())
            })
        }
        ServiceAction::Remove => remove(dir, systemctl).map(|removed| match removed {
            Removed::Absent(_) => "no hub service installed".to_owned(),
            Removed::Gone(path) => format!("removed {}", path.display()),
        }),
    };
    match outcome {
        Ok(message) => {
            let _ = writeln!(out, "zaf {path}: {message}");
            OK
        }
        Err(error) => {
            let _ = writeln!(err, "zaf {path}: {error}");
            BACKEND
        }
    }
}

#[cfg(test)]
mod tests;
