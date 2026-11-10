use std::os::unix::fs::PermissionsExt;

use super::*;
use crate::fakes::FakeReleases;

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
