use std::path::{Path, PathBuf};

use super::*;
use crate::domain::Profile;
use crate::manifest::{
    BranchPattern, CommandLine, Effort, ModelName, ProjectName, RepoName, Rung, RunnerName,
};

const MINIMAL: &str = r#"
[project]
name = "bookshelf"

[board]
kind = "github-issues"
repo = "example/bookshelf-board"
"#;

const FULL: &str = r#"
[project]
name = "bookshelf"

[requires]
zaf = "2.0.0"
toolkit = "toolkit-v2.0.0"
assistant = ">=2.1"
jj = "*"

[board]
kind = "github-issues"
repo = "example/bookshelf-board"

[vcs]
kind = "jj"

[forge]
kind = "github"
repo = "example/bookshelf"

[design]
kind = "git-sibling"
repo = "example/bookshelf-design"
path = "../bookshelf-design"
index = "docs/index.md"

[workspaces]
path = "../bookshelf-workspaces"

[branches]
integration = "release/{version}"
task = "release/{version}-{task}"

[gate]
full = "just gate"
light = "just gate-light"
tests = "cargo test -p {crate} {name}"

[runners.reviewer]
command = "review-cli -p {prompt} --add-dir {dir}"

[roles.programmer]
model = "small-model"
effort = "medium"
profile = "lean"
turns = 40

[check.pr]
steps = ["ci", "bot:reviewer", "runner:reviewer:critique"]

[ask]
ladder = ["local", "haiku", "sonnet"]
max = 3

[spawn]
slots = 2
guard = [60, 80, 90]

[isolation]
kind = "container"
image = "example/runner:1"

[forbidden]
commands = ["rm -rf /"]
"#;

fn load(text: &str) -> Result<Manifest, ManifestError> {
    text.parse::<Manifest>()
}

fn with(extra: &str) -> String {
    format!("{MINIMAL}\n{extra}")
}

#[test]
fn project_and_board_alone_load_with_the_pack_defaults() {
    let manifest = load(MINIMAL).expect("the minimal manifest loads");
    assert_eq!(manifest.project.name, ProjectName::from("bookshelf"));
    assert_eq!(
        manifest.board.repo,
        Some(RepoName::from("example/bookshelf-board"))
    );
    assert!(manifest.requires.is_empty());
    assert_eq!(manifest.vcs, None);
    assert_eq!(manifest.forge, None);
    assert_eq!(manifest.gate, None);
    assert_eq!(manifest.design.kind, DesignKind::None);
    assert_eq!(manifest.workspaces.path, PathBuf::from(".zaf/workspaces"));
    assert_eq!(
        manifest.branches.integration,
        BranchPattern::from("feature/{version}")
    );
    assert_eq!(
        manifest.branches.task,
        BranchPattern::from("feature/{version}-{epic}-{task}")
    );
    assert!(manifest.runners.is_empty());
    assert!(manifest.check.is_empty());
    assert_eq!(
        manifest.ask.ladder,
        [Rung::from("haiku"), Rung::from("sonnet")]
    );
    assert_eq!(manifest.ask.max, 5);
    assert_eq!(manifest.ask.cache_hours, 24);
    assert_eq!(manifest.style, None);
    assert_eq!(manifest.spawn.slots, 1);
    assert_eq!(manifest.spawn.guard, [70, 85, 95]);
    assert_eq!(manifest.isolation.kind, IsolationKind::Process);
    assert!(manifest.forbidden.commands.is_empty());
    let role_names: Vec<&str> = manifest.roles.keys().map(AsRef::as_ref).collect();
    assert_eq!(role_names, ["programmer", "team"]);
    let team = &manifest.roles[&RoleName::from("team")];
    assert_eq!(team.profile, Profile::Build);
    assert_eq!(team.batch, Some(4));
}

#[test]
fn every_section_reads() {
    let manifest = load(FULL).expect("the full manifest loads");
    assert_eq!(manifest.requires.len(), 4);
    assert_eq!(manifest.design.kind, DesignKind::GitSibling);
    assert_eq!(
        manifest.gate.map(|gate| gate.light),
        Some(Some(CommandLine::from("just gate-light")))
    );
    assert_eq!(manifest.check[&CheckTarget::Pr].steps.len(), 3);
    assert_eq!(manifest.ask.max, 3);
    assert_eq!(
        manifest.isolation.image.as_deref(),
        Some("example/runner:1")
    );
    let programmer = &manifest.roles[&RoleName::from("programmer")];
    assert_eq!(programmer.model, ModelName::from("small-model"));
    assert_eq!(programmer.effort, Effort::Medium);
    assert!(
        manifest.roles.contains_key(&RoleName::from("team")),
        "an unnamed pack role is filled in"
    );
}

#[test]
fn requires_versions_parse() {
    let manifest = load(FULL).expect("the full manifest loads");
    let tag = &manifest.requires[&ToolName::from("toolkit")];
    let expected_tag = "toolkit-v2.0.0".parse::<Requirement>().expect("a tag");
    assert_eq!(tag, &expected_tag);
    assert!(matches!(
        manifest.requires[&ToolName::from("jj")],
        Requirement::Version(_)
    ));
    assert!(matches!(
        manifest.requires[&ToolName::from("assistant")],
        Requirement::Version(_)
    ));
    let bad = with("[requires]\nzaf = \"latest\"\n");
    let message = load(&bad).expect_err("a bad requirement fails").to_string();
    assert!(
        message.contains("latest"),
        "the error names the value: {message}"
    );
}

#[test]
fn an_unknown_key_is_an_error_that_names_it() {
    let unknown_in_section = MINIMAL.replace(
        "kind = \"github-issues\"",
        "kind = \"github-issues\"\ncolour = \"blue\"",
    );
    let message = load(&unknown_in_section)
        .expect_err("an unknown key fails")
        .to_string();
    assert!(
        message.contains("colour"),
        "the error names the key: {message}"
    );
    let unknown_section = with("[extras]\nanything = 1\n");
    let message = load(&unknown_section)
        .expect_err("an unknown section fails")
        .to_string();
    assert!(
        message.contains("extras"),
        "the error names the section: {message}"
    );
    let unknown_check_target = with("[check.nightly]\nsteps = []\n");
    let message = load(&unknown_check_target)
        .expect_err("an unknown target fails")
        .to_string();
    assert!(
        message.contains("nightly"),
        "the error names the target: {message}"
    );
}

#[test]
fn project_and_board_are_required() {
    let without_board = "[project]\nname = \"bookshelf\"\n";
    let message = load(without_board).expect_err("no board fails").to_string();
    assert!(message.contains("board"), "{message}");
    let without_project = "[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";
    let message = load(without_project)
        .expect_err("no project fails")
        .to_string();
    assert!(message.contains("project"), "{message}");
}

#[test]
fn a_kind_needs_its_keys() {
    let board_without_repo = "[project]\nname = \"b\"\n[board]\nkind = \"github-issues\"\n";
    assert!(matches!(
        load(board_without_repo),
        Err(ManifestError::MissingKey {
            section: "board",
            key: "repo",
            ..
        })
    ));
    let repo_dir_without_path = with("[design]\nkind = \"repo-dir\"\n");
    assert!(matches!(
        load(&repo_dir_without_path),
        Err(ManifestError::MissingKey {
            section: "design",
            key: "path",
            ..
        })
    ));
    let container_without_image = with("[isolation]\nkind = \"container\"\n");
    assert!(matches!(
        load(&container_without_image),
        Err(ManifestError::MissingKey {
            section: "isolation",
            key: "image",
            ..
        })
    ));
    let unknown_kind = with("[vcs]\nkind = \"tarball\"\n");
    let message = load(&unknown_kind)
        .expect_err("an unknown kind fails")
        .to_string();
    assert!(message.contains("tarball"), "{message}");
}

#[test]
fn machine_paths_and_out_of_range_values_are_rejected() {
    let absolute = with("[workspaces]\npath = \"/home/someone/workspaces\"\n");
    assert!(matches!(
        load(&absolute),
        Err(ManifestError::AbsolutePath {
            section: "workspaces",
            ..
        })
    ));
    let shared_branch = with("[branches]\ntask = \"feature/{version}\"\n");
    assert!(matches!(
        load(&shared_branch),
        Err(ManifestError::Invalid { key: "task", .. })
    ));
    let descending_guard = with("[spawn]\nguard = [90, 80]\n");
    assert!(matches!(
        load(&descending_guard),
        Err(ManifestError::Invalid { key: "guard", .. })
    ));
    let no_slots = with("[spawn]\nslots = 0\n");
    assert!(matches!(
        load(&no_slots),
        Err(ManifestError::Invalid { key: "slots", .. })
    ));
    let too_many_questions = with("[ask]\nmax = 6\n");
    assert!(matches!(
        load(&too_many_questions),
        Err(ManifestError::Invalid { key: "max", .. })
    ));
    let empty_ladder = with("[ask]\nladder = []\n");
    assert!(matches!(
        load(&empty_ladder),
        Err(ManifestError::Invalid { key: "ladder", .. })
    ));
}

#[test]
fn a_partial_section_keeps_the_defaults_it_leaves_out() {
    let only_slots = with("[spawn]\nslots = 3\n");
    let manifest = load(&only_slots).expect("loads");
    assert_eq!(manifest.spawn.slots, 3);
    assert_eq!(manifest.spawn.guard, [70, 85, 95]);
}

#[test]
fn style_cache_hours_and_runner_timeout_read_and_default() {
    let declared = with(
        "[style]\npath = \"docs/style.md\"\n[ask]\ncache_hours = 6\n\
         [runners.reviewer]\ncommand = \"review\"\ntimeout_minutes = 10\n\
         [runners.plain]\ncommand = \"plain\"\n",
    );
    let manifest = load(&declared).expect("loads");
    assert_eq!(
        manifest.style.map(|style| style.path),
        Some(PathBuf::from("docs/style.md"))
    );
    assert_eq!(manifest.ask.cache_hours, 6);
    assert_eq!(
        manifest.ask.max, 5,
        "a partial [ask] keeps its other defaults"
    );
    assert_eq!(
        manifest.runners[&RunnerName::from("reviewer")].timeout_minutes,
        10
    );
    assert_eq!(
        manifest.runners[&RunnerName::from("plain")].timeout_minutes,
        45
    );
}

#[test]
fn the_new_keys_are_checked() {
    let absolute = with("[style]\npath = \"/home/someone/style.md\"\n");
    assert!(matches!(
        load(&absolute),
        Err(ManifestError::AbsolutePath {
            section: "style",
            ..
        })
    ));
    let no_path = with("[style]\n");
    assert!(load(&no_path).is_err(), "[style] needs its path");
    let no_time = with("[runners.slow]\ncommand = \"x\"\ntimeout_minutes = 0\n");
    assert!(matches!(
        load(&no_time),
        Err(ManifestError::Invalid {
            key: "timeout_minutes",
            ..
        })
    ));
    let unknown = with("[style]\npath = \"s.md\"\nformat = \"md\"\n");
    assert!(load(&unknown).is_err(), "unknown keys still fail");
}

#[test]
fn a_design_path_is_relative_to_the_project_and_a_sibling_defaults_to_the_repo_name() {
    let root = Path::new("/work/bookshelf");
    let design = |text: &str| load(&with(text)).expect("loads").design;
    let beside = design(
        "[design]\nkind = \"git-sibling\"\nrepo = \"example/bookshelf-design\"\npath = \"../bookshelf-design\"\n",
    );
    assert_eq!(
        beside.dir(root),
        Some(PathBuf::from("/work/bookshelf/../bookshelf-design"))
    );
    let defaulted =
        design("[design]\nkind = \"git-sibling\"\nrepo = \"example/bookshelf-design.git\"\n");
    assert_eq!(
        defaulted.dir(root),
        Some(PathBuf::from("/work/bookshelf-design"))
    );
    let inside = load(&with("[design]\nkind = \"repo-dir\"\npath = \"docs\"\n"))
        .expect("loads")
        .design;
    assert_eq!(
        inside.dir(root),
        Some(PathBuf::from("/work/bookshelf/docs"))
    );
    assert_eq!(DesignSection::default().dir(root), None);
}

#[test]
fn the_pack_comes_from_a_path_or_a_tag_never_both() {
    let none = load(MINIMAL).expect("loads");
    assert_eq!(none.pack, None);
    let path = load(&with("[pack]\npath = \"plugins/kit\"\n")).expect("loads");
    let pack = path.pack.expect("declared");
    assert_eq!(
        pack.dir(Path::new("/work/project")),
        Some(PathBuf::from("/work/project/plugins/kit"))
    );
    let tag = load(&with("[pack]\ntag = \"kit--v1.2.0\"\n")).expect("loads");
    assert_eq!(
        tag.pack.expect("declared").tag.as_deref(),
        Some("kit--v1.2.0")
    );
    for text in ["[pack]\n", "[pack]\npath = \"p\"\ntag = \"t\"\n"] {
        assert!(
            matches!(
                load(&with(text)),
                Err(ManifestError::Invalid {
                    section: "pack",
                    ..
                })
            ),
            "{text}"
        );
    }
    assert!(matches!(
        load(&with("[pack]\npath = \"/abs/kit\"\n")),
        Err(ManifestError::AbsolutePath {
            section: "pack",
            ..
        })
    ));
    assert!(load(&with("[pack]\nbranch = \"x\"\n")).is_err());
}
