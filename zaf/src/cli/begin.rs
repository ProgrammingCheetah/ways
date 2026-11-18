//! `zaf begin --version <version>`: the Engineer's start of a release.
//!
//! Exit 2 with one sentence inside an instance or where stdin or stdout is no terminal; with a
//! phase (objectives or design) due, the action of that name runs on the terminal with the phase
//! environment. When it ends with 0 the phase's output is checked (exit 1 naming what is missing)
//! and, when it passes and its document is not signed, the Engineer is asked `Sign <path>? [y/N]`:
//! `y` signs and goes on to what is due next in the same run, anything else says the document is
//! not signed and exits 1. An action that ends with another code exits 1 printing it; one that
//! never ran (no such action) exits 2. With `--phase objectives|design` that phase's shell opens
//! even when it is not due (design is refused, exit 2, while the objectives are not signed), and
//! runs through the same run, check and sign steps. With nothing due, exit 0 printing each open epic of the
//! version with its stage and, for one not signed, `zaf epic sign <id>`.

use std::io::{self, Write};
use std::time::SystemTime;

use serde_json::json;

use super::Cli;
use super::do_action::{print_outcome, run_named_phase, this_terminal};
use super::exit::{self, BACKEND, OK, REFUSED};
use crate::adapters::process;
use crate::app::begin::{
    self, BeginOutcome, BeginRequest, CompleteError, Completion, PhaseRun, phase_word,
};
use crate::app::epic::{EpicError, SignGuard};
use crate::app::release::{Phase, ReleaseFolder};
use crate::composition::Composition;
use crate::ports::Board;

/// The phases `--phase` may name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum BeginPhase {
    /// The release objectives.
    Objectives,
    /// The release design.
    Design,
}

impl From<BeginPhase> for Phase {
    fn from(phase: BeginPhase) -> Self {
        match phase {
            BeginPhase::Objectives => Self::Objectives,
            BeginPhase::Design => Self::Design,
        }
    }
}

/// Runs the verb.
pub fn run(cli: &Cli, version: &str, phase: Option<BeginPhase>) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let folder = ReleaseFolder::of(composition.root(), composition.manifest(), version);
    let ancestors = process::ancestors();
    let state_dir = composition.state_dir();
    let guard = SignGuard {
        state_dir: &state_dir,
        ancestors: &ancestors,
        alive: &process::alive,
    };
    let mut request = BeginRequest {
        version,
        folder: &folder,
        terminal: this_terminal(),
        guard: &guard,
        phase: phase.map(Phase::from),
    };
    // A signed phase leads on to the next in the same run: each pass works out what is due again.
    let mut last: Option<Phase> = None;
    loop {
        match pass(cli, board.as_ref(), &request, last) {
            Pass::Over(code) => return code,
            Pass::Signed(phase) => {
                // The named phase was asked for once: what follows is what is due.
                request.phase = None;
                last = Some(phase);
            }
        }
    }
}

/// How one pass of the verb ended.
enum Pass {
    /// The verb is over, with this exit code.
    Over(u8),
    /// The phase ran, passed its check and its document is signed: work out what is due next.
    Signed(Phase),
}

/// Reads one line from the terminal after printing `prompt`.
fn ask_on_terminal(prompt: &str) -> io::Result<String> {
    let mut out = io::stdout().lock();
    write!(out, "{prompt}")?;
    out.flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line)
}

/// One pass: what is due, run, checked and, on the Engineer's `y`, signed.
fn pass(cli: &Cli, board: &dyn Board, request: &BeginRequest, last: Option<Phase>) -> Pass {
    let path = cli.verb.path();
    let version = request.version;
    let folder = request.folder;
    let code = match begin::begin(board, request) {
        Ok(BeginOutcome::Refused(refusal)) => {
            if cli.json {
                println!("{}", json!({ "refused": refusal.to_string() }));
            }
            eprintln!("zaf {path}: {refusal}");
            BACKEND
        }
        Ok(BeginOutcome::Due { phase, malformed }) => {
            if last == Some(phase) {
                eprintln!(
                    "zaf {path}: {} is signed and still due; stopping",
                    phase_word(phase)
                );
                return Pass::Over(REFUSED);
            }
            let word = phase_word(phase);
            for block in &malformed {
                eprintln!(
                    "zaf {path}: {}:{}: {}",
                    block.path.display(),
                    block.line,
                    block.reason
                );
            }
            let terminal = this_terminal();
            let run = |name: &str, env| run_named_phase(name, terminal, env);
            match begin::run_phase(folder, version, phase, &run) {
                Ok(PhaseRun::Done) => {
                    let completion = begin::complete_phase(
                        board,
                        folder,
                        version,
                        phase,
                        SystemTime::now(),
                        &ask_on_terminal,
                    );
                    match completion {
                        Ok(Completion::Signed(document)) => {
                            if !cli.json {
                                println!("signed {}", document.display());
                            }
                            return Pass::Signed(phase);
                        }
                        Ok(Completion::NotSigned(document)) => {
                            if cli.json {
                                println!(
                                    "{}",
                                    json!({ "version": version, "phase": word, "result": "not signed" })
                                );
                            }
                            eprintln!("zaf {path}: {} is not signed", document.display());
                            REFUSED
                        }
                        Ok(Completion::Changed(document)) => {
                            if cli.json {
                                println!(
                                    "{}",
                                    json!({ "version": version, "phase": word, "result": "changed" })
                                );
                            }
                            eprintln!(
                                "zaf {path}: {} changed while it asked; running zaf begin again asks again",
                                document.display()
                            );
                            REFUSED
                        }
                        Ok(Completion::Missing(missing)) => {
                            if cli.json {
                                println!(
                                    "{}",
                                    json!({ "version": version, "phase": word, "result": "missing", "missing": missing })
                                );
                            }
                            for line in &missing {
                                eprintln!("zaf {path}: {line}");
                            }
                            REFUSED
                        }
                        Err(CompleteError::Board(error)) => exit::fail(&path, &error),
                        Err(error) => {
                            eprintln!("zaf {path}: {error}");
                            BACKEND
                        }
                    }
                }
                Ok(PhaseRun::Undone { outcome }) => {
                    print_outcome(cli, &outcome);
                    eprintln!(
                        "zaf {path}: {word} ended with code {}: the phase is not done",
                        outcome.exit
                    );
                    REFUSED
                }
                Ok(PhaseRun::NotRun { outcome }) => {
                    print_outcome(cli, &outcome);
                    BACKEND
                }
                Err(error) => {
                    eprintln!(
                        "zaf {path}: cannot make {}: {error}",
                        folder.path().display()
                    );
                    BACKEND
                }
            }
        }
        Ok(BeginOutcome::NothingDue(epics)) => {
            if cli.json {
                println!(
                    "{}",
                    json!({ "version": version, "due": "nothing", "epics": epics })
                );
            } else {
                for epic in &epics {
                    println!("{}", epic.render());
                }
            }
            OK
        }
        Err(EpicError::Board(error)) => exit::fail(&path, &error),
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            BACKEND
        }
    };
    Pass::Over(code)
}
