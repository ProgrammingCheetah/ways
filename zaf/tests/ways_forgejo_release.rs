//! Forgejo's release workflow (`.forgejo/workflows/zaf-release.yml`, the design's M14): the
//! release is made as a draft whose notes a JSON encoder wrote from the tag's commit message,
//! carries `install.sh` with the tag's version, and is published last.

use std::fs;
use std::path::PathBuf;

/// The workflow as the repository ships it.
fn workflow() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join(".forgejo/workflows/zaf-release.yml");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// The byte offset of `needle`, which must be there.
fn at(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("the workflow has no `{needle}`"))
}

#[test]
fn forgejo_release_workflow_publishes_install_script() {
    let text = workflow();

    // install.sh is attached with the tag's version written in, and a script without the line fails.
    assert!(text.contains("install.sh"));
    assert!(text.contains("ZAF_VERSION=\\\"$version\\\""));
    assert!(text.contains("grep -qx 'ZAF_VERSION=\"0.0.0-dev\"' zaf/install.sh"));

    // The release is created as a draft, from an encoder's output, and published last.
    assert!(text.contains("\"draft\": true") || text.contains("\"draft\": True"));
    assert!(text.contains("jq -n --arg tag"));
    assert!(text.contains("json.dumps"));
    let create = at(&text, "--data \"@$RUNNER_TEMP/release.json\"");
    let upload = at(&text, "attachment=@$file");
    let publish = at(&text, "-X PATCH");
    assert!(create < upload && upload < publish);
    assert!(text[publish..].contains("{\"draft\":false}"));

    // No hand-built JSON holds the message, and the token stays off the command line.
    assert!(
        !text.contains("\\\"tag_name\\\""),
        "a request built by hand"
    );
    assert!(!text.contains("\\\"body\\\""), "a request built by hand");
    assert!(text.contains("curl --config -"));
    assert!(!text.contains("Authorization: token $"));
}
