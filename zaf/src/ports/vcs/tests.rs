use super::*;
use crate::fakes::FakeVcs;

fn names() -> (SliceName, BaseName) {
    (
        SliceName::from("feature/v1-export-2"),
        BaseName::from("feature/v1"),
    )
}

#[test]
fn slices_open_once_per_base() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let (slice, base) = names();
    assert_eq!(port.open_slice(&slice, &base), Ok(SliceOpening::Opened));
    assert_eq!(
        port.open_slice(&slice, &base),
        Ok(SliceOpening::AlreadyOpen)
    );
    let other_base = BaseName::from("feature/v2");
    assert!(matches!(
        port.open_slice(&slice, &other_base),
        Err(PortError::Conflict(_))
    ));
    port.drop_slice(&slice).expect("drop");
    port.drop_slice(&slice).expect("a second drop is quiet");
    assert!(!vcs.has_slice(&slice));
    assert!(matches!(port.is_clean(&slice), Err(PortError::NotFound(_))));
}

#[test]
fn commit_and_publish_follow_the_work() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let (slice, base) = names();
    port.open_slice(&slice, &base).expect("open");
    assert_eq!(port.is_clean(&slice), Ok(true));
    assert!(matches!(
        port.commit(&slice, "empty"),
        Err(PortError::Rejected(_))
    ));
    vcs.edit(&slice, "src/export.rs", 12, 3, "@@ -1,3 +1,12 @@")
        .expect("edit");
    assert_eq!(port.is_clean(&slice), Ok(false));
    let commit = port.commit(&slice, "Add the CSV writer").expect("commit");
    assert_eq!(commit, CommitId::from("fake-1"));
    assert_eq!(port.is_clean(&slice), Ok(true));
    assert_eq!(port.publish(&slice), Ok(Publication::Published));
    assert_eq!(port.publish(&slice), Ok(Publication::AlreadyPublished));
}

#[test]
fn base_merges_report_their_outcome() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let (slice, base) = names();
    port.open_slice(&slice, &base).expect("open");
    assert_eq!(port.merge_base(&slice), Ok(BaseMerge::UpToDate));
    vcs.advance_base(&base);
    assert_eq!(port.merge_base(&slice), Ok(BaseMerge::Merged));
    assert_eq!(port.merge_base(&slice), Ok(BaseMerge::UpToDate));
    vcs.conflict_on_merge(&slice, &["src/export.rs"])
        .expect("arrange conflict");
    let conflicted = BaseMerge::Conflicted {
        paths: vec!["src/export.rs".to_owned()],
    };
    assert_eq!(port.merge_base(&slice), Ok(conflicted));
}

#[test]
fn diffs_come_as_stat_first_and_hunks_on_request() {
    let vcs = FakeVcs::default();
    let port: &dyn Vcs = &vcs;
    let (slice, base) = names();
    port.open_slice(&slice, &base).expect("open");
    vcs.edit(&slice, "src/b.rs", 4, 1, "@@ b @@").expect("edit");
    vcs.edit(&slice, "src/a.rs", 2, 0, "@@ a @@").expect("edit");
    let stat = port.diff_stat(&slice).expect("stat");
    assert_eq!(
        stat.to_string(),
        "src/a.rs +2 -0\nsrc/b.rs +4 -1\n2 files +6 -1\n"
    );
    assert_eq!(
        port.diff_hunks(&slice, "src/a.rs"),
        Ok("@@ a @@".to_owned())
    );
    assert!(matches!(
        port.diff_hunks(&slice, "src/c.rs"),
        Err(PortError::NotFound(_))
    ));
}
