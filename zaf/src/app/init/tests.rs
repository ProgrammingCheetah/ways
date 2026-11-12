use std::fs;
use std::io::Cursor;
use std::process::Command;

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
    assert!(prompts.contains("project name ["), "{prompts}");
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
    assert!(matches!(
        run(dir.path(), "\n\n").0,
        Err(InitError::Blank(_))
    ));
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

/// A folder `zabo` holding a git repo, with `origin` at `url` when given.
fn repo_folder(url: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().expect("temp dir");
    let dir = root.path().join("zabo");
    fs::create_dir(&dir).expect("folder");
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(&dir)
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "-q"]);
    if let Some(url) = url {
        git(&["remote", "add", "origin", url]);
    }
    (root, dir)
}

#[test]
fn init_defaults_from_folder_and_remote() {
    for url in ["git@github.com:o/zabo.git", "https://github.com/o/zabo.git"] {
        let (_root, dir) = repo_folder(Some(url));
        let (result, prompts) = run(&dir, "\n\n\n\n");
        let manifest = Manifest::load(&result.expect("written")).expect("loads");
        assert_eq!(manifest.project.name.as_ref(), "zabo", "{url}");
        assert_eq!(
            manifest.board.repo.as_ref().map(AsRef::as_ref),
            Some("o/zabo"),
            "{url}"
        );
        let forge = manifest.forge.expect("a forge section");
        assert_eq!(
            forge.repo.as_ref().map(AsRef::as_ref),
            Some("o/zabo"),
            "{url}"
        );
        assert!(manifest.gate.is_none(), "{url}");
        assert!(prompts.contains("project name [zabo]: "), "{prompts}");
        assert!(
            prompts.contains("board repo (owner/name) [o/zabo]: "),
            "{prompts}"
        );
    }
}

#[test]
fn init_typed_answer_wins() {
    let (_root, dir) = repo_folder(Some("git@github.com:o/zabo.git"));
    let (result, _) = run(&dir, "shelf\nx/board\nx/code\ncargo test\n");
    let manifest = Manifest::load(&result.expect("written")).expect("loads");
    assert_eq!(manifest.project.name.as_ref(), "shelf");
    assert_eq!(
        manifest.board.repo.as_ref().map(AsRef::as_ref),
        Some("x/board")
    );
    let forge = manifest.forge.expect("a forge section");
    assert_eq!(forge.repo.as_ref().map(AsRef::as_ref), Some("x/code"));
    assert_eq!(manifest.gate.expect("a gate").full.as_ref(), "cargo test");
}

#[test]
fn init_no_remote_requires_repo() {
    let (_root, dir) = repo_folder(None);
    let (result, _) = run(&dir, "\n\n\n\n");
    assert!(
        matches!(result, Err(InitError::Blank(question)) if question.starts_with("board repo")),
        "a blank board repo is refused by name"
    );
    assert!(!dir.join(MANIFEST_FILE).exists());

    let (_root, dir) = repo_folder(Some("git@gitlab.com:o/zabo.git"));
    let (result, _) = run(&dir, "\n\n\n\n");
    assert!(
        matches!(result, Err(InitError::Blank(_))),
        "not GitHub: no default"
    );
    assert!(!dir.join(MANIFEST_FILE).exists());
}

#[test]
fn github_urls_read_as_owner_name() {
    for url in [
        "git@github.com:o/n.git",
        "git@github.com:o/n",
        "ssh://git@github.com/o/n.git",
        "https://github.com/o/n.git",
        "https://github.com/o/n/",
        "https://user:tok@github.com/o/n.git\n",
    ] {
        assert_eq!(github_repo(url).as_deref(), Some("o/n"), "{url}");
    }
    for url in [
        "git@gitlab.com:o/n.git",
        "https://github.com/o",
        "/srv/repo.git",
        "",
    ] {
        assert_eq!(github_repo(url), None, "{url}");
    }
}
