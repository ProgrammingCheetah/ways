//! `zaf begin --version <version>`: the Engineer's start of a release.
//!
//! The verb is the Engineer's alone, so it refuses where no Engineer is: inside an instance (by
//! process ancestry, the check `zaf epic sign` makes through [`SignGuard`]) and where stdin or
//! stdout is no terminal. Past that it works out the phase due ([`phase_due`]) from the release
//! folder and the board's open epics each time and stores nothing. Where no phase is due it hands
//! back each open epic of the version to sign; where one is due, [`run_phase`] runs the action
//! named for that phase on the terminal and waits for it. A phase named in the request opens in
//! place of the one due, the design only once the objectives are signed. zaf reads nothing of the session but the
//! action's exit code: a code other than 0 leaves the phase undone.
//!
//! Run from the chair's phase pane, the action is also told `ZAF_READY` ([`ready_file`]): the
//! session writes that file with `zaf begin ready` ([`mark_ready`]) once it judges its document
//! done, and the chair ends the session. The file, or the action's own exit 0, then leads to the
//! signature ([`leads_to_sign`]), whatever the code the ended session left.

use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::actions::{PhaseEnv, RunOutcome, RunResult, Terminal};
use super::epic::{EpicError, EpicLine, SignGuard, instance_above};
use super::release::{EpicToSign, MalformedBlock, Phase, ReleaseFolder, phase_due};
use super::spawn::runs;
use crate::composition::STATE_DIR;
use crate::domain::{DocumentSignature, Epic, SignatureState, of_version};
use crate::ports::{Board, PortError};

/// What `zaf begin` is asked.
pub struct BeginRequest<'a> {
    /// The version to begin.
    pub version: &'a str,
    /// Where the version's release documents are.
    pub folder: &'a ReleaseFolder,
    /// Whether this process has a terminal.
    pub terminal: Terminal,
    /// Who runs above this process.
    pub guard: &'a SignGuard<'a>,
    /// A phase to open though it is not due (`objectives` or `design`), to change a signed
    /// document; none for the phase due.
    pub phase: Option<Phase>,
}

/// Why `zaf begin` refused: no Engineer is here to begin.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BeginRefusal {
    /// This process runs inside an instance.
    #[error("only the Engineer begins a release: {instance} runs above this process")]
    InsideInstance {
        /// The instance found above.
        instance: String,
    },
    /// Stdin or stdout is not a terminal.
    #[error("only the Engineer begins a release: this needs a terminal, and there is none")]
    NoTerminal,
    /// The design was asked for while the objectives are not signed.
    #[error("the design cannot be opened while the objectives are not signed")]
    ObjectivesNotSigned,
}

/// What `zaf begin` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeginOutcome {
    /// No one may begin here.
    Refused(BeginRefusal),
    /// A phase is due: the phase (never [`Phase::Nothing`]) and the malformed block that holds
    /// its document back, if any.
    Due {
        /// The phase due.
        phase: Phase,
        /// The malformed block of the document the phase is due for.
        malformed: Vec<MalformedBlock>,
    },
    /// Nothing is due: the open epics of the version.
    NothingDue(Vec<EpicToSign>),
}

/// The word a phase goes by.
pub fn phase_word(phase: Phase) -> &'static str {
    match phase {
        Phase::Objectives => "objectives",
        Phase::Design => "design",
        Phase::Nothing => "nothing",
    }
}

/// Begins `request.version`: refuses where no Engineer is, else reports the phase due or, with
/// none due, the version's open epics.
pub fn begin(board: &dyn Board, request: &BeginRequest) -> Result<BeginOutcome, EpicError> {
    let runs = runs::read_all(request.guard.state_dir)?;
    if let Some(instance) = instance_above(request.guard.ancestors, &runs, request.guard.alive) {
        return Ok(BeginOutcome::Refused(BeginRefusal::InsideInstance {
            instance,
        }));
    }
    if matches!(request.terminal, Terminal::Absent) {
        return Ok(BeginOutcome::Refused(BeginRefusal::NoTerminal));
    }
    let epics = board
        .epics()
        .map_err(|error: PortError| EpicError::Board(error))?;
    let due = phase_due(request.folder, &epics, request.version);
    if let Some(phase) = request.phase {
        if phase == Phase::Design && due.phase == Phase::Objectives {
            return Ok(BeginOutcome::Refused(BeginRefusal::ObjectivesNotSigned));
        }
        let malformed = if due.phase == phase {
            due.malformed
        } else {
            Vec::new()
        };
        return Ok(BeginOutcome::Due { phase, malformed });
    }
    if due.phase != Phase::Nothing {
        return Ok(BeginOutcome::Due {
            phase: due.phase,
            malformed: due.malformed,
        });
    }
    let open: Vec<EpicToSign> = of_version(&epics, request.version)
        .map(|epic: &Epic| EpicToSign {
            epic: EpicLine::from(epic),
            unsigned: !epic.signed(),
        })
        .collect();
    Ok(BeginOutcome::NothingDue(open))
}

/// What running the due phase's action came to.
#[derive(Debug, Clone, PartialEq)]
pub enum PhaseRun {
    /// The action ended with code 0.
    Done,
    /// The action ran and did not end with 0 (a timeout included): the phase stays undone.
    Undone {
        /// The action's outcome, with its code.
        outcome: RunOutcome,
    },
    /// The action never ran: there is none of that name, or a mode refused it.
    NotRun {
        /// The outcome, `NOT_RUN` with its reason.
        outcome: RunOutcome,
    },
}

/// The environment `phase` runs with: its name, the version, the release folder and, for design,
/// the signed objectives (or scope) document as input.
pub fn phase_env(folder: &ReleaseFolder, version: &str, phase: Phase) -> PhaseEnv {
    let input: Option<PathBuf> = match phase {
        Phase::Design => folder.objectives_document(),
        Phase::Objectives | Phase::Nothing => None,
    };
    PhaseEnv {
        version: version.to_owned(),
        phase: phase_word(phase).to_owned(),
        release_dir: folder.path().to_owned(),
        input,
        ready: None,
    }
}

/// The folder under `.zaf/` that holds the ready files.
pub const READY_DIR: &str = "begin";

/// The file a session run from the chair writes when it judges `phase`'s document of `version`
/// done: `<root>/.zaf/begin/<version>/<phase>.<token>.ready`, the chair's `ZAF_READY`. `token`
/// is the session's own ([`session_token`]), so no file or link left there beforehand can stand
/// in for it.
pub fn ready_file(root: &Path, version: &str, phase: Phase, token: &str) -> PathBuf {
    root.join(STATE_DIR)
        .join(READY_DIR)
        .join(version)
        .join(format!("{}.{token}.ready", phase_word(phase)))
}

/// A token no one can guess, for a session's ready file: 16 hex digits from the system's random
/// source, else from a randomly keyed hash of the time and the pid.
pub fn session_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    use std::io::Read;
    let mut bytes = [0_u8; 8];
    let random = std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .is_ok();
    let number = if random {
        u64::from_le_bytes(bytes)
    } else {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos()),
        );
        hasher.write_u32(std::process::id());
        hasher.finish()
    };
    format!("{number:016x}")
}

/// Whether the session wrote its ready file at `path`: a regular file there, never a link.
pub fn is_ready(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

/// Whether a phase run that ended as `run` goes on to the signature: the action's own exit 0, or
/// the ready file the session wrote (`ready`) whatever the code, since the chair ends a ready
/// session itself and a hang-up leaves a code of its own. A run that never started never does.
pub fn leads_to_sign(ready: bool, run: &PhaseRun) -> bool {
    match run {
        PhaseRun::Done => true,
        PhaseRun::Undone { .. } => ready,
        PhaseRun::NotRun { .. } => false,
    }
}

/// Why `zaf begin ready` wrote nothing.
#[derive(Debug, thiserror::Error)]
pub enum ReadyError {
    /// No one watches for the file: the session was not opened by the chair.
    #[error(
        "ZAF_READY is unset: no one watches this session for its ready file; end it with /exit and \
         zaf asks for the signature"
    )]
    Unset,
    /// `ZAF_READY` names something that is no ready file.
    #[error("ZAF_READY names {}, which is no .ready file", .0.display())]
    NotReadyFile(PathBuf),
    /// The file could not be written.
    #[error("cannot write {}: {source}", path.display())]
    Io {
        /// The ready file.
        path: PathBuf,
        /// What failed.
        source: io::Error,
    },
}

/// `zaf begin ready`: writes the ready file `ready` names (`ZAF_READY`), its folder made when
/// missing, holding the time `at`. Refused when no file is named, or when the name is no `.ready`
/// file, so the variable cannot point the write at any other file. The file is made new, never
/// through a link (`O_EXCL`, `O_NOFOLLOW`); one already there as a regular file is the session
/// saying it twice.
pub fn mark_ready(ready: Option<&Path>, at: SystemTime) -> Result<PathBuf, ReadyError> {
    let path = ready
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or(ReadyError::Unset)?;
    if path
        .extension()
        .is_none_or(|extension| extension != "ready")
    {
        return Err(ReadyError::NotReadyFile(path.to_owned()));
    }
    let io_error = |source| ReadyError::Io {
        path: path.to_owned(),
        source,
    };
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder).map_err(io_error)?;
    }
    let stamp = humantime::format_rfc3339_seconds(at).to_string();
    let made = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path);
    match made {
        Ok(mut file) => file
            .write_all(format!("{stamp}\n").as_bytes())
            .map_err(io_error)?,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && is_ready(path) => {}
        Err(error) => return Err(io_error(error)),
    }
    Ok(path.to_owned())
}

/// Writes `contents` to `path` whole or not at all: into a new file beside it, then renamed over
/// it, so a chair that closes or a process killed midway leaves the document as it was. A path
/// that is a link is followed first: the real file is replaced, in its own folder, and the link
/// stays a link to it.
fn replace_whole(path: &Path, contents: &str) -> io::Result<()> {
    let path = &path.canonicalize()?;
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let temp = folder.join(format!(".{name}.{}.signing", std::process::id()));
    let _ = std::fs::remove_file(&temp);
    let written = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        if let Ok(meta) = std::fs::metadata(path) {
            file.set_permissions(meta.permissions())?;
        }
        file.write_all(contents.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&temp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    written
}

/// Runs the action named for `phase` (`objectives` or `design`) once. The release folder is made
/// first when it is missing. `run` runs a named action with the phase's environment on the
/// terminal and waits for it to end.
pub fn run_phase(
    folder: &ReleaseFolder,
    version: &str,
    phase: Phase,
    run: &dyn Fn(&str, PhaseEnv) -> RunOutcome,
) -> std::io::Result<PhaseRun> {
    folder.ensure()?;
    let outcome = run(phase_word(phase), phase_env(folder, version, phase));
    Ok(match outcome.result {
        RunResult::Ok => PhaseRun::Done,
        RunResult::NotRun => PhaseRun::NotRun { outcome },
        RunResult::Fail => PhaseRun::Undone { outcome },
    })
}

/// What checking and signing a finished phase came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    /// The phase's output is not all there: one line for each thing missing.
    Missing(Vec<String>),
    /// The output is there and the Engineer did not sign the document.
    NotSigned(PathBuf),
    /// The Engineer said yes, but the document changed while the prompt waited: nothing was written.
    Changed(PathBuf),
    /// The document carries a valid block: just written on the Engineer's `y`, or there already.
    Signed(PathBuf),
}

/// Why checking or signing a finished phase failed.
#[derive(Debug, thiserror::Error)]
pub enum CompleteError {
    /// The board failed.
    #[error(transparent)]
    Board(PortError),
    /// A document could not be read or written, or the answer not read.
    #[error("{}: {source}", path.display())]
    Io {
        /// The document.
        path: PathBuf,
        /// What failed.
        source: io::Error,
    },
}

/// The body of the document at `path` (what follows its block); none when it cannot be read.
fn document_body(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(DocumentSignature::body(&text).to_owned())
}

/// What one line typed at `Sign <path>? [Y/n]` comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Answer {
    /// Enter (a line holding only its newline), `y` or `yes`.
    Yes,
    /// An answer starting with `n`, or a closed input (no line at all, so an empty string).
    No,
    /// Anything else: ask again.
    Again,
}

/// Reads the Engineer's line the way `ask_yes` does, with yes as the default.
fn read_answer(answer: &str) -> Answer {
    if answer.is_empty() {
        return Answer::No;
    }
    match answer.trim().to_ascii_lowercase().as_str() {
        "" | "y" | "yes" => Answer::Yes,
        other if other.starts_with('n') => Answer::No,
        _ => Answer::Again,
    }
}

/// Whether an epic's body holds anything past its `Release:` line.
fn has_body_past_release(epic: &Epic) -> bool {
    let rest = epic
        .body
        .as_str()
        .split_once('\n')
        .map_or("", |(_, rest)| rest);
    !rest.trim().is_empty()
}

/// The lines naming what `phase` left missing; none when its output is all there. It reads only
/// what the phase must leave: the document with a non-empty body and, after design, an open epic
/// of the version with more than its `Release:` line.
pub fn missing_after(
    board: &dyn Board,
    folder: &ReleaseFolder,
    version: &str,
    phase: Phase,
) -> Result<Vec<String>, CompleteError> {
    let mut missing = Vec::new();
    match phase {
        Phase::Objectives => {
            let written = folder
                .objectives_document()
                .and_then(|path| document_body(&path))
                .is_some_and(|body| !body.trim().is_empty());
            if !written {
                missing.push("the objectives were not written".to_owned());
            }
        }
        Phase::Design => {
            let design = folder.design_document();
            if !document_body(&design).is_some_and(|body| !body.trim().is_empty()) {
                missing.push(format!("{} was not written", design.display()));
            }
            let epics = board.epics().map_err(CompleteError::Board)?;
            if !of_version(&epics, version).any(has_body_past_release) {
                missing.push(format!(
                    "no open epic of {version} holds more than its Release line"
                ));
            }
        }
        Phase::Nothing => {}
    }
    Ok(missing)
}

/// Checks what `phase` left and, when it is all there and its document is not signed, asks the
/// Engineer `Sign <path>? [Y/n]` through `ask` (given the prompt, it returns the one line
/// answered, newline included; dropping what was typed ahead is the asker's job). On Enter, `y` or
/// `yes` it writes the block at `at`, replacing one already there, provided the document is as it
/// was read before the prompt; one changed meanwhile is left as it is. An answer starting with `n`,
/// and a closed input, leave the document as it is; any other answer asks again. A document
/// already signed is not asked about.
pub fn complete_phase(
    board: &dyn Board,
    folder: &ReleaseFolder,
    version: &str,
    phase: Phase,
    at: SystemTime,
    ask: &dyn Fn(&str) -> io::Result<String>,
) -> Result<Completion, CompleteError> {
    let missing = missing_after(board, folder, version, phase)?;
    if !missing.is_empty() {
        return Ok(Completion::Missing(missing));
    }
    let path = match phase {
        Phase::Objectives => folder.objectives_document(),
        Phase::Design => Some(folder.design_document()),
        Phase::Nothing => None,
    };
    let Some(path) = path else {
        return Ok(Completion::Missing(Vec::new()));
    };
    let io_error = |source: io::Error| CompleteError::Io {
        path: path.clone(),
        source,
    };
    let text = std::fs::read_to_string(&path).map_err(io_error)?;
    if matches!(
        DocumentSignature::read(&text),
        SignatureState::Signed { .. }
    ) {
        return Ok(Completion::Signed(path));
    }
    let prompt = format!("Sign {}? [Y/n] ", path.display());
    loop {
        match read_answer(&ask(&prompt).map_err(io_error)?) {
            Answer::Yes => break,
            Answer::No => return Ok(Completion::NotSigned(path)),
            Answer::Again => {}
        }
    }
    // Sign what is on disk now, never the reading from before the prompt.
    if std::fs::read_to_string(&path).map_err(io_error)? != text {
        return Ok(Completion::Changed(path));
    }
    replace_whole(&path, &DocumentSignature::sign(&text, at)).map_err(io_error)?;
    Ok(Completion::Signed(path))
}

#[cfg(test)]
mod tests;
