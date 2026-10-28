use super::*;

#[test]
fn the_three_kinds_parse_and_print_back() {
    for text in [
        "ci",
        "command:cargo deny check",
        "runner:local:critique",
        "runner:local:security?flagged",
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
    for text in ["bot:reviewer", "codeql", "ci:extra"] {
        assert_eq!(
            text.parse::<Step>(),
            Err(StepError::UnknownKind(text.to_owned())),
            "{text}"
        );
    }
}
