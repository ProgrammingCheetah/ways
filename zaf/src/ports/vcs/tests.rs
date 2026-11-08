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
