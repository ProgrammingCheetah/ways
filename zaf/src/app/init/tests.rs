use std::fs;
use std::io::Cursor;

use super::*;

fn run(dir: &Path, answers: &str) -> (Result<PathBuf, InitError>, String) {
    let mut output = Vec::new();
    let result = init(dir, &mut Cursor::new(answers.to_owned()), &mut output);
    (result, String::from_utf8(output).expect("utf-8 prompts"))
}

#[test]
fn scripted_answers_write_a_manifest_the_loader_accepts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, prompts) = run(dir.path(), "bookshelf\nexample/shelf\n\ncargo test --all\n");
    let path = result.expect("written");
    assert_eq!(path, dir.path().join(MANIFEST_FILE));
    assert!(prompts.contains("project name: "), "{prompts}");
    let manifest = Manifest::load(&path).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "bookshelf");
    assert_eq!(
        manifest.board.repo.as_ref().map(AsRef::as_ref),
        Some("example/shelf")
    );
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(
        forge.repo.as_ref().map(AsRef::as_ref),
        Some("example/shelf"),
        "a blank forge answer takes the board repo"
    );
    assert!(manifest.vcs.is_some());
    assert_eq!(
        manifest.gate.expect("a gate").full.as_ref(),
        "cargo test --all"
    );
}

#[test]
fn a_separate_forge_repo_and_a_blank_gate_are_kept() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, _) = run(dir.path(), "bookshelf\nexample/board\nexample/code\n\n");
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(forge.repo.as_ref().map(AsRef::as_ref), Some("example/code"));
    assert!(manifest.gate.is_none());
}

#[test]
fn quotes_in_an_answer_survive() {
    let dir = tempfile::tempdir().expect("temp dir");
    let (result, _) = run(
        dir.path(),
        "the \"shelf\"\nexample/b\n\necho \"hi\" \\ done\n",
    );
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "the \"shelf\"");
    assert_eq!(
        manifest.gate.expect("a gate").full.as_ref(),
        "echo \"hi\" \\ done"
    );
}

#[test]
fn an_existing_manifest_is_refused_untouched_and_unasked() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join(MANIFEST_FILE);
    fs::write(&path, "keep me").expect("existing manifest");
    let (result, prompts) = run(dir.path(), "a\nb\n\n\n");
    assert!(matches!(result, Err(InitError::Exists(_))));
    assert_eq!(fs::read_to_string(&path).expect("read"), "keep me");
    assert_eq!(prompts, "", "refused before the first question");
}

#[test]
fn a_blank_name_or_board_and_a_short_input_write_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert!(matches!(run(dir.path(), "\n").0, Err(InitError::Blank(_))));
    assert!(matches!(
        run(dir.path(), "a\n\n").0,
        Err(InitError::Blank(_))
    ));
    assert!(matches!(
        run(dir.path(), "a\nb/c\n").0,
        Err(InitError::Closed(_))
    ));
    assert!(!dir.path().join(MANIFEST_FILE).exists());
}
