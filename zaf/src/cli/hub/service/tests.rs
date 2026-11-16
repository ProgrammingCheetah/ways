use std::cell::RefCell;

use clap::Parser;

use super::*;
use crate::app::hub::service::{UNIT_NAME, unit_path};
use crate::cli::{Cli, HubAction, Verb};

/// A `systemctl` that records what it was asked and fails on the arguments it is told to.
#[derive(Default)]
struct FakeSystemctl {
    calls: RefCell<Vec<String>>,
    fail_on: Option<&'static str>,
    /// The manager knows no such unit: it refuses to stop or disable it.
    unknown: bool,
}

impl Systemctl for FakeSystemctl {
    fn user(&self, args: &[&str]) -> Result<(), String> {
        self.calls.borrow_mut().push(args.join(" "));
        let verb = args.first().copied();
        if self.unknown && matches!(verb, Some("stop" | "disable")) {
            return Err("Unit zaf-hub.service not loaded.".to_owned());
        }
        if self.fail_on == verb {
            return Err("it refused".to_owned());
        }
        Ok(())
    }
}

fn action(args: &[&str]) -> ServiceAction {
    let cli = Cli::try_parse_from(["zaf", "hub", "service"].iter().chain(args)).expect("parses");
    match cli.verb {
        Verb::Hub {
            action: Some(HubAction::Service { action }),
            ..
        } => action,
        other => panic!("not a service verb: {other:?}"),
    }
}

fn host(dir: &tempfile::TempDir, os: &'static str) -> Host {
    Host {
        os,
        unit_dir: Some(dir.path().join("systemd/user")),
        exe: PathBuf::from("/opt/zaf/bin/zaf"),
        project: PathBuf::from("/work/demo"),
        zaf_home: None,
    }
}

struct Run {
    code: u8,
    out: String,
    err: String,
    calls: Vec<String>,
}

fn run_on(args: &[&str], host: &Host, fake: &FakeSystemctl) -> Run {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = run_with(&action(args), "hub service", host, fake, &mut out, &mut err);
    Run {
        code,
        out: String::from_utf8(out).expect("utf8"),
        err: String::from_utf8(err).expect("utf8"),
        calls: fake.calls.borrow().clone(),
    }
}

fn unit_in(host: &Host) -> PathBuf {
    unit_path(host.unit_dir.as_deref().expect("a unit dir"))
}

#[test]
fn install_writes_unit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let host = host(&dir, "linux");
    let run = run_on(
        &["install", "--port", "9100"],
        &host,
        &FakeSystemctl::default(),
    );
    assert_eq!(run.code, 0, "{}", run.err);
    let unit = std::fs::read_to_string(unit_in(&host)).expect("the unit");
    assert!(
        unit.contains("ExecStart=/opt/zaf/bin/zaf hub --port 9100\n"),
        "{unit}"
    );
    assert!(unit.contains("Restart=on-failure\n"), "{unit}");
    assert!(unit.contains("WorkingDirectory=/work/demo\n"), "{unit}");
    assert!(unit.contains("WantedBy=default.target"), "{unit}");

    // With no port the verb's default is named, and a unit already there is replaced.
    let again = run_on(&["install"], &host, &FakeSystemctl::default());
    assert_eq!(again.code, 0, "{}", again.err);
    assert!(again.out.contains("replaced"), "{}", again.out);
    let unit = std::fs::read_to_string(unit_in(&host)).expect("the unit");
    assert!(unit.contains("hub --port 7777\n"), "{unit}");
    assert!(!unit.contains("9100"), "{unit}");
    // The running hub is stopped so the new one takes the new port.
    assert!(again.calls.contains(&format!("stop {UNIT_NAME}")));
}

#[test]
fn install_enables_and_starts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let host = host(&dir, "linux");
    let run = run_on(&["install"], &host, &FakeSystemctl::default());
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        run.calls,
        [
            "daemon-reload".to_owned(),
            format!("enable {UNIT_NAME}"),
            format!("start {UNIT_NAME}"),
        ]
    );

    // A systemctl that refuses is a backend failure, and the start is not tried.
    let other = tempfile::tempdir().expect("tempdir");
    let host = self::host(&other, "linux");
    let fake = FakeSystemctl {
        fail_on: Some("enable"),
        ..FakeSystemctl::default()
    };
    let run = run_on(&["install"], &host, &fake);
    assert_eq!(run.code, 2);
    assert!(run.err.contains("systemctl --user enable"), "{}", run.err);
    assert!(!run.calls.iter().any(|call| call.starts_with("start")));
}

#[test]
fn remove_undoes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let host = host(&dir, "linux");
    run_on(&["install"], &host, &FakeSystemctl::default());
    assert!(unit_in(&host).exists());

    let fake = FakeSystemctl::default();
    let run = run_on(&["remove"], &host, &fake);
    assert_eq!(run.code, 0, "{}", run.err);
    assert_eq!(
        &run.calls[..2],
        [format!("stop {UNIT_NAME}"), format!("disable {UNIT_NAME}")]
    );
    assert!(!unit_in(&host).exists());

    // A stop that fails still deletes the unit, and says so.
    run_on(&["install"], &host, &FakeSystemctl::default());
    let fake = FakeSystemctl {
        fail_on: Some("stop"),
        ..FakeSystemctl::default()
    };
    let run = run_on(&["remove"], &host, &fake);
    assert_eq!(run.code, 2);
    assert!(!unit_in(&host).exists());
}

#[test]
fn remove_when_absent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let host = host(&dir, "linux");
    let fake = FakeSystemctl {
        unknown: true,
        ..FakeSystemctl::default()
    };
    let run = run_on(&["remove"], &host, &fake);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.contains("no hub service installed"), "{}", run.out);
}

#[test]
fn remove_stops_a_unit_whose_file_is_gone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let host = host(&dir, "linux");
    run_on(&["install"], &host, &FakeSystemctl::default());
    // The file was deleted by hand: the hub still runs and the unit is still enabled.
    std::fs::remove_file(unit_in(&host)).expect("deleted");

    let run = run_on(&["remove"], &host, &FakeSystemctl::default());
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.contains("removed"), "{}", run.out);
    assert_eq!(
        run.calls,
        [
            format!("stop {UNIT_NAME}"),
            format!("disable {UNIT_NAME}"),
            "daemon-reload".to_owned(),
        ]
    );
}

#[test]
fn a_replacement_whose_stop_fails_is_no_install() {
    let dir = tempfile::tempdir().expect("tempdir");
    let host = host(&dir, "linux");
    run_on(
        &["install", "--port", "9100"],
        &host,
        &FakeSystemctl::default(),
    );

    let fake = FakeSystemctl {
        fail_on: Some("stop"),
        ..FakeSystemctl::default()
    };
    let run = run_on(&["install"], &host, &fake);
    assert_eq!(run.code, 2);
    assert!(run.err.contains("systemctl --user stop"), "{}", run.err);
    assert!(!run.calls.iter().any(|call| call.starts_with("start")));
}

#[test]
fn not_built_off_linux() {
    let dir = tempfile::tempdir().expect("tempdir");
    let host = host(&dir, "macos");
    for args in [&["install"][..], &["remove"]] {
        let fake = FakeSystemctl::default();
        let run = run_on(args, &host, &fake);
        assert_eq!(run.code, 64, "{args:?}");
        assert!(run.err.contains("not built"), "{}", run.err);
        assert!(run.calls.is_empty());
    }
    assert!(!unit_in(&host).exists());
}

#[test]
fn the_verb_is_named_with_its_action() {
    let cli = Cli::try_parse_from(["zaf", "hub", "service", "remove"]).expect("parses");
    assert_eq!(cli.verb.path(), "hub service remove");
}
