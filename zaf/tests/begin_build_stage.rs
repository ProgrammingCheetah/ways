//! `zaf begin --version <v>` at the build stage (every open epic of the version signed, one not
//! done): the screen draws the four stages with the fourth marked and asks `Every epic of <v> is
//! signed. Start the build now? [Y/n]`. A yes becomes `zaf spawn --watch --version <v>` in place
//! (a stub `zaf` on PATH records its arguments); a no prints the spawn line and exits 0. The
//! terminal is a pty the test opens.

mod common;

use std::fs;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::UNIX_EPOCH;

use common::Project;
use zaf::domain::{BodyHash, DocumentSignature, TicketBody};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n";

const QUESTION: &str = "Every epic of 0.3.0 is signed. Start the build now?";

/// A signed epic of `version` as `issue list --label epic` prints it.
fn signed_epic(number: u32, version: &str) -> serde_json::Value {
    let body = format!("Release: {version}\n\nEpic {number}.");
    let hash = BodyHash::of(&TicketBody::from(body.as_str()));
    serde_json::json!({
        "number": number,
        "title": format!("Epic {number}"),
        "body": body,
        "labels": [{"name": "epic"}],
        "comments": [{
            "body": format!(
                "<!-- zaf:signature sha256={} at=2026-11-01T00:00:00Z -->\nSigned.",
                hash.as_str()
            ),
            "createdAt": "2026-11-01T00:00:00Z",
        }],
    })
}

/// A project at the build stage (signed documents, signed epics with no tickets) and a stub `zaf`
/// on PATH that records its arguments.
fn project() -> (Project, PathBuf) {
    let mut project = Project::new(MANIFEST);
    project
        .on(
            "issue list --label epic *",
            &serde_json::json!([signed_epic(1, "0.3.0")]).to_string(),
        )
        .on("*", "[]");
    let folder = project.root.join(".teams/releases/0.3.0");
    fs::create_dir_all(&folder).expect("folder");
    for name in ["objectives.md", "design.md"] {
        fs::write(
            folder.join(name),
            DocumentSignature::sign("Signed.\n", UNIX_EPOCH),
        )
        .expect("document");
    }
    let stubs = project.temp.path().join("stubs");
    fs::create_dir_all(&stubs).expect("stubs");
    let record = project.temp.path().join("spawned");
    let stub = stubs.join("zaf");
    fs::write(
        &stub,
        format!("#!/bin/sh\necho \"$@\" > {}\n", record.display()),
    )
    .expect("stub");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("mode");
    (project, record)
}

/// `zaf begin --version 0.3.0` on a pty under `NO_COLOR`: `input` typed, the exit code and
/// everything it printed.
fn begin_on_pty(project: &Project, input: &str) -> (i32, String) {
    let stubs = project.temp.path().join("stubs");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let search = std::env::join_paths(
        [stubs, fixtures]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    let mut command: Command = project.command_in(&project.root, &["begin", "--version", "0.3.0"]);
    command.env("PATH", search).env("NO_COLOR", "1");

    let (mut master, mut slave) = (-1, -1);
    // SAFETY: both out-pointers are valid ints; the rest may be null.
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0, "openpty");
    // SAFETY: openpty gave two fresh descriptors nothing else owns.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    command
        .stdin(Stdio::from(slave.try_clone().expect("dup")))
        .stdout(Stdio::from(slave.try_clone().expect("dup")))
        .stderr(Stdio::from(slave));
    let mut child = command.spawn().expect("start zaf");
    drop(command);

    let mut terminal = fs::File::from(master);
    let mut shown = Vec::new();
    let mut chunk = [0u8; 4096];
    let mut typed = false;
    // The read ends with an error (EIO) once the last holder of the slave is gone. The answer is
    // typed once the question is on screen: what is typed before it is discarded.
    while let Ok(count) = terminal.read(&mut chunk) {
        if count == 0 {
            break;
        }
        shown.extend_from_slice(&chunk[..count]);
        if !typed && String::from_utf8_lossy(&shown).contains("[Y/n]") {
            terminal.write_all(input.as_bytes()).expect("type");
            typed = true;
        }
    }
    let status = child.wait().expect("wait");
    (
        status.code().expect("exit code"),
        String::from_utf8_lossy(&shown).into_owned(),
    )
}

#[test]
fn begin_build_stage_asks_to_start() {
    let (project, spawned) = project();

    let (code, shown) = begin_on_pty(&project, "n\n");

    assert_eq!(code, 0, "{shown}");
    assert!(shown.contains("You are at the fourth:"), "{shown}");
    assert!(shown.contains("▸ 4. Build"), "{shown}");
    assert!(shown.contains("✓ 3. Epics"), "{shown}");
    assert!(shown.contains(QUESTION), "{shown}");
    assert!(shown.contains("[Y/n]"), "{shown}");
    assert!(!spawned.exists(), "a no starts nothing");
}

#[test]
fn begin_build_stage_no_prints_spawn_line() {
    let (project, spawned) = project();

    let (code, shown) = begin_on_pty(&project, "n\n");

    assert_eq!(code, 0, "{shown}");
    assert!(
        shown.contains("zaf spawn --watch --version 0.3.0"),
        "{shown}"
    );
    assert!(!spawned.exists(), "a no starts nothing");
}

#[test]
fn begin_build_stage_yes_execs_spawn_watch() {
    let (project, spawned) = project();

    let (code, shown) = begin_on_pty(&project, "y\n");

    // The exec runs this binary, not the stub `zaf` first on PATH: `zaf spawn` answers (here it
    // refuses, the manifest having no [vcs]) and the stub never ran.
    assert_eq!(code, 2, "{shown}");
    assert!(shown.contains(QUESTION), "{shown}");
    assert!(shown.contains("zaf spawn:"), "{shown}");
    assert!(!spawned.exists(), "the stub on PATH ran: {shown}");
}
