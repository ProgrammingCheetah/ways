use std::path::PathBuf;

use super::*;
use crate::fakes::FakeRunner;

fn request(role: &str) -> RunRequest {
    RunRequest {
        role: RoleName::from(role),
        profile: Profile::Lean,
        prompt: "Read the brief at .zaf/runs/1/brief.md".to_owned(),
        dir: PathBuf::from("."),
        env: std::collections::BTreeMap::new(),
    }
}

fn output(transcript: &str) -> RunOutput {
    RunOutput {
        exit_code: 0,
        transcript: transcript.to_owned(),
    }
}

#[test]
fn recorded_runs_replay_in_order_per_role() {
    let runner = FakeRunner::default();
    runner.record(RoleName::from("keeper"), output("first"));
    runner.record(RoleName::from("keeper"), output("second"));
    let port: &dyn Runner = &runner;
    assert_eq!(port.run(&request("keeper")), Ok(output("first")));
    assert_eq!(port.run(&request("keeper")), Ok(output("second")));
    assert!(matches!(
        port.run(&request("keeper")),
        Err(PortError::Unavailable(_))
    ));
    assert!(matches!(
        port.run(&request("programmer")),
        Err(PortError::Unavailable(_))
    ));
    assert_eq!(runner.requests().len(), 4);
    assert_eq!(runner.requests()[0], request("keeper"));
}

#[test]
fn the_fake_records_the_requests_environment() {
    let runner = FakeRunner::default();
    runner.record(RoleName::from("keeper"), output("ok"));
    let mut sent = request("keeper");
    sent.env
        .insert("ZAF_VERSION".to_owned(), "0.3.0".to_owned());
    let port: &dyn Runner = &runner;
    assert_eq!(port.run(&sent), Ok(output("ok")));
    assert_eq!(runner.requests()[0].env["ZAF_VERSION"], "0.3.0");
}
