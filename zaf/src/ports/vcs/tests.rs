use std::path::Path;

use super::*;
use crate::fakes::FakeVcs;

fn names() -> (SliceName, BaseName) {
    (
        SliceName::from("feature/v1-export-2"),
        BaseName::from("feature/v1"),
    )
}

#[test]
fn the_latest_commit_follows_the_committed_work() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let (slice, base) = names();
    let workspace = Path::new("/project/.zaf/workspaces/feature/v1-export-2");
    assert!(matches!(
        port.latest_commit(workspace),
        Err(PortError::NotFound(_))
    ));
    vcs.open(&slice, &base);
    vcs.place(&slice, workspace).expect("place");
    let start = port.latest_commit(workspace).expect("base commit");
    vcs.edit(&slice, "src/export.rs", 12, 3, "@@ -1,3 +1,12 @@")
        .expect("edit");
    assert_eq!(port.latest_commit(workspace), Ok(start));
    let commit = vcs.commit(&slice, "Add the CSV writer").expect("commit");
    assert_eq!(commit, CommitId::from("fake-1"));
    assert_eq!(port.latest_commit(workspace), Ok(commit));
}

#[test]
fn diffs_come_as_stat_first_and_hunks_on_request() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let (slice, base) = names();
    let workspace = Path::new("/project/.zaf/workspaces/feature/v1-export-2");
    vcs.open(&slice, &base);
    vcs.place(&slice, workspace).expect("place");
    vcs.edit(&slice, "src/b.rs", 4, 1, "@@ b @@").expect("edit");
    vcs.edit(&slice, "src/a.rs", 2, 0, "@@ a @@").expect("edit");
    let stat = port.diff_stat(workspace, &base).expect("stat");
    assert_eq!(
        stat.to_string(),
        "src/a.rs +2 -0\nsrc/b.rs +4 -1\n2 files +6 -1\n"
    );
    assert_eq!(
        port.diff_hunks(workspace, &base, "src/a.rs"),
        Ok("@@ a @@".to_owned())
    );
    assert!(matches!(
        port.diff_hunks(workspace, &base, "src/c.rs"),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn a_repository_says_whether_it_has_a_remote() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let root = Path::new("/project");
    assert_eq!(port.has_remote(root), Ok(false));
    vcs.add_remote();
    assert_eq!(port.has_remote(root), Ok(true));
}

#[test]
fn fake_tree_of_matches_the_rule() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let (slice, base) = names();
    let root = Path::new("/project");
    vcs.open(&slice, &base);
    vcs.edit(&slice, "src/export.rs", 12, 3, "@@")
        .expect("edit");
    vcs.place(
        &slice,
        Path::new("/project/.zaf/workspaces/feature/v1-export-2"),
    )
    .expect("place");
    let latest = vcs.commit(&slice, "Add the CSV writer").expect("commit");
    // An empty commit has its parent's tree, however deep; a change has another; a sibling shares.
    let tree = port.tree_of(root, &latest).expect("tree");
    let empty = vcs.empty_commit(&latest).expect("empty");
    assert_eq!(port.tree_of(root, &empty), Ok(tree.clone()));
    let emptier = vcs.empty_commit(&empty).expect("empty");
    assert_eq!(port.tree_of(root, &emptier), Ok(tree.clone()));
    let changed = vcs.changed_commit(&latest).expect("changed");
    assert_ne!(port.tree_of(root, &changed), Ok(tree.clone()));
    let beside = vcs.sibling_commit(&latest).expect("sibling");
    assert_eq!(port.tree_of(root, &beside), Ok(tree));
    // A commit the repository does not hold is not found.
    assert!(matches!(
        port.tree_of(root, &CommitId::from("never-seen")),
        Err(PortError::NotFound(_))
    ));
}

#[test]
fn fake_holds_matches_the_rule() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let (slice, base) = names();
    let workspace = Path::new("/project/.zaf/workspaces/feature/v1-export-2");
    assert!(matches!(
        port.holds(workspace, &CommitId::from("fake-1")),
        Err(PortError::NotFound(_))
    ));
    vcs.open(&slice, &base);
    vcs.place(&slice, workspace).expect("place");
    vcs.edit(&slice, "src/export.rs", 12, 3, "@@")
        .expect("edit");
    let latest = vcs.commit(&slice, "Add the CSV writer").expect("commit");
    // The latest commit holds itself, and an empty commit on top of it does too, however deep.
    assert_eq!(port.holds(workspace, &latest), Ok(true));
    let empty = vcs.empty_commit(&latest).expect("empty");
    assert_eq!(port.holds(workspace, &empty), Ok(true));
    let emptier = vcs.empty_commit(&empty).expect("empty");
    assert_eq!(port.holds(workspace, &emptier), Ok(true));
    // A commit that changes anything does not, nor does any commit above one that did.
    let changed = vcs.changed_commit(&latest).expect("changed");
    assert_eq!(port.holds(workspace, &changed), Ok(false));
    let above = vcs.empty_commit(&changed).expect("empty");
    assert_eq!(port.holds(workspace, &above), Ok(false));
    // A commit off the line does not, even with the same tree; nor does an ancestor.
    let beside = vcs.sibling_commit(&latest).expect("sibling");
    assert_eq!(port.holds(workspace, &beside), Ok(false));
    let base_commit = CommitId::from("fake-base-feature/v1");
    assert_eq!(port.holds(workspace, &base_commit), Ok(false));
    // A head the repository does not hold is false, not an error.
    assert_eq!(
        port.holds(workspace, &CommitId::from("never-seen")),
        Ok(false)
    );
}

#[test]
fn path_at_answers_from_the_paths_a_test_gives_it() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let root = Path::new("/project");
    vcs.set_paths("v1", &["a.txt", "src/lib.rs"]);
    vcs.set_paths("v2", &["a.txt"]);
    assert_eq!(port.path_at(root, "v1", "src/lib.rs"), Ok(true));
    assert_eq!(port.path_at(root, "v2", "src/lib.rs"), Ok(false));
    assert_eq!(port.path_at(root, "v2", "a.txt"), Ok(true));
    // A revision no test gave is an error naming it, never `false`.
    let Err(error) = port.path_at(root, "v3", "a.txt") else {
        panic!("an unknown revision is an error");
    };
    assert!(error.to_string().contains("v3"), "{error}");
}
