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
        holder_file: None,
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

fn instance_of(root: &str, slug: &str, memory: u64) -> MachineInstance {
    MachineInstance {
        run: PathBuf::from(root).join(".zaf/runs").join(slug),
        root: PathBuf::from(root),
        instance: None,
        size: None,
        pids: vec![1],
        memory: Mebibytes::new(memory),
    }
}

#[test]
fn an_e_is_live_when_an_instance_carries_size_e() {
    let sized = |size| MachineInstance {
        size,
        ..instance_of("/work/bookshelf", "team-instance-1-on-2", 512)
    };
    assert!(!e_is_live(&[]));
    assert!(!e_is_live(&[sized(None), sized(Some(TaskSize::XL))]));
    assert!(e_is_live(&[
        sized(Some(TaskSize::S)),
        sized(Some(TaskSize::E))
    ]));
}

#[test]
fn live_use_sums_the_machine() {
    let machine = [
        instance_of("/work/bookshelf", "team-instance-1-on-2", 512),
        instance_of("/work/bookshelf", "team-instance-2-on-3", 300),
        instance_of("/work/ledger", "planning-instance-4-on-9", 1024),
    ];

    let in_use = live_use(&machine);

    assert_eq!(
        in_use.mebibytes(),
        Mebibytes::new(1836),
        "every instance counts, from any project"
    );
    assert_eq!(in_use.to_string(), "1.8");
    assert_eq!(
        live_use(&[]),
        Units::default(),
        "an empty machine uses nothing"
    );
}
