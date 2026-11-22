//! `zaf begin --version <v>` on the built binary: its stdout is a pipe here, so there is no
//! terminal, and it refuses with one sentence and exit 2 before it reads the board. `zaf begin
//! ready` writes the file `ZAF_READY` names, and refuses with exit 1 when it is unset.

mod common;

use std::fs;
use std::io::Read;
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Stdio;
use std::time::UNIX_EPOCH;

use common::{Project, outcome};
use zaf::domain::{BodyHash, DocumentSignature, TicketBody};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

#[test]
fn begin_refuses_without_a_terminal() {
    let project = Project::new(MANIFEST);
    // The first run makes the zaf home and says so; the verb under test then runs alone.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["begin", "--version", "0.6.0"]));
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("terminal"), "{stderr}");
    assert_eq!(stderr.trim().lines().count(), 1, "{stderr}");
}

#[test]
fn begin_refuses_a_phase_that_is_not_objectives_or_design() {
    let project = Project::new(MANIFEST);
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) =
        outcome(&project.zaf(&["begin", "--version", "0.6.0", "--phase", "review"]));
    assert_eq!(code, 2, "{stdout}{stderr}");
    assert_eq!(stdout, "");
}

#[test]
fn begin_ready_writes_zaf_ready_and_refuses_without_it() {
    let project = Project::new(MANIFEST);
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");

    let unset = project
        .command_in(&project.root, &["begin", "ready"])
        .env_remove("ZAF_READY")
        .output()
        .expect("run zaf");
    let (code, stdout, stderr) = outcome(&unset);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(stdout, "");
    assert!(stderr.contains("ZAF_READY is unset"), "{stderr}");
    assert_eq!(stderr.trim().lines().count(), 1, "{stderr}");

    let ready = project.root.join(".zaf/begin/0.6.0/objectives.ready");
    let set = project
        .command_in(&project.root, &["begin", "ready"])
        .env("ZAF_READY", &ready)
        .output()
        .expect("run zaf");
    let (code, stdout, stderr) = outcome(&set);
    assert_eq!((code, stdout.as_str(), stderr.as_str()), (0, "", ""));
    assert!(ready.is_file(), "the ready file is written");

    // `ready` takes no --version, and --version alone still begins.
    let (code, _, stderr) = outcome(&project.zaf(&["begin", "--version", "0.6.0", "ready"]));
    assert_eq!(code, 2, "{stderr}");
}

/// The fallback list (nothing due, the stage unreadable) draws a title with an ESC byte without
/// it: the board answers the first epic listing and fails the later ones, and the terminal is a
/// pty the test opens.
#[test]
fn fallback_list_cleans_epic_titles() {
    let mut project = Project::new(MANIFEST);
    let body = "Release: 0.3.0\n\nEpic 1.";
    let hash = BodyHash::of(&TicketBody::from(body));
    let epic = serde_json::json!([{
        "number": 1,
        "title": "a\u{1b}[2Jb",
        "body": body,
        "labels": [{"name": "epic"}],
        "comments": [{
            "body": format!(
                "<!-- zaf:signature sha256={} at=2026-11-01T00:00:00Z -->\nSigned.",
                hash.as_str()
            ),
            "createdAt": "2026-11-01T00:00:00Z",
        }],
    }]);
    project
        .on("issue list --label epic *", &epic.to_string())
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
    // A `gh` ahead of the scene's: the first epic listing is answered, every later one fails.
    let stubs = project.temp.path().join("stubs");
    fs::create_dir_all(&stubs).expect("stubs");
    let count = project.temp.path().join("listed");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gh");
    let stub = stubs.join("gh");
    let script = [
        "#!/bin/sh".to_owned(),
        "case \"$*\" in \"issue list --label epic\"*)".to_owned(),
        format!("  n=$(cat {} 2>/dev/null || echo 0)", count.display()),
        "  n=$((n + 1))".to_owned(),
        format!("  echo $n > {}", count.display()),
        "  if [ $n -gt 1 ]; then echo down >&2; exit 1; fi;;".to_owned(),
        "esac".to_owned(),
        format!("exec {} \"$@\"", fixture.display()),
        String::new(),
    ]
    .join("\n");
    fs::write(&stub, script).expect("stub");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).expect("mode");

    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search =
        std::env::join_paths(std::iter::once(stubs).chain(std::env::split_paths(&inherited)))
            .expect("search path");
    let mut command = project.command_in(&project.root, &["begin", "--version", "0.3.0"]);
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
    // The read ends with an error (EIO) once the last holder of the slave is gone.
    while let Ok(read) = terminal.read(&mut chunk) {
        if read == 0 {
            break;
        }
        shown.extend_from_slice(&chunk[..read]);
    }
    let code = child.wait().expect("wait").code();
    let shown = String::from_utf8_lossy(&shown);

    assert_eq!(code, Some(0), "{shown:?}");
    assert!(shown.contains("a [2Jb"), "{shown:?}");
    assert!(!shown.contains('\u{1b}'), "{shown:?}");
}
