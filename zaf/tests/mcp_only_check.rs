//! `tools/mcp-only-check <since> [<orchestrator transcript>]` (M16, D24): prints each Bash tool
//! call that runs `zaf` in the transcripts it reads, one line each, and exits 1 when there is any.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The ways repository's root, one folder above the crate.
fn ways_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_owned()
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/transcripts")
        .join(name)
}

/// A project whose first profile holds the named fixtures as its transcripts.
fn project_with(names: &[&str]) -> tempfile::TempDir {
    let temp = tempfile::tempdir().expect("temp dir");
    let folder = temp.path().join(".zaf/profiles/lean/projects/-work-p");
    fs::create_dir_all(&folder).expect("profile folder");
    for name in names {
        fs::copy(fixture(name), folder.join(name)).expect("copy fixture");
    }
    temp
}

/// Runs the tool inside `project`: its exit code, stdout and stderr.
fn run(project: &Path, args: &[&str]) -> (i32, String, String) {
    // Through `sh`, as `tools/run-test` is run, so a checkout that lost the executable bit still tests.
    let out = Command::new("sh")
        .arg(ways_root().join("tools/mcp-only-check"))
        .args(args)
        .current_dir(project)
        .output()
        .expect("run tools/mcp-only-check");
    (
        out.status.code().expect("exit code"),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const SINCE: &str = "2026-11-21T00:00:00Z";

#[test]
fn mcp_only_check_clean_transcript_passes() {
    let project = project_with(&["clean.jsonl"]);
    let (code, stdout, stderr) = run(project.path(), &[SINCE]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "");
}

#[test]
fn mcp_only_check_bash_zaf_fails_with_line() {
    let project = project_with(&["bash-zaf.jsonl"]);
    let (code, stdout, stderr) = run(project.path(), &[SINCE]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(
        stdout,
        ".zaf/profiles/lean/projects/-work-p/bash-zaf.jsonl:3: Bash runs zaf: zaf board ready\n"
    );

    // The orchestrator's transcript, given by path, is read whole.
    let clean = project_with(&[]);
    let orchestrator = fixture("bash-zaf.jsonl");
    let (code, stdout, stderr) = run(clean.path(), &["1m", orchestrator.to_str().unwrap()]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(
        stdout.ends_with("bash-zaf.jsonl:3: Bash runs zaf: zaf board ready\n"),
        "{stdout}"
    );
}

#[test]
fn mcp_only_check_chained_zaf_fails() {
    let project = project_with(&["chained.jsonl"]);
    let (code, stdout, stderr) = run(project.path(), &[SINCE]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{stdout}");
    assert!(
        lines[0].ends_with("chained.jsonl:2: Bash runs zaf: cd /x && ZAF_INSTANCE=y zaf status"),
        "{stdout}"
    );
    assert!(
        lines[1].contains("chained.jsonl:3: "),
        "$( ) in quotes: {stdout}"
    );
    assert!(
        lines[2].contains("chained.jsonl:4: "),
        "env and a pipe: {stdout}"
    );
}

#[test]
fn mcp_only_check_since_skips_older() {
    let project = project_with(&["old-and-new.jsonl"]);
    let (code, stdout, stderr) = run(project.path(), &[SINCE]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(stdout.contains("old-and-new.jsonl:3: "), "{stdout}");

    // Counted from the entries' own clock, a later `since` leaves nothing to report.
    let (code, stdout, stderr) = run(project.path(), &["2026-11-23T00:00:00Z"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "");
}

#[test]
fn mcp_only_check_bad_since_exits_2() {
    let project = project_with(&["bash-zaf.jsonl"]);
    for bad in ["yesterday", "2h30m", "2026-13-45T00:00:00Z", ""] {
        let (code, stdout, stderr) = run(project.path(), &[bad]);
        assert_eq!(code, 2, "{bad:?}: {stdout}{stderr}");
        assert_eq!(stdout, "");
        assert!(stderr.contains("usage:"), "{bad:?}: {stderr}");
    }
    let (code, _, stderr) = run(project.path(), &[SINCE, "/no/such/transcript.jsonl"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("usage:"), "{stderr}");
}

#[test]
fn mcp_only_check_spawn_phase_fails_with_line() {
    let project = project_with(&[]);
    let orchestrator = fixture("lane-spawn-phase.jsonl");
    let (code, stdout, stderr) = run(project.path(), &["1m", orchestrator.to_str().unwrap()]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    let expected = format!(
        "{}:2: lane tool: spawn phase 1234 implement\n",
        orchestrator.display()
    );
    assert_eq!(stdout, expected);
}

#[test]
fn mcp_only_check_claim_and_phase_action_fail() {
    let project = project_with(&[]);
    let orchestrator = fixture("lane-claim-phase-action.jsonl");
    let path = orchestrator.display();
    let (code, stdout, stderr) = run(project.path(), &["1m", orchestrator.to_str().unwrap()]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    let expected: String = [
        "board claim 12",
        "do --tier machine implement 12",
        "do slice-open 12",
        "do fix 12",
        "slice open 12",
        "spawn stop 9",
        "board done 12",
    ]
    .iter()
    .zip(2..)
    .map(|(call, line)| format!("{path}:{line}: lane tool: {call}\n"))
    .collect();
    assert_eq!(stdout, expected);

    // The project's own [phases] names the phase actions; `implement` is then no phase.
    fs::write(
        project.path().join(".zaf.toml"),
        "[phases]\nTask = [\n  \"build\", # first\n  \"ship-it\",\n]\n\n[spawn]\nslots = 4\n",
    )
    .expect("manifest");
    let (code, stdout, stderr) = run(project.path(), &["1m", orchestrator.to_str().unwrap()]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(!stdout.contains("do --tier machine implement"), "{stdout}");
    assert!(
        stdout.contains(":4: lane tool: do slice-open 12"),
        "{stdout}"
    );
}

#[test]
fn mcp_only_check_clean_orchestrator_passes() {
    let project = project_with(&[]);
    let orchestrator = fixture("lane-clean.jsonl");
    let (code, stdout, stderr) = run(project.path(), &["1m", orchestrator.to_str().unwrap()]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "");
}

#[test]
fn mcp_only_check_lane_only_in_orchestrator() {
    // The same lane calls in a profile transcript are not reported.
    let project = project_with(&["lane-spawn-phase.jsonl", "lane-claim-phase-action.jsonl"]);
    let (code, stdout, stderr) = run(project.path(), &[SINCE]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(stdout, "");
}
