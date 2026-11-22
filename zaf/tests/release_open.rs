//! The teams pack's `release-open` action (I12): run through `zaf do` on a throwaway jj repository
//! with a bare remote and the fake `gh` on `PATH`, it opens the pull request from the version's
//! integration branch to the forge's default branch and hands back its number on line 1 and its
//! link on line 2. One already open is handed back and nothing new is opened; an integration
//! branch that is not on the remote is refused, and a forge that fails is a backend failure.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use zaf::app::build::open_release_with_home;
use zaf::composition::Composition;

use common::{Project, REPO, fixtures, jj, outcome, path_with_zaf, run, write};

const VERSION: &str = "1.2.0";
const BRANCH: &str = "feature/1.2.0";

const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"

[forge]
kind = "github"
repo = "example/tracker"

[vcs]
kind = "jj"

[branches]
integration = "feature/{version}"
task = "feature/{version}-{epic}-{task}"

[roles.team]
model = "opus"
effort = "xhigh"
profile = "build"
turns = 200
batch = 1

[roles.release]
model = "opus"
effort = "xhigh"
profile = "build"
turns = 200
"#;

/// The action as the teams pack ships it.
fn action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join("plugins/teams/actions/release-open");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a jj repository with a bare remote, the fake `gh` first on `PATH` and
/// the action in its actions folder. The integration branch is pushed when `pushed` says so.
fn project(pushed: bool) -> Project {
    let mut project = Project::new(MANIFEST);
    let temp = project.temp.path().to_owned();
    let root = project.root.clone();
    run(&temp, "git", &["init", "-q", "--bare", "remote.git"]);
    jj(&temp, &["git", "init", "repo"]);
    jj(
        &root,
        &["config", "set", "--repo", "user.name", "Test Author"],
    );
    jj(
        &root,
        &[
            "config",
            "set",
            "--repo",
            "user.email",
            "author@example.com",
        ],
    );
    let remote = temp.join("remote.git");
    jj(
        &root,
        &["git", "remote", "add", "origin", &remote.to_string_lossy()],
    );
    write(&root, ".gitignore", ".zaf/*\n");
    write(&root, "notes.txt", "one\ntwo\n");
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", BRANCH, "-r", "@-"]);
    if pushed {
        jj(&root, &["git", "push", "--bookmark", BRANCH]);
    }

    project.env("ZAF_VERSION", VERSION);
    project.env("PATH", &path_with_zaf(&[&fixtures()]));
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = root.join(".zaf/actions");
    write(&actions, "release-open", &action_text());
    fs::set_permissions(
        actions.join("release-open"),
        fs::Permissions::from_mode(0o750),
    )
    .expect("mode");
    project
}

/// `zaf do release-open <version>`: the exit code, what it printed and what it said.
fn release_open(project: &Project) -> (i32, String, String) {
    let output = project.zaf(&["do", "--more", "release-open", VERSION]);
    outcome(&output)
}

/// The action's own two lines, from the end of its log: the number, then the link.
fn handed_back(text: &str) -> (String, String) {
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    let [.., number, link] = lines.as_slice() else {
        panic!("no two lines handed back: {text}");
    };
    ((*number).to_owned(), (*link).to_owned())
}

fn creates(project: &Project) -> Vec<String> {
    project
        .gh_calls()
        .into_iter()
        .filter(|call| call.starts_with("pr create"))
        .collect()
}

#[test]
fn release_open_opens_one() {
    let mut project = project(true);
    let url = format!("https://github.com/{REPO}/pull/9");
    project
        .on("api *", "main\n")
        .on("pr list *", "")
        .on("pr create *", &format!("{url}\n"));

    let (code, stdout, stderr) = release_open(&project);

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(handed_back(&stdout), ("9".to_owned(), url), "{stdout}");
    // One pull request, from the integration branch into the forge's default branch.
    let made = creates(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(
        made[0].starts_with(&format!(
            "pr create --head {BRANCH} --base main --title Release"
        )),
        "{made:?}"
    );
}

#[test]
fn release_open_hands_back_open_one() {
    let mut project = project(true);
    let url = format!("https://github.com/{REPO}/pull/5");
    project
        .on("api *", "main\n")
        .on("pr list *", &format!("5\t{url}\n"));

    let (code, stdout, stderr) = release_open(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(
        handed_back(&stdout),
        ("5".to_owned(), url.clone()),
        "{stdout}"
    );

    // Run again: the same number and link, and nothing opened either time.
    let (code, stdout, stderr) = release_open(&project);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(handed_back(&stdout), ("5".to_owned(), url), "{stdout}");
    assert_eq!(creates(&project), Vec::<String>::new());
}

#[test]
fn release_open_refuses_without_branch() {
    let mut project = project(false);
    project.on("api *", "main\n").on("pr list *", "");

    let (code, stdout, stderr) = release_open(&project);

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stdout.contains("is not on the remote"), "{stdout}{stderr}");
    // The forge was never asked.
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn release_open_refuses_a_malformed_version() {
    let project = project(true);
    let output = project.zaf(&["do", "--more", "release-open", "--", "-1.2;x"]);
    let (code, stdout, stderr) = outcome(&output);

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}

#[test]
fn release_open_passes_doctor() {
    let mut project = project(true);
    project.provide_slice_actions();
    project.on("*", "main\n");

    let (_, stdout, stderr) = outcome(&project.zaf(&["doctor"]));

    // The doctor's own exit also reads the machine's logins, which no test has: the action's
    // line is what it says of this one.
    let text = format!("{stdout}{stderr}");
    assert!(
        text.contains("action release-open (project): ok, mode 750"),
        "{text}"
    );
}

#[test]
fn release_open_backend_failure_exits_2() {
    let mut project = project(true);
    project
        .on("api *", "main\n")
        .on("pr list *", "")
        .fail_on("pr create *", "the forge is down");

    let (code, stdout, stderr) = release_open(&project);

    assert_eq!(code, 2, "{stdout}{stderr}");
}

#[test]
fn release_open_a_forge_with_no_number_exits_2() {
    let mut project = project(true);
    project
        .on("api *", "main\n")
        .on("pr list *", "")
        .on("pr create *", "nothing useful\n");

    let (code, stdout, stderr) = release_open(&project);

    assert_eq!(code, 2, "{stdout}{stderr}");
}

/// The release pull request through the build's own reader: the shipped action runs under
/// `open_release` with the fake `gh`, and what it said on stderr comes before the number in its log.
#[test]
fn open_release_reads_the_number_past_stderr_lines() {
    let mut project = project(true);
    let url = format!("https://github.com/{REPO}/pull/5");
    project
        .on("api *", "main\n")
        .on("pr list *", &format!("5\t{url}\n"));
    // A line on stderr straight before the action's two, as another tool would write it.
    let action = project.root.join(".zaf/actions/release-open");
    let text = fs::read_to_string(&action).expect("action");
    let text = text.replace(
        "echo \"$number\"\n",
        "echo \"Nothing changed.\" >&2\necho \"$number\"\n",
    );
    assert!(
        text.contains("Nothing changed."),
        "the action's tail changed"
    );
    fs::write(&action, text).expect("action");
    // The run's environment: the fake `gh` first, then the built `zaf`.
    let scene = project.temp.path().join("gh-scene");
    let manifest = format!(
        "{MANIFEST}\n[env]\nPATH = \"{}\"\nFAKE_GH_DIR = \"{}\"\n",
        path_with_zaf(&[&fixtures()]),
        scene.display()
    );
    fs::write(project.root.join(".zaf.toml"), manifest).expect("manifest");
    common::no_zaf_link_folder(&project.home);

    let composition =
        Composition::load_with_home(&project.root, Some(&project.home)).expect("the manifest");
    let opened = open_release_with_home(&composition, VERSION, &project.home).expect("opened");

    assert_eq!(opened.number, zaf::ports::PrNumber::from(5));
    assert_eq!(opened.link.as_deref(), Some(url.as_str()));
}

/// The action as the teams pack ships its git version.
fn git_action_text() -> String {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits inside the repository")
        .join("plugins/teams/actions/release-open.git");
    fs::read_to_string(&file).unwrap_or_else(|error| panic!("{}: {error}", file.display()))
}

/// A project whose root is a plain git repository (no jj) with a bare `origin`, the fake `gh`
/// first on `PATH` and the git action in its actions folder. The integration branch is pushed to
/// `origin` when `pushed` says so.
fn git_project(pushed: bool) -> Project {
    let manifest = MANIFEST.replace("kind = \"jj\"", "kind = \"git\"");
    let mut project = Project::new(&manifest);
    let temp = project.temp.path().to_owned();
    let root = project.root.clone();
    run(&temp, "git", &["init", "-q", "--bare", "remote.git"]);
    run(&root, "git", &["init", "-q", "-b", "main"]);
    run(&root, "git", &["config", "user.name", "Test Author"]);
    run(
        &root,
        "git",
        &["config", "user.email", "author@example.com"],
    );
    let remote = temp.join("remote.git");
    run(
        &root,
        "git",
        &["remote", "add", "origin", &remote.to_string_lossy()],
    );
    write(&root, ".gitignore", ".zaf/*\n");
    write(&root, "notes.txt", "one\ntwo\n");
    run(&root, "git", &["add", "-A"]);
    run(&root, "git", &["commit", "-q", "-m", "Start the project"]);
    run(&root, "git", &["branch", BRANCH]);
    if pushed {
        run(&root, "git", &["push", "-q", "origin", BRANCH]);
    }

    project.env("ZAF_VERSION", VERSION);
    project.env("PATH", &path_with_zaf(&[&fixtures()]));
    // Any verb makes the missing homes and says so; do it now so the notice stays out of a run.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let actions = root.join(".zaf/actions");
    write(&actions, "release-open.git", &git_action_text());
    fs::set_permissions(
        actions.join("release-open.git"),
        fs::Permissions::from_mode(0o750),
    )
    .expect("mode");
    project
}

#[test]
fn git_opens_the_release_pull_request() {
    let mut project = git_project(true);
    let url = format!("https://github.com/{REPO}/pull/9");
    project
        .on("api *", "main\n")
        .on("pr list *", "")
        .on("pr create *", &format!("{url}\n"));

    let (code, stdout, stderr) = release_open(&project);

    assert_eq!(code, 0, "{stdout}{stderr}");
    assert_eq!(handed_back(&stdout), ("9".to_owned(), url), "{stdout}");
    let made = creates(&project);
    assert_eq!(made.len(), 1, "{made:?}");
    assert!(
        made[0].starts_with(&format!(
            "pr create --head {BRANCH} --base main --title Release"
        )),
        "{made:?}"
    );
}

#[test]
fn git_hands_back_the_open_one() {
    let mut project = git_project(true);
    let url = format!("https://github.com/{REPO}/pull/5");
    project
        .on("api *", "main\n")
        .on("pr list *", &format!("5\t{url}\n"));

    for _ in 0..2 {
        let (code, stdout, stderr) = release_open(&project);
        assert_eq!(code, 0, "{stdout}{stderr}");
        assert_eq!(
            handed_back(&stdout),
            ("5".to_owned(), url.clone()),
            "{stdout}"
        );
    }
    assert_eq!(creates(&project), Vec::<String>::new());
}

#[test]
fn git_refuses_without_the_branch_on_origin() {
    let mut project = git_project(false);
    project.on("api *", "main\n").on("pr list *", "");

    let (code, stdout, stderr) = release_open(&project);

    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stdout.contains("is not on the remote"), "{stdout}{stderr}");
    // The forge was never asked.
    assert_eq!(project.gh_calls(), Vec::<String>::new());
}
