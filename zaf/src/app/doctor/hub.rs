//! The hub service line of `zaf doctor` (docs/zaf-actions.md §5): whether the user unit that
//! serves the hub is installed and running, and on which port.
//!
//! The unit is asked of `systemctl --user` through [`Commands`]; the port is the live
//! [`HubLock`]'s. The line is informational: it never fails the report, so a machine that does
//! not run the hub as a service, or has no `systemctl`, still passes.

use std::net::IpAddr;
use std::path::Path;

use super::Commands;
use super::paint::{bold, piece, plain, with_commands};
use crate::app::hub::lan_url;
use crate::app::hub::lock::HubLock;
use crate::domain::screen::{Line, Meaning};
use crate::ports::Holder;

/// The hub's user unit, the name `zaf hub service install` writes.
pub const HUB_UNIT: &str = "zaf-hub.service";

/// What fixes a machine that has no unit.
const INSTALL_COMMAND: &str = "zaf hub service install";

/// How the hub service stands.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum HubCheck {
    /// The unit is active and a live hub holds the lock.
    Running {
        /// The hub's process.
        pid: u32,
        /// The port it serves.
        port: u16,
        /// The page's URL on this machine's LAN address.
        url: String,
    },
    /// The unit is active but no live hub holds the lock: it is starting, or serves no port.
    NoLock,
    /// The unit is installed and not active.
    Stopped {
        /// The unit's `ActiveState`: `inactive`, `failed`, `activating`…
        state: String,
    },
    /// No unit of that name.
    NotInstalled,
    /// `systemctl --user` could not answer: not installed, or no user session bus.
    Unknown {
        /// Why.
        detail: String,
    },
}

impl HubCheck {
    /// `hub service: …`, one line.
    pub fn line(&self) -> String {
        plain(&self.painted())
    }

    /// [`HubCheck::line`] as pieces: a running hub done, the rest an aside, the URL a noun and a
    /// command to run cyan.
    pub fn painted(&self) -> Line {
        let mut line = vec![bold("hub service: ")];
        match self {
            Self::Running { port, url, .. } => line.extend([
                piece(Meaning::Done, "running"),
                bold(format!(" on port {port} (")),
                piece(Meaning::Noun, url),
                bold(")"),
            ]),
            Self::NoLock => line.extend(with_commands(
                Meaning::Dim,
                "running, but no live hub holds the lock (check `zaf hub` logs)",
            )),
            Self::Stopped { state } => line.extend(with_commands(
                Meaning::Dim,
                &format!("installed, stopped ({state}; fix: systemctl --user start {HUB_UNIT})"),
            )),
            Self::NotInstalled => line.extend(with_commands(
                Meaning::Dim,
                &format!("not installed (fix: {INSTALL_COMMAND})"),
            )),
            Self::Unknown { detail } => {
                line.push(piece(Meaning::Dim, format!("unknown ({detail})")));
            }
        }
        line
    }
}

/// Asks `systemctl --user` about [`HUB_UNIT`], and the lock in `zaf_home` for the port of a unit
/// that runs. `address` is what this machine's LAN reaches it by; `alive` says whether a
/// process still runs.
pub fn check(
    zaf_home: Option<&Path>,
    commands: &dyn Commands,
    address: IpAddr,
    alive: &dyn Fn(&Holder) -> bool,
) -> HubCheck {
    let output = match commands.run(
        "systemctl",
        &[
            "--user",
            "show",
            HUB_UNIT,
            "--property=LoadState,ActiveState",
        ],
    ) {
        Ok(output) => output,
        Err(error) => {
            return HubCheck::Unknown {
                detail: error.to_string(),
            };
        }
    };
    let property = |name: &str| {
        output
            .text
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))
            .map(str::trim)
    };
    let (Some(load), Some(active)) = (property("LoadState"), property("ActiveState")) else {
        let said = output.text.lines().next().unwrap_or("no answer").trim();
        return HubCheck::Unknown {
            detail: format!("systemctl --user: {said}"),
        };
    };
    if load == "not-found" {
        return HubCheck::NotInstalled;
    }
    if active != "active" {
        return HubCheck::Stopped {
            state: active.to_owned(),
        };
    }
    match zaf_home.and_then(|home| HubLock::held(home, alive)) {
        Some(lock) => HubCheck::Running {
            pid: lock.pid,
            port: lock.port,
            url: lan_url(address, lock.port),
        },
        None => HubCheck::NoLock,
    }
}

#[cfg(test)]
mod tests;
