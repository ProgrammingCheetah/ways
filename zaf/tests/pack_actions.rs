//! The teams pack carries the 24 standard actions a project copies into its `.zaf/actions/`
//! (docs/zaf-actions.md §2, §7). Every file in `plugins/teams/actions/` is executable, parses as an
//! action, and has the mode, group and phases of the ways repository's copy. Outside the ways
//! repository, where the pack is not beside the crate, nothing runs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use zaf::app::actions::{ActionMode, PhaseEnd, parse};

const NAMES: [&str; 24] = [
    "base-open",
    "ci",
    "ci-wait",
    "commit",
    "design",
    "fix",
    "implement",
    "merge",
    "merge-base",
    "objectives",
    "papers-commit",
    "pr-merge",
    "pr-open",
    "push",
    "release-open",
    "review",
    "review-epic",
    "slice-drop",
    "slice-merge",
    "slice-open",
    "slice-pr",
    "sync-base",
    "workspace-drop",
    "workspace-open",
];

#[test]
fn pack_actions_parse() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let pack = root.join("plugins/teams/actions");
    if !pack.is_dir() {
        return;
    }
    let mut found: Vec<String> = fs::read_dir(&pack)
        .expect("the pack's actions folder reads")
        .map(|entry| entry.expect("entry").file_name().to_string_lossy().into())
        .collect();
    found.sort();
    assert_eq!(found, NAMES, "the pack holds exactly the 24 actions");

    for name in NAMES {
        let file = pack.join(name);
        let mode = fs::metadata(&file).expect("metadata").permissions().mode();
        assert_ne!(mode & 0o111, 0, "{name} is not executable");
        let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let parsed = parse(&file, &text).unwrap_or_else(|error| panic!("{error}"));

        let ways_file = root.join(".zaf/actions").join(name);
        let ways_text =
            fs::read_to_string(&ways_file).unwrap_or_else(|error| panic!("ways' {name}: {error}"));
        let ways = parse(&ways_file, &ways_text).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(parsed.metadata.mode, ways.metadata.mode, "{name}: mode");
        assert_eq!(parsed.metadata.group, ways.metadata.group, "{name}: group");
        assert_eq!(
            parsed.metadata.phases, ways.metadata.phases,
            "{name}: phases"
        );
    }
}

/// Under a CI-only gate no review is ever posted on the forge, so the `review` phase ends when its
/// check exits 0 (docs/zaf-actions.md §5.1), in ways' copy and in the pack's alike.
#[test]
fn review_phase_ends_when_its_check_exits() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let ways_file = root.join(".zaf/actions/review");
    let pack_file = root.join("plugins/teams/actions/review");
    if !pack_file.is_file() {
        return;
    }
    let read = |file: &PathBuf| {
        let text = fs::read_to_string(file).unwrap_or_else(|error| panic!("{file:?}: {error}"));
        parse(file, &text).unwrap_or_else(|error| panic!("{error}"))
    };
    let (ways, pack) = (read(&ways_file), read(&pack_file));

    assert_eq!(ways.metadata.ends, Some(PhaseEnd::Exit), "ways' review");
    assert_eq!(
        pack.metadata.ends,
        Some(PhaseEnd::Exit),
        "the pack's review"
    );
    assert_eq!(pack.metadata.kind, ways.metadata.kind, "kind");
    assert_eq!(pack.metadata.on_fail, ways.metadata.on_fail, "on-fail");
    assert_eq!(
        pack.metadata.description, ways.metadata.description,
        "description"
    );
}

/// The two Designer actions (docs/zaf-actions.md §7): `design` keeps the Engineer's terminal and is
/// the Engineer's alone; `review-epic` runs unattended under a declared bound and is no phase
/// action. Neither calls `gh`, and ways' copies are byte for byte the pack's.
#[test]
fn designer_actions_declare_their_terminal_and_mode() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let pack = root.join("plugins/teams/actions");
    if !pack.is_dir() {
        return;
    }
    let read = |name: &str| {
        let file = pack.join(name);
        let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let ways = fs::read_to_string(root.join(".zaf/actions").join(name))
            .unwrap_or_else(|error| panic!("ways' {name}: {error}"));
        assert_eq!(text, ways, "{name}: ways' copy is the pack's");
        assert!(
            !text
                .lines()
                .any(|line| line.split_whitespace().any(|word| word == "gh")),
            "{name} calls gh"
        );
        parse(&file, &text).unwrap_or_else(|error| panic!("{error}"))
    };

    let design = read("design").metadata;
    assert!(design.terminal, "design keeps the terminal");
    assert_eq!(design.mode, ActionMode::parse("700").expect("a mode"));
    assert_eq!((design.kind, design.ends), (None, None));

    let review = read("review-epic").metadata;
    assert!(!review.terminal, "review-epic runs unattended");
    assert_eq!(review.mode, ActionMode::DEFAULT);
    assert_eq!(review.timeout, Some(45));
    assert_eq!((review.kind, review.ends), (None, None));
    assert_eq!(review.codes.keys().copied().collect::<Vec<_>>(), [1, 2]);
}

/// A merge whose base conflicts starts `fix` (slice 8): `merge` declares `fix` as its `on-fail` and
/// tells its failures apart by code, and `sync-base`, which brings the base in for the programmer,
/// is a task action the implement and fix roles may run in every phase from `build` to `land`.
/// Ways' copies are byte for byte the pack's.
#[test]
fn merge_starts_fix_and_sync_base_is_the_programmers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let pack = root.join("plugins/teams/actions");
    if !pack.is_dir() {
        return;
    }
    let read = |name: &str| {
        let file = pack.join(name);
        let text = fs::read_to_string(&file).unwrap_or_else(|error| panic!("{name}: {error}"));
        let ways = fs::read_to_string(root.join(".zaf/actions").join(name))
            .unwrap_or_else(|error| panic!("ways' {name}: {error}"));
        assert_eq!(text, ways, "{name}: ways' copy is the pack's");
        parse(&file, &text).unwrap_or_else(|error| panic!("{error}"))
    };

    let merge = read("merge").metadata;
    assert_eq!(merge.on_fail.as_ref().map(AsRef::as_ref), Some("fix"));
    assert_eq!(merge.codes.keys().copied().collect::<Vec<_>>(), [1, 2, 4]);
    assert!(
        merge.codes[&1].starts_with("conflicted"),
        "{:?}",
        merge.codes
    );

    let sync = read("sync-base").metadata;
    assert_eq!(sync.group, ["team", "release", "implement", "fix"]);
    assert_eq!(sync.mode, ActionMode::parse("750").expect("a mode"));
    assert_eq!(
        sync.phases
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["build", "check", "publish", "review", "land"]
    );
    assert_eq!((sync.kind, sync.ends), (None, None));
    // `sync-base` runs `merge-base` nested, under the fix or implement role it was called with.
    let merge_base = read("merge-base").metadata;
    assert_eq!(
        merge_base.group,
        ["team", "release", "implement", "fix", "merge"]
    );
    for name in ["fix", "slice-pr"] {
        read(name);
    }
}
