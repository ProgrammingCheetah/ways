//! The ways repository runs on what the framework built: its manifest is `.zaf.toml`, the old
//! `ZAF.toml` is gone, and `zaf doctor` finds every action name it needs in the project's own
//! `.zaf/actions`, with nothing taken from the machine.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use zaf::manifest::Manifest;

/// The ways repository's root, one folder above the crate.
fn ways_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_owned()
}

/// Copies `from` into `to`, files and folders alike.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create dir");
    for entry in fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy");
        }
    }
}

/// `zaf --json doctor` on a copy of the repo's `.zaf.toml` and `.zaf/actions`, with an empty zaf
/// home so no machine action counts. The copy keeps a slice workspace (which sits inside the
/// main checkout) from resolving to the checkout's own root. The tools and logins rows depend on
/// the computer, so a test reads only the action rows.
fn doctor_report() -> Value {
    let temp = tempfile::tempdir().expect("temp dir");
    let (project, home) = (temp.path().join("repo"), temp.path().join("home"));
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&project).expect("project");
    let root = ways_root();
    fs::copy(root.join(".zaf.toml"), project.join(".zaf.toml")).expect("manifest");
    copy_dir(&root.join(".zaf/actions"), &project.join(".zaf/actions"));
    let output = Command::new(env!("CARGO_BIN_EXE_zaf"))
        .args(["--json", "doctor"])
        .current_dir(&project)
        .env("ZAF_HOME", &home)
        .env_remove("ZAF_INSTANCE")
        .env_remove("ZAF_VERSION")
        .output()
        .expect("run zaf doctor");
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "doctor printed no json ({error}): {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn ways_manifest_loads() {
    let root = ways_root();
    assert!(
        !root.join("ZAF.toml").exists(),
        "the retired ZAF.toml is still in the repository"
    );
    assert!(root.join(".zaf.toml").is_file(), "no .zaf.toml at the root");

    let manifest = Manifest::load_layers(&root, None).expect("the ways manifest loads");
    assert_eq!(manifest.project.name.to_string(), "ways");

    // The merge paths of #143 and the pull request's one step of ruling 39.
    let text = fs::read_to_string(root.join(".zaf.toml")).expect("the manifest");
    let document: toml::Table = text.parse().expect("the manifest is toml");
    assert_eq!(
        document["check"]["pr"]["steps"]
            .as_array()
            .expect("steps")
            .iter()
            .map(|step| step.as_str().expect("a step"))
            .collect::<Vec<_>>(),
        ["ci"],
        "[check.pr] steps"
    );
    let forbidden = document["forbidden"]["commands"]
        .as_array()
        .expect("[forbidden] commands");
    assert!(!forbidden.is_empty(), "[forbidden] lists nothing");
}

#[test]
fn ways_needs_no_machine_action() {
    let report = doctor_report();
    let actions = report["actions"].as_array().expect("action rows");
    assert!(!actions.is_empty(), "the repo provides actions");
    for row in actions {
        assert_eq!(row["ok"], true, "an invalid action: {row}");
        assert_eq!(
            row["tier"], "project",
            "an action outside the project tier: {row}"
        );
        let dependents = row["machine_dependents"].as_array().expect("dependents");
        assert!(
            dependents.is_empty(),
            "the project depends on this machine for {}: {dependents:?}",
            row["name"]
        );
    }
}

#[test]
fn ways_provides_every_needed_name() {
    let report = doctor_report();
    let needed = report["needed"].as_array().expect("needed rows");
    assert!(!needed.is_empty(), "the repo needs the slice verbs at least");
    for row in needed {
        assert_eq!(row["ok"], true, "a needed name is not provided: {row}");
        assert_eq!(
            row["tier"], "project",
            "a needed name the project tier does not provide: {row}"
        );
    }
}
