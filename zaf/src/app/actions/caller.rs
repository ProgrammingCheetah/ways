//! Who asks for a run, and whether an action's mode lets them (docs/zaf-actions.md §3).
//!
//! The caller comes from `ZAF_INSTANCE`, which a body inherits, so a nested call is decided for
//! the same role as the call above it. Modes guard against mistakes, not against a process that
//! sets the variable itself; the guard that holds is the phase.

use std::ffi::OsStr;
use std::fmt;
use std::str::FromStr;

use super::metadata::ActionMode;
use crate::domain::{Instance, RoleName};

/// Names the instance a process runs as; absent on a hand run.
pub const INSTANCE_VAR: &str = "ZAF_INSTANCE";

/// Who asks for a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Caller {
    /// A hand run: no `ZAF_INSTANCE`.
    Engineer,
    /// The role `ZAF_INSTANCE` names, whatever its name: there is no fixed list of roles, and a
    /// role called `engineer` is still a role.
    Role(RoleName),
}

impl Caller {
    /// The caller this process runs as.
    pub fn from_env() -> Result<Self, String> {
        Self::from_value(std::env::var_os(INSTANCE_VAR).as_deref())
    }

    /// The caller a value of `ZAF_INSTANCE` names. Only an absent value is the Engineer: one
    /// that does not parse, blank included, is an error, so the guard fails closed.
    pub fn from_value(value: Option<&OsStr>) -> Result<Self, String> {
        let Some(value) = value else {
            return Ok(Self::Engineer);
        };
        value
            .to_str()
            .and_then(|text| Instance::from_str(text).ok())
            .map(|instance| Self::Role(instance.role))
            .ok_or_else(|| {
                format!("{INSTANCE_VAR} is {value:?}, not `<role> instance <n> on #<epic>`")
            })
    }

    /// Whether `mode` lets this caller run an action naming `group`. Only the caller's own digit
    /// counts, as with a file: a group member the group digit refuses is refused, whatever the
    /// others digit says.
    pub fn permits(&self, mode: ActionMode, group: &[String]) -> bool {
        let digit = match self {
            Self::Engineer => mode.owner(),
            Self::Role(role) if in_group(role, group) => mode.group(),
            Self::Role(_) => mode.others(),
        };
        digit.run()
    }
}

/// Whether the group digit covers `role`: `group` names it.
fn in_group(role: &RoleName, group: &[String]) -> bool {
    let role: &str = role.as_ref();
    group.iter().any(|named| named == role)
}

/// The word a refusal prints. A role is whatever the variable held, so it is escaped: it stays
/// on the one line a refusal is.
impl fmt::Display for Caller {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engineer => formatter.write_str("the Engineer"),
            Self::Role(role) => {
                let role: &str = role.as_ref();
                write!(formatter, "{}", role.escape_debug())
            }
        }
    }
}
