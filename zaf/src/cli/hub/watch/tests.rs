use std::os::unix::fs::PermissionsExt;
use std::process::Command as Run;
use std::time::{Duration, Instant, UNIX_EPOCH};

use super::*;
use crate::app::spawn::record::watch_running;
use crate::app::spawn::watch::Watches;
use crate::domain::DocumentSignature;

const MANIFEST: &str = "[project]\nname = \"example\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n[vcs]\nkind = \"jj\"\n";
const FOLDER: &str = ".teams/releases/0.4.0";

/// A stand-in for `zaf` that runs `body` as a shell script, in a folder of its own.
fn fake_zaf(dir: &Path, body: &str) -> PathBuf {
    let file = dir.join("zaf");
    std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).expect("script");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("mode");
    file
}

fn process(program: PathBuf, root: &Path) -> Process {
    Process {
        program,
        root: root.to_owned(),
        version: "0.4.0".to_owned(),
        home: None,
    }
}

fn run(dir: &Path, program: &str, args: &[&str]) {
    let output = Run::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn");
    assert!(
        output.status.success(),
        "{program} {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn jj(dir: &Path, args: &[&str]) {
    let mut full = vec!["--no-pager", "--color=never"];
    full.extend_from_slice(args);
    run(dir, "jj", &full);
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, text).expect("write");
}

/// A jj project in `temp` with its default branch on a bare remote, the shipped `base-open` and
/// `papers-commit` actions, and the release folder of 0.4.0 signed (its design too, when
/// `design`). Gives its root.
fn project(temp: &Path, design: bool) -> PathBuf {
    let root = temp.join("repo");
    jj(temp, &["git", "init", "repo"]);
    jj(
        &root,
        &["config", "set", "--repo", "user.name", "Test Author"],
    );
    jj(
        &root,
        &["config", "set", "--repo", "user.email", "a@example.com"],
    );
    // `base-open` asks `zaf forge default-branch`: a `zaf` that says `main`, first on the actions'
    // path, stands for the forge.
    let bin = temp.join("forge-bin");
    write(&bin.join("zaf"), "#!/bin/sh\necho main\n");
    std::fs::set_permissions(bin.join("zaf"), std::fs::Permissions::from_mode(0o755))
        .expect("mode");
    let search = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .expect("search path");
    write(
        &root.join(".zaf.toml"),
        &format!(
            "{MANIFEST}\n[env]\nPATH = \"{}\"\n",
            search.to_string_lossy()
        ),
    );
    write(&root.join(".gitignore"), ".zaf/*\n!.zaf/actions/\n");
    for name in ["base-open", "papers-commit"] {
        let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the crate sits in the repository")
            .join("plugins/teams/actions")
            .join(name);
        let file = root.join(".zaf/actions").join(name);
        write(&file, &std::fs::read_to_string(shipped).expect("shipped"));
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o750)).expect("mode");
    }
    run(temp, "git", &["init", "-q", "--bare", "remote.git"]);
    let bare = temp.join("remote.git");
    jj(
        &root,
        &["git", "remote", "add", "origin", &bare.to_string_lossy()],
    );
    jj(&root, &["commit", "-m", "Start the project"]);
    jj(&root, &["bookmark", "create", "main", "-r", "@-"]);
    jj(&root, &["git", "push", "--bookmark", "main"]);
    let sign = |name: &str, body: &str| {
        write(
            &root.join(FOLDER).join(name),
            &DocumentSignature::sign(body, UNIX_EPOCH),
        );
    };
    sign("objectives.md", "# Objectives\n");
    if design {
        sign("design.md", "# Design\n");
    } else {
        write(&root.join(FOLDER).join("design.md"), "# Design\n");
    }
    std::fs::canonicalize(root).expect("root")
}

#[test]
fn a_tick_is_the_verb_run_in_the_projects_root_for_the_hubs_scheduler() {
    let (bin, project) = (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("project"),
    );
    let tick = process(fake_zaf(bin.path(), "true"), project.path());

    let command = tick.command();

    let args: Vec<_> = command.get_args().collect();
    assert_eq!(args, ["spawn", "--once", "--version", "0.4.0"]);
    assert_eq!(command.get_current_dir(), Some(project.path()));
    let envs: Vec<_> = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect();
    assert!(envs.contains(&(INSTANCE_VAR.to_owned(), None)), "{envs:?}");
    assert!(envs.contains(&(RUN_VAR.to_owned(), None)), "{envs:?}");
    assert!(
        envs.contains(&(
            SCHEDULED_BY_VAR.to_owned(),
            Some(std::process::id().to_string())
        )),
        "the tick is told whose it is: {envs:?}"
    );
}

#[test]
fn a_tick_gives_the_exit_of_the_process_and_two_for_one_that_cannot_start() {
    let (bin, project) = (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("project"),
    );
    let asked = Asked::new(["hub".to_owned()]);
    let fails = process(fake_zaf(bin.path(), "exit 7"), project.path());
    let missing = process(bin.path().join("nothing-here"), project.path());

    let code = (0..20).find_map(|_| {
        // A script written a moment ago can meet `ETXTBSY` in another test's fork.
        let code = fails.tick(&asked);
        (code == 7).then_some(code).or_else(|| {
            std::thread::sleep(Duration::from_millis(50));
            None
        })
    });

    assert_eq!(code, Some(7));
    assert_eq!(missing.tick(&asked), 2);
}

#[test]
fn the_hubs_watch_ticks_the_projects_root_at_once_and_ends_with_the_hub() {
    let (bin, home, temp) = (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("home"),
        tempfile::tempdir().expect("project"),
    );
    // The build start comes first, so the project has signed papers and a remote.
    let root = project(temp.path(), true);
    let seen = bin.path().join("seen");
    // Each tick leaves where it ran, with what, and whose it was told it is.
    let zaf = fake_zaf(
        bin.path(),
        &format!(
            "echo \"$(pwd) $@ by=$ZAF_SCHEDULED_BY\" >> '{}'",
            seen.display()
        ),
    );
    let watches = Watches::new();
    let hub = Hub::of(watches.handle(), Some(home.path().to_owned()), zaf);

    let started = hub.start(&root, "0.4.0").expect("started");

    assert_eq!(started.version, "0.4.0");
    assert!(watch_running(&root.join(STATE_DIR)));
    let limit = Instant::now() + Duration::from_secs(20);
    while std::fs::read_to_string(&seen)
        .unwrap_or_default()
        .is_empty()
    {
        assert!(Instant::now() < limit, "no tick ran");
        std::thread::sleep(Duration::from_millis(20));
    }
    let ticked = std::fs::read_to_string(&seen).expect("seen");
    assert_eq!(
        ticked.lines().next(),
        Some(
            format!(
                "{} spawn --once --version 0.4.0 by={}",
                root.display(),
                std::process::id()
            )
            .as_str()
        )
    );

    drop(watches);
    assert!(
        !watch_running(&root.join(STATE_DIR)),
        "it ended with the hub"
    );
}

#[test]
fn hub_watch_start_refuses_unsigned_design_with_409() {
    let (bin, home, temp) = (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("home"),
        tempfile::tempdir().expect("project"),
    );
    let root = project(temp.path(), false);
    let watches = Watches::new();
    let hub = Hub::of(
        watches.handle(),
        Some(home.path().to_owned()),
        fake_zaf(bin.path(), "true"),
    );

    let refused = hub.start(&root, "0.4.0").expect_err("refused");

    // The hub answers a `Failure::Refused` with 409.
    match refused {
        Failure::Refused(sentence) => {
            assert!(sentence.contains("design.md"), "{sentence}");
            assert!(sentence.contains("zaf begin --version 0.4.0"), "{sentence}");
        }
        other => panic!("a refusal was expected: {other:?}"),
    }
    assert!(!watch_running(&root.join(STATE_DIR)), "no watch runs");
}

#[test]
fn hub_watch_start_opens_the_base_then_watches() {
    let (bin, home, temp) = (
        tempfile::tempdir().expect("bin"),
        tempfile::tempdir().expect("home"),
        tempfile::tempdir().expect("project"),
    );
    let root = project(temp.path(), true);
    let watches = Watches::new();
    let hub = Hub::of(
        watches.handle(),
        Some(home.path().to_owned()),
        fake_zaf(bin.path(), "true"),
    );

    hub.start(&root, "0.4.0").expect("started");

    assert!(watch_running(&root.join(STATE_DIR)));
    let held = Run::new("git")
        .arg("-C")
        .arg(temp.path().join("remote.git"))
        .args(["rev-parse", "--verify", "-q", "refs/heads/feature/0.4.0"])
        .output()
        .expect("git");
    assert!(
        held.status.success(),
        "the remote holds the integration branch"
    );
}
