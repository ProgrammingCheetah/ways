//! `zaf begin --version <version>`: the Engineer's start of a release.
//!
//! Exit 2 with one sentence inside an instance or where stdin or stdout is no terminal; with a
//! phase (objectives or design) due, the action of that name runs on the terminal with the phase
//! environment. When it ends with 0 the phase's output is checked (exit 1 naming what is missing)
//! and, when it passes and its document is not signed, the Engineer is asked `Sign it? ... [Y/n]`:
//! `y` signs and goes on to what is due next in the same run, anything else says the document is
//! not signed and exits 1. Before a session the screen draws the release's four stages (the
//! begin mock) and asks `Open it? [Y/n]`; `n` changes nothing and exits 0. `--json` draws none of
//! it. At the build stage (every open epic signed, one not done) it draws the stages with the
//! fourth marked and asks `Every epic of <v> is signed. Start the build now? [Y/n]`: yes becomes
//! `zaf spawn --watch --version <v>` in place, no or closed input prints that command and exits
//! 0. An action that ends with another code exits 1 printing it; one that
//! never ran (no such action) exits 2. With `--phase objectives|design` that phase's shell opens
//! even when it is not due (design is refused, exit 2, while the objectives are not signed), and
//! runs through the same run, check and sign steps. With nothing due, exit 0 printing each open epic of the
//! version with its stage and, for one not signed, `zaf epic sign <id>`; while one is not signed
//! (the epics stage) the screen draws the four stages, each epic with its read and sign commands
//! and the spawn line (the begin mock) instead; `--json` keeps its keys. At the built stage (every
//! open epic of the version done) it draws the four stages done, runs the action `release-open` for
//! the release pull request and prints it with `zaf begin --version <next>`; a `release-open` that
//! is refused or fails prints its sentence and exits 1 or 2.

use std::io::{self, BufRead, Write};
use std::path::Path;
use std::time::SystemTime;

use serde_json::json;

use super::Cli;
use super::do_action::{print_outcome, run_named_phase, this_terminal};
use super::epic::{build_question, exec_spawn_watch};
use super::exit::{self, BACKEND, OK, REFUSED};
use super::screen::{DefaultAnswer, Screen, ask_yes};
use crate::adapters::process;
use crate::app::begin::{
    self, BeginOutcome, BeginRequest, CompleteError, Completion, PhaseRun, phase_word,
};
use crate::app::build::{BuildError, ReleasePullRequest, open_release};
use crate::app::epic::{EpicError, SignGuard};
use crate::app::release::{EpicToSign, Phase, ReleaseFolder, Stage, next_version, stage};
use crate::composition::Composition;
use crate::domain::screen::{Meaning, clean};
use crate::manifest::Manifest;
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
        let place = Place {
            root: composition.root(),
            manifest: composition.manifest(),
            composition: &composition,
        };
        match pass(cli, board.as_ref(), &request, last, &place) {
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
    process::flush_typed();
    write!(out, "{prompt}")?;
    out.flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line)
}

/// One pass: what is due, run, checked and, on the Engineer's `y`, signed.
fn pass(
    cli: &Cli,
    board: &dyn Board,
    request: &BeginRequest,
    last: Option<Phase>,
    place: &Place,
) -> Pass {
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
            let screen = Screen::detect();
            if !cli.json {
                let here = stage_index(
                    request.phase.is_none(),
                    stage(place.root, place.manifest, board, version).ok(),
                    phase,
                );
                let opened = before_session(
                    &screen,
                    version,
                    here,
                    phase,
                    &mut io::stdin().lock(),
                    &mut io::stdout().lock(),
                );
                if !opened.unwrap_or(false) {
                    return Pass::Over(OK);
                }
            }
            let terminal = this_terminal();
            let run = |name: &str, env| run_named_phase(name, terminal, env);
            match begin::run_phase(folder, version, phase, &run) {
                Ok(PhaseRun::Done) => {
                    let file = document_shown(folder, place.root, phase);
                    let ask_sign = |prompt: &str| -> io::Result<String> {
                        if cli.json {
                            return ask_on_terminal(prompt);
                        }
                        let mut out = io::stdout().lock();
                        let yes = sign_it(&screen, &file, &mut io::stdin().lock(), &mut out)?;
                        Ok(if yes { "y\n" } else { "n\n" }.to_owned())
                    };
                    let completion = begin::complete_phase(
                        board,
                        folder,
                        version,
                        phase,
                        SystemTime::now(),
                        &ask_sign,
                    );
                    match completion {
                        Ok(Completion::Signed(_)) => {
                            if !cli.json {
                                println!("{}", signed_text(&screen, &file));
                            }
                            return Pass::Signed(phase);
                        }
                        Ok(Completion::NotSigned(document)) => {
                            if cli.json {
                                println!(
                                    "{}",
                                    json!({ "version": version, "phase": word, "result": "not signed" })
                                );
                                eprintln!("zaf {path}: {} is not signed", document.display());
                            } else {
                                print!("{}", not_signed_text(&screen, version));
                            }
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
            if !cli.json
                && matches!(
                    stage(place.root, place.manifest, board, version),
                    Ok(Stage::Build)
                )
            {
                return Pass::Over(build_stage(&Screen::detect(), version));
            }
            if cli.json {
                println!("{}", nothing_due_json(version, &epics));
            } else if epics.iter().any(|epic| epic.unsigned) {
                let screen = Screen::detect();
                let here = stage_index(
                    true,
                    stage(place.root, place.manifest, board, version).ok(),
                    Phase::Nothing,
                );
                print!("{}", epics_stage_text(&screen, version, here, &epics));
            } else if matches!(
                stage(place.root, place.manifest, board, version),
                Ok(Stage::Built)
            ) {
                let vcs = place.composition.vcs().ok();
                let next = next_version(place.root, place.manifest, vcs.as_deref());
                return Pass::Over(built_stage(
                    &Screen::detect(),
                    version,
                    &next.to_string(),
                    || open_release(place.composition, version),
                    &mut io::stdout().lock(),
                    &mut io::stderr().lock(),
                ));
            } else {
                for epic in &epics {
                    println!("{}", epic_line(epic));
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

/// Where `pass` stands: the project root and its manifest, to read the stage and show paths.
struct Place<'a> {
    root: &'a Path,
    manifest: &'a Manifest,
    composition: &'a Composition,
}

/// The built stage: all four stages drawn done, then the release pull request `open` names and the
/// command that begins `next`. Exit 0; when `open` fails its sentence goes to `err` and the exit is
/// its class (1 refused, 2 a backend).
fn built_stage<W: Write, E: Write>(
    screen: &Screen,
    version: &str,
    next: &str,
    open: impl FnOnce() -> Result<ReleasePullRequest, BuildError>,
    out: &mut W,
    err: &mut E,
) -> u8 {
    // A closed stream has no one to tell, so a failed write is let go.
    let _ = write!(out, "{}", stages_text(screen, version, STAGES));
    let _ = out.flush();
    let pull_request = match open() {
        Ok(pull_request) => pull_request,
        Err(error) => {
            let _ = writeln!(err, "zaf begin: {error}");
            return match error {
                BuildError::Refused(_) | BuildError::Unsigned { .. } | BuildError::NoRemote => {
                    REFUSED
                }
                BuildError::Backend(_) => BACKEND,
            };
        }
    };
    let named = match &pull_request.link {
        Some(link) => format!("#{} {link}", pull_request.number),
        None => format!("#{}", pull_request.number),
    };
    let _ = writeln!(
        out,
        "\n  Release pull request {}\n  Next release: {}",
        screen.paint(Meaning::Noun, &named),
        begin_again(screen, next),
    );
    OK
}

/// How many stages the release goes through.
const STAGES: usize = 4;

const ORDINALS: [&str; STAGES] = ["first", "second", "third", "fourth"];

/// The stages: name and what each is for, from the begin mock.
fn stage_lines(version: &str) -> [(&'static str, String); STAGES] {
    [
        (
            "Objectives",
            format!("you say what {version} is for; Claude asks, writes it down, you sign"),
        ),
        (
            "Design",
            "Claude drafts how to build it; you answer its questions, you sign".to_owned(),
        ),
        (
            "Epics",
            "the design becomes epics of tickets; you sign each epic".to_owned(),
        ),
        (
            "Build",
            "zaf builds the signed tickets; you watch, and answer when asked".to_owned(),
        ),
    ]
}

/// The index (from 0) of the stage the release is at, `STAGES` once it is built. A phase named
/// with `--phase` is the stage that opens, whatever the board says.
fn stage_index(follow_stage: bool, found: Option<Stage>, phase: Phase) -> usize {
    match (follow_stage, found) {
        (true, Some(Stage::Objectives)) => 0,
        (true, Some(Stage::Design)) => 1,
        (true, Some(Stage::Epics(_))) => 2,
        (true, Some(Stage::Build)) => 3,
        (true, Some(Stage::Built)) => STAGES,
        _ => usize::from(phase == Phase::Design),
    }
}

/// `text` in the colour of `meaning`, `zaf` in it keeping its own colour and the meaning resumed
/// after it.
fn styled(screen: &Screen, meaning: Meaning, text: &str) -> String {
    if !screen.colour {
        return text.to_owned();
    }
    let open = screen.paint(meaning, "\0");
    let open = open.split('\0').next().unwrap_or_default();
    let inner = screen
        .program(text)
        .replace("\x1b[0m", &format!("\x1b[0m{open}"));
    format!("{open}{inner}\x1b[0m")
}

/// The header and the four stage lines: each stage before `here` is done (`✓`), the one at it is
/// marked `▸`, the rest are later. A blank line leads and follows the header.
fn stages_text(screen: &Screen, version: &str, here: usize) -> String {
    let mut out = format!(
        "\nRelease {} goes through {}.",
        screen.paint(Meaning::Noun, version),
        screen.paint(Meaning::Bold, &format!("{STAGES} stages")),
    );
    if let Some(ordinal) = ORDINALS.get(here) {
        out.push_str(&format!(" You are at the {ordinal}:"));
    }
    out.push_str("\n\n");
    for (at, (name, what)) in stage_lines(version).iter().enumerate() {
        let mark = if at < here {
            screen.paint(Meaning::Done, "✓")
        } else if at == here {
            screen.paint(Meaning::Verb, "▸")
        } else {
            " ".to_owned()
        };
        let name = format!("{name:<11}");
        let name = if at == here {
            screen.paint(Meaning::Bold, &name)
        } else {
            name
        };
        out.push_str(&format!(
            "  {mark} {}. {name} {}\n",
            screen.paint(Meaning::Dim, &(at + 1).to_string()),
            styled(screen, Meaning::Dim, what),
        ));
    }
    out
}

/// The JSON of a version with nothing due: its open epics.
fn nothing_due_json(version: &str, epics: &[EpicToSign]) -> serde_json::Value {
    json!({ "version": version, "due": "nothing", "epics": epics })
}

/// `1 ticket`, `6 tickets`.
fn ticket_count(total: usize) -> String {
    match total {
        1 => "1 ticket".to_owned(),
        _ => format!("{total} tickets"),
    }
}

/// One epic at the epics stage: its number, title and ticket count (or, unsigned, that zaf plans
/// its tickets once it is signed), over the command that reads it and the one that signs it.
/// `epic` as the fallback list prints it, its title cleaned of control characters as the epics
/// screen does: the number and the `zaf epic sign <n>` line are as they were.
fn epic_line(epic: &EpicToSign) -> String {
    let mut cleaned = epic.clone();
    cleaned.epic.title = clean(epic.epic.title.as_str()).into();
    cleaned.render()
}

fn epic_text(screen: &Screen, epic: &EpicToSign) -> String {
    let number = epic.epic.number.get();
    let note = if epic.unsigned {
        "zaf plans its tickets once you sign it".to_owned()
    } else {
        ticket_count(epic.epic.total)
    };
    let sign = if epic.unsigned {
        format!(
            "{} {}",
            screen.paint(Meaning::Dim, "sign"),
            styled(screen, Meaning::Verb, &format!("zaf epic sign {number}"))
        )
    } else {
        screen.paint(Meaning::Done, "signed")
    };
    format!(
        "  {} {:<36} {}\n       {} {}   {sign}\n",
        screen.paint(Meaning::Noun, &format!("{number:<4}")),
        clean(epic.epic.title.as_str()),
        styled(screen, Meaning::Dim, &note),
        screen.paint(Meaning::Dim, "read"),
        styled(screen, Meaning::Verb, &format!("zaf epic show {number}")),
    )
}

/// What the epics stage draws: the stages with `here` marked, each open epic with the commands
/// that read and sign it, and the command that starts the build once they are signed.
fn epics_stage_text(screen: &Screen, version: &str, here: usize, epics: &[EpicToSign]) -> String {
    let count = epics.len();
    let mut out = stages_text(screen, version, here);
    out.push_str(&format!(
        "\nThe design became {}. Read each, then sign it to let {} build it:\n\n",
        screen.paint(
            Meaning::Bold,
            &format!("{count} {}", if count == 1 { "epic" } else { "epics" })
        ),
        screen.program("zaf"),
    ));
    for epic in epics {
        out.push_str(&epic_text(screen, epic));
    }
    out.push_str("\nOnce they are signed, start the build:\n\n");
    out.push_str(&format!(
        "  {}   {}\n\n",
        styled(
            screen,
            Meaning::Verb,
            &format!("zaf spawn --watch --version {version}")
        ),
        styled(
            screen,
            Meaning::Dim,
            "leave it running; zaf hub shows it live"
        ),
    ));
    out
}

/// What `phase`'s session is called on the screen.
fn session_word(phase: Phase) -> &'static str {
    match phase {
        Phase::Objectives => "objectives interview",
        Phase::Design => "design",
        Phase::Nothing => "nothing",
    }
}

/// What zaf says before it opens Claude Code, up to the question.
fn preface_text(screen: &Screen, phase: Phase) -> String {
    format!(
        "\n{}\n{}\n\n",
        screen.program(&format!(
            "zaf now opens {} for the {}.",
            screen.paint(Meaning::Bold, "Claude Code"),
            session_word(phase)
        )),
        screen.paint(
            Meaning::Dim,
            &format!(
                "Answer in your own words. Leave Claude ({}) when the file says what you mean.",
                screen.paint(Meaning::Verb, "/exit")
            )
        ),
    )
}

/// The command that opens the version again.
fn begin_again(screen: &Screen, version: &str) -> String {
    styled(
        screen,
        Meaning::Verb,
        &format!("zaf begin --version {version}"),
    )
}

/// What the screen shows before a session: the stages, the preface and the `Open it?` question;
/// true to go on.
fn before_session<R: BufRead, W: Write>(
    screen: &Screen,
    version: &str,
    here: usize,
    phase: Phase,
    input: &mut R,
    out: &mut W,
) -> io::Result<bool> {
    write!(out, "{}", stages_text(screen, version, here))?;
    write!(out, "{}", preface_text(screen, phase))?;
    open_it(screen, version, input, out)
}

/// Asks `Open it? [Y/n]`: true to go on; on `n` (or closed input) says nothing changed.
fn open_it<R: BufRead, W: Write>(
    screen: &Screen,
    version: &str,
    input: &mut R,
    out: &mut W,
) -> io::Result<bool> {
    let prompt = screen.paint(Meaning::Bold, "Open it?");
    if ask_yes(screen, &prompt, DefaultAnswer::Yes, input, out) {
        writeln!(out)?;
        return Ok(true);
    }
    writeln!(
        out,
        "\nNothing changed. Run {} when you are ready.",
        begin_again(screen, version)
    )?;
    Ok(false)
}

/// The build stage: the stages with the fourth marked, then Q5's question. Yes becomes
/// `zaf spawn --watch --version <version>` in this process's place (exit 2 when that cannot
/// start); no, or closed input, prints the spawn line and exits 0.
fn build_stage(screen: &Screen, version: &str) -> u8 {
    let yes = {
        let mut out = io::stdout().lock();
        start_build_question(screen, version, &mut io::stdin().lock(), &mut out)
    };
    match yes {
        Ok(false) => OK,
        Err(_) => BACKEND,
        Ok(true) => {
            let error = exec_spawn_watch(version);
            eprintln!("zaf begin: cannot start `zaf spawn --watch --version {version}`: {error}");
            BACKEND
        }
    }
}

/// Draws the stages with the fourth marked and asks Q5's question; true on a yes. On a no (or
/// closed input) it prints the spawn line.
fn start_build_question<R: BufRead, W: Write>(
    screen: &Screen,
    version: &str,
    input: &mut R,
    out: &mut W,
) -> io::Result<bool> {
    write!(out, "{}", stages_text(screen, version, STAGES - 1))?;
    writeln!(out)?;
    let prompt = screen.paint(Meaning::Bold, &build_question(version));
    if ask_yes(screen, &prompt, DefaultAnswer::Yes, input, out) {
        writeln!(out)?;
        return Ok(true);
    }
    write!(out, "{}", spawn_line(screen, version))?;
    Ok(false)
}

/// What the Engineer runs to start the build later: the command and what it does.
fn spawn_line(screen: &Screen, version: &str) -> String {
    format!(
        "\nTo start the build later, run:\n\n  {}   {}\n",
        styled(
            screen,
            Meaning::Verb,
            &format!("zaf spawn --watch --version {version}")
        ),
        screen.paint(Meaning::Dim, "leave it running; zaf hub shows it live"),
    )
}

/// The document as the Engineer reads it: under the project root when it is.
fn document_shown(folder: &ReleaseFolder, root: &Path, phase: Phase) -> String {
    let path = match phase {
        Phase::Design => Some(folder.design_document()),
        _ => folder.objectives_document(),
    }
    .unwrap_or_else(|| folder.path().to_owned());
    path.strip_prefix(root)
        .unwrap_or(&path)
        .display()
        .to_string()
}

/// After the session: `✓ <file> written`, then `Sign it? ... [Y/n]`; true on Enter, `y` or `yes`.
fn sign_it<R: BufRead, W: Write>(
    screen: &Screen,
    file: &str,
    input: &mut R,
    out: &mut W,
) -> io::Result<bool> {
    writeln!(
        out,
        "\n  {} {} written",
        screen.paint(Meaning::Done, "✓"),
        screen.paint(Meaning::Noun, file)
    )?;
    let prompt = format!(
        "{} It says what you mean, and an edit later unsigns it.",
        screen.paint(Meaning::Bold, "Sign it?")
    );
    Ok(ask_yes(screen, &prompt, DefaultAnswer::Yes, input, out))
}

/// `✓ <file> signed`.
fn signed_text(screen: &Screen, file: &str) -> String {
    format!(
        "  {} {} signed",
        screen.paint(Meaning::Done, "✓"),
        screen.paint(Meaning::Noun, file)
    )
}

/// What an unsigned document leaves the Engineer with.
fn not_signed_text(screen: &Screen, version: &str) -> String {
    format!(
        "\nNot signed. Run {} to open it again.\n",
        begin_again(screen, version)
    )
}

#[cfg(test)]
mod tests;
