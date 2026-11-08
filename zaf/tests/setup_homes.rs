//! The two homes through the built binary: what `zaf setup` makes, what every other verb
//! makes first, the `.zaf/.gitignore` allow-list as `git check-ignore` sees it, and
//! `zaf init`. Every run gets a temp project and a temp zaf home (`ZAF_HOME`).

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use common::{Project, outcome, run, write};

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;

const ALLOW_LIST: &str =
    "*\n!.gitignore\n!config.toml\n!actions/\n!actions/**\n!roles/\n!roles/**\n";

/// A project that provides the four `slice-*` actions, so setup has nothing missing to report.
fn project() -> Project {
    let project = Project::new(MANIFEST);
    project.provide_slice_actions();
    project
}

/// Every path under `dir` with its text (`None` for a folder), relative to `dir`.
fn tree(dir: &Path) -> BTreeMap<PathBuf, Option<String>> {
    fn walk(base: &Path, dir: &Path, found: &mut BTreeMap<PathBuf, Option<String>>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("entry").path();
            let relative = path.strip_prefix(base).expect("under base").to_owned();
            if path.is_dir() {
                found.insert(relative, None);
                walk(base, &path, found);
            } else {
                found.insert(relative, Some(fs::read_to_string(&path).expect("text")));
            }
        }
    }
    let mut found = BTreeMap::new();
    walk(dir, dir, &mut found);
    found
}

/// Whether `git check-ignore` says the project at `root` ignores `path`.
fn ignored(root: &Path, path: &str) -> bool {
    let status = Command::new("git")
        .args(["check-ignore", "-q", path])
        .current_dir(root)
        .status()
        .expect("run git");
    match status.code() {
        Some(0) => true,
        Some(1) => false,
        other => panic!("git check-ignore {path}: {other:?}"),
    }
}

#[test]
fn setup_makes_both_homes() {
    let project = project();
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");

    for dir in ["actions", "roles", "logs"] {
        assert!(project.root.join(".zaf").join(dir).is_dir(), "{dir}");
    }
    assert!(project.root.join(".zaf/.gitignore").is_file());
    assert!(project.home.join("actions").is_dir());
    assert_eq!(
        fs::read_dir(project.home.join("actions"))
            .expect("actions")
            .count(),
        0,
        "the home's actions folder starts empty"
    );
    let config = fs::read_to_string(project.home.join("config.toml")).expect("config.toml");
    assert!(!config.trim().is_empty());
    for line in config.lines() {
        assert!(
            line.is_empty() || line.starts_with('#'),
            "not a comment: {line}"
        );
    }
}

#[test]
fn setup_from_a_task_workspace_acts_on_the_root() {
    let manifest =
        format!("{MANIFEST}\n[design]\nkind = \"git-sibling\"\nrepo = \"example/b-design\"\n");
    let project = Project::new(&manifest);
    project.provide_slice_actions();
    let root = project.root.canonicalize().expect("root");
    // The design repo is already beside the root; setup run from a workspace must not look
    // for it, or clone it, beside the workspace.
    let sibling = root.parent().expect("parent").join("b-design");
    write(&sibling, "index.md", "");
    let workspaces = root.join(".zaf/workspaces");
    let workspace = workspaces.join("slice-1");
    write(&workspace, ".zaf.toml", &manifest);
    let before = tree(&workspaces);

    let (code, stdout, stderr) = outcome(&project.zaf_in(&workspace, &["setup"]));

    assert_eq!(code, 0, "{stdout}{stderr}");
    for dir in ["actions", "roles", "logs"] {
        assert!(root.join(".zaf").join(dir).is_dir(), "{dir} on the root");
    }
    assert!(root.join(".zaf/.gitignore").is_file());
    assert!(
        !workspace.join(".zaf").exists(),
        "no .zaf/ is made inside the workspace"
    );
    assert_eq!(
        tree(&workspaces),
        before,
        "nothing is made beside the workspace"
    );
    assert!(!workspaces.join("b-design").exists());
    assert!(
        project
            .gh_calls()
            .iter()
            .all(|call| !call.contains("clone")),
        "{:?}",
        project.gh_calls()
    );
}

#[test]
fn setup_is_idempotent() {
    let project = project();
    let (code, ..) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0);
    fs::write(project.home.join("config.toml"), "# my own words\n").expect("an edit");
    write(&project.root, ".zaf/roles/keep.md", "mine");
    let before = (tree(&project.root), tree(&project.home));

    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stderr.is_empty(),
        "nothing was made, nothing said: {stderr}"
    );
    assert_eq!((tree(&project.root), tree(&project.home)), before);
    assert_eq!(
        fs::read_to_string(project.home.join("config.toml")).expect("config.toml"),
        "# my own words\n"
    );
}

#[test]
fn gitignore_keeps_actions_and_ignores_state() {
    let project = project();
    run(&project.root, "git", &["init", "--quiet"]);
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        fs::read_to_string(project.root.join(".zaf/.gitignore")).expect(".gitignore"),
        ALLOW_LIST
    );

    for tracked in [
        ".zaf/actions/x",
        ".zaf/roles/x",
        ".zaf/config.toml",
        ".zaf/.gitignore",
    ] {
        if tracked != ".zaf/.gitignore" {
            write(&project.root, tracked, "x");
        }
        assert!(
            !ignored(&project.root, tracked),
            "{tracked} must be tracked"
        );
    }
    for state in [
        ".zaf/local.toml",
        ".zaf/logs/x",
        ".zaf/runs/x",
        ".zaf/profiles/x",
        ".zaf/workspaces/x",
        ".zaf/slices/x",
        ".zaf/spawn.state",
        ".zaf/guard.json",
        ".zaf/asklog.jsonl",
    ] {
        write(&project.root, state, "x");
        assert!(ignored(&project.root, state), "{state} must be ignored");
    }
}

#[test]
fn old_star_gitignore_is_replaced() {
    let project = project();
    write(&project.root, ".zaf/.gitignore", "*\n");
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        fs::read_to_string(project.root.join(".zaf/.gitignore")).expect(".gitignore"),
        ALLOW_LIST
    );

    // Anything else is the Engineer's: kept, with a worksheet step to merge by hand.
    let mine = "# mine\n*\n!notes/\n";
    write(&project.root, ".zaf/.gitignore", mine);
    let (code, stdout, _) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("Merge these lines"), "{stdout}");
    assert!(stdout.contains("!actions/**"), "{stdout}");
    assert_eq!(
        fs::read_to_string(project.root.join(".zaf/.gitignore")).expect(".gitignore"),
        mine
    );
}

#[test]
fn first_run_keeps_an_old_gitignore() {
    let project = project();
    write(&project.root, ".zaf/.gitignore", "*\n");
    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        fs::read_to_string(project.root.join(".zaf/.gitignore")).expect(".gitignore"),
        "*\n",
        "only an explicit setup replaces it"
    );
    assert!(
        project.root.join(".zaf/actions").is_dir(),
        "the folders are still made"
    );

    // Absent, a first run writes the allow-list.
    let fresh = self::project();
    let (code, stdout, stderr) = outcome(&fresh.zaf(&["doctor"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        fs::read_to_string(fresh.root.join(".zaf/.gitignore")).expect(".gitignore"),
        ALLOW_LIST
    );
}

#[test]
fn root_gitignore_makes_a_worksheet_step() {
    let project = project();
    write(&project.root, ".gitignore", ".zaf/\n");
    let (code, stdout, _) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 1, "{stdout}");
    assert!(stdout.contains("root `.gitignore`"), "{stdout}");
    assert!(stdout.contains("Done when:"), "{stdout}");
    assert_eq!(
        fs::read_to_string(project.root.join(".gitignore")).expect("root ignore"),
        ".zaf/\n",
        "setup never edits the Engineer's own file"
    );

    // `.zaf/*` keeps the allow-list working, so it is no step.
    write(&project.root, ".gitignore", ".zaf/*\n");
    let (code, stdout, stderr) = outcome(&project.zaf(&["setup"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
}

#[test]
fn verb_creates_a_missing_home_first() {
    let mut project = project();
    let home = project.temp.path().join("fresh-home");
    project.env("ZAF_HOME", &home.to_string_lossy());

    let first = project.zaf(&["doctor"]);
    let (code, first_out, first_err) = outcome(&first);
    assert_eq!(code, 0, "{first_out}{first_err}");
    assert_eq!(
        first_err.lines().count(),
        1,
        "one line on stderr: {first_err:?}"
    );
    assert!(project.root.join(".zaf/roles").is_dir());
    assert!(home.join("actions").is_dir());
    assert!(home.join("config.toml").is_file());

    let (code, second_out, second_err) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 0, "{second_out}{second_err}");
    assert_eq!(second_err, "", "nothing is missing the second time");
    assert_eq!(
        first_out, second_out,
        "stdout is the same with and without the first run"
    );

    // Outside a project only the zaf home is made.
    let outside = project.temp.path().join("elsewhere");
    fs::create_dir(&outside).expect("outside dir");
    let other_home = project.temp.path().join("other-home");
    project.env("ZAF_HOME", &other_home.to_string_lossy());
    let (code, ..) = outcome(&project.zaf_in(&outside, &["doctor"]));
    assert_ne!(code, 0, "no project here");
    assert!(other_home.join("actions").is_dir());
    assert!(!outside.join(".zaf").exists());
}

/// Runs `zaf init` in `dir` with `answers` on stdin and the zaf home at `home`.
fn init_in(dir: &Path, home: &Path, answers: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zaf"))
        .arg("init")
        .current_dir(dir)
        .env("ZAF_HOME", home)
        .env_remove("ZAF_INSTANCE")
        .env_remove("ZAF_VERSION")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn zaf init");
    // A `zaf init` that exits early closes the pipe: the write may fail with a broken pipe,
    // and the child is judged by its exit and output, not by the write.
    let mut stdin = child.stdin.take().expect("stdin");
    let _ = stdin.write_all(answers.as_bytes());
    drop(stdin);
    child.wait_with_output().expect("zaf init")
}

#[test]
fn init_writes_dot_zaf_toml() {
    let temp = tempfile::tempdir().expect("temp dir");
    let home = temp.path().join("zaf-home");
    let dir = temp.path().join("new-project");
    fs::create_dir(&dir).expect("project dir");

    let answers = "bookshelf\nexample/b\n\n\n";
    let (code, stdout, stderr) = outcome(&init_in(&dir, &home, answers));
    assert_eq!(code, 0, "{stdout}{stderr}");
    // The prompts end without a newline, so the confirmation closes the last line.
    assert!(stdout.ends_with("wrote .zaf.toml\n"), "{stdout}");
    let text = fs::read_to_string(dir.join(".zaf.toml")).expect(".zaf.toml");
    assert!(text.contains("name = \"bookshelf\""), "{text}");
    assert!(!dir.join("ZAF.toml").exists());
    assert!(
        !dir.join(".zaf").exists(),
        "init makes the manifest, not the homes"
    );

    // It refuses when any manifest file exists, and leaves it as it is.
    let (code, _, stderr) = outcome(&init_in(&dir, &home, answers));
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains(".zaf.toml"), "{stderr}");
    assert_eq!(
        fs::read_to_string(dir.join(".zaf.toml")).expect(".zaf.toml"),
        text
    );
    for existing in [".zaf/config.toml", "ZAF.toml"] {
        let other = temp
            .path()
            .join(format!("with-{}", existing.replace('/', "-")));
        fs::create_dir(&other).expect("other dir");
        write(&other, existing, "# mine\n");
        let (code, _, stderr) = outcome(&init_in(&other, &home, answers));
        assert_eq!(code, 1, "{existing}: {stderr}");
        assert!(!other.join(".zaf.toml").exists(), "{existing}");
    }

    // The zaf home's own config.toml is the machine's, not a project's.
    let inside = temp.path().join("home-inside");
    fs::create_dir(&inside).expect("inside dir");
    write(&inside, ".zaf/config.toml", "# machine\n");
    let (code, stdout, stderr) = outcome(&init_in(&inside, &inside.join(".zaf"), answers));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(inside.join(".zaf.toml").is_file());
}
