//! `zaf setup`: in a folder with no manifest ask for one, move a root `ZAF.toml`, make the two homes, generate the profiles, then hand
//! the Engineer what it cannot do.

use std::fs;
use std::io::{self, BufRead, ErrorKind, Write};
use std::time::Duration;

use super::brief::INSTANCE_VAR;
use super::do_action::this_terminal;
use super::doctor::{ShellCommands, add_backend_checks};
use super::exit::{self, BACKEND, OK, REFUSED};
use super::{Cli, first_run};

use crate::adapters::forgejo::ForgejoApi;
use crate::adapters::shell::SystemShell;
use crate::app::actions::Terminal;
use crate::app::init::{InitError, init_with_home};
use crate::app::profile::{ProfileError, current_pack_tag};
use crate::app::setup::{ReplaceAction, SetupError, move_legacy_manifest, setup};
use crate::composition::{Composition, CompositionError, zaf_home};
use crate::manifest::{EnvVarName, ForgeUrl, RepoName};

/// Whether this run is an instance's: `ZAF_INSTANCE` set and not blank.
fn as_instance() -> bool {
    std::env::var_os(INSTANCE_VAR).is_some_and(|name| !name.to_string_lossy().trim().is_empty())
}

/// Asks the Engineer on the terminal whether a differing machine action is replaced: writes the
/// difference, then `replace <name>? [y/N]`, and reads one line. Only `y` replaces; anything else,
/// an empty line, an unreadable line or closed input keeps.
struct TerminalAsker<R: BufRead, W: Write> {
    input: R,
    output: W,
}

impl<R: BufRead, W: Write> ReplaceAction for TerminalAsker<R, W> {
    fn replace(&mut self, name: &str, difference: &str) -> bool {
        let separator = if difference.ends_with('\n') { "" } else { "\n" };
        let shown = write!(self.output, "{difference}{separator}replace {name}? [y/N] ")
            .and_then(|()| self.output.flush());
        let mut answer = String::new();
        let read = self.input.read_line(&mut answer);
        // A typed answer's own Enter ended the line; a closed input, or one that ended in the
        // middle of a line, leaves the question's line open, so end it.
        if !answer.ends_with('\n') {
            let _ = writeln!(self.output);
        }
        shown.is_ok() && read.is_ok() && answer.trim() == "y"
    }
}

/// The asker setup is given: one on this terminal's stdin and stdout, only when the run is the
/// Engineer's (`as_instance` false) and `terminal` says someone is there; otherwise none, and
/// setup keeps every differing action.
fn terminal_asker(
    as_instance: bool,
    terminal: Terminal,
) -> Option<TerminalAsker<io::StdinLock<'static>, io::Stdout>> {
    (!as_instance && terminal == Terminal::Present).then(|| TerminalAsker {
        input: io::stdin().lock(),
        output: io::stdout(),
    })
}

/// The marketplace's current pack tag, read from a scratch profile that is removed after.
fn learn_pack_tag() -> Result<String, ProfileError> {
    let scratch = std::env::temp_dir().join(format!("zaf-pack-tag-{}", std::process::id()));
    let shell = SystemShell::new(&std::env::temp_dir(), Duration::from_secs(120));
    let tag = fs::create_dir_all(&scratch)
        .map_err(|error| ProfileError::Install {
            line: format!("mkdir {}", scratch.display()),
            output: error.to_string(),
        })
        .and_then(|()| current_pack_tag(&shell, &scratch));
    let _ = fs::remove_dir_all(&scratch);
    tag
}

/// Seconds a probe waits to connect to one candidate, and to hear from it in all.
const PROBE_CONNECT_SECONDS: u32 = 2;
const PROBE_TOTAL_SECONDS: u32 = 4;

/// Whether `url` answers `GET /api/v1/version` as a Forgejo or Gitea instance; needs no token.
fn probe_forgejo(url: &ForgeUrl) -> bool {
    ForgejoApi::new(
        url.clone(),
        RepoName::from("probe/probe"),
        EnvVarName::from("FORGEJO_TOKEN"),
        None,
    )
    .time_limits(PROBE_CONNECT_SECONDS, PROBE_TOTAL_SECONDS)
    .version()
    .is_ok()
}

/// The exit code for an asking failure: 2 for a backend failure (the pack's tag, reading or
/// writing), 1 for a refusal.
fn init_exit(error: &InitError) -> u8 {
    match error {
        InitError::Pack(_) | InitError::Io(_) => BACKEND,
        _ => REFUSED,
    }
}

/// Where no manifest is found: asks init's questions on stdin and stdout, writes `.zaf.toml`
/// in `dir`, says so in one line and loads it. Closed input or a blank required answer writes
/// nothing and exits 1 naming the question; a failed pack tag read exits 2.
fn ask_for_manifest(
    dir: &std::path::Path,
    zaf_home: Option<&std::path::Path>,
) -> Result<Composition, u8> {
    let written = init_with_home(
        dir,
        zaf_home,
        &mut learn_pack_tag,
        &mut probe_forgejo,
        &mut io::stdin().lock(),
        &mut io::stdout(),
    )
    .map_err(|error| {
        eprintln!("zaf setup: {error}");
        init_exit(&error)
    })?;
    let shown = written.strip_prefix(dir).unwrap_or(&written);
    println!("wrote {}", shown.display());
    Composition::load_with_home(dir, zaf_home).map_err(|error| {
        eprintln!("zaf setup: {error}");
        REFUSED
    })
}

/// Renames a root `ZAF.toml` to `.zaf.toml` (the Engineer's run only, said in one line),
/// makes the project's `.zaf/` and the zaf home, replaces an old `.zaf/.gitignore` that holds
/// only `*`, clones a missing design repo, generates the profiles as `zaf profile init` does,
/// runs doctor (with the board's and the forge's backends asked, as `zaf doctor` does, so a Forgejo
/// project's missing token is reported) and prints the worksheet; exits 1 while anything is left for the Engineer (a
/// pack refusal included) and 2 when a profile cannot be generated. A `ZAF.toml` beside a new manifest file exits 1 and
/// moves nothing. It installs no tool; the one thing it copies is the installed pack's actions into
/// the zaf home's `actions/`, in one line: a missing action is copied, and an installed one that
/// differs is replaced only on the Engineer's yes at the terminal, its old copy kept.
///
/// Where no manifest is found it first asks init's questions and writes `.zaf.toml` (see
/// [`ask_for_manifest`]), then goes on in the same run; a folder with any manifest is asked nothing.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    // The one verb that does not warn about the retired name: it is the one that moves it.
    let zaf_home = zaf_home();
    let composition = match Composition::discover_here_quiet() {
        Ok(composition) => composition,
        Err(CompositionError::NoProject { start }) => {
            match ask_for_manifest(&start, zaf_home.as_deref()) {
                Ok(composition) => composition,
                Err(code) => return code,
            }
        }
        Err(error) => {
            first_run();
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    match move_legacy_manifest(composition.root(), zaf_home.as_deref(), as_instance()) {
        Ok(moved) => {
            if let Some(line) = moved.line() {
                println!("{line}");
            }
        }
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return if error.kind() == ErrorKind::AlreadyExists {
                REFUSED
            } else {
                exit::BACKEND
            };
        }
    }
    let mut asker = terminal_asker(as_instance(), this_terminal());
    let mut report = match setup(
        composition.root(),
        zaf_home.as_deref(),
        composition.manifest(),
        &ShellCommands::new(composition.shell()),
        composition.shell().as_ref(),
        asker.as_mut().map(|asker| asker as &mut dyn ReplaceAction),
    ) {
        Ok(report) => report,
        Err(error) => {
            if let SetupError::Actions(failed) = &error {
                for line in failed.lines() {
                    println!("{line}");
                }
            }
            eprintln!("zaf {path}: {error}");
            return exit::BACKEND;
        }
    };
    super::join_the_list(Some(composition.root()), zaf_home.as_deref());
    add_backend_checks(&mut report.doctor, &composition);
    if let Some(profiles) = report
        .profiles
        .as_ref()
        .filter(|profiles| !profiles.unchanged())
    {
        for line in profiles.lines() {
            println!("{line}");
        }
    }
    for line in report.actions.lines() {
        println!("{line}");
    }
    for line in report.doctor.lines() {
        println!("{line}");
    }
    for line in report.worksheet_lines() {
        println!("{line}");
    }
    if report.is_ready() { OK } else { REFUSED }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/design b/design\n-old\n+new\n";

    /// What the asker returned and wrote, given `input` as the Engineer's side.
    fn ask(input: &str, difference: &str) -> (bool, String) {
        let mut written = Vec::new();
        let answer = TerminalAsker {
            input: input.as_bytes(),
            output: &mut written,
        }
        .replace("design", difference);
        (answer, String::from_utf8(written).expect("text"))
    }

    #[test]
    fn terminal_asker_replaces_only_on_y() {
        assert!(ask("y\n", DIFF).0);
        assert!(ask("y", DIFF).0);
        for other in ["n\n", "N\n", "Y\n", "yes\n", "ok\n", "yn\n"] {
            assert!(!ask(other, DIFF).0, "{other:?} must keep");
        }
    }

    #[test]
    fn terminal_asker_keeps_on_enter() {
        assert!(!ask("\n", DIFF).0);
    }

    #[test]
    fn terminal_asker_keeps_on_closed_input() {
        assert!(!ask("", DIFF).0);
    }

    #[test]
    fn a_typed_answer_leaves_no_empty_line() {
        for typed in ["y\n", "n\n"] {
            let (_, written) = ask(typed, DIFF);
            assert_eq!(written, format!("{DIFF}replace design? [y/N] "));
        }
    }

    #[test]
    fn a_closed_input_ends_the_prompts_line() {
        let (kept, written) = ask("", DIFF);
        assert!(!kept);
        assert_eq!(written, format!("{DIFF}replace design? [y/N] \n"));
        // An input that closes in the middle of a line ends it too.
        let (replaced, written) = ask("y", DIFF);
        assert!(replaced);
        assert_eq!(written, format!("{DIFF}replace design? [y/N] \n"));
    }

    #[test]
    fn terminal_asker_prints_the_difference_and_the_question() {
        let (_, written) = ask("n\n", DIFF);
        assert!(
            written.starts_with(&format!("{DIFF}replace design? [y/N]")),
            "{written}"
        );
        // A difference with no final newline still leaves the question on its own line.
        let (_, written) = ask("n\n", "+new");
        assert!(
            written.starts_with("+new\nreplace design? [y/N]"),
            "{written}"
        );
    }

    #[test]
    fn setup_has_no_asker_with_zaf_instance_set() {
        assert!(terminal_asker(true, Terminal::Present).is_none());
        assert!(terminal_asker(true, Terminal::Absent).is_none());
        assert!(terminal_asker(false, Terminal::Absent).is_none());
        assert!(terminal_asker(false, Terminal::Present).is_some());
    }
}
