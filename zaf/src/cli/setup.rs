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
use crate::app::init::{InitError, Question, init_with_home};
use crate::app::profile::{ProfileError, current_pack_tag};
use crate::app::release::next_version;
use crate::app::setup::{
    LegacyMove, ReplaceAction, SetupError, SetupReport, move_legacy_manifest, plan, setup, writes,
};
use crate::app::steps::StepReport;
use crate::composition::{Composition, CompositionError, zaf_home};
use crate::domain::screen::{Fix, Meaning, PlanRow, StepEnd, Text};
use crate::manifest::{EnvVarName, ForgeUrl, RepoName};

use super::screen::{DefaultAnswer, Screen, ask_yes};
use super::steps::ScreenSteps;

/// Whether this run is an instance's: `ZAF_INSTANCE` set and not blank.
pub(super) fn as_instance() -> bool {
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
    fn replace(&mut self, name: &str, _why: Option<&Text>, difference: &str) -> bool {
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

/// `text` in [`Meaning::Dim`], each whole word `zaf` in its own colour and the dim taken up
/// again after it, as the setup mock does.
fn dim_with_program(screen: &Screen, text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = whole_word(rest, "zaf") {
        if at > 0 {
            out.push_str(&screen.paint(Meaning::Dim, &rest[..at]));
        }
        out.push_str(&screen.paint(Meaning::Program, "zaf"));
        rest = &rest[at + 3..];
    }
    if !rest.is_empty() {
        out.push_str(&screen.paint(Meaning::Dim, rest));
    }
    out
}

/// Where `word` first stands in `text` between spaces or the ends.
fn whole_word(text: &str, word: &str) -> Option<usize> {
    text.match_indices(word).map(|(at, _)| at).find(|&at| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        before.is_none_or(|c| c == ' ') && after.is_none_or(|c| c == ' ')
    })
}

/// A question as the setup mock draws it: its label in bold padded to 14, its meaning dim, then
/// the prompt `›` with the default in brackets when there is one. With colour off it is the
/// plain text [`Question::plain`] gives.
pub fn draw_question(screen: &Screen, question: &Question) -> String {
    let label = screen.paint(Meaning::Bold, &format!("{:<14}", question.label));
    let meaning = dim_with_program(screen, &question.meaning);
    let prompt = screen.paint(Meaning::Verb, "›");
    match &question.default {
        Some(default) => format!(
            "  {label} {meaning}\n  {prompt} [{}] ",
            screen.paint(Meaning::Noun, default)
        ),
        None => format!("  {label} {meaning}\n  {prompt} "),
    }
}

/// Where no manifest is found: asks init's questions on stdin and stdout, writes `.zaf.toml`
/// in `dir` and loads it, handing back the name written (the caller says so). Closed input or a
/// blank required answer writes nothing and exits 1 naming the question; a failed pack tag read
/// exits 2.
fn ask_for_manifest(
    dir: &std::path::Path,
    zaf_home: Option<&std::path::Path>,
) -> Result<(Composition, String), u8> {
    let screen = Screen::detect();
    let written = init_with_home(
        dir,
        zaf_home,
        &mut learn_pack_tag,
        &mut probe_forgejo,
        &|question| draw_question(&screen, question),
        &mut io::stdin().lock(),
        &mut io::stdout(),
    )
    .map_err(|error| {
        crate::eprintln_zaf!("zaf setup: {error}");
        init_exit(&error)
    })?;
    let shown = written
        .strip_prefix(dir)
        .unwrap_or(&written)
        .display()
        .to_string();
    let composition = Composition::load_with_home(dir, zaf_home).map_err(|error| {
        crate::eprintln_zaf!("zaf setup: {error}");
        REFUSED
    })?;
    Ok((composition, shown))
}

/// The plan as the setup mock draws it: the heading, one numbered row each (verb, noun padded to 24,
/// why) and the line that says nothing else changes. With colour off it is plain text.
pub fn draw_plan(screen: &Screen, rows: &[PlanRow]) -> String {
    let things = match rows.len() {
        1 => "1 thing".to_owned(),
        n => format!("{n} things"),
    };
    let mut out = format!(
        "\n{} connects {} to this project. It will do {}:\n\n",
        screen.paint(Meaning::Verb, "zaf setup"),
        screen.paint(Meaning::Program, "zaf"),
        screen.paint(Meaning::Bold, &things),
    );
    for (at, row) in rows.iter().enumerate() {
        out.push_str(&format!(
            "  {} {} {} {}\n",
            screen.paint(Meaning::Dim, &format!("{}.", at + 1)),
            screen.paint(Meaning::Verb, &format!("{:<8}", row.verb.as_str())),
            screen.paint(Meaning::Noun, &format!("{:<24}", row.noun.as_str())),
            dim_with_program(screen, row.why.as_str()),
        ));
    }
    out.push_str(&format!(
        "\nIt changes {} in the project.\n\n",
        screen.paint(Meaning::Bold, "nothing else")
    ));
    out
}

/// Whether setup goes on: shows the plan and asks "Continue?" (Enter agrees) when the plan
/// writes something and the Engineer is there to answer. With a plan that writes nothing, with
/// `ZAF_INSTANCE` set (`as_instance`) or with no terminal it shows nothing and asks nothing: setup
/// runs as it always did. On `n`, or on input that is closed, it says "Nothing changed." and
/// returns false before anything was written.
fn go_ahead<R: BufRead, W: Write>(
    rows: &[PlanRow],
    as_instance: bool,
    terminal: Terminal,
    screen: &Screen,
    input: &mut R,
    output: &mut W,
) -> bool {
    if as_instance || terminal == Terminal::Absent || !writes(rows) {
        return true;
    }
    let _ = write!(output, "{}", draw_plan(screen, rows));
    let prompt = screen.paint(Meaning::Bold, "Continue?");
    if ask_yes(screen, &prompt, DefaultAnswer::Yes, input, output) {
        let _ = writeln!(output);
        return true;
    }
    let _ = writeln!(output, "\nNothing changed.");
    false
}

/// Renames a root `ZAF.toml` to `.zaf.toml` (the Engineer's run only, said in one line),
/// makes the project's `.zaf/` and the zaf home, replaces an old `.zaf/.gitignore` that holds
/// only `*`, clones a missing design repo, generates the profiles as `zaf profile init` does,
/// runs doctor (with the board's and the forge's backends asked, as `zaf doctor` does, so a Forgejo
/// project's missing token is reported) and prints the fixes; exits 1 while anything is left for the Engineer (a
/// pack refusal included) and 2 when a profile cannot be generated. A `ZAF.toml` beside a new manifest file exits 1 and
/// moves nothing. It installs no tool; the one thing it copies is the installed pack's actions into
/// the zaf home's `actions/`, in one line: a missing action is copied, and an installed one that
/// differs is replaced only on the Engineer's yes at the terminal, its old copy kept.
///
/// Before any of that it works out the plan of the run and, on a terminal outside an instance and
/// when the plan writes, shows it and asks "Continue? [Y/n]" (see [`go_ahead`]); `n` or closed input
/// prints "Nothing changed." and exits 0 with nothing written. Only after the yes, where no manifest
/// is found, it asks init's questions and writes `.zaf.toml` (see [`ask_for_manifest`]), then goes
/// on in the same run; a folder with any manifest is asked nothing about it.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    // The one verb that does not warn about the retired name: it is the one that moves it.
    let zaf_home = zaf_home();
    let found = match Composition::discover_here_quiet() {
        Ok(composition) => Ok(composition),
        Err(CompositionError::NoProject { start }) => Err(start),
        Err(error) => {
            first_run();
            crate::eprintln_zaf!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    let here = match &found {
        Ok(composition) => composition.root().to_owned(),
        Err(start) => start.clone(),
    };
    // Nothing is written before the answer: not the manifest, not its move, not the homes.
    let rows = plan(&here, zaf_home.as_deref(), as_instance());
    if !go_ahead(
        &rows,
        as_instance(),
        this_terminal(),
        &Screen::detect(),
        &mut io::stdin().lock(),
        &mut io::stdout(),
    ) {
        return OK;
    }
    let json = cli.json;
    let screen = Screen::detect();
    let mut steps = ScreenSteps::stdout(screen, rows.clone());
    let (composition, wrote) = match found {
        Ok(composition) => (composition, None),
        Err(start) => match ask_for_manifest(&start, zaf_home.as_deref()) {
            Ok((composition, shown)) => (composition, Some(shown)),
            Err(code) => return code,
        },
    };
    if json {
        if let Some(shown) = &wrote {
            println!("wrote {shown}");
        }
    } else if wrote.is_some() {
        // The last answer's line is closed, then the steps start on their own.
        println!();
        for verb in ["Ask", "Write"] {
            end_row(&mut steps, &rows, verb, None);
        }
    }
    match move_legacy_manifest(composition.root(), zaf_home.as_deref(), as_instance()) {
        Ok(moved) => {
            if !json && moved == LegacyMove::Moved {
                end_row(&mut steps, &rows, "Move", None);
            } else if let Some(line) = moved.line() {
                println!("{line}");
            }
        }
        Err(error) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            return if error.kind() == ErrorKind::AlreadyExists {
                REFUSED
            } else {
                exit::BACKEND
            };
        }
    }
    let mut asker = terminal_asker(as_instance(), this_terminal());
    // A spinner would draw over the question the asker may put to the Engineer.
    if !json
        && asker.is_none()
        && let Some(first) = ["Make", "Install", "Check"]
            .iter()
            .find_map(|verb| row_of(&rows, verb))
    {
        steps.started(first);
    }
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
            // Dropping the reporter stops a spinner and clears its line.
            drop(steps);
            if let SetupError::Actions(failed) = &error {
                for line in failed.lines() {
                    println!("{line}");
                }
            }
            crate::eprintln_zaf!("zaf {path}: {error}");
            return exit::BACKEND;
        }
    };
    super::join_the_list(Some(composition.root()), zaf_home.as_deref());
    add_backend_checks(&mut report.doctor, &composition);
    let ready = report.is_ready();
    if json {
        print_report(&report);
        return if ready { OK } else { REFUSED };
    }
    for verb in ["Make", "Clone", "Refresh", "Add"] {
        end_row(&mut steps, &rows, verb, None);
    }
    end_row(&mut steps, &rows, "Install", Some(installed_end(&report)));
    let fixes = left_to_fix(&report);
    let check = (!ready).then(|| StepEnd::Failed(fixes_text(fixes.len()).as_str().into()));
    end_row(&mut steps, &rows, "Check", check);
    drop(steps);
    if ready {
        let vcs = composition.vcs().ok();
        let version = next_version(composition.root(), composition.manifest(), vcs.as_deref());
        print!("{}", draw_next(&screen, &version.to_string()));
        OK
    } else {
        print!("{}", draw_fixes(&screen, &fixes));
        REFUSED
    }
}

/// What setup printed before its screen drew steps: the profile, actions and doctor lines and
/// plain fixes. `--json` keeps it.
fn print_report(report: &SetupReport) {
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
    for fix in report.fixes() {
        println!("fix: {}", fix.problem);
        if let Some(command) = &fix.command {
            println!("  run: {command}");
        }
        println!("  why: {}", fix.why);
    }
}

/// The plan's row whose verb is `verb`.
fn row_of(rows: &[PlanRow], verb: &str) -> Option<usize> {
    rows.iter().position(|row| row.verb.as_str() == verb)
}

/// Ends the plan's `verb` row through `steps`: as `end` says, else done with the row's noun. A
/// plan with no such row reports nothing.
fn end_row<S: StepReport>(steps: &mut S, rows: &[PlanRow], verb: &str, end: Option<StepEnd>) {
    if let Some(at) = row_of(rows, verb) {
        let end = end.unwrap_or_else(|| StepEnd::Done(rows[at].noun.clone()));
        steps.ended(at, &end);
    }
}

/// The Install row's end: the actions the machine now holds, counted ("12 standard actions"),
/// or the row's own noun when there are none.
fn installed_end(report: &SetupReport) -> StepEnd {
    let actions = &report.actions;
    let count = actions.installed.len() + actions.kept.len() + actions.replaced.len();
    StepEnd::Done(match count {
        0 => "the standard actions".into(),
        1 => "1 standard action".into(),
        n => format!("{n} standard actions").as_str().into(),
    })
}

/// The fixes of a run that is not ready; a run no fix explains still points to `zaf doctor`.
fn left_to_fix(report: &SetupReport) -> Vec<Fix> {
    if report.is_ready() {
        return Vec::new();
    }
    let fixes = report.fixes();
    if !fixes.is_empty() {
        return fixes;
    }
    vec![Fix {
        problem: "a check did not pass".into(),
        command: Some("zaf doctor".into()),
        why: "it names each check that failed".into(),
    }]
}

/// The Check row's end text on failure.
fn fixes_text(count: usize) -> String {
    match count {
        1 => "1 thing to fix".to_owned(),
        n => format!("{n} things to fix"),
    }
}

/// `text` on one line: every run of white space, newlines included, is one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A command as the Engineer types it, in [`Meaning::Verb`], its leading program word painted as
/// the program.
fn draw_command(screen: &Screen, command: &str) -> String {
    match command.strip_prefix("zaf ") {
        Some(rest) => format!(
            "{} {}",
            screen.program("zaf"),
            screen.paint(Meaning::Verb, rest)
        ),
        None => screen.paint(Meaning::Verb, command),
    }
}

/// What is left for the Engineer, as the setup mock draws it: the heading, then one line each
/// with the problem (failed), the command (verb) and why (dim), the columns aligned.
pub fn draw_fixes(screen: &Screen, fixes: &[Fix]) -> String {
    let lines: Vec<(String, String, String)> = fixes
        .iter()
        .map(|fix| {
            (
                one_line(fix.problem.as_str()),
                fix.command
                    .as_ref()
                    .map(|command| one_line(command.as_str()))
                    .unwrap_or_default(),
                one_line(fix.why.as_str()),
            )
        })
        .collect();
    let problems = lines
        .iter()
        .map(|line| line.0.chars().count())
        .max()
        .unwrap_or(0);
    let commands = lines
        .iter()
        .map(|line| line.1.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = format!(
        "\n{}, then run {} {} again:\n\n",
        screen.paint(Meaning::Bold, "Fix it"),
        screen.program("zaf"),
        screen.paint(Meaning::Verb, "setup"),
    );
    for (problem, command, why) in &lines {
        out.push_str(&format!(
            "  {}{} {}{} {}\n",
            screen.paint(Meaning::Failed, problem),
            " ".repeat(problems - problem.chars().count()),
            draw_command(screen, command),
            " ".repeat(commands - command.chars().count()),
            dim_with_program(screen, why),
        ));
    }
    out.push('\n');
    out
}

/// The green end, as the setup mock draws it: zaf is set up, and the command that starts the
/// work on `version`.
pub fn draw_next(screen: &Screen, version: &str) -> String {
    format!(
        "\n{} is set up here. {}\n\n  {} {}\n\n",
        screen.program("zaf"),
        screen.paint(Meaning::Bold, "Next:"),
        draw_command(screen, &format!("zaf begin --version {version}")),
        dim_with_program(
            screen,
            "you say what you want; it becomes objectives, a design, tickets"
        ),
    )
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
        .replace("design", None, difference);
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

    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use crate::app::setup::ensure_homes;

    /// Every file under `dir` with its bytes, and every folder: what "byte for byte" compares.
    fn tree(dir: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn walk(dir: &Path, into: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            for entry in fs::read_dir(dir).expect("a folder") {
                let path = entry.expect("an entry").path();
                if path.is_dir() {
                    into.insert(path.clone(), None);
                    walk(&path, into);
                } else {
                    let bytes = fs::read(&path).expect("a file");
                    into.insert(path, Some(bytes));
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(dir, &mut found);
        found
    }

    /// A folder holding only a legacy `ZAF.toml`, and a zaf home that does not exist yet.
    fn legacy_folder() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        fs::create_dir(&root).expect("project");
        fs::write(root.join("ZAF.toml"), "[project]\nname = \"x\"\n").expect("legacy");
        let home = dir.path().join("zaf-home");
        (dir, root, home)
    }

    /// What `go_ahead` answered, wrote and left of the tree, given `input` as the Engineer's side.
    fn agree(
        rows: &[PlanRow],
        as_instance: bool,
        terminal: Terminal,
        input: &str,
    ) -> (bool, String) {
        let mut written = Vec::new();
        let went = go_ahead(
            rows,
            as_instance,
            terminal,
            &PLAIN,
            &mut input.as_bytes(),
            &mut written,
        );
        (went, String::from_utf8(written).expect("text"))
    }

    #[test]
    fn no_at_continue_changes_nothing() {
        let (dir, root, home) = legacy_folder();
        let before = tree(dir.path());
        let rows = plan(&root, Some(&home), false);
        assert!(writes(&rows));
        let (went, said) = agree(&rows, false, Terminal::Present, "n\n");
        assert!(!went);
        assert!(said.contains("Continue? [Y/n] "), "{said}");
        assert!(said.contains("It will do 4 things:"), "{said}");
        assert!(said.contains("Move"), "{said}");
        assert!(
            said.contains("It changes nothing else in the project."),
            "{said}"
        );
        assert!(said.ends_with("Nothing changed.\n"), "{said}");
        assert_eq!(tree(dir.path()), before);
        assert!(!home.exists());
    }

    #[test]
    fn closed_input_at_continue_changes_nothing() {
        let (dir, root, home) = legacy_folder();
        let before = tree(dir.path());
        let rows = plan(&root, Some(&home), false);
        let (went, said) = agree(&rows, false, Terminal::Present, "");
        assert!(!went);
        assert!(said.contains("Nothing changed."), "{said}");
        assert_eq!(tree(dir.path()), before);
    }

    #[test]
    fn enter_and_yes_go_ahead() {
        let (_dir, root, home) = legacy_folder();
        let rows = plan(&root, Some(&home), false);
        for answer in ["\n", "y\n", "Yes\n"] {
            assert!(
                agree(&rows, false, Terminal::Present, answer).0,
                "{answer:?}"
            );
        }
    }

    #[test]
    fn a_rerun_with_everything_in_place_asks_nothing() {
        let (_dir, root, home) = legacy_folder();
        fs::rename(root.join("ZAF.toml"), root.join(".zaf.toml")).expect("renamed");
        ensure_homes(Some(&root), Some(&home)).expect("homes");
        let rows = plan(&root, Some(&home), false);
        assert!(!writes(&rows));
        // Closed input would refuse any question: it goes ahead, so none was asked.
        let (went, said) = agree(&rows, false, Terminal::Present, "");
        assert!(went);
        assert_eq!(said, "");
    }

    #[test]
    fn an_instance_is_never_asked() {
        let (dir, root, home) = legacy_folder();
        let before = tree(dir.path());
        let rows = plan(&root, Some(&home), true);
        let (went, said) = agree(&rows, true, Terminal::Present, "n\n");
        assert!(went);
        assert_eq!(said, "");
        // Nor is anyone asked with no terminal, a plan that writes included.
        let rows = plan(&root, Some(&home), false);
        assert!(writes(&rows));
        let (went, said) = agree(&rows, false, Terminal::Absent, "n\n");
        assert!(went);
        assert_eq!(said, "");
        assert_eq!(tree(dir.path()), before, "asking writes nothing");
    }

    #[test]
    fn a_design_clone_row_is_asked_before_anything_is_cloned() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let home = dir.path().join("zaf-home");
        fs::create_dir(&root).expect("project");
        fs::write(
            root.join(".zaf.toml"),
            concat!(
                "[project]\nname = \"b\"\n[board]\nkind = \"github-issues\"\nrepo = \"e/b\"\n",
                "[design]\nkind = \"git-sibling\"\nrepo = \"e/b-design\"\npath = \"../b-design\"\n"
            ),
        )
        .expect("manifest");
        ensure_homes(Some(&root), Some(&home)).expect("homes");
        let before = tree(dir.path());
        let rows = plan(&root, Some(&home), false);
        assert!(writes(&rows));
        let (went, said) = agree(&rows, false, Terminal::Present, "n\n");
        assert!(!went);
        assert!(said.contains("Clone"), "{said}");
        assert!(said.contains("Continue? [Y/n] "), "{said}");
        assert!(said.ends_with("Nothing changed.\n"), "{said}");
        assert_eq!(tree(dir.path()), before);
    }

    #[test]
    fn the_plan_is_drawn_as_the_mock() {
        let dir = tempfile::tempdir().expect("temp dir");
        let rows = plan(&dir.path().join("empty"), None, false);
        let shown = draw_plan(&PLAIN, &rows);
        assert_eq!(
            shown,
            "\nzaf setup connects zaf to this project. It will do 5 things:\n\n  \
             1. Ask      4 questions              what zaf needs to know about this project\n  \
             2. Write    .zaf.toml                your answers, so zaf remembers them\n  \
             3. Make     .zaf/                    where zaf keeps its state\n  \
             4. Install  the standard actions     the steps zaf runs: open a PR, merge, release\n  \
             5. Check    your tools and access    so the first real run does not fail halfway\n\
             \nIt changes nothing else in the project.\n\n"
        );
    }

    fn question(label: &'static str, meaning: &str, default: Option<&str>) -> Question {
        Question {
            label,
            meaning: meaning.to_owned(),
            default: default.map(str::to_owned),
            required: true,
        }
    }

    const PLAIN: Screen = Screen {
        colour: false,
        live: false,
        rows: 24,
        columns: 80,
    };

    #[test]
    fn a_question_is_drawn_as_the_mock() {
        let name = question(
            "Project name",
            "what zaf calls this project",
            Some("my-app"),
        );
        assert_eq!(
            draw_question(&PLAIN, &name),
            "  Project name   what zaf calls this project\n  › [my-app] "
        );
        assert_eq!(draw_question(&PLAIN, &name), name.plain());
        let check = question("Check command", "the command", None);
        assert_eq!(
            draw_question(&PLAIN, &check),
            "  Check command  the command\n  › "
        );

        let coloured = Screen {
            colour: true,
            ..PLAIN
        };
        assert_eq!(
            draw_question(&coloured, &name),
            "  \x1b[1mProject name  \x1b[0m \x1b[2mwhat \x1b[0m\x1b[1;38;5;201mzaf\x1b[0m\
             \x1b[2m calls this project\x1b[0m\n  \x1b[1;36m›\x1b[0m [\x1b[33mmy-app\x1b[0m] "
        );
    }

    const COLOUR: Screen = Screen {
        colour: true,
        ..PLAIN
    };

    fn gh_fix() -> Fix {
        Fix {
            problem: "gh is not signed in".into(),
            command: Some("gh auth login".into()),
            why: "zaf opens pull requests through gh".into(),
        }
    }

    /// `text` with every escape sequence taken out.
    fn bare(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                chars.by_ref().find(|c| c.is_ascii_alphabetic());
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn a_green_setup_names_the_next_begin() {
        assert_eq!(
            draw_next(&PLAIN, "0.1.0"),
            "\nzaf is set up here. Next:\n\n  zaf begin --version 0.1.0 \
             you say what you want; it becomes objectives, a design, tickets\n\n"
        );
    }

    #[test]
    fn a_failed_setup_shows_only_its_fixes() {
        let shown = draw_fixes(&PLAIN, &[gh_fix()]);
        assert_eq!(
            shown,
            "\nFix it, then run zaf setup again:\n\n  \
             gh is not signed in gh auth login zaf opens pull requests through gh\n\n"
        );
        // A fix with no command and a problem on two lines still takes one line, columns aligned.
        let two = draw_fixes(
            &PLAIN,
            &[
                gh_fix(),
                Fix {
                    problem: "the folder\nis ignored".into(),
                    command: None,
                    why: "git cannot track it".into(),
                },
            ],
        );
        let lines: Vec<&str> = two
            .lines()
            .skip(3)
            .filter(|line| !line.is_empty())
            .collect();
        assert_eq!(lines.len(), 2, "{two}");
        assert!(lines[1].starts_with("  the folder is ignored  "), "{two}");
        assert_eq!(lines[0].find("gh auth login"), Some(24), "{two}");
        assert_eq!(lines[1].find("git cannot"), Some(38), "{two}");
        assert_eq!(fixes_text(1), "1 thing to fix");
        assert_eq!(fixes_text(2), "2 things to fix");
    }

    #[test]
    fn no_color_gives_the_same_words_with_no_codes() {
        for (plain, coloured) in [
            (draw_next(&PLAIN, "0.2.0"), draw_next(&COLOUR, "0.2.0")),
            (
                draw_fixes(&PLAIN, &[gh_fix()]),
                draw_fixes(&COLOUR, &[gh_fix()]),
            ),
        ] {
            assert!(
                !plain.contains('\x1b') && !plain.contains('\r'),
                "{plain:?}"
            );
            assert!(coloured.contains('\x1b'));
            assert_eq!(bare(&coloured), plain);
        }
    }

    #[test]
    fn a_piped_setup_neither_colours_nor_redraws() {
        let piped = Screen::from_environment(crate::cli::screen::Environment {
            stdout_is_terminal: false,
            no_color: None,
            term: Some("xterm"),
        });
        assert!(!piped.colour && !piped.live);
        for text in [draw_next(&piped, "0.1.0"), draw_fixes(&piped, &[gh_fix()])] {
            assert!(!text.contains('\x1b') && !text.contains('\r'), "{text:?}");
        }
    }

    #[test]
    fn the_program_word_is_painted_but_not_paths() {
        let next = draw_next(&COLOUR, "0.1.0");
        assert!(
            next.contains("\x1b[1;38;5;201mzaf\x1b[0m is set up here."),
            "{next:?}"
        );
        let fixes = draw_fixes(&COLOUR, &[gh_fix()]);
        assert!(fixes.contains("\x1b[1;38;5;201mzaf\x1b[0m"), "{fixes:?}");
        // The Write row's file is a noun in its own colour, and the program's colour is never in it.
        assert_eq!(COLOUR.program(".zaf.toml"), ".zaf.toml");
        assert!(!draw_next(&COLOUR, "0.1.0").contains("38;5;201m.zaf"));
    }
}
