//! The metadata that opens an action file (docs/zaf-actions.md §3).
//!
//! Every metadata line is a comment marker, `zaf`, a key, a colon and a value.
//! The block starts at the top of the file, right after a shebang when there is
//! one, and the body starts at the first line that is not metadata. The parser
//! is pure text-in: it reads no file and runs nothing.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// The mode an action declares nothing for.
const DEFAULT_MODE: u16 = 0o755;

/// The exit code `zaf` keeps for itself: the body never ran.
const RESERVED_CODE: u8 = 125;

/// The Team Phases an action may name, in lifecycle order.
const PHASE_NAMES: &str = "claim, build, check, publish, review, land, done";

/// A Team Phase a `target: task` action may run in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum Phase {
    /// Claimed under a live lease, no slice yet.
    Claim,
    /// Its slice open, a programmer at work.
    Build,
    /// Its task checks have passed on the workspace.
    Check,
    /// Its pull request open, review steps not yet run.
    Publish,
    /// The pull request's steps have run.
    Review,
    /// The last `check pr` result is clean for the pull request's head.
    Land,
    /// Merged.
    Done,
}

/// What an action's first argument names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "lowercase")]
pub enum Target {
    /// A task: its phase is checked.
    Task,
}

/// One digit of a mode: the `r`, `w` and `x` bits for one subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bits(u8);

impl Bits {
    /// `r` (4): the action is listed and shown.
    pub fn read(self) -> bool {
        self.0 & 0o4 != 0
    }

    /// `w` (2): a file in a higher tier may override it.
    pub fn write(self) -> bool {
        self.0 & 0o2 != 0
    }

    /// `x` (1): it may be run.
    pub fn run(self) -> bool {
        self.0 & 0o1 != 0
    }
}

/// An action's mode: three octal digits for the Engineer, the action's group
/// of roles and every other role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionMode(u16);

impl ActionMode {
    /// `755`, what an action without a `mode` line gets.
    pub const DEFAULT: Self = Self(DEFAULT_MODE);

    /// The mode for exactly three octal digits; none for anything else.
    pub fn parse(text: &str) -> Option<Self> {
        let octal = text.chars().all(|digit| ('0'..='7').contains(&digit));
        if !octal || text.chars().count() != 3 {
            return None;
        }
        u16::from_str_radix(text, 8).ok().map(Self)
    }

    /// The Engineer's bits.
    pub fn owner(self) -> Bits {
        Bits(((self.0 >> 6) & 0o7) as u8)
    }

    /// The bits of the roles the action's `group` names.
    pub fn group(self) -> Bits {
        Bits(((self.0 >> 3) & 0o7) as u8)
    }

    /// The bits of every other role.
    pub fn others(self) -> Bits {
        Bits((self.0 & 0o7) as u8)
    }
}

impl Default for ActionMode {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for ActionMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:03o}", self.0)
    }
}

/// What an action file declares about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionMetadata {
    /// One line: what `zaf do` prints beside the name.
    pub description: String,
    /// Every non-zero exit code the body can return, with its meaning. Holds
    /// at least code 1, never 125.
    pub codes: BTreeMap<u8, String>,
    /// Who may see, override and run it. Default `755`.
    pub mode: ActionMode,
    /// The roles the mode's group digit covers. Default none.
    pub group: Vec<String>,
    /// What the first argument names. Default nothing.
    pub target: Option<Target>,
    /// The Team Phases it may run in; empty means any. Needs `target: task`.
    pub phases: Vec<Phase>,
    /// Minutes before the run is killed; none means the manifest's default.
    pub timeout: Option<u64>,
}

/// A parsed action file: its metadata and where its body starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAction {
    /// The metadata block.
    pub metadata: ActionMetadata,
    /// The 1-based line of the first line that is not metadata; one past the
    /// last line when the file is all metadata.
    pub body_line: usize,
}

/// Why a file is not an action: the file, the line and the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetadataError {
    /// The file.
    pub file: PathBuf,
    /// The 1-based line the problem is on.
    pub line: usize,
    /// What is wrong.
    pub reason: String,
}

impl fmt::Display for MetadataError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}: {}",
            self.file.display(),
            self.line,
            self.reason
        )
    }
}

impl std::error::Error for MetadataError {}

/// Parses the metadata block that opens `text`, the contents of `file`.
///
/// `file` only names the file in a refusal; nothing is read from it.
pub fn parse(file: &Path, text: &str) -> Result<ParsedAction, MetadataError> {
    let lines: Vec<&str> = text.lines().collect();
    let first = usize::from(lines.first().is_some_and(|line| line.starts_with("#!")));
    let mut fields = Fields::default();
    let mut body_line = lines.len() + 1;
    for (index, line) in lines.iter().enumerate().skip(first) {
        let number = index + 1;
        match metadata_line(line) {
            Some((key, value)) => {
                fields
                    .take(key, value, number)
                    .map_err(|reason| MetadataError {
                        file: file.to_path_buf(),
                        line: number,
                        reason,
                    })?;
            }
            None => {
                body_line = number;
                break;
            }
        }
    }
    let block_start = first + 1;
    if body_line == block_start {
        return Err(MetadataError {
            file: file.to_path_buf(),
            line: block_start,
            reason: "no metadata: the file must open with `<comment> zaf description: …` lines"
                .into(),
        });
    }
    fields
        .finish()
        .map(|metadata| ParsedAction {
            metadata,
            body_line,
        })
        .map_err(|(line, reason)| MetadataError {
            file: file.to_path_buf(),
            line: line.unwrap_or(block_start),
            reason,
        })
}

/// The key and value of a metadata line; none when the line is not one.
///
/// A line is metadata when it is a comment marker (a run of characters that
/// are neither letters, digits nor blanks, or `REM`), blanks, `zaf`, blanks, a
/// key without blanks, a colon and a value.
fn metadata_line(line: &str) -> Option<(&str, &str)> {
    let after_marker = match line.strip_prefix("REM") {
        Some(rest) => rest,
        None => {
            let end = line
                .find(|c: char| c.is_alphanumeric() || c.is_whitespace())
                .unwrap_or(line.len());
            if end == 0 {
                return None;
            }
            &line[end..]
        }
    };
    let rest = after_marker.strip_prefix(char::is_whitespace)?.trim_start();
    let rest = rest.strip_prefix("zaf")?;
    let rest = rest.strip_prefix(char::is_whitespace)?;
    let (key, value) = rest.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    Some((key, value.trim()))
}

/// The keys seen so far, each at most once.
#[derive(Default)]
struct Fields {
    description: Option<String>,
    codes: BTreeMap<u8, String>,
    mode: Option<ActionMode>,
    group: Option<Vec<String>>,
    target: Option<Target>,
    phases: Option<(usize, Vec<Phase>)>,
    timeout: Option<u64>,
}

impl Fields {
    /// Records one metadata line, or says why it is refused.
    fn take(&mut self, key: &str, value: &str, line: usize) -> Result<(), String> {
        let repeated = || format!("`{key}` is declared twice");
        match key {
            "description" => {
                if self.description.is_some() {
                    return Err(repeated());
                }
                if value.is_empty() {
                    return Err("`description` is empty".into());
                }
                self.description = Some(value.to_owned());
            }
            "mode" => {
                if self.mode.is_some() {
                    return Err(repeated());
                }
                let mode = ActionMode::parse(value).ok_or_else(|| {
                    format!("`mode: {value}` is not three octal digits, such as 755")
                })?;
                self.mode = Some(mode);
            }
            "group" => {
                if self.group.is_some() {
                    return Err(repeated());
                }
                self.group = Some(list(value, "group")?);
            }
            "target" => {
                if self.target.is_some() {
                    return Err(repeated());
                }
                let target = Target::from_str(value).map_err(|_| {
                    format!("`target: {value}` is not `task`; omit it for no target")
                })?;
                self.target = Some(target);
            }
            "phases" => {
                if self.phases.is_some() {
                    return Err(repeated());
                }
                let phases = list(value, "phases")?
                    .iter()
                    .map(|name| {
                        Phase::from_str(name).map_err(|_| {
                            format!("unknown phase `{name}`; the phases are {PHASE_NAMES}")
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.phases = Some((line, phases));
            }
            "timeout" => {
                if self.timeout.is_some() {
                    return Err(repeated());
                }
                let minutes = value
                    .parse::<u64>()
                    .ok()
                    .filter(|minutes| *minutes >= 1)
                    .ok_or_else(|| {
                        format!("`timeout: {value}` is not a whole number of minutes, at least 1")
                    })?;
                self.timeout = Some(minutes);
            }
            _ if key.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+') => {
                self.code(key, value)?;
            }
            _ => {
                return Err(format!(
                    "unknown key `{key}`; the keys are description, an exit code, mode, group, target, phases and timeout"
                ));
            }
        }
        Ok(())
    }

    /// Records the meaning of one exit code.
    fn code(&mut self, key: &str, value: &str) -> Result<(), String> {
        let number: i64 = key
            .parse()
            .map_err(|_| format!("`{key}` is not an integer exit code"))?;
        if number == 0 {
            return Err("code 0 is success and is not declared".into());
        }
        let code = u8::try_from(number)
            .map_err(|_| format!("code {number} is not an exit code from 1 to 255"))?;
        if code == RESERVED_CODE {
            return Err("code 125 is reserved for zaf (`NOT_RUN`) and cannot be declared".into());
        }
        if value.is_empty() {
            return Err(format!("code {code} has no meaning"));
        }
        if self.codes.insert(code, value.to_owned()).is_some() {
            return Err(format!("code {code} is declared twice"));
        }
        Ok(())
    }

    /// The metadata, or the line (none for the block as a whole) and reason it
    /// is incomplete.
    fn finish(self) -> Result<ActionMetadata, (Option<usize>, String)> {
        let Some(description) = self.description else {
            return Err((None, "`description` is required".into()));
        };
        if !self.codes.contains_key(&1) {
            return Err((None, "code 1 is not declared; declare at least `1`".into()));
        }
        let target = self.target;
        let phases = match self.phases {
            Some((line, _)) if target.is_none() => {
                return Err((Some(line), "`phases` needs `target: task`".into()));
            }
            Some((_, phases)) => phases,
            None => Vec::new(),
        };
        Ok(ActionMetadata {
            description,
            codes: self.codes,
            mode: self.mode.unwrap_or_default(),
            group: self.group.unwrap_or_default(),
            target,
            phases,
            timeout: self.timeout,
        })
    }
}

/// A comma-separated value; every item must be non-empty.
fn list(value: &str, key: &str) -> Result<Vec<String>, String> {
    let items: Vec<String> = value
        .split(',')
        .map(|item| item.trim().to_owned())
        .collect();
    if items.iter().any(String::is_empty) {
        return Err(format!("`{key}` has an empty entry"));
    }
    Ok(items)
}
