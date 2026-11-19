//! The GitHub release workflow (design 0.5.2, Part D, M14): a release is made as a draft with the
//! tag's commit message as its notes, every file (`install.sh` among them) goes up, and only then
//! is it published, so "latest" never points at a release with no `install.sh`.

use std::fs;
use std::path::PathBuf;

/// The workflow as the repository ships it.
fn workflow() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".github/workflows/zaf-release.yml");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The text of the `release` job: from its key to the next job or the end.
fn release_job(text: &str) -> String {
    let mut lines = text.lines();
    lines
        .by_ref()
        .find(|line| line.trim_end() == "  release:")
        .expect("a release job");
    lines
        .take_while(|line| !line.starts_with("  ") || line.starts_with("   "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn at(job: &str, needle: &str) -> usize {
    job.find(needle)
        .unwrap_or_else(|| panic!("the release job has no `{needle}`:\n{job}"))
}

/// The version check both release workflows carry, word for word.
const VERSION_CHECK: &str = r#"version="${TAG#zaf-v}"
          case "$version" in
            ''|*[!0-9A-Za-z.+-]*) echo "tag '$TAG' holds a version that cannot go into install.sh" >&2; exit 1 ;;
          esac"#;

#[test]
fn release_workflows_check_the_version_before_use() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .to_path_buf();
    for path in [
        ".github/workflows/zaf-release.yml",
        ".forgejo/workflows/zaf-release.yml",
    ] {
        let text = fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(
            text.matches(VERSION_CHECK).count(),
            1,
            "{path}: the version check is not there exactly once"
        );
        let check = at(&text, VERSION_CHECK);
        // The version first goes into install.sh through sed, after the check.
        let write = at(&text, "sed \"s/^ZAF_VERSION");
        assert!(
            check < write,
            "{path}: the version is used before it is checked"
        );
    }
}

#[test]
fn zaf_release_workflow_publishes_install_script() {
    let job = release_job(&workflow());

    assert!(job.contains("actions/checkout@"), "no checkout:\n{job}");
    assert!(
        job.contains("ref: ${{ github.ref }}"),
        "not the tag:\n{job}"
    );
    assert!(job.contains("install.sh"), "no install.sh:\n{job}");
    assert!(job.contains("ZAF_VERSION"), "no version written:\n{job}");
    assert!(!job.contains("--generate-notes"), "generated notes:\n{job}");

    // The notes come from a file, never from the command line.
    assert!(
        job.contains("git log -1 --format=%B"),
        "no commit message:\n{job}"
    );
    assert!(
        job.contains("--notes-file"),
        "notes not from a file:\n{job}"
    );
    assert!(
        !job.contains("--notes "),
        "notes on the command line:\n{job}"
    );

    // A draft is made, every file is uploaded, and only then is the release published.
    let create = at(&job, "gh release create");
    let draft = at(&job, "--draft ");
    let upload = at(&job, "gh release upload");
    let publish = at(&job, "--draft=false");
    assert!(draft > create, "the create is not a draft:\n{job}");
    assert!(create < upload && upload < publish, "wrong order:\n{job}");
    assert!(job.contains("--clobber"), "a re-run cannot upload:\n{job}");
}
