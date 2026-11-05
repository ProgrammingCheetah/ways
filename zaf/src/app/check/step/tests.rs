use std::path::Path;
use std::sync::Arc;

use super::*;
use crate::app::check::{Engine, EngineSettings, Shell, StepPorts, Verdict};
use crate::domain::{Location, Severity};
use crate::fakes::{FakeForge, FakeRunner};
use crate::manifest::RunnerName;
use crate::ports::{
    Author, BaseName, Check, Forge, GateRun, GateVerdict, PrMerge, PrRequest, ReviewComment,
    RuleId, RunOutput, RunRequest, Runner, ScanningAlert, SliceName,
};

#[test]
fn the_three_kinds_parse_and_print_back() {
    for text in [
        "ci",
        "command:cargo deny check",
        "runner:local:critique",
        "runner:local:security?flagged",
        "bot:review-bot",
        "bot:review-bot?flagged",
        "codeql",
    ] {
        let step: Step = text.parse().expect(text);
        assert_eq!(step.to_string(), text);
    }
}

#[test]
fn flagged_is_a_suffix_and_stays_out_of_the_source() {
    let step: Step = "runner:local:security?flagged".parse().expect("parses");
    assert!(step.flagged_only);
    assert_eq!(step.source(), FindingSource::from("runner:local:security"));
    let runner_step = RunnerStep {
        runner: RunnerName::from("local"),
        role: RoleName::from("security"),
    };
    assert_eq!(step.kind, StepKind::Runner(runner_step));
    let command: Step = "command: make lint ".parse().expect("parses");
    assert_eq!(
        command.kind,
        StepKind::Command(CommandLine::from("make lint"))
    );
    assert!(!command.flagged_only);
}

#[test]
fn malformed_and_unknown_steps_are_refused() {
    assert_eq!("".parse::<Step>(), Err(StepError::Empty));
    assert_eq!("?flagged".parse::<Step>(), Err(StepError::Empty));
    for text in [
        "command:",
        "bot:",
        "bot: ",
        "bot:a:b",
        "runner:local",
        "runner::critique",
        "runner:a:b:c",
    ] {
        assert_eq!(
            text.parse::<Step>(),
            Err(StepError::MissingPart(text.to_owned())),
            "{text}"
        );
    }
    for text in ["codeql:extra", "ci:extra", "scanner"] {
        assert_eq!(
            text.parse::<Step>(),
            Err(StepError::UnknownKind(text.to_owned())),
            "{text}"
        );
    }
}

#[test]
fn action_step_parses() {
    let step: Step = "action:nodes-check".parse().expect("parses");
    assert_eq!(step.kind, StepKind::Action("nodes-check".to_owned()));
    assert!(!step.flagged_only);
    assert_eq!(step.to_string(), "action:nodes-check");
    assert_eq!(step.source(), FindingSource::from("action:nodes-check"));
    let flagged: Step = "action: nodes-check ?flagged".parse().expect("parses");
    assert_eq!(flagged.kind, StepKind::Action("nodes-check".to_owned()));
    assert!(flagged.flagged_only);
    let command: Step = "command:cargo test".parse().expect("parses");
    assert_eq!(
        command.kind,
        StepKind::Command(CommandLine::from("cargo test"))
    );
}

#[test]
fn bare_action_step_is_an_error() {
    for text in ["action:", "action: ", "action:a:b", "action:?flagged"] {
        assert_eq!(
            text.parse::<Step>(),
            Err(StepError::MissingPart(text.to_owned())),
            "{text}"
        );
    }
    let message = StepError::MissingPart("action:".to_owned()).to_string();
    assert!(message.contains("action:<name>"), "{message}");
    let unknown = "scan:x"
        .parse::<Step>()
        .expect_err("unknown kind")
        .to_string();
    assert!(unknown.contains("action:<name>"), "{unknown}");
}

/// Lends a shared fake forge to a step's thread.
pub(super) struct SharedForge(pub Arc<FakeForge>);

impl Forge for SharedForge {
    fn open_pr(&self, request: &PrRequest) -> Result<PrNumber, PortError> {
        self.0.open_pr(request)
    }

    fn checks(&self, pr: PrNumber) -> Result<Vec<Check>, PortError> {
        self.0.checks(pr)
    }

    fn review_comments(&self, pr: PrNumber) -> Result<Vec<ReviewComment>, PortError> {
        self.0.review_comments(pr)
    }

    fn head(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        self.0.head(pr)
    }

    fn last_changed_file(&self, pr: PrNumber) -> Result<Option<String>, PortError> {
        self.0.last_changed_file(pr)
    }

    fn scanning_alerts(&self, pr: PrNumber) -> Result<Vec<ScanningAlert>, PortError> {
        self.0.scanning_alerts(pr)
    }

    fn merge(&self, pr: PrNumber) -> Result<PrMerge, PortError> {
        self.0.merge(pr)
    }
}

/// The ports of a forge-only step: a shared fake forge, no shell and no runner.
pub(super) struct ForgePorts(pub Arc<FakeForge>);

impl Shell for ForgePorts {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        Err(PortError::Unavailable(format!("no shell for {line}")))
    }
}

impl StepPorts for ForgePorts {
    fn forge(&self) -> Result<Box<dyn Forge>, PortError> {
        Ok(Box::new(SharedForge(Arc::clone(&self.0))))
    }

    fn runner(&self, name: &RunnerName) -> Result<Box<dyn Runner>, PortError> {
        Err(PortError::Unavailable(format!("no runner {name}")))
    }
}

/// A fake forge with one open pull request.
pub(super) fn forge_with_pr() -> (Arc<FakeForge>, PrNumber) {
    let forge = Arc::new(FakeForge::default());
    let request = PrRequest {
        slice: SliceName::from("feature/v1-export-2"),
        base: BaseName::from("feature/v1"),
        title: "Add the CSV writer".to_owned(),
        body: "Closes #2".to_owned(),
    };
    let pr = forge.open_pr(&request).expect("open");
    (forge, pr)
}

/// A step context that gives up `wait` from now and polls every 5 ms.
pub(super) fn context(pr: PrNumber, wait: Duration) -> StepContext {
    StepContext {
        pr,
        root: PathBuf::from("."),
        dir: PathBuf::from("."),
        deadline: Instant::now() + wait,
        poll: Duration::from_millis(5),
        grace: Duration::ZERO,
    }
}

pub(super) fn located(author: &str, body: &str, path: &str, line: u32) -> ReviewComment {
    ReviewComment {
        author: Author::from(author),
        body: body.to_owned(),
        location: Some(Location {
            path: path.to_owned(),
            line: Some(line),
        }),
        head: None,
    }
}

/// A located comment made on a given commit.
pub(super) fn located_on(
    author: &str,
    body: &str,
    path: &str,
    line: u32,
    head: &str,
) -> ReviewComment {
    ReviewComment {
        head: Some(head.to_owned()),
        ..located(author, body, path, line)
    }
}

#[test]
fn briefs_ask_for_reviewed() {
    for brief in [CRITIQUE_BRIEF, SECURITY_BRIEF] {
        assert!(brief.contains("REVIEWED: <the path of the last file in the diff>"));
        assert!(brief.contains("MUST: <path>:<line> <title>"));
        assert!(brief.contains("NOTE: <path>:<line> <title>"));
        assert!(brief.contains("not even a code fence"));
        assert!(!brief.to_lowercase().contains("print nothing"));
    }
}

#[test]
fn a_bot_finding_and_a_scanner_alert_on_the_same_lines_are_one() {
    let (forge, pr) = forge_with_pr();
    let comment = located(
        "review-bot",
        "_Potential issue_ | _Major_\n\n**js/sql-injection**\n\nBuilt from user input.",
        "src/export.rs",
        40,
    );
    forge.add_review_comment(pr, comment).expect("comment");
    let alert = ScanningAlert {
        rule: RuleId::from("js/sql-injection"),
        location: Some(Location {
            path: "src/export.rs".to_owned(),
            line: Some(42),
        }),
    };
    forge.set_scanning_alerts(pr, vec![alert]).expect("alert");
    let steps: Vec<Step> = ["bot:review-bot", "codeql"]
        .iter()
        .map(|text| text.parse().expect("step"))
        .collect();
    let mut settings = EngineSettings::new(PathBuf::from("."), PathBuf::from("."));
    settings.timeout = Duration::from_secs(5);
    settings.poll = Duration::from_millis(5);
    let report = Engine::new(Arc::new(ForgePorts(forge)), settings).run(&steps, pr, false);
    assert_eq!(report.verdict(), Verdict::MustFix);
    assert_eq!(report.findings.len(), 1);
    let kept = &report.findings[0];
    assert_eq!(kept.finding.source, FindingSource::from("bot:review-bot"));
    assert_eq!(kept.finding.severity, Severity::MustFix);
    assert_eq!(kept.also_found_by, [FindingSource::from("codeql")]);
}

/// A shell that prints `output` and ends as `verdict`, beside a forge and a runner
/// that replays recorded runs.
struct LoggingPorts {
    forge: Arc<FakeForge>,
    runner: Arc<FakeRunner>,
    verdict: GateVerdict,
    output: String,
}

impl Shell for LoggingPorts {
    fn run(&self, _line: &CommandLine) -> Result<GateRun, PortError> {
        Ok(GateRun {
            verdict: self.verdict,
            output: self.output.clone(),
        })
    }
}

/// Lends a shared fake runner to a step.
struct SharedRunner(Arc<FakeRunner>);

impl Runner for SharedRunner {
    fn run(&self, request: &RunRequest) -> Result<RunOutput, PortError> {
        self.0.run(request)
    }
}

impl StepPorts for LoggingPorts {
    fn forge(&self) -> Result<Box<dyn Forge>, PortError> {
        Ok(Box::new(SharedForge(Arc::clone(&self.forge))))
    }

    fn runner(&self, _name: &RunnerName) -> Result<Box<dyn Runner>, PortError> {
        Ok(Box::new(SharedRunner(Arc::clone(&self.runner))))
    }
}

fn logging_ports(verdict: GateVerdict, output: &str) -> LoggingPorts {
    LoggingPorts {
        forge: Arc::new(FakeForge::default()),
        runner: Arc::new(FakeRunner::default()),
        verdict,
        output: output.to_owned(),
    }
}

/// Queues one run of the `critique` role that exits `exit_code` and prints `transcript`.
fn record_critique(ports: &LoggingPorts, exit_code: i32, transcript: &str) {
    let output = RunOutput {
        exit_code,
        transcript: transcript.to_owned(),
    };
    ports.runner.record(RoleName::from("critique"), output);
}

/// A step context rooted in `root`, the project the logs go under.
fn context_in(root: &Path, pr: PrNumber) -> StepContext {
    StepContext {
        root: root.to_owned(),
        dir: root.join(".zaf/runs/step"),
        ..context(pr, Duration::from_secs(5))
    }
}

/// The files of the project's log folder, by name.
fn logs_of(root: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<PathBuf> = std::fs::read_dir(root.join(".zaf/logs"))
        .expect("the log folder exists")
        .map(|entry| entry.expect("entry").path())
        .collect();
    logs.sort();
    logs
}

/// Whether `name` is `<YYYYMMDDTHHMMSSZ>_<stem>.log`.
fn is_stamped(name: &str, stem: &str) -> bool {
    let Some((stamp, rest)) = name.split_once('_') else {
        return false;
    };
    let shape: Vec<bool> = stamp
        .chars()
        .map(|c| c.is_ascii_digit() || c == 'T' || c == 'Z')
        .collect();
    stamp.len() == 16
        && shape.iter().all(|ok| *ok)
        && stamp.as_bytes()[8] == b'T'
        && stamp.as_bytes()[15] == b'Z'
        && rest == format!("{stem}.log")
}

fn step_of(text: &str) -> Step {
    text.parse().expect("step parses")
}

#[test]
fn step_output_is_kept_in_a_timestamped_log() {
    let report =
        "Looked at the diff.\nNOTE: src/export.rs:30 name the constant\nREVIEWED: src/export.rs\n";
    let cases = [
        (
            "command:make lint",
            "lint: 3 files\nall fine\n",
            "command-make-lint",
        ),
        ("runner:local:critique", report, "runner-local-critique"),
    ];
    for (text, output, stem) in cases {
        let root = tempfile::tempdir().expect("temp dir");
        let ports = logging_ports(GateVerdict::Passed, output);
        record_critique(&ports, 0, output);
        let request = PrRequest {
            slice: SliceName::from("feature/v1-export-2"),
            base: BaseName::from("feature/v1"),
            title: "Add the CSV writer".to_owned(),
            body: String::new(),
        };
        let pr = ports.forge.open_pr(&request).expect("open");
        ports
            .forge
            .set_changed_files(pr, &["src/export.rs"])
            .expect("files");
        run(&step_of(text), &ports, &context_in(root.path(), pr)).expect(text);
        let logs = logs_of(root.path());
        assert_eq!(logs.len(), 1, "{text}: one log");
        let name = logs[0].file_name().expect("name").to_string_lossy();
        assert!(is_stamped(&name, stem), "{text}: {name}");
        assert_eq!(std::fs::read_to_string(&logs[0]).expect("reads"), output);
    }
}

#[test]
fn a_failed_command_keeps_its_whole_output_too() {
    let (_, pr) = forge_with_pr();
    let root = tempfile::tempdir().expect("temp dir");
    let output = "compiling\nerror: boom\nsecond line\n";
    let ports = logging_ports(GateVerdict::Failed, output);
    run(
        &step_of("command:make lint"),
        &ports,
        &context_in(root.path(), pr),
    )
    .expect("a failed command still ran");
    let logs = logs_of(root.path());
    assert_eq!(logs.len(), 1);
    assert_eq!(std::fs::read_to_string(&logs[0]).expect("reads"), output);
}

#[test]
fn failed_step_names_its_log() {
    let (_, pr) = forge_with_pr();

    let root = tempfile::tempdir().expect("temp dir");
    let ports = logging_ports(GateVerdict::Failed, "compiling\nerror: boom\nsecond line\n");
    let step = step_of("command:make lint");
    let findings = run(&step, &ports, &context_in(root.path(), pr)).expect("ran");
    let log = logs_of(root.path()).remove(0);
    assert_eq!(findings.len(), 1);
    let message = &findings[0].message;
    assert!(message.contains(&log.display().to_string()), "{message}");
    assert!(
        !message.contains("boom") && !message.contains("second line"),
        "{message}"
    );

    let root = tempfile::tempdir().expect("temp dir");
    let transcript = "Looked.\nMUST: src/export.rs:12 the empty list is dropped\n";
    let ports = logging_ports(GateVerdict::Passed, "");
    record_critique(&ports, 0, transcript);
    let step = step_of("runner:local:critique");
    let error = run(&step, &ports, &context_in(root.path(), pr)).expect_err("no REVIEWED line");
    let log = logs_of(root.path()).remove(0);
    let why = error.to_string();
    assert!(why.contains(&log.display().to_string()), "{why}");
    assert!(
        !why.contains("the empty list"),
        "the reason stays short: {why}"
    );
    assert_eq!(std::fs::read_to_string(&log).expect("reads"), transcript);
}
