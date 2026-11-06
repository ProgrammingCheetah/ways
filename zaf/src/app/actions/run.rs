//! Running an action: the one runner every caller uses (docs/zaf-actions.md §3, §5).
//!
//! The file runs in the caller's directory with its arguments as given and the caller's
//! environment plus the `ZAF_*` of §3. Everything it writes goes to a log of its own. A run
//! past its time bound is killed with all its children, nested runs included. What comes back
//! says whether the body ran: [`RunResult::NotRun`] and exit 125 mean it never did, so a caller
//! never reads a refusal as a declared code.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use super::caller::Caller;
use super::metadata::{ActionMetadata, parse};
use super::phase::{derive_phase, gather_phase_facts};
use super::records::{SliceRecords, forget_records, keep_hand_back};
use super::resolve::{
    ResolveError, Resolved, Tier, can_shadow, check_name, list, resolve, resolve_in, same_file,
};
use super::slice_env::{SliceContext, SliceEnv};
use crate::composition::{STATE_DIR, ZAF_HOME_VAR};
use crate::domain::IssueNumber;

/// The exit code of a body that never ran; no action may declare it.
pub const NOT_RUN_EXIT: u8 = 125;

/// The exit code of a run past its time bound.
pub const TIMEOUT_EXIT: u8 = 2;

/// A call this deep starts nothing: a body calling itself stops here.
pub const MAX_DEPTH: u32 = 8;

/// The project root, unset outside a project.
pub const ROOT_VAR: &str = "ZAF_ROOT";

/// The action's name.
pub const ACTION_VAR: &str = "ZAF_ACTION";

/// How many `zaf do` calls are above this one.
pub const DEPTH_VAR: &str = "ZAF_DEPTH";

/// The names of the running calls, outermost first: what a refusal at the depth bound names.
pub const CHAIN_VAR: &str = "ZAF_ACTION_CHAIN";

/// The running action's log: where a nested call writes the path of its own.
pub const LOG_VAR: &str = "ZAF_ACTION_LOG";

/// The prefix of zaf's own variables, which no `[env]` name may start with.
const RESERVED_PREFIX: &str = "ZAF_";

/// What an executable action opens with: the kernel's marker for an interpreter line.
const SHEBANG: &[u8] = b"#!";

/// How [`CHAIN_VAR`] and a refusal join the names.
const CHAIN_SEPARATOR: &str = " > ";

/// The folder of a tier's home that holds the logs.
const LOGS_DIR: &str = "logs";

/// How often the run looks at the child.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// How many times a start is tried while the file is held open for writing.
const BUSY_TRIES: u32 = 5;

/// How long a start waits before trying a busy file again.
const BUSY_WAIT: Duration = Duration::from_millis(50);

/// How a run ended, in the words zaf prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum RunResult {
    /// The body ran and exited 0.
    Ok,
    /// The body ran and did not exit 0, or ran past its time bound.
    Fail,
    /// The body never ran.
    NotRun,
}

/// Where a call sits among the calls above it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nesting {
    /// How many `zaf do` calls are above this one.
    pub above: u32,
    /// Their names, outermost first.
    pub chain: Vec<String>,
    /// The log of the call right above this one.
    pub outer_log: Option<PathBuf>,
}

impl Nesting {
    /// Reads the variables a running action leaves for a call from its body. A depth that is
    /// no number is an error: the guard fails closed rather than starting from zero.
    pub fn from_env() -> Result<Self, String> {
        let Some(depth) = std::env::var_os(DEPTH_VAR) else {
            return Ok(Self::default());
        };
        let depth = depth
            .to_str()
            .and_then(|depth| depth.parse::<u32>().ok())
            .ok_or_else(|| format!("{DEPTH_VAR} is {depth:?}, not a number"))?;
        let chain = std::env::var(CHAIN_VAR)
            .or_else(|_| std::env::var(ACTION_VAR))
            .unwrap_or_default();
        Ok(Self {
            above: depth.saturating_add(1),
            chain: chain
                .split(CHAIN_SEPARATOR)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
            outer_log: std::env::var_os(LOG_VAR).map(PathBuf::from),
        })
    }
}

/// The action a `[forbidden]` command names, when it is a `zaf do` line: `zaf do land`,
/// `zaf do --tier machine land` and `zaf --more do land 12` all name `land`. Flags anywhere
/// before the name are skipped (`--tier` takes a value); `None` for any other command, and for a
/// bare `zaf do`, which names no action.
pub fn forbidden_action(command: &str) -> Option<&str> {
    let mut words = command.split_whitespace();
    if words.next()? != "zaf" {
        return None;
    }
    let mut after_do = false;
    while let Some(word) = words.next() {
        match word {
            "--tier" => {
                words.next();
            }
            flag if flag.starts_with('-') => {}
            "do" if !after_do => after_do = true,
            _ if !after_do => return None,
            name => return Some(name),
        }
    }
    None
}

/// The refusal of a forbidden name (docs/zaf-actions.md §5): an instance calling `name` when no
/// `zaf do` is above the call. The merge paths are forbidden to every profile, yet their action
/// bodies still run nested inside `slice-merge` and `release`, where the permission layer does
/// not see them. The Engineer is under no profile and is never refused. Returns the words of
/// the `NOT_RUN` line.
pub fn refuse_forbidden<'a>(
    name: &str,
    forbidden: impl IntoIterator<Item = &'a str>,
    caller: &Caller,
    nesting: &Nesting,
) -> Option<String> {
    let Caller::Role(_) = caller else {
        return None;
    };
    if nesting.above > 0 {
        return None;
    }
    forbidden
        .into_iter()
        .filter_map(forbidden_action)
        .any(|forbidden| forbidden == name)
        .then(|| format!("forbidden for {caller}"))
}

/// One run to make.
#[derive(Debug, Clone)]
pub struct RunRequest<'a> {
    /// The action's name.
    pub name: &'a str,
    /// The arguments, as given.
    pub args: &'a [OsString],
    /// The caller's directory.
    pub cwd: &'a Path,
    /// The project root; `None` outside a project, where only the machine's actions exist.
    pub project_root: Option<&'a Path>,
    /// The zaf home.
    pub home: &'a Path,
    /// `[actions] timeout_minutes`, for a file that declares no `timeout`.
    pub timeout_minutes: u64,
    /// How long one minute of a time bound lasts: [`MINUTE`] for every real run, shorter in
    /// a test that watches a kill without waiting for one.
    pub minute: Duration,
    /// Where this call sits.
    pub nesting: Nesting,
    /// Who asks: the file's mode decides for them before anything starts.
    pub caller: Caller,
    /// `zaf do --tier machine`: look in this tier alone, skipping the other. `None` takes the
    /// first tier that has the name. Only the tier is skipped: the mode check applies as ever.
    pub only: Option<Tier>,
    /// What names a task's slice and the integration branch; `None` outside a project, where
    /// `ZAF_BASE` is unset and a `target: task` action starts nothing.
    pub slice: Option<SliceContext<'a>>,
    /// The manifest's `[env]` with its placeholders filled: set on the body on top of the
    /// inherited environment, a name here replacing an inherited one. Empty outside a project.
    pub env: BTreeMap<String, String>,
}

/// One minute of a time bound.
pub const MINUTE: Duration = Duration::from_secs(60);

/// What a run came to.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    /// `OK`, `FAIL` or `NOT_RUN`.
    pub result: RunResult,
    /// The action's name.
    pub name: String,
    /// The tier the file is in; `None` when no file was found.
    pub tier: Option<Tier>,
    /// The exit code the caller passes on.
    pub exit: u8,
    /// What the code means, `undeclared` when the file does not say, why for a body that
    /// never ran; `None` on `OK`.
    pub meaning: Option<String>,
    /// How long the body ran.
    pub seconds: f64,
    /// The log; `None` for a body that never ran.
    pub log: Option<PathBuf>,
}

impl RunOutcome {
    /// A body that never ran, for a caller refused before it could ask for a run.
    pub fn not_run(name: &str, tier: Option<Tier>, reason: String) -> Self {
        Self {
            result: RunResult::NotRun,
            name: name.to_owned(),
            tier,
            exit: NOT_RUN_EXIT,
            meaning: Some(reason),
            seconds: 0.0,
            log: None,
        }
    }

    /// The one line a run prints, `None` on `OK`.
    pub fn headline(&self) -> Option<String> {
        let Self {
            name,
            exit,
            meaning,
            log,
            ..
        } = self;
        let meaning = meaning.as_deref().unwrap_or_default();
        match self.result {
            RunResult::Ok => None,
            RunResult::NotRun => Some(format!("{}: {meaning}", self.prefix(name))),
            RunResult::Fail => {
                let log = log.as_deref().unwrap_or(Path::new("")).display();
                Some(format!(
                    "{}: exit {exit}, {meaning}, log {log}",
                    self.prefix(name)
                ))
            }
        }
    }

    fn prefix(&self, name: &str) -> String {
        format!("{} {name}", self.result)
    }
}

/// One line of `zaf do`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedAction {
    /// The name.
    pub name: String,
    /// The tier it resolves to.
    pub tier: Tier,
    /// Its description; for a file without valid metadata, what is wrong with it.
    pub description: String,
}

/// The file a name resolves to; outside a project only the machine's tier exists. With `only`,
/// that tier alone is looked in: the other is skipped, and a name it alone has resolves to
/// nothing. A file found this way shadows nothing, since the tier above it was never read.
fn find_action(
    project_root: Option<&Path>,
    home: &Path,
    name: &str,
    only: Option<Tier>,
) -> Result<Option<Resolved>, ResolveError> {
    match (only, project_root) {
        (Some(Tier::Project), None) => {
            check_name(name)?;
            Ok(None)
        }
        (Some(tier), root) => resolve_in(tier, root.unwrap_or(home), home, name),
        (None, Some(root)) => resolve(root, home, name),
        (None, None) => resolve_in(Tier::Machine, home, home, name),
    }
}

/// Whether `caller` may read a file whose metadata came out as `metadata`. Without metadata there
/// is no mode to grant anything, so a role is closed out; the Engineer, who repairs the file, is
/// not, and sees it to do so.
fn may_read(caller: &Caller, metadata: &Result<ActionMetadata, String>) -> bool {
    match metadata {
        Ok(metadata) => caller.permits_read(metadata.mode, &metadata.group),
        Err(_) => *caller == Caller::Engineer,
    }
}

/// Every name the tiers provide that `caller` may read, with its tier and description.
pub fn list_actions(
    project_root: Option<&Path>,
    home: &Path,
    caller: &Caller,
) -> Result<Vec<ListedAction>, ResolveError> {
    // `list` always reads a project tier. Outside a project it is pointed at a folder that
    // cannot be a project, so a `.zaf/actions` lying about is never taken for one.
    let no_project = home.join(super::resolve::ACTIONS_DIR);
    let found = list(project_root.unwrap_or(&no_project), home)?;
    Ok(found
        .into_iter()
        .filter_map(|found| {
            let metadata = read_metadata(&found.path);
            may_read(caller, &metadata).then(|| ListedAction {
                description: metadata.map_or_else(
                    |reason| format!("invalid metadata: {reason}"),
                    |metadata| metadata.description,
                ),
                name: found.name,
                tier: found.tier,
            })
        })
        .collect())
}

/// The bytes of the file `name` resolves to, when `caller` may read it. The decision is made on
/// the bytes returned, so an edit between the two cannot slip a file past its mode.
pub fn read_action(
    project_root: Option<&Path>,
    home: &Path,
    name: &str,
    only: Option<Tier>,
    caller: &Caller,
) -> Result<Vec<u8>, Refused> {
    let found = find_action(project_root, home, name, only)
        .map_err(|error| Refused::before_the_file(error.to_string()))?
        .ok_or_else(|| Refused::before_the_file("no such action".to_owned()))?;
    let refused = |reason: String| Refused {
        tier: Some(found.tier),
        reason,
    };
    let bytes = fs::read(&found.path)
        .map_err(|error| refused(format!("cannot read {}: {error}", found.path.display())))?;
    if !may_read(caller, &metadata_in(&found.path, &bytes)) {
        return Err(refused(format!("not permitted for {caller}")));
    }
    Ok(bytes)
}

/// Runs the action, under the contract of §3, keeps what a slice name hands back and forgets the
/// records of a slice that ended. Every way a run can end is in the outcome.
pub fn run_action(request: &RunRequest<'_>) -> RunOutcome {
    forget_records(request, keep_hand_back(request, run_body(request)))
}

/// The run itself: the body from start to end, nothing kept of it.
fn run_body(request: &RunRequest<'_>) -> RunOutcome {
    let started_at = Instant::now();
    let started = match start(request) {
        Ok(started) => started,
        Err(refused) => return RunOutcome::not_run(request.name, refused.tier, refused.reason),
    };
    let Started {
        mut child,
        tier,
        metadata,
        log,
        limit_minutes,
    } = started;
    if let Some(outer) = &request.nesting.outer_log {
        write_path_into(outer, request.name, &log);
    }
    let bound = request.minute.saturating_mul(saturated(limit_minutes));
    let ended = wait(&mut child, started_at, bound);
    let (result, exit, meaning) = match ended {
        Ended::Exited(status) => judge(status, &metadata),
        Ended::TimedOut => (
            RunResult::Fail,
            TIMEOUT_EXIT,
            Some(format!("timed out after {limit_minutes} min")),
        ),
        Ended::Lost(error) => (
            RunResult::Fail,
            TIMEOUT_EXIT,
            Some(format!("cannot wait for the body: {error}")),
        ),
    };
    RunOutcome {
        result,
        name: request.name.to_owned(),
        tier: Some(tier),
        exit,
        meaning,
        seconds: started_at.elapsed().as_secs_f64(),
        log: Some(log),
    }
}

/// The last `count` lines of a log, bytes that are no text replaced.
pub fn last_lines(log: &Path, count: usize) -> io::Result<Vec<String>> {
    let mut reader = BufReader::new(File::open(log)?);
    let mut tail = std::collections::VecDeque::with_capacity(count + 1);
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line)? > 0 {
        tail.push_back(
            String::from_utf8_lossy(&line)
                .trim_end_matches(['\r', '\n'])
                .to_owned(),
        );
        if tail.len() > count {
            tail.pop_front();
        }
        line.clear();
    }
    Ok(tail.into())
}

/// A body on its way.
struct Started {
    child: Child,
    tier: Tier,
    metadata: ActionMetadata,
    log: PathBuf,
    limit_minutes: u64,
}

/// Why a body never ran, or a file was not shown.
#[derive(Debug)]
pub struct Refused {
    /// The tier of the file, once a name has resolved to one.
    pub tier: Option<Tier>,
    /// What stopped it.
    pub reason: String,
}

impl Refused {
    /// A refusal that came before any file: the caller, the place, the name.
    pub fn before_the_file(reason: String) -> Self {
        Self { tier: None, reason }
    }
}

/// Everything up to a running body: the guard, the file, what it shadows, its metadata, its mode,
/// the log, the start.
fn start(request: &RunRequest<'_>) -> Result<Started, Refused> {
    let RunRequest { name, nesting, .. } = request;
    if nesting.above >= MAX_DEPTH {
        return Err(Refused::before_the_file(format!(
            "depth {MAX_DEPTH} reached: {}",
            chain_with(nesting, name)
        )));
    }
    let found = find_action(request.project_root, request.home, name, request.only)
        .map_err(|error| Refused::before_the_file(error.to_string()))?
        .ok_or_else(|| Refused::before_the_file("no such action".to_owned()))?;
    let refused = |reason: String| Refused {
        tier: Some(found.tier),
        reason,
    };
    let shadowed = shadowed_metadata(request, &found).map_err(refused)?;
    let metadata = read_metadata(&found.path).map_err(refused)?;
    if !may_run(&request.caller, &metadata, shadowed.as_ref()) {
        return Err(refused(format!("not permitted for {}", request.caller)));
    }
    let mode = fs::metadata(&found.path)
        .map_err(|error| refused(format!("cannot read {}: {error}", found.path.display())))?
        .permissions()
        .mode();
    if mode & 0o111 == 0 {
        return Err(refused(format!(
            "{} is not executable",
            found.path.display()
        )));
    }
    // On Linux the C library would run a file with no shebang under `/bin/sh`, on macOS
    // `posix_spawnp` does: the refusal is zaf's, so every platform answers the same.
    if !has_shebang(&found.path)
        .map_err(|error| refused(format!("cannot read {}: {error}", found.path.display())))?
    {
        return Err(refused(format!(
            "{} has no shebang (#!) on its first line",
            found.path.display()
        )));
    }
    // Before a log exists, like the next: a task in the wrong phase, or one whose phase cannot be
    // read, starts nothing and leaves nothing behind.
    check_phase(request, &metadata).map_err(refused)?;
    // Before a log exists: a board that cannot answer starts nothing and leaves nothing behind.
    let slice_env = SliceEnv::for_run(request, metadata.target.is_some()).map_err(refused)?;
    let (log, log_file) = open_log(&logs_dir(request), name)
        .map_err(|error| refused(format!("cannot open a log: {error}")))?;
    let child = spawn_body(request, &found.path, &log, log_file, &slice_env).map_err(|error| {
        // Nothing ran, so nothing is left to read: no empty log beside the real ones.
        let _ = fs::remove_file(&log);
        refused(format!("cannot start {}: {error}", found.path.display()))
    })?;
    Ok(Started {
        child,
        tier: found.tier,
        limit_minutes: metadata.timeout.unwrap_or(request.timeout_minutes),
        metadata,
        log,
    })
}

/// The phase guard (docs/zaf-actions.md §3): an action with `target: task` and `phases:` runs only
/// while its task is in one of them. An action naming no phase may run in any, and so reads none.
/// Every way the phase cannot be told is a refusal too, the guard failing closed: a first argument
/// that is no task, no project, and a board, a forge or a version control that cannot be built or
/// read. The error is the words of the `NOT_RUN` line.
fn check_phase(request: &RunRequest<'_>, metadata: &ActionMetadata) -> Result<(), String> {
    if metadata.target.is_none() || metadata.phases.is_empty() {
        return Ok(());
    }
    let task = request
        .args
        .first()
        .and_then(|task| task.to_str())
        .and_then(|task| task.parse::<IssueNumber>().ok())
        .ok_or("a `target: task` action needs the task number as its first argument")?;
    let unavailable = |what: &str, reason: &String| {
        format!("cannot read the phase of {task}: no {what}: {reason}")
    };
    let (Some(root), Some(context)) = (request.project_root, request.slice.as_ref()) else {
        return Err(format!(
            "cannot read the phase of {task}: no project to read it in"
        ));
    };
    let board = context
        .board
        .as_ref()
        .map_err(|reason| unavailable("board", reason))?;
    let forge = context
        .forge
        .as_ref()
        .map_err(|reason| unavailable("forge", reason))?;
    let vcs = context
        .vcs
        .as_ref()
        .map_err(|reason| unavailable("version control", reason))?;
    let records = SliceRecords::new(root);
    let facts = gather_phase_facts(*board, *forge, *vcs, &records, task)
        .map_err(|unreadable| unreadable.to_string())?;
    let phase = derive_phase(&facts);
    if metadata.phases.contains(&phase) {
        return Ok(());
    }
    let allowed: Vec<String> = metadata.phases.iter().map(ToString::to_string).collect();
    Err(format!("{task} is in {phase}, not {}", allowed.join(", ")))
}

/// Whether the file's first two bytes are `#!`.
fn has_shebang(file: &Path) -> io::Result<bool> {
    let mut start = Vec::with_capacity(SHEBANG.len());
    File::open(file)?
        .take(SHEBANG.len() as u64)
        .read_to_end(&mut start)?;
    Ok(start == SHEBANG)
}

/// The metadata of the file `found` shadows, none when it shadows nothing; an error when `found`
/// may not stand in for it. A shadowed file whose mode cannot be read refuses like one whose mode
/// says no: the guard fails closed.
fn shadowed_metadata(
    request: &RunRequest<'_>,
    found: &Resolved,
) -> Result<Option<ActionMetadata>, String> {
    let (Some(tier), Some(root)) = (found.shadows, request.project_root) else {
        return Ok(None);
    };
    let shadowed =
        resolve_in(tier, root, request.home, &found.name).map_err(|error| error.to_string())?;
    let Some(shadowed) = shadowed.filter(|shadowed| !same_file(&found.path, &shadowed.path)) else {
        return Ok(None);
    };
    let (file, over) = (found.path.display(), shadowed.path.display());
    let metadata = read_metadata(&shadowed.path)
        .map_err(|reason| format!("{file} may not shadow {over}: it has no mode: {reason}"))?;
    if !can_shadow(metadata.mode) {
        let mode = metadata.mode;
        return Err(format!(
            "{file} may not shadow {over}: its mode {mode} has no owner w"
        ));
    }
    Ok(Some(metadata))
}

/// Whether `caller` may run a file: by its own mode and group, and by those of the file it shadows.
/// Each file is asked about the caller, rather than the two modes masked digit by digit, because a
/// role a shadow adds to its `group` reads the group digit there and the others digit here: only
/// asking both keeps that from widening.
fn may_run(caller: &Caller, metadata: &ActionMetadata, shadowed: Option<&ActionMetadata>) -> bool {
    std::iter::once(metadata)
        .chain(shadowed)
        .all(|file| caller.permits(file.mode, &file.group))
}

/// The file's metadata, or why it has none that holds.
fn read_metadata(file: &Path) -> Result<ActionMetadata, String> {
    let bytes =
        fs::read(file).map_err(|error| format!("cannot read {}: {error}", file.display()))?;
    metadata_in(file, &bytes)
}

/// The metadata `bytes` of `file` hold, or why they hold none. Bytes that are not text are none.
fn metadata_in(file: &Path, bytes: &[u8]) -> Result<ActionMetadata, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|error| format!("cannot read {}: {error}", file.display()))?;
    parse(file, text)
        .map(|parsed| parsed.metadata)
        .map_err(|error| error.to_string())
}

/// The names of the calls above, then this one.
fn chain_with(nesting: &Nesting, name: &str) -> String {
    nesting
        .chain
        .iter()
        .map(String::as_str)
        .chain([name])
        .collect::<Vec<_>>()
        .join(CHAIN_SEPARATOR)
}

/// `.zaf/logs` of the project, else `logs` of the zaf home.
fn logs_dir(request: &RunRequest<'_>) -> PathBuf {
    request.project_root.map_or_else(
        || request.home.join(LOGS_DIR),
        |root| root.join(STATE_DIR).join(LOGS_DIR),
    )
}

/// Makes the run's log: `<stamp>_<name>.log`, then `.2.log`, `.3.log` for runs in the same
/// second. The file is created exclusively, so two runs at once never share one, and is the
/// caller's alone: a body may write what its environment holds.
fn open_log(dir: &Path, name: &str) -> io::Result<(PathBuf, File)> {
    fs::create_dir_all(dir)?;
    let stamp: String = humantime::format_rfc3339_seconds(SystemTime::now())
        .to_string()
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect();
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

/// Starts the file in a process group of its own, its output in the log and nothing on stdin:
/// no one can answer a prompt whose question went to a file.
fn spawn_body(
    request: &RunRequest<'_>,
    file: &Path,
    log: &Path,
    log_file: File,
    slice_env: &SliceEnv,
) -> io::Result<Child> {
    let RunRequest { name, nesting, .. } = request;
    let mut command = Command::new(file);
    command.args(request.args).current_dir(request.cwd);
    // Before every zaf variable, so nothing in the table can stand in for one.
    apply_manifest_env(&mut command, &request.env);
    command
        .env(ZAF_HOME_VAR, request.home)
        .env(ACTION_VAR, name)
        .env(DEPTH_VAR, nesting.above.to_string())
        .env(CHAIN_VAR, chain_with(nesting, name))
        .env(LOG_VAR, log)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file.try_clone()?))
        .stderr(Stdio::from(log_file))
        .process_group(0);
    match request.project_root {
        Some(root) => command.env(ROOT_VAR, root),
        None => command.env_remove(ROOT_VAR),
    };
    // Last, so nothing the caller or an earlier line set can stand in for these.
    slice_env.apply(&mut command);
    let mut tries = 1;
    loop {
        match command.spawn() {
            Err(error)
                if error.kind() == io::ErrorKind::ExecutableFileBusy && tries < BUSY_TRIES =>
            {
                tries += 1;
                thread::sleep(BUSY_WAIT);
            }
            other => return other,
        }
    }
}

/// Puts the manifest's `[env]` on `command`, on top of the inherited environment: a name in the
/// table replaces an inherited one. A name starting with `ZAF_` (whatever its case) is skipped:
/// the manifest refuses such a table when it loads, and this keeps a table that got here some
/// other way from setting `ZAF_INSTANCE`, which would dodge the modes (docs/zaf-actions.md §3).
fn apply_manifest_env(command: &mut Command, env: &BTreeMap<String, String>) {
    for (name, value) in env {
        if !name.to_ascii_uppercase().starts_with(RESERVED_PREFIX) {
            command.env(name, value);
        }
    }
}

/// Writes the inner log's path into the outer log, straight to the file so a body capturing
/// or silencing the call does not lose it. Best effort, and it never makes a file: the path
/// comes from the environment.
fn write_path_into(outer: &Path, name: &str, log: &Path) {
    if let Ok(mut file) = OpenOptions::new().append(true).open(outer) {
        let _ = writeln!(file, "zaf do {name}: log {}", log.display());
    }
}

/// How the wait ended.
enum Ended {
    Exited(ExitStatus),
    TimedOut,
    Lost(io::Error),
}

/// Waits for the body until the bound, measured from `from`; past it, kills all it started.
fn wait(child: &mut Child, from: Instant, bound: Duration) -> Ended {
    let deadline = from.checked_add(bound);
    let ended = loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ended::Exited(status),
            Ok(None) => {}
            Err(error) => break Ended::Lost(error),
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break Ended::TimedOut;
        }
        thread::sleep(POLL_INTERVAL);
    };
    kill_all_under(child.id());
    let _ = child.kill();
    let _ = child.wait();
    ended
}

/// The code a finished body passes on, the result, and what the code means. A body killed by
/// a signal has no code, so it gets the shell's `128 + n`.
fn judge(status: ExitStatus, metadata: &ActionMetadata) -> (RunResult, u8, Option<String>) {
    let Some(code) = status.code() else {
        let signal = status.signal().unwrap_or_default();
        return (
            RunResult::Fail,
            u8::try_from(128 + signal).unwrap_or(u8::MAX),
            Some(format!("killed by signal {signal}")),
        );
    };
    let code = u8::try_from(code).unwrap_or(u8::MAX);
    if code == 0 {
        return (RunResult::Ok, code, None);
    }
    let meaning = if code == NOT_RUN_EXIT {
        "a nested action did not run".to_owned()
    } else {
        metadata
            .codes
            .get(&code)
            .cloned()
            .unwrap_or_else(|| "undeclared".to_owned())
    };
    (RunResult::Fail, code, Some(meaning))
}

/// Kills the body's process group and the groups of the nested runs under it, each of which
/// is its own: a nested `zaf do` dying does not take its body along.
fn kill_all_under(leader: u32) {
    for group in groups_under(leader) {
        let _ = Command::new("kill")
            .arg("-KILL")
            .arg("--")
            .arg(format!("-{group}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// `leader`'s group and every group holding a descendant of it, from one reading of the
/// process table. Without a table only the leader's group is known, and that is still killed.
fn groups_under(leader: u32) -> BTreeSet<u32> {
    let mut groups = BTreeSet::from([leader]);
    let Ok(output) = Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid=", "-o", "pgid="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return groups;
    };
    let table = process_table(&String::from_utf8_lossy(&output.stdout));
    let mut pids = BTreeSet::from([leader]);
    loop {
        let known = pids.len() + groups.len();
        for &(pid, parent, group) in &table {
            if pids.contains(&parent) || groups.contains(&group) {
                pids.insert(pid);
                groups.insert(group);
            }
        }
        if pids.len() + groups.len() == known {
            break;
        }
    }
    // The run's own group is the caller's: a kill sent there would end zaf, never the body.
    let me = std::process::id();
    if let Some(&(_, _, own)) = table.iter().find(|row| row.0 == me) {
        groups.remove(&own);
    }
    groups
}

/// The rows of `ps`: pid, parent and group.
fn process_table(text: &str) -> Vec<(u32, u32, u32)> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace().map(str::parse::<u32>);
            match (fields.next(), fields.next(), fields.next()) {
                (Some(Ok(pid)), Some(Ok(parent)), Some(Ok(group))) => Some((pid, parent, group)),
                _ => None,
            }
        })
        .collect()
}

/// A count of minutes as the multiplier a duration takes.
fn saturated(minutes: u64) -> u32 {
    u32::try_from(minutes).unwrap_or(u32::MAX)
}
