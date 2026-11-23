use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::app::doctor::fake::FakeCommands;
use crate::fakes::FakeReleases;
use crate::manifest::CommandLine;
use crate::ports::{GateRun, GateVerdict};

const TARGET: &str = "x86_64-unknown-linux-musl";

fn version(text: &str) -> Version {
    Version::parse(text).expect("a version")
}

fn tags(list: &[&str]) -> Vec<String> {
    list.iter().map(|tag| (*tag).to_owned()).collect()
}

/// Publishes `tag` under `files/<tag>/` as the release workflow would: a tarball holding
/// `<tag>-<target>/zaf` (a script printing `says`), and its `.sha256`.
fn publish(files: &Path, tag: &str, says: &str) {
    let stage = files.join("stage");
    let inner = stage.join(format!("{tag}-{TARGET}"));
    std::fs::create_dir_all(&inner).expect("stage");
    std::fs::write(inner.join("zaf"), format!("#!/bin/sh\necho '{says}'\n")).expect("binary");
    let release = files.join(tag);
    std::fs::create_dir_all(&release).expect("release folder");
    let name = archive(tag, TARGET);
    let status = Command::new("tar")
        .arg("-czf")
        .arg(release.join(&name))
        .arg("-C")
        .arg(&stage)
        .arg(format!("{tag}-{TARGET}"))
        .status()
        .expect("tar runs");
    assert!(status.success());
    let sum = sha256(&release.join(&name)).expect("hashes");
    std::fs::write(
        release.join(format!("{name}.sha256")),
        format!("{sum}  {name}\n"),
    )
    .expect("sum");
    std::fs::remove_dir_all(&stage).expect("clean stage");
}

/// A binary standing where zaf is installed.
fn installed(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let exe = bin.join("zaf");
    std::fs::write(&exe, "the old zaf").expect("old binary");
    exe
}

fn leftovers(exe: &Path) -> Vec<String> {
    std::fs::read_dir(exe.parent().expect("a folder"))
        .expect("lists")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != "zaf")
        .collect()
}

#[test]
fn the_newest_stable_zaf_tag_wins_and_other_tags_are_passed_by() {
    let found = newest(&tags(&[
        "zaf-v0.1.1",
        "zaf-v0.10.0",
        "zaf-v0.2.0",
        "zaf-v0.11.0-rc.1",
        "teams-v9.0.0",
        "zaf-vnext",
    ]));
    assert_eq!(found, Some((version("0.10.0"), "zaf-v0.10.0".to_owned())));
    assert_eq!(newest(&tags(&["teams-v2.1.0"])), None);
}

#[test]
fn the_archive_is_named_as_the_release_workflow_names_it() {
    assert_eq!(
        archive("zaf-v0.2.1", "aarch64-apple-darwin"),
        "zaf-v0.2.1-aarch64-apple-darwin.tar.gz"
    );
}

#[test]
fn a_newer_release_replaces_the_binary_and_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("temp dir");
    let files = dir.path().join("releases");
    publish(&files, "zaf-v0.3.0", "zaf 0.3.0");
    let releases = FakeReleases::default();
    releases.publish(&["zaf-v0.2.0", "zaf-v0.3.0"]);
    releases.serve_from(&files);
    let exe = installed(dir.path());

    let outcome = update(&releases, &version("0.2.0"), &exe, Some(TARGET)).expect("updates");

    assert_eq!(
        outcome,
        Outcome::Updated {
            from: version("0.2.0"),
            to: version("0.3.0"),
            path: exe.clone(),
        }
    );
    let output = Command::new(&exe).arg("--version").output().expect("runs");
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "zaf 0.3.0");
    let mode = std::fs::metadata(&exe)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
    assert!(leftovers(&exe).is_empty(), "{:?}", leftovers(&exe));
    assert_eq!(
        releases.downloads(),
        [(
            "zaf-v0.3.0".to_owned(),
            vec![
                format!("zaf-v0.3.0-{TARGET}.tar.gz"),
                format!("zaf-v0.3.0-{TARGET}.tar.gz.sha256"),
            ]
        )]
    );
}

#[test]
fn an_archive_that_fails_its_sum_leaves_the_old_binary() {
    let dir = tempfile::tempdir().expect("temp dir");
    let files = dir.path().join("releases");
    publish(&files, "zaf-v0.3.0", "zaf 0.3.0");
    let name = archive("zaf-v0.3.0", TARGET);
    std::fs::write(
        files.join("zaf-v0.3.0").join(format!("{name}.sha256")),
        format!("{}  {name}\n", "0".repeat(64)),
    )
    .expect("a wrong sum");
    let releases = FakeReleases::default();
    releases.publish(&["zaf-v0.3.0"]);
    releases.serve_from(&files);
    let exe = installed(dir.path());

    let error = update(&releases, &version("0.2.0"), &exe, Some(TARGET)).expect_err("refused");

    assert!(error.to_string().contains("sha256"), "{error}");
    assert_eq!(std::fs::read_to_string(&exe).expect("old"), "the old zaf");
    assert!(leftovers(&exe).is_empty(), "{:?}", leftovers(&exe));
}

#[test]
fn a_binary_that_is_not_the_release_it_claims_is_not_installed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let files = dir.path().join("releases");
    publish(&files, "zaf-v0.3.0", "zaf 0.2.9");
    let releases = FakeReleases::default();
    releases.publish(&["zaf-v0.3.0"]);
    releases.serve_from(&files);
    let exe = installed(dir.path());

    let error = update(&releases, &version("0.2.0"), &exe, Some(TARGET)).expect_err("refused");

    assert!(error.to_string().contains("zaf 0.2.9"), "{error}");
    assert_eq!(std::fs::read_to_string(&exe).expect("old"), "the old zaf");
}

#[test]
fn an_up_to_date_zaf_downloads_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let releases = FakeReleases::default();
    releases.publish(&["zaf-v0.2.0", "zaf-v0.1.1"]);
    let exe = installed(dir.path());

    for running in ["0.2.0", "0.3.0"] {
        let outcome = update(&releases, &version(running), &exe, Some(TARGET)).expect("current");
        assert_eq!(
            outcome,
            Outcome::Current {
                version: version(running)
            }
        );
    }
    assert!(releases.downloads().is_empty());
}

#[test]
fn a_check_names_the_newer_release_and_changes_nothing() {
    let releases = FakeReleases::default();
    releases.publish(&["zaf-v0.2.1"]);
    assert_eq!(
        check(&releases, &version("0.2.0")).expect("checks"),
        Outcome::Behind {
            current: version("0.2.0"),
            newest: version("0.2.1"),
        }
    );
    assert!(releases.downloads().is_empty());
}

#[test]
fn a_platform_without_an_archive_is_told_how_to_build_the_release() {
    let dir = tempfile::tempdir().expect("temp dir");
    let releases = FakeReleases::default();
    releases.publish(&["zaf-v0.3.0"]);
    let exe = installed(dir.path());
    let error = update(&releases, &version("0.2.0"), &exe, None).expect_err("no archive");
    assert!(
        error.to_string().contains(
            "cargo install --git https://github.com/ProgrammingCheetah/ways --tag zaf-v0.3.0"
        ),
        "{error}"
    );
}

#[test]
fn no_release_at_all_is_an_error() {
    let releases = FakeReleases::default();
    releases.publish(&["teams-v2.1.0"]);
    assert!(matches!(
        check(&releases, &version("0.2.0")),
        Err(UpdateError::NoRelease)
    ));
}

const LISTED: &str =
    "Configured marketplaces:\n\n  ❯ ways\n    Source: GitHub (ProgrammingCheetah/ways)\n";
const LOCAL: &str = "Configured marketplaces:\n\n  ❯ other\n    Source: GitHub (a/b)\n\n  ❯ ways\n    Source: Directory (/home/zuri/ways)\n";

fn claude_calls(commands: &FakeCommands) -> Vec<String> {
    commands
        .calls()
        .iter()
        .map(|call| call[1..].join(" "))
        .collect()
}

#[test]
fn plugin_step_updates_marketplace_then_plugin() {
    let commands = FakeCommands::new().with("claude", LISTED);
    assert!(matches!(update_plugin(&commands), StepOutcome::Done(_)));
    assert_eq!(
        claude_calls(&commands),
        [
            "plugin marketplace list",
            "plugin marketplace update ways",
            "plugin update teams@ways"
        ]
    );
}

#[test]
fn plugin_step_installs_when_missing() {
    let commands = FakeCommands::new().with(
        "claude",
        "Configured marketplaces:\n\n  ❯ other\n    Source: GitHub (a/b)\n",
    );
    assert!(matches!(update_plugin(&commands), StepOutcome::Done(_)));
    assert_eq!(
        claude_calls(&commands),
        [
            "plugin marketplace list",
            "plugin marketplace add ProgrammingCheetah/ways",
            "plugin install teams@ways"
        ]
    );
}

#[test]
fn plugin_step_leaves_a_local_marketplace() {
    let commands = FakeCommands::new().with("claude", LOCAL);
    let StepOutcome::Skipped(text) = update_plugin(&commands) else {
        panic!("a local marketplace is skipped");
    };
    assert!(text.contains("local folder"));
    assert_eq!(claude_calls(&commands), ["plugin marketplace list"]);
}

#[test]
fn plugin_step_without_claude_fails_alone() {
    let commands = FakeCommands::new();
    let StepOutcome::Failed(text) = update_plugin(&commands) else {
        panic!("no claude is a failure");
    };
    assert!(text.contains("claude"));
    assert_eq!(commands.calls().len(), 1);
}

#[test]
fn plugin_step_names_the_first_error_line_of_a_failed_command() {
    let commands = FakeCommands::new().failing("claude", "\nboom\nmore\n");
    let StepOutcome::Failed(text) = update_plugin(&commands) else {
        panic!("a failed command is a failure");
    };
    assert!(text.contains("claude plugin marketplace list") && text.contains("boom"));
    assert!(!text.contains("more"));
}

/// A shell that records the lines it is given. A passing one writes what Claude records for
/// an install: the plugin at `installs` in the profile the line names.
struct PackShell {
    installs: &'static str,
    passes: bool,
    lines: std::sync::Mutex<Vec<String>>,
}

impl PackShell {
    fn new(installs: &'static str, passes: bool) -> Self {
        Self {
            installs,
            passes,
            lines: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("lock").clone()
    }
}

impl Shell for PackShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        let text = line.as_ref();
        // zaf's MCP is already in every profile, and these tests are about the plugin lines.
        if text.contains(" claude mcp get ") {
            return Ok(GateRun {
                verdict: GateVerdict::Passed,
                output: crate::app::profile::GET_SHOWS_OURS.to_owned(),
            });
        }
        self.lines.lock().expect("lock").push(line.to_string());
        if self.passes && text.contains(" claude plugin install ") {
            let home = text.split('\'').nth(1).expect("a profile home");
            let record = serde_json::json!({
                "version": 2,
                "plugins": { "teams@ways": [{ "scope": "user", "version": self.installs }] },
            });
            std::fs::create_dir_all(Path::new(home).join("plugins")).expect("plugins dir");
            std::fs::write(
                Path::new(home).join("plugins/installed_plugins.json"),
                record.to_string(),
            )
            .expect("writes");
        }
        let (verdict, output) = if !self.passes {
            (GateVerdict::Failed, "said no")
        } else if text.ends_with("claude plugin marketplace list") {
            (GateVerdict::Passed, "No marketplaces configured\n")
        } else {
            (GateVerdict::Passed, "ok")
        };
        Ok(GateRun {
            verdict,
            output: output.to_owned(),
        })
    }
}

const PROJECT_HEAD: &str =
    "[project]\nname = \"bookshelf\"\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

/// Writes `.zaf.toml` in `dir`: the head, a comment, `pack`, then `after`; returns its text
/// and the manifest it reads as.
fn project(dir: &Path, pack: &str, after: &str) -> (String, Manifest) {
    let text = format!("{PROJECT_HEAD}\n# the pack\n{pack}{after}");
    std::fs::write(dir.join(".zaf.toml"), &text).expect("manifest");
    let manifest = Manifest::load_layers(dir, None).expect("a valid manifest");
    (text, manifest)
}

#[test]
fn stale_pack_shows_the_one_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, manifest) = project(dir.path(), "[pack]\ntag = \"teams-v2.1.4\"\n", "");
    let change = stale_pack(dir.path(), &manifest, &version("2.1.5")).expect("stale");
    assert_eq!(change.old, "tag = \"teams-v2.1.4\"");
    assert_eq!(change.new, "tag = \"teams-v2.1.5\"");
    assert_eq!(change.file, dir.path().join(".zaf.toml"));

    // The quoting, the spacing, the comment and the prefix form stay.
    let (_, manifest) = project(dir.path(), "[pack]\n  tag   =  'v2.1.4'  # pinned\n", "");
    let change = stale_pack(dir.path(), &manifest, &version("2.2.0")).expect("stale");
    assert_eq!(change.old, "  tag   =  'v2.1.4'  # pinned");
    assert_eq!(change.new, "  tag   =  'v2.2.0'  # pinned");
}

#[test]
fn apply_pack_rewrites_only_that_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let after = "\n[forbidden]\ncommands = [\"rm -rf\"]\n# tag = \"teams-v2.1.4\" stays\n";
    let (before, manifest) = project(dir.path(), "[pack]\ntag = \"teams-v2.1.4\"\n", after);
    let change = stale_pack(dir.path(), &manifest, &version("2.1.5")).expect("stale");
    let shell = PackShell::new("2.1.5", true);
    let outcome = apply_pack_with(dir.path(), &change, &shell, None, None);
    assert!(matches!(outcome, StepOutcome::Done(_)), "{outcome:?}");
    let now = std::fs::read_to_string(dir.path().join(".zaf.toml")).expect("manifest");
    let expected = before.replacen("teams-v2.1.4", "teams-v2.1.5", 1);
    assert_eq!(now, expected);
    assert!(
        shell
            .lines()
            .iter()
            .any(|line| line.contains("claude plugin install 'teams@ways'")),
        "{:?}",
        shell.lines()
    );
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .expect("lists")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, [".zaf", ".zaf.toml"]);
}

#[test]
fn a_refused_generation_fails_the_step_and_keeps_the_rewritten_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, manifest) = project(dir.path(), "[pack]\ntag = \"teams-v2.1.4\"\n", "");
    let change = stale_pack(dir.path(), &manifest, &version("2.1.5")).expect("stale");
    let shell = PackShell::new("2.1.5", false);
    let StepOutcome::Failed(text) = apply_pack_with(dir.path(), &change, &shell, None, None) else {
        panic!("a refused generation must fail the step");
    };
    assert!(text.contains("profiles were refused"), "{text}");
    assert!(text.contains("claude plugin"), "{text}");
    let now = std::fs::read_to_string(dir.path().join(".zaf.toml")).expect("manifest");
    assert!(now.contains("tag = \"teams-v2.1.5\""), "{now}");
}

#[test]
fn a_pack_path_is_left_alone() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, manifest) = project(dir.path(), "[pack]\npath = \"plugins/teams\"\n", "");
    assert_eq!(stale_pack(dir.path(), &manifest, &version("9.9.9")), None);
    assert_eq!(
        skip_reason(&manifest),
        Some("pack from a folder; left as it is")
    );
    let (_, manifest) = project(dir.path(), "", "");
    assert_eq!(stale_pack(dir.path(), &manifest, &version("9.9.9")), None);
    assert_eq!(skip_reason(&manifest), None);
}

#[test]
fn a_current_tag_is_no_change() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (_, manifest) = project(dir.path(), "[pack]\ntag = \"teams-v2.1.5\"\n", "");
    assert_eq!(stale_pack(dir.path(), &manifest, &version("2.1.5")), None);
    assert_eq!(stale_pack(dir.path(), &manifest, &version("2.1.4")), None);
}

fn texts(lines: &[&str]) -> Vec<Text> {
    lines.iter().map(|line| Text::from(*line)).collect()
}

#[test]
fn what_is_new_takes_at_most_five_lines() {
    let notes =
        "Release 0.5.2\n\n## What is new\n- one\n* two\n• three\n- four\n- five\n- six\n- seven\n";
    assert_eq!(
        what_is_new(notes),
        texts(&["one", "two", "three", "four", "five"])
    );
}

#[test]
fn what_is_new_stops_at_a_blank_line() {
    let notes = "What is new\n- one\n- two\n\n- not this\n";
    assert_eq!(what_is_new(notes), texts(&["one", "two"]));
}

#[test]
fn what_is_new_skips_blank_lines_after_the_heading() {
    let notes = "Release\n\n## What is new\n\n\n- one\n- two\n\n- not this\n";
    assert_eq!(what_is_new(notes), texts(&["one", "two"]));
}

#[test]
fn what_is_new_without_a_gap_is_unchanged() {
    let notes = "## What is new\n- one\n- two\n\nMore.\n";
    assert_eq!(what_is_new(notes), texts(&["one", "two"]));
}

#[test]
fn what_is_new_falls_back_to_the_first_line() {
    assert_eq!(
        what_is_new("\n\nFixes a thing.\nAnd more.\n"),
        texts(&["Fixes a thing."])
    );
    assert!(what_is_new("").is_empty());
}

#[test]
fn skipped_lists_newer_stable_releases_newest_first() {
    let list = tags(&["zaf-v0.4.3", "zaf-v0.5.1", "zaf-v0.5.2", "zaf-v0.6.0-rc.1"]);
    assert_eq!(
        skipped(&list, &version("0.5.0")),
        vec![
            (version("0.5.2"), "zaf-v0.5.2".to_owned()),
            (version("0.5.1"), "zaf-v0.5.1".to_owned()),
        ]
    );
}
