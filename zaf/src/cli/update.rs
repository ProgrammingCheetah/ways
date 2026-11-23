//! `zaf update`: on a terminal, the plan, one question and a line per step; with `--check` say
//! whether a newer release is out.
//!
//! The newest release is read from GitHub's public API over `curl` each time; nothing is stored.
//! `--check` and `--json` keep their old words: up to date prints nothing and exits 0, an update
//! prints one line, `zaf <old> → <new> (<path>)`, a check that finds a newer release prints it and
//! exits 1, and a failure leaves the old binary in place.
//!
//! Otherwise the plan is worked out first and changes nothing ([`crate::app::update::plan`]), shown
//! with what is new, and `Continue? [Y/n]` is asked once. Then each step runs on its line (the
//! binary, the teams plugin, the machine's actions, the project's pack), and one that fails does not
//! stop the others. With no terminal, or `ZAF_INSTANCE` set, the plan is printed with a line saying
//! to run `zaf update` in a terminal, nothing is updated and the exit is 0.
//!
//! `--yes` runs that plan with [`update::Assumed`]: it needs no terminal, asks nothing, and ends on
//! one line, `zaf 0.5.4 → 0.5.5; replaced: plugin teams 2.3.0 → 2.4.0, actions ci, merge; kept:
//! action fix (yours); pack line raised`. A step that failed is named on it (`failed: plugin (...)`)
//! and the exit is 2. With `ZAF_INSTANCE` set it changes nothing, says so, and exits 0.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use semver::Version;

use super::Cli;
use super::brief::INSTANCE_VAR;
use super::do_action::this_terminal;
use super::doctor::ShellCommands;
use super::exit::{BACKEND, OK, REFUSED};
use super::screen::{DefaultAnswer, Screen, ask_yes};
use super::steps::ScreenSteps;
use crate::adapters::github::PublicReleases;
use crate::adapters::shell::SystemShell;
use crate::app::actions::Terminal;
use crate::app::doctor::Commands;
use crate::app::profile;
use crate::app::setup::{REPLACED_DIR, ReplaceAction, install_machine_actions};
use crate::app::steps::StepReport;
use crate::app::update::{
    self, ActionDiff, HOME_REPO, Outcome, PackChange, Places, Plan, Step, StepOutcome,
    UpdateAnswers, UpdateError,
};
use crate::composition::{Composition, zaf_home};
use crate::domain::screen::{Meaning, PlanRow, StepEnd, Text};
use crate::domain::without_deleted;
use crate::manifest::{Manifest, RepoName};
use crate::ports::{Releases, Shell};

/// The seconds a `claude` or `git` line may take.
const COMMAND_SECONDS: u64 = 300;

/// Runs the verb.
pub fn run(cli: &Cli, check: bool, yes: bool) -> u8 {
    if yes && cli.json {
        crate::eprintln_zaf!("zaf update: --yes has no --json form");
        return BACKEND;
    }
    // Over curl on GitHub's public API: no `gh` needed.
    let releases = PublicReleases::from_env(RepoName::from(HOME_REPO));
    // The file to replace is the image's own, links followed, not the path it was started from;
    // one already replaced since it started is the file now at that path, not ` (deleted)`.
    let exe = match std::env::current_exe().map(without_deleted) {
        Ok(exe) => exe,
        Err(error) => {
            crate::eprintln_zaf!("zaf update: cannot find this binary: {error}");
            return BACKEND;
        }
    };
    if check || cli.json {
        return run_with(cli, check, &releases, &exe, &running_version());
    }
    run_on_this_machine(&releases, &exe, yes)
}

/// The plain verb on this machine: its terminal, its homes and the project it stands in, if any.
/// With `yes` no terminal is needed and the answers are assumed.
fn run_on_this_machine(releases: &dyn Releases, exe: &Path, yes: bool) -> u8 {
    let zaf_home = zaf_home();
    let claude_home = profile::claude_home();
    // A folder with no manifest, or one that does not load, is no project: only its pack is left out.
    let composition = Composition::discover_here_quiet().ok();
    let here = composition
        .as_ref()
        .map(|found| found.root().to_owned())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| exe.to_owned());
    let timeout = Duration::from_secs(COMMAND_SECONDS);
    let commands = ShellCommands::new(Box::new(SystemShell::new(&here, timeout)));
    let project_shell = composition.as_ref().map(Composition::shell);
    let machine = Machine {
        current: running_version(),
        exe,
        target: update::target(),
        claude_home: claude_home.as_deref(),
        zaf_home: zaf_home.as_deref(),
        project: composition
            .as_ref()
            .map(|found| (found.root(), found.manifest())),
        commands: &commands,
        shell: project_shell.as_deref(),
        interactive: !as_instance() && (yes || this_terminal() == Terminal::Present),
        now: SystemTime::now(),
    };
    if yes {
        return run_yes(
            &machine,
            releases,
            Screen::detect(),
            Out::new(Box::new(io::stdout())),
        );
    }
    run_plan(
        &machine,
        releases,
        Screen::detect(),
        &mut io::stdin().lock(),
        Out::new(Box::new(io::stdout())),
    )
}

/// Whether this run is an instance's: `ZAF_INSTANCE` set and not blank.
fn as_instance() -> bool {
    std::env::var_os(INSTANCE_VAR).is_some_and(|name| !name.to_string_lossy().trim().is_empty())
}

/// The verb over the given releases and binary path, for a zaf running at `current`.
pub fn run_with(
    cli: &Cli,
    check: bool,
    releases: &dyn Releases,
    exe: &std::path::Path,
    current: &Version,
) -> u8 {
    let outcome = if check {
        update::check(releases, current)
    } else {
        update::update(releases, current, exe, update::target())
    };
    match outcome {
        Ok(outcome) => report(cli, &outcome),
        Err(error) => failed(&error),
    }
}

/// Says why the update could not go on and gives the exit code of that.
fn failed(error: &UpdateError) -> u8 {
    crate::eprintln_zaf!("zaf update: {error}");
    match error {
        UpdateError::NoRelease => REFUSED,
        UpdateError::Port(_) | UpdateError::NoArchive { .. } | UpdateError::Install { .. } => {
            BACKEND
        }
    }
}

/// This binary's version.
pub fn running_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0))
}

fn report(cli: &Cli, outcome: &Outcome) -> u8 {
    if cli.json {
        let json = match outcome {
            Outcome::Current { version } => serde_json::json!({
                "current": version.to_string(), "newest": version.to_string(), "updated": false,
            }),
            Outcome::Behind { current, newest } => serde_json::json!({
                "current": current.to_string(), "newest": newest.to_string(), "updated": false,
            }),
            Outcome::Updated { from, to, path } => serde_json::json!({
                "current": to.to_string(), "from": from.to_string(), "newest": to.to_string(),
                "updated": true, "path": path.display().to_string(),
            }),
        };
        println!("{json}");
    }
    match outcome {
        Outcome::Current { .. } => OK,
        Outcome::Behind { current, newest } => {
            if !cli.json {
                println!("zaf {current}, newest {newest}: run zaf update");
            }
            REFUSED
        }
        Outcome::Updated { from, to, path } => {
            if !cli.json {
                println!("zaf {from} → {to} ({})", path.display());
            }
            OK
        }
    }
}

/// A writer the screen's step lines and this verb's own prints share, so they come in order.
#[derive(Clone)]
pub struct Out(Arc<Mutex<Box<dyn Write + Send>>>);

impl Out {
    /// Shares `writer`.
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self(Arc::new(Mutex::new(writer)))
    }
}

impl Write for Out {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let mut writer = self.0.lock().unwrap_or_else(|e| e.into_inner());
        writer.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut writer = self.0.lock().unwrap_or_else(|e| e.into_inner());
        writer.flush()
    }
}

/// What the plain `zaf update` needs from the machine it runs on.
pub struct Machine<'a> {
    /// The running version.
    pub current: Version,
    /// The binary to replace.
    pub exe: &'a Path,
    /// The release archive's target, none where zaf publishes none.
    pub target: Option<&'a str>,
    /// The Engineer's Claude home.
    pub claude_home: Option<&'a Path>,
    /// The zaf home.
    pub zaf_home: Option<&'a Path>,
    /// The project's root and manifest, when the verb runs inside one.
    pub project: Option<(&'a Path, &'a Manifest)>,
    /// Runs `claude` and `git`.
    pub commands: &'a dyn Commands,
    /// Runs the profile generation of the pack step, inside a project.
    pub shell: Option<&'a dyn Shell>,
    /// Whether someone is there to answer: a terminal, outside an instance.
    pub interactive: bool,
    /// The time the old copies of replaced actions are named by.
    pub now: SystemTime,
}

/// The plan, the question, the steps and the close, as the mock draws them. Nothing is changed before
/// the yes; without `machine.interactive` nothing is asked and nothing is changed at all.
pub fn run_plan<R: BufRead>(
    machine: &Machine<'_>,
    releases: &dyn Releases,
    screen: Screen,
    input: &mut R,
    out: Out,
) -> u8 {
    let root = machine.project.map(|(root, _)| root);
    let mut asked = Asked::new(&screen, input, out.clone(), machine.zaf_home, root);
    run_plan_with(machine, releases, screen, &mut asked, out)
}

/// [`run_plan`] with its yes-or-no answers from `answers`: [`Asked`] on a terminal, or
/// [`update::Assumed`] where no one is asked.
pub fn run_plan_with(
    machine: &Machine<'_>,
    releases: &dyn Releases,
    screen: Screen,
    answers: &mut dyn UpdateAnswers,
    out: Out,
) -> u8 {
    run_core(machine, releases, screen, answers, out, false)
}

/// `zaf update --yes`: the plan with [`update::Assumed`], ending on one line (see the module's
/// words). Nothing is asked, so `machine.interactive` is only false inside an instance.
pub fn run_yes(machine: &Machine<'_>, releases: &dyn Releases, screen: Screen, out: Out) -> u8 {
    run_core(
        machine,
        releases,
        screen,
        &mut update::Assumed::default(),
        out,
        true,
    )
}

/// What the steps did, for the last line of `--yes`.
#[derive(Default)]
struct Tally {
    /// The binary was replaced.
    binary: bool,
    /// The plugin step did its work (not left as it is).
    plugin: bool,
    /// The actions replaced or new on the machine, sorted.
    actions: Vec<String>,
    /// The actions kept as the Engineer has them, in the order met.
    kept: Vec<String>,
    /// The pack line was raised.
    pack: bool,
    /// Each step that failed: its name and its words.
    failed: Vec<(&'static str, String)>,
}

/// The one line `--yes` ends on.
fn closing_line(plan: &Plan, tally: &Tally) -> String {
    let mut line = if tally.binary {
        format!("zaf {} → {}", plan.current, plan.newest)
    } else {
        format!("zaf {}", plan.current)
    };
    let mut replaced = Vec::new();
    if let (true, Some(row)) = (tally.plugin, &plan.plugin) {
        replaced.push(match &row.installed {
            Some(installed) => format!("plugin teams {installed} → {}", row.to),
            None => format!("plugin teams {}", row.to),
        });
    }
    if !tally.actions.is_empty() {
        replaced.push(named("action", &tally.actions));
    }
    if !replaced.is_empty() {
        line.push_str(&format!("; replaced: {}", replaced.join(", ")));
    }
    if !tally.kept.is_empty() {
        line.push_str(&format!("; kept: {} (yours)", named("action", &tally.kept)));
    }
    if tally.pack {
        line.push_str("; pack line raised");
    }
    if !tally.failed.is_empty() {
        let failed: Vec<String> = tally
            .failed
            .iter()
            .map(|(step, said)| {
                let said = said.split_whitespace().collect::<Vec<_>>().join(" ");
                format!("{step} ({said})")
            })
            .collect();
        line.push_str(&format!("; failed: {}", failed.join(", ")));
    }
    line
}

/// `noun` and the names, the noun in the plural for more than one.
fn named(noun: &str, names: &[String]) -> String {
    let plural = if names.len() > 1 { "s" } else { "" };
    format!("{noun}{plural} {}", names.join(", "))
}

fn run_core(
    machine: &Machine<'_>,
    releases: &dyn Releases,
    screen: Screen,
    answers: &mut dyn UpdateAnswers,
    mut out: Out,
    yes: bool,
) -> u8 {
    let places = Places {
        exe: machine.exe,
        claude_home: machine.claude_home,
        zaf_home: machine.zaf_home,
        project: machine.project,
    };
    let plan = match update::plan(releases, &machine.current, machine.target, &places) {
        Ok(plan) => plan,
        Err(error) => return failed(&error),
    };
    answers.planned(&plan);
    let rows = plan.rows();
    if rows.is_empty() {
        let _ = writeln!(out, "{}", screen.program("zaf is up to date."));
        return OK;
    }
    let _ = write!(out, "{}", draw_plan(&screen, &plan, &rows));
    if !machine.interactive {
        let said = if yes {
            "Nothing is changed here: zaf update --yes does not run inside an instance."
        } else {
            "Nothing is changed here: run zaf update in a terminal to do this."
        };
        let _ = writeln!(out, "{}\n", screen.paint(Meaning::Dim, said));
        return OK;
    }
    if !answers.proceed() {
        let _ = writeln!(out, "\nNothing changed.\n");
        return OK;
    }
    let _ = writeln!(out);
    let shown: Vec<PlanRow> = rows.iter().map(|(_, row)| row.clone()).collect();
    let mut report = ScreenSteps::new(screen, shown, out.clone());
    let mut tally = Tally::default();
    for (at, (step, _)) in rows.iter().enumerate() {
        let end = match step {
            Step::Binary => {
                report.started(at);
                binary_step(machine, releases, &mut tally.binary)
            }
            Step::Plugin => {
                report.started(at);
                match update::update_plugin(machine.commands) {
                    StepOutcome::Done(text) => {
                        tally.plugin = true;
                        StepEnd::Done(Text::from(text))
                    }
                    StepOutcome::Skipped(text) => StepEnd::Done(Text::from(text)),
                    StepOutcome::Failed(text) => StepEnd::Failed(Text::from(text)),
                }
            }
            // The questions of these two draw on the lines below the plan, so no spinner runs.
            Step::Actions => actions_step(machine, answers, &out, &mut tally),
            Step::Pack => match &plan.pack {
                Some(change) => pack_step(machine, change, answers, &mut tally.pack),
                None => StepEnd::Done(Text::from("nothing to change")),
            },
        };
        if let StepEnd::Failed(said) = &end {
            let name = match step {
                Step::Binary => "binary",
                Step::Plugin => "plugin",
                Step::Actions => "actions",
                Step::Pack => "pack",
            };
            tally.failed.push((name, said.as_str().to_owned()));
        }
        report.ended(at, &end);
    }
    drop(report);
    if yes {
        let _ = writeln!(out, "\n{}", closing_line(&plan, &tally));
        return if tally.failed.is_empty() { OK } else { BACKEND };
    }
    let (failures, kept, replaced) = (tally.failed.len(), tally.kept.len(), tally.binary);
    if failures > 0 {
        let _ = writeln!(
            out,
            "\n{} {failures} step(s) failed; {} runs them again.\n",
            screen.paint(Meaning::Failed, "✗"),
            screen.paint(Meaning::Verb, "zaf update"),
        );
        return BACKEND;
    }
    let _ = write!(
        out,
        "\n{} {}",
        screen.paint(Meaning::Done, "✓"),
        screen.program("zaf is up to date.")
    );
    if kept > 0 {
        let _ = write!(
            out,
            " {}{}{}",
            screen.paint(Meaning::Dim, &format!("{kept} action(s) kept as yours; ")),
            screen.paint(Meaning::Verb, "zaf update"),
            screen.paint(Meaning::Dim, " shows them again."),
        );
    }
    let _ = writeln!(out);
    if replaced {
        let _ = writeln!(
            out,
            "{}",
            screen.paint(
                Meaning::Dim,
                &format!(
                    "Running instances finish on the old version; new ones start on {}.",
                    plan.newest
                )
            )
        );
    }
    let _ = writeln!(out);
    OK
}

/// The binary step: the sum, `--version` and the rename of [`update::update`].
fn binary_step(machine: &Machine<'_>, releases: &dyn Releases, replaced: &mut bool) -> StepEnd {
    match update::update(releases, &machine.current, machine.exe, machine.target) {
        Ok(Outcome::Updated { to, .. }) => {
            *replaced = true;
            StepEnd::Done(Text::from(format!("zaf {to}")))
        }
        Ok(
            Outcome::Current { version }
            | Outcome::Behind {
                current: version, ..
            },
        ) => StepEnd::Done(Text::from(format!("zaf {version}"))),
        Err(error) => StepEnd::Failed(Text::from(error.to_string())),
    }
}

/// The actions step: the installed plugin's actions into the zaf home, each differing one asked.
fn actions_step(
    machine: &Machine<'_>,
    answers: &mut dyn UpdateAnswers,
    out: &Out,
    tally: &mut Tally,
) -> StepEnd {
    let (Some(home), Some(claude_home)) = (machine.zaf_home, machine.claude_home) else {
        return StepEnd::Failed(Text::from("no home to read or write the actions in"));
    };
    let Some(folder) = update::pack_actions_dir(claude_home) else {
        return StepEnd::Done(Text::from(
            "the plugin is not installed; no actions to bring",
        ));
    };
    let mut asker = ActionAsker {
        answers,
        declined: Vec::new(),
    };
    let result = install_machine_actions(
        home,
        &folder,
        machine.commands,
        Some(&mut asker as &mut dyn ReplaceAction),
        machine.now,
    );
    tally.kept.append(&mut asker.declined);
    match result {
        Ok(done) => {
            tally.kept.extend(done.unshown.iter().cloned());
            tally.actions = done
                .replaced
                .iter()
                .map(|(name, _)| name.clone())
                .chain(done.installed.iter().cloned())
                .collect();
            tally.actions.sort();
            tally.actions.dedup();
            StepEnd::Done(Text::from(update::actions_summary(&done)))
        }
        Err(failure) => {
            let mut out = out.clone();
            for line in failure.lines() {
                let _ = writeln!(out, "  {line}");
            }
            StepEnd::Failed(Text::from(failure.error.to_string()))
        }
    }
}

/// The pack step: asks `answers` whether its line is raised, then the line rewritten and the
/// profiles made again.
fn pack_step(
    machine: &Machine<'_>,
    change: &PackChange,
    answers: &mut dyn UpdateAnswers,
    raised: &mut bool,
) -> StepEnd {
    let (Some((root, _)), Some(shell)) = (machine.project, machine.shell) else {
        return StepEnd::Failed(Text::from("no project to raise the pack of"));
    };
    if answers.raise_pack(change) {
        match update::apply_pack(root, change, shell) {
            StepOutcome::Done(text) => {
                *raised = true;
                StepEnd::Done(Text::from(text))
            }
            StepOutcome::Skipped(text) => StepEnd::Done(Text::from(text)),
            StepOutcome::Failed(text) => StepEnd::Failed(Text::from(text)),
        }
    } else {
        StepEnd::Done(Text::from("pack left as it is"))
    }
}

/// Carries the installer's `replace` questions to `answers`, counting the ones answered no.
struct ActionAsker<'a> {
    answers: &'a mut dyn UpdateAnswers,
    /// The names kept, in the order asked.
    declined: Vec<String>,
}

impl ReplaceAction for ActionAsker<'_> {
    fn replace(&mut self, name: &str, why: Option<&Text>, difference: &str) -> bool {
        let diff = ActionDiff {
            name: name.to_owned(),
            why: why.cloned(),
            difference: difference.to_owned(),
        };
        let yes = self.answers.replace(&diff);
        if !yes {
            self.declined.push(name.to_owned());
        }
        yes
    }

    /// `zaf update` does not touch the registration in the Engineer's Claude.
    fn register_mcp(&mut self, _name: &str, _difference: &str) -> bool {
        false
    }
}

/// The terminal's answers, as 0.5.4 asked them: `Continue?`, for each machine action that differs
/// why, the path, the coloured difference and `Replace it?`, and the pack's line and `Raise it?`.
pub struct Asked<'a, R: BufRead> {
    screen: &'a Screen,
    input: &'a mut R,
    out: Out,
    /// Where the old copies go, as shown.
    replaced_in: String,
    /// The machine's `actions/` folder, as shown.
    shown_in: String,
    /// The project's root, which the pack's file is shown from.
    root: Option<PathBuf>,
}

impl<'a, R: BufRead> Asked<'a, R> {
    /// Asks on `input` and draws on `out`; `zaf_home` is where the actions and their old copies are,
    /// `root` the project the pack belongs to.
    pub fn new(
        screen: &'a Screen,
        input: &'a mut R,
        out: Out,
        zaf_home: Option<&Path>,
        root: Option<&Path>,
    ) -> Self {
        let home = zaf_home.map(update::tilde).unwrap_or_default();
        Self {
            screen,
            input,
            out,
            replaced_in: format!("{home}/{REPLACED_DIR}"),
            shown_in: format!("{home}/actions"),
            root: root.map(Path::to_owned),
        }
    }
}

impl<R: BufRead> UpdateAnswers for Asked<'_, R> {
    fn proceed(&mut self) -> bool {
        let prompt = self.screen.paint(Meaning::Bold, "Continue?");
        ask_yes(
            self.screen,
            &prompt,
            DefaultAnswer::Yes,
            self.input,
            &mut self.out,
        )
    }

    fn replace(&mut self, diff: &ActionDiff) -> bool {
        let screen = self.screen;
        let name = &diff.name;
        let reason = match &diff.why {
            Some(why) => format!("Why: {why}"),
            None => "Why: not said; see the difference".to_owned(),
        };
        let _ = writeln!(
            self.out,
            "\n  {} {}\n  {}",
            screen.paint(Meaning::Verb, name),
            screen.paint(Meaning::Dim, &format!("will be replaced. {reason}")),
            screen.paint(Meaning::Dim, &format!("{}/{name}", self.shown_in)),
        );
        let _ = write!(self.out, "{}", draw_difference(screen, &diff.difference));
        let prompt = format!(
            "  {} The old one is kept in {}.",
            screen.paint(Meaning::Bold, "Replace it?"),
            self.replaced_in
        );
        let yes = ask_yes(
            screen,
            &prompt,
            DefaultAnswer::Yes,
            self.input,
            &mut self.out,
        );
        if yes {
            let _ = writeln!(
                self.out,
                "  {} {name} replaced",
                screen.paint(Meaning::Done, "✓")
            );
        } else {
            let _ = writeln!(self.out, "  {}", screen.paint(Meaning::Dim, "kept yours"));
        }
        yes
    }

    fn raise_pack(&mut self, change: &PackChange) -> bool {
        let screen = self.screen;
        let file = match &self.root {
            Some(root) => change.file.strip_prefix(root).unwrap_or(&change.file),
            None => &change.file,
        };
        let _ = writeln!(
            self.out,
            "\n  {} {}",
            screen.paint(Meaning::Verb, "pack"),
            screen.paint(
                Meaning::Dim,
                &format!("will be raised in {}.", file.display())
            )
        );
        let _ = writeln!(
            self.out,
            "  {}\n  {}",
            screen.paint(Meaning::Failed, &format!("-{}", change.old)),
            screen.paint(Meaning::Done, &format!("+{}", change.new)),
        );
        let prompt = format!(
            "  {} Its profiles are made again.",
            screen.paint(Meaning::Bold, "Raise it?")
        );
        let yes = ask_yes(
            screen,
            &prompt,
            DefaultAnswer::Yes,
            self.input,
            &mut self.out,
        );
        if !yes {
            let _ = writeln!(self.out, "  {}", screen.paint(Meaning::Dim, "kept yours"));
        }
        yes
    }
}

/// The hunks of a `git diff`, from the first `@@` on (the whole of it, less the file headers, when it
/// has none), added lines in the colour of done and removed ones in the colour of failed.
fn draw_difference(screen: &Screen, difference: &str) -> String {
    let lines: Vec<&str> = difference.lines().collect();
    let hunks = lines.iter().position(|line| line.starts_with("@@"));
    let mut out = String::new();
    for line in &lines[hunks.unwrap_or(0)..] {
        if hunks.is_none() && (line.starts_with("diff --git") || line.starts_with("index ")) {
            continue;
        }
        let painted = if line.starts_with("@@") {
            screen.paint(Meaning::Dim, line)
        } else if line.starts_with('+') {
            screen.paint(Meaning::Done, line)
        } else if line.starts_with('-') {
            screen.paint(Meaning::Failed, line)
        } else {
            (*line).to_owned()
        };
        out.push_str(&format!("  {painted}\n"));
    }
    out
}

/// The plan as the mock draws it: the heading, a numbered row each (verb padded to 8, noun to 30,
/// why), then what is new and the link to all of it.
fn draw_plan(screen: &Screen, plan: &Plan, rows: &[(Step, PlanRow)]) -> String {
    let things = match rows.len() {
        1 => "1 thing".to_owned(),
        n => format!("{n} things"),
    };
    let heading = if plan.binary.is_some() || plan.plugin.is_some() {
        "Newer versions are out."
    } else {
        "Some of what zaf set up is behind."
    };
    let mut out = format!(
        "\n{} {heading} This will do {}:\n\n",
        screen.program("zaf update:"),
        screen.paint(Meaning::Bold, &things),
    );
    for (at, (_, row)) in rows.iter().enumerate() {
        out.push_str(&format!(
            "  {} {} {} {}\n",
            screen.paint(Meaning::Dim, &format!("{}.", at + 1)),
            screen.paint(Meaning::Verb, &format!("{:<8}", row.verb.as_str())),
            screen.paint(Meaning::Noun, &format!("{:<30}", row.noun.as_str())),
            screen.paint(Meaning::Dim, row.why.as_str()),
        ));
    }
    if plan.binary.is_some() {
        let lines: Vec<&Text> = plan.news.iter().flat_map(|news| &news.lines).collect();
        out.push('\n');
        if !lines.is_empty() {
            out.push_str(&format!("{}\n", screen.paint(Meaning::Bold, "What is new")));
            for line in lines {
                out.push_str(&format!(
                    "  {} {}\n",
                    screen.paint(Meaning::Verb, "•"),
                    screen.program(line.as_str())
                ));
            }
        }
        let link = format!(
            "All of it: https://github.com/{HOME_REPO}/releases/tag/{}",
            plan.tag
        );
        out.push_str(&format!("  {}\n", screen.paint(Meaning::Dim, &link)));
    }
    out.push('\n');
    out
}

#[cfg(test)]
mod tests;
