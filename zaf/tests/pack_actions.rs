//! The teams pack carries the 17 standard actions a project copies into its `.zaf/actions/`
//! (docs/zaf-actions.md §2, §7). Every file in `plugins/teams/actions/` is executable, parses as an
//! action, and has the mode, group and phases of the ways repository's copy. Outside the ways
//! repository, where the pack is not beside the crate, nothing runs.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use zaf::app::actions::{PhaseEnd, parse};

const NAMES: [&str; 17] = [
    "ci",
    "ci-wait",
    "commit",
    "fix",
    "implement",
    "merge",
    "merge-base",
    "pr-merge",
    "pr-open",
    "push",
    "review",
    "slice-drop",
    "slice-merge",
    "slice-open",
    "slice-pr",
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
    assert_eq!(found, NAMES, "the pack holds exactly the 17 actions");

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
