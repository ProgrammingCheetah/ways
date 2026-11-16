use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use super::{PROJECTS_FILE, Projects};
use crate::app::setup::ensure_homes;

fn project(parent: &Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    fs::create_dir_all(&root).expect("project folder");
    fs::write(root.join(".zaf.toml"), "").expect("manifest");
    fs::canonicalize(root).expect("canonical")
}

#[test]
fn joins_once() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let root = project(dir.path(), "alpha");
    Projects::add(&home, &root).expect("add");
    Projects::add(&home, &root).expect("add again");
    Projects::add(&home, &root.join("..").join("alpha")).expect("add by another path");
    let entries = Projects::read(&home);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].folder, root);
    assert_eq!(entries[0].name, "alpha");
    assert!(!entries[0].missing);
}

#[test]
fn setup_adds() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let root = project(dir.path(), "fresh");
    // What `zaf setup` does for a folder: make the homes, then put the project on the list.
    ensure_homes(Some(&root), Some(&home)).expect("homes");
    Projects::add(&home, &root).expect("add");
    let entries = Projects::read(&home);
    assert_eq!(
        entries
            .iter()
            .map(|entry| &entry.folder)
            .collect::<Vec<_>>(),
        [&root]
    );
}

#[test]
fn missing_folder_is_marked() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let root = project(dir.path(), "gone");
    let kept = project(dir.path(), "kept");
    Projects::add(&home, &root).expect("add");
    Projects::add(&home, &kept).expect("add");
    fs::remove_dir_all(&root).expect("remove");
    let entries = Projects::read(&home);
    assert_eq!(entries.len(), 2);
    assert!(entries[0].missing);
    assert!(!entries[1].missing);
}

#[test]
fn missing_manifest_is_marked() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let root = project(dir.path(), "bare");
    Projects::add(&home, &root).expect("add");
    fs::remove_file(root.join(".zaf.toml")).expect("remove manifest");
    let entries = Projects::read(&home);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].missing);
}

#[test]
fn drop_removes() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let one = project(dir.path(), "one");
    let two = project(dir.path(), "two");
    Projects::add(&home, &one).expect("add");
    Projects::add(&home, &two).expect("add");
    Projects::drop(&home, &one).expect("drop");
    Projects::drop(&home, &one).expect("a second drop is nothing");
    let entries = Projects::read(&home);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].folder, two);
    assert!(one.is_dir(), "the folder itself is left alone");
}

#[test]
fn drop_removes_a_folder_that_is_gone() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let root = project(dir.path(), "gone");
    Projects::add(&home, &root).expect("add");
    fs::remove_dir_all(&root).expect("remove");
    Projects::drop(&home, &root).expect("drop");
    assert!(Projects::read(&home).is_empty());
}

#[test]
fn concurrent_adds_keep_every_entry() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let roots: Vec<PathBuf> = (0..16)
        .map(|n| project(dir.path(), &format!("p{n}")))
        .collect();
    let workers: Vec<_> = roots
        .iter()
        .cloned()
        .map(|root| {
            let home = home.clone();
            thread::spawn(move || Projects::add(&home, &root).expect("add"))
        })
        .collect();
    for worker in workers {
        worker.join().expect("worker");
    }
    assert_eq!(Projects::read(&home).len(), roots.len());
}

#[test]
fn an_unreadable_list_reads_empty_and_is_not_overwritten() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    fs::create_dir_all(&home).expect("home");
    fs::write(home.join(PROJECTS_FILE), "this is [not toml").expect("write");
    assert!(Projects::read(&home).is_empty());
    let root = project(dir.path(), "alpha");
    assert!(Projects::add(&home, &root).is_err());
    assert_eq!(
        fs::read_to_string(home.join(PROJECTS_FILE)).expect("read"),
        "this is [not toml"
    );
}

#[test]
fn a_project_has_no_version_until_one_is_remembered() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let root = project(dir.path(), "alpha");
    Projects::add(&home, &root).expect("add");
    assert_eq!(Projects::read(&home)[0].version, None);
    let text = fs::read_to_string(home.join(PROJECTS_FILE)).expect("list");
    assert!(!text.contains("version"), "no key for none: {text}");
}

#[test]
fn a_list_written_before_versions_were_kept_still_reads() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let root = project(dir.path(), "alpha");
    fs::create_dir_all(&home).expect("home");
    fs::write(
        home.join(PROJECTS_FILE),
        format!("[[project]]\nfolder = {:?}\n", root.display().to_string()),
    )
    .expect("an old list");

    let entries = Projects::read(&home);

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].version, None);
}

#[test]
fn a_remembered_version_stays_through_adds_and_is_replaced_by_the_next() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let alpha = project(dir.path(), "alpha");
    let beta = project(dir.path(), "beta");
    Projects::add(&home, &alpha).expect("add");
    Projects::add(&home, &beta).expect("add");

    Projects::remember_version(&home, &alpha, "0.4.0").expect("remembered");
    // Another verb running in the project adds it again, which leaves the entry as it is.
    Projects::add(&home, &alpha).expect("add again");

    let entries = Projects::read(&home);
    assert_eq!(entries[0].version.as_deref(), Some("0.4.0"));
    assert_eq!(entries[1].version, None, "the other project's is its own");

    Projects::remember_version(&home, &alpha.join("..").join("alpha"), "0.5.0").expect("replaced");
    assert_eq!(Projects::read(&home)[0].version.as_deref(), Some("0.5.0"));
}

#[test]
fn a_version_is_not_remembered_for_a_project_off_the_list() {
    let dir = tempfile::tempdir().expect("temp");
    let home = dir.path().join("home");
    let root = project(dir.path(), "alpha");

    let error = Projects::remember_version(&home, &root, "0.4.0").expect_err("not listed");

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(Projects::read(&home).is_empty(), "nothing was added");
}
