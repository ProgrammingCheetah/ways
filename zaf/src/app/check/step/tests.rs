use std::sync::Arc;

use super::*;
use crate::app::check::{Engine, EngineSettings, Shell, StepPorts, Verdict};
use crate::domain::{Location, Severity};
use crate::fakes::FakeForge;
use crate::manifest::RunnerName;
use crate::ports::{
    Author, BaseName, Check, Forge, GateRun, PrMerge, PrRequest, ReviewComment, RuleId, Runner,
    ScanningAlert, SliceName,
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
