//! `zaf epic sign` asks `Every epic of <v> is signed. Start the build now? [Y/n]` when its new
//! signature is the last one of the version, and only where someone can answer: no instance
//! (`ZAF_INSTANCE`), a terminal on stdin and stdout, and no `--json`. A yes becomes
//! `zaf spawn --watch --version <v>` in place (a stub `zaf` on PATH records its arguments); a no,
//! or none, ends as signing does. The terminal is a pty the test opens.

mod common;

use std::fs;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use common::{Project, outcome};
use zaf::domain::{BodyHash, TicketBody};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n";

const QUESTION: &str = "Every epic of 0.3.0 is signed. Start the build now?";

/// An epic of `version` as `issue list --label epic` prints it, signed when `signed`.
fn epic(number: u32, version: &str, signed: bool) -> serde_json::Value {
    let body = format!("Release: {version}\n\nEpic {number}.");
    let mut comments = Vec::new();
    if signed {
        let hash = BodyHash::of(&TicketBody::from(body.as_str()));
        comments.push(serde_json::json!({
            "body": format!(
                "<!-- zaf:signature sha256={} at=2026-11-01T00:00:00Z -->\nSigned.",
                hash.as_str()
            ),
            "createdAt": "2026-11-01T00:00:00Z",
        }));
    }
    serde_json::json!({
        "number": number,
        "title": format!("Epic {number}"),
        "body": body,
        "labels": [{"name": "epic"}],
        "comments": comments,
    })
}

/// A project whose open epics are `epics`, and a stub `zaf` on PATH that records its arguments.
fn project(epics: &[serde_json::Value]) -> (Project, PathBuf) {
    let mut project = Project::new(MANIFEST);
    project
        .on_first(
            "issue list --label epic *",
            &serde_json::json!(epics).to_string(),
        )
        .on_first(
            "issue comment * *",
            "https://github.com/example/tracker/issues/1#c\n",
        )
        .on("*", "[]");
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

/// `zaf epic sign 1` (with `extra` arguments before it) on a pty: `input` typed, the exit code and
/// everything it printed.
fn sign_on_pty(
    project: &Project,
    extra: &[&str],
    env: &[(&str, &str)],
    input: &str,
) -> (i32, String) {
    let stubs = project.temp.path().join("stubs");
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let search = std::env::join_paths(
        [stubs, fixtures]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    let mut args = extra.to_vec();
    args.extend(["epic", "sign", "1"]);
    let mut command: Command = project.command_in(&project.root, &args);
    command.env("PATH", search);
    for (key, value) in env {
        command.env(key, value);
    }

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
fn epic_sign_asks_when_last_epic_signed() {
    let (project, spawned) = project(&[epic(1, "0.3.0", false), epic(2, "0.3.0", true)]);

    let (code, shown) = sign_on_pty(&project, &[], &[], "n\n");

    assert_eq!(code, 0, "{shown}");
    assert!(shown.contains("signed #1 sha256:"), "{shown}");
    assert!(shown.contains(QUESTION), "{shown}");
    assert!(shown.contains("[Y/n]"), "{shown}");
    assert!(!spawned.exists(), "a no starts nothing");
}

#[test]
fn epic_sign_no_keeps_signature() {
    let (project, spawned) = project(&[epic(1, "0.3.0", false)]);

    let (code, shown) = sign_on_pty(&project, &[], &[], "n\n");

    assert_eq!(code, 0, "{shown}");
    assert!(
        project
            .gh_calls()
            .iter()
            .any(|call| { call.starts_with("issue comment 1 ") && call.contains("zaf:signature") }),
        "the signature is recorded: {:?}",
        project.gh_calls()
    );
    assert!(!spawned.exists());
}

#[test]
fn epic_sign_quiet_while_one_unsigned() {
    let (project, spawned) = project(&[epic(1, "0.3.0", false), epic(2, "0.3.0", false)]);

    let (code, shown) = sign_on_pty(&project, &[], &[], "y\n");

    assert_eq!(code, 0, "{shown}");
    assert!(shown.contains("signed #1 sha256:"), "{shown}");
    assert!(!shown.contains("Start the build"), "{shown}");
    assert!(!spawned.exists());
}

#[test]
fn epic_sign_quiet_inside_instance() {
    let (project, spawned) = project(&[epic(1, "0.3.0", false)]);

    let (code, shown) = sign_on_pty(
        &project,
        &[],
        &[("ZAF_INSTANCE", "team instance 1 on #1")],
        "y\n",
    );

    assert_eq!(code, 0, "{shown}");
    assert!(shown.contains("signed #1 sha256:"), "{shown}");
    assert!(!shown.contains("Start the build"), "{shown}");
    assert!(!spawned.exists());
}

#[test]
fn epic_sign_quiet_with_json() {
    let (project, spawned) = project(&[epic(1, "0.3.0", false)]);

    let (code, shown) = sign_on_pty(&project, &["--json"], &[], "y\n");

    assert_eq!(code, 0, "{shown}");
    assert!(shown.contains("\"signed\":true"), "{shown}");
    assert!(!shown.contains("Start the build"), "{shown}");
    assert!(!spawned.exists());
}

/// No `issue comment` call on the board: the epic was left unsigned.
fn assert_unsigned(project: &Project) {
    assert!(
        !project
            .gh_calls()
            .iter()
            .any(|call| call.starts_with("issue comment")),
        "the epic was signed: {:?}",
        project.gh_calls()
    );
}

#[test]
fn epic_sign_refused_with_piped_stdin() {
    let (project, spawned) = project(&[epic(1, "0.3.0", false)]);
    let mut command = project.command_in(&project.root, &["epic", "sign", "1"]);
    command.stdin(Stdio::piped());

    let (code, stdout, stderr) = outcome(&command.output().expect("run zaf"));

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stderr.contains("Sign in zaf's chair."), "{stderr}");
    assert!(!stdout.contains("signed #1"), "{stdout}");
    assert_unsigned(&project);
    assert!(!spawned.exists());
}

#[test]
fn epic_sign_refused_with_claudecode() {
    let (project, spawned) = project(&[epic(1, "0.3.0", false)]);

    // On a terminal, so it is CLAUDECODE alone that refuses.
    let (code, shown) = sign_on_pty(&project, &[], &[("CLAUDECODE", "1")], "y\n");

    assert_eq!(code, 1, "{shown}");
    assert!(shown.contains("Sign in zaf's chair."), "{shown}");
    assert!(!shown.contains("signed #1"), "{shown}");
    assert_unsigned(&project);
    assert!(!spawned.exists());
}

#[test]
fn epic_sign_on_terminal_still_signs() {
    let (project, _spawned) = project(&[epic(1, "0.3.0", false), epic(2, "0.3.0", false)]);

    let (code, shown) = sign_on_pty(&project, &[], &[], "");

    assert_eq!(code, 0, "{shown}");
    assert!(shown.contains("signed #1 sha256:"), "{shown}");
    assert!(!shown.contains("Sign in zaf's chair."), "{shown}");
}

#[test]
fn epic_sign_yes_execs_spawn_watch() {
    let (project, spawned) = project(&[epic(1, "0.3.0", false), epic(2, "0.3.0", true)]);

    // The pty is a terminal for stderr too, so colour is off here: the line is read as written.
    let (code, shown) = sign_on_pty(&project, &[], &[("NO_COLOR", "1")], "y\n");

    // The exec runs this binary, not the stub `zaf` first on PATH: `zaf spawn` answers (here it
    // refuses, the release having no signed objectives) and the stub never ran.
    assert_eq!(code, 1, "{shown}");
    assert!(shown.contains(QUESTION), "{shown}");
    assert!(shown.contains("zaf spawn:"), "{shown}");
    assert!(!spawned.exists(), "the stub on PATH ran: {shown}");
}
