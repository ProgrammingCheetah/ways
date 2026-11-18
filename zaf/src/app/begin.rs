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

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::actions::{PhaseEnv, RunOutcome, RunResult, Terminal};
use super::epic::{EpicError, EpicLine, SignGuard, instance_above};
use super::release::{MalformedBlock, Phase, ReleaseFolder, phase_due};
use super::spawn::runs;
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

/// An open epic of the version, as `zaf begin` prints it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EpicToSign {
    /// The epic's line: its id, stage and tally.
    #[serde(flatten)]
    pub epic: EpicLine,
    /// Whether it still wants the Engineer's signature (not signed, or edited since).
    pub unsigned: bool,
}

impl EpicToSign {
    /// The lines printed: the epic's line, then for an epic not signed `  zaf epic sign <id>`.
    pub fn render(&self) -> String {
        if self.unsigned {
            format!(
                "{}\n  zaf epic sign {}",
                self.epic.render(),
                self.epic.number
            )
        } else {
            self.epic.render()
        }
    }
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
    }
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

/// Whether the Engineer's answer is a yes: only `y` or `yes` signs.
fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
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
/// Engineer `Sign <path>? [y/N]` through `ask` (given the prompt, it returns the one line
/// answered) and, on `y`, writes the block at `at`, replacing one already there, provided the
/// document is as it was read before the prompt; one changed meanwhile is left as it is. Any other
/// answer leaves the document as it is. A document already signed is not asked about.
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
    let answer = ask(&format!("Sign {}? [y/N] ", path.display())).map_err(io_error)?;
    if !is_yes(&answer) {
        return Ok(Completion::NotSigned(path));
    }
    // Sign what is on disk now, never the reading from before the prompt.
    if std::fs::read_to_string(&path).map_err(io_error)? != text {
        return Ok(Completion::Changed(path));
    }
    std::fs::write(&path, DocumentSignature::sign(&text, at)).map_err(io_error)?;
    Ok(Completion::Signed(path))
}

#[cfg(test)]
mod tests;
