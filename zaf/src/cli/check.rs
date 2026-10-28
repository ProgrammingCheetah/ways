//! `zaf check` and `zaf verify`: run a target's checks, print the record, store it
//! under `.zaf/runs/` for `--status`, and exit by its verdict.
//!
//! Exit codes: 0 clean, 1 must-fix present (or a task line failed), 2 a step
//! failed or timed out, or a backend is missing.

use std::path::PathBuf;
use std::sync::Arc;

use super::Cli;
use super::exit::{self, BACKEND, REFUSED};
use crate::adapters::gate::RUNS_DIR;
use crate::app::check::{
    CheckRecord, Engine, EngineSettings, Shell, Step, StepPorts, Target, TaskInputs, check_task,
};
use crate::composition::{Composition, CompositionError};
use crate::domain::{IssueNumber, SecurityTrigger, TaskTemplate};
use crate::manifest::{CheckTarget, CommandLine, RunnerName};
use crate::ports::{Forge, GateRun, PortError, PrNumber, Runner};

/// Runs the configured check steps for a target, or reads the last result.
pub fn run(cli: &Cli, target: &str, status: bool) -> u8 {
    match target.parse::<Target>() {
        Ok(target) => check(cli, target, status),
        Err(error) => {
            eprintln!("zaf {}: {error}", cli.verb.path());
            REFUSED
        }
    }
}

/// Checks a task's acceptance lines and declared interfaces (`check <task>` by another name).
pub fn verify(cli: &Cli, task: IssueNumber) -> u8 {
    check(cli, Target::Task(task), false)
}

fn check(cli: &Cli, target: Target, status: bool) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let stored = stored_path(&composition, &target);
    if status {
        return show_stored(cli, &path, &stored);
    }
    let outcome = match target {
        Target::Task(task) => check_task_target(&path, &composition, &target, task),
        Target::Pr { pr, task } => check_pr_target(&path, &composition, &target, pr, task),
    };
    let record = match outcome {
        Ok(record) => record,
        Err(code) => return code,
    };
    if let Err(error) = store(&stored, &record) {
        eprintln!("zaf {path}: the result was not stored for --status: {error}");
    }
    print(cli, &record);
    record.verdict.code()
}

fn check_task_target(
    path: &str,
    composition: &Composition,
    target: &Target,
    task: IssueNumber,
) -> Result<CheckRecord, u8> {
    let template = task_template(path, composition, task)?;
    let gate = match composition.gate() {
        Ok(gate) => Some(gate),
        Err(CompositionError::Absent { .. }) => None,
        Err(error) => return Err(exit::fail_composition(path, &error)),
    };
    let tests = composition
        .manifest()
        .gate
        .as_ref()
        .and_then(|section| section.tests.as_ref());
    let shell = LivePorts {
        composition: composition.clone(),
    };
    let inputs = TaskInputs {
        gate: gate.as_deref(),
        tests,
        shell: &shell,
        root: composition.root(),
    };
    let check = check_task(&template, &inputs);
    Ok(CheckRecord::from_task(target, &check))
}

fn check_pr_target(
    path: &str,
    composition: &Composition,
    target: &Target,
    pr: PrNumber,
    task: Option<IssueNumber>,
) -> Result<CheckRecord, u8> {
    let configured = composition
        .manifest()
        .check
        .get(&CheckTarget::Pr)
        .map(|section| section.steps.as_slice())
        .unwrap_or_default();
    let steps = configured
        .iter()
        .map(|text| text.parse::<Step>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            eprintln!("zaf {path}: [check.pr] {error}");
            BACKEND
        })?;
    // Without the task, a `?flagged` step runs: a security review is never skipped by omission.
    let flagged = match task {
        Some(task) => {
            task_template(path, composition, task)?.security_trigger != SecurityTrigger::No
        }
        None => true,
    };
    let ports: Arc<dyn StepPorts> = Arc::new(LivePorts {
        composition: composition.clone(),
    });
    let settings = EngineSettings::new(composition.root().to_owned(), runs_dir(composition));
    let report = Engine::new(ports, settings).run(&steps, pr, flagged);
    Ok(CheckRecord::from_report(target, &report))
}

fn task_template(
    path: &str,
    composition: &Composition,
    task: IssueNumber,
) -> Result<TaskTemplate, u8> {
    let board = composition
        .board()
        .map_err(|error| exit::fail_composition(path, &error))?;
    let ticket = board.task(task).map_err(|error| exit::fail(path, &error))?;
    ticket.body.parse::<TaskTemplate>().map_err(|error| {
        eprintln!("zaf {path}: {task} is not a task ticket: {error}");
        REFUSED
    })
}

/// The live ports a check reaches, built from the composition inside each step's thread.
struct LivePorts {
    composition: Composition,
}

impl Shell for LivePorts {
    /// Runs the line through the composition's shell, at the project root.
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        self.composition.shell().run(line)
    }
}

impl StepPorts for LivePorts {
    fn forge(&self) -> Result<Box<dyn Forge>, PortError> {
        self.composition.forge().map_err(unavailable)
    }

    fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, PortError> {
        self.composition.runner(name).map_err(unavailable)
    }
}

fn unavailable(error: CompositionError) -> PortError {
    PortError::Unavailable(error.to_string())
}

fn runs_dir(composition: &Composition) -> PathBuf {
    composition.root().join(RUNS_DIR)
}

fn stored_path(composition: &Composition, target: &Target) -> PathBuf {
    runs_dir(composition).join(format!("check-{}.json", target.slug()))
}

fn store(stored: &std::path::Path, record: &CheckRecord) -> std::io::Result<()> {
    if let Some(dir) = stored.parent() {
        std::fs::create_dir_all(dir)?;
        let ignore = dir.join(".gitignore");
        if !ignore.exists() {
            std::fs::write(&ignore, "*\n")?;
        }
    }
    let text = serde_json::to_string_pretty(record).map_err(std::io::Error::other)?;
    std::fs::write(stored, text)
}

fn show_stored(cli: &Cli, path: &str, stored: &std::path::Path) -> u8 {
    let text = match std::fs::read_to_string(stored) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("zaf {path}: no stored result at {}", stored.display());
            return REFUSED;
        }
        Err(error) => {
            eprintln!("zaf {path}: cannot read {}: {error}", stored.display());
            return BACKEND;
        }
    };
    match serde_json::from_str::<CheckRecord>(&text) {
        Ok(record) => {
            print(cli, &record);
            record.verdict.code()
        }
        Err(error) => {
            eprintln!(
                "zaf {path}: {} is not a check result: {error}",
                stored.display()
            );
            BACKEND
        }
    }
}

fn print(cli: &Cli, record: &CheckRecord) {
    if cli.json {
        match serde_json::to_string(record) {
            Ok(text) => println!("{text}"),
            Err(error) => eprintln!("zaf check: cannot print the result as JSON: {error}"),
        }
    } else {
        println!("{record}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(manifest: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("ZAF.toml"), manifest).expect("manifest");
        dir
    }

    const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
        [board]\nkind = \"github-issues\"\nrepo = \"owner/example\"\n\n\
        [gate]\nfull = \"true\"\ntests = \"test {name} = passes\"\n";

    #[test]
    fn the_live_shell_runs_a_line_at_the_project_root() {
        let dir = project(MANIFEST);
        let composition = Composition::load(dir.path()).expect("loads");
        let shell = LivePorts { composition };
        let passed = shell.run(&CommandLine::from("echo hello")).expect("runs");
        assert_eq!(passed.output, "hello\n");
        let failed = shell.run(&CommandLine::from("exit 4")).expect("runs");
        assert_eq!(failed.verdict, crate::ports::GateVerdict::Failed);
        let at_root = shell.run(&CommandLine::from("cat ZAF.toml")).expect("runs");
        assert!(
            at_root.output.starts_with("[project]"),
            "{}",
            at_root.output
        );
    }

    #[test]
    fn an_absent_forge_or_runner_is_unavailable() {
        let dir = project(MANIFEST);
        let composition = Composition::load(dir.path()).expect("loads");
        let ports = LivePorts { composition };
        assert!(matches!(ports.forge(), Err(PortError::Unavailable(_))));
        let missing = ports.runner(&RunnerName::from("local"));
        assert!(matches!(missing, Err(PortError::Unavailable(_))));
    }

    #[test]
    fn a_stored_record_reads_back_with_its_exit_code() {
        let dir = project(MANIFEST);
        let composition = Composition::load(dir.path()).expect("loads");
        let target = Target::Pr {
            pr: PrNumber::from(5),
            task: None,
        };
        let stored = stored_path(&composition, &target);
        let record = CheckRecord {
            target: target.to_string(),
            verdict: crate::app::check::Verdict::MustFix,
            lines: Vec::new(),
        };
        store(&stored, &record).expect("stores");
        assert!(stored.ends_with(".zaf/runs/check-pr-5.json"));
        let cli = <Cli as clap::Parser>::try_parse_from(["zaf", "check", "pr:5", "--status"])
            .expect("parses");
        assert_eq!(show_stored(&cli, "check", &stored), 1);
        let absent = stored.with_file_name("check-pr-6.json");
        assert_eq!(show_stored(&cli, "check", &absent), REFUSED);
    }
}
