use std::net::{IpAddr, Ipv4Addr};

use super::*;
use crate::app::doctor::fake::FakeCommands;

const ADDRESS: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5));

fn holder() -> Holder {
    Holder {
        pid: 10,
        start: "100".to_owned(),
    }
}

fn unit(load: &str, active: &str) -> FakeCommands {
    FakeCommands::new().with(
        "systemctl",
        &format!("LoadState={load}\nActiveState={active}\n"),
    )
}

#[test]
fn hub_running() {
    let home = tempfile::tempdir().expect("home");
    HubLock::take(home.path(), &holder(), 8080, &|_| true).expect("lock");
    let commands = unit("loaded", "active");
    let check = check(Some(home.path()), &commands, ADDRESS, &|_| true);
    assert_eq!(
        check,
        HubCheck::Running {
            pid: 10,
            port: 8080,
            url: "http://192.168.1.5:8080/".to_owned()
        }
    );
    assert!(check.line().contains("http://192.168.1.5:8080/"));
    assert_eq!(
        commands.calls(),
        [[
            "systemctl",
            "--user",
            "show",
            HUB_UNIT,
            "--property=LoadState,ActiveState"
        ]]
    );
}

#[test]
fn hub_active_without_a_live_lock() {
    let home = tempfile::tempdir().expect("home");
    let none = check(
        Some(home.path()),
        &unit("loaded", "active"),
        ADDRESS,
        &|_| true,
    );
    assert_eq!(none, HubCheck::NoLock);
    HubLock::take(home.path(), &holder(), 8080, &|_| true).expect("lock");
    let dead = check(
        Some(home.path()),
        &unit("loaded", "active"),
        ADDRESS,
        &|_| false,
    );
    assert_eq!(dead, HubCheck::NoLock);
}

#[test]
fn hub_stopped() {
    let home = tempfile::tempdir().expect("home");
    // A live lock of a hand-run hub does not make a stopped unit run.
    HubLock::take(home.path(), &holder(), 8080, &|_| true).expect("lock");
    let check = check(
        Some(home.path()),
        &unit("loaded", "inactive"),
        ADDRESS,
        &|_| true,
    );
    assert_eq!(
        check,
        HubCheck::Stopped {
            state: "inactive".to_owned()
        }
    );
    assert!(check.line().starts_with("hub service: installed, stopped"));
}

#[test]
fn hub_not_installed() {
    let check = check(None, &unit("not-found", "inactive"), ADDRESS, &|_| true);
    assert_eq!(check, HubCheck::NotInstalled);
    assert!(check.line().contains("zaf hub service install"));
}

#[test]
fn hub_unknown_without_systemctl_or_a_bus() {
    let missing = check(None, &FakeCommands::new(), ADDRESS, &|_| true);
    assert!(matches!(missing, HubCheck::Unknown { .. }));
    let no_bus =
        FakeCommands::new().failing("systemctl", "Failed to connect to bus: No medium found\n");
    let check = check(None, &no_bus, ADDRESS, &|_| true);
    assert_eq!(
        check.line(),
        "hub service: unknown (systemctl --user: Failed to connect to bus: No medium found)"
    );
}
