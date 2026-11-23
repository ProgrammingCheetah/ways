use std::path::PathBuf;
use std::process::Command;

use clap::Parser;

use super::*;
use crate::app::doctor::fake::FakeCommands;
use crate::cli::screen::Environment;
use crate::fakes::FakeReleases;

const TARGET: &str = "x86_64-unknown-linux-musl";
const OLD_MERGE: &str =
    "#!/bin/sh\n# zaf description: land it\n# zaf 1: failed\npr=$1\ngh pr merge\n";
const NEW_MERGE: &str = "#!/bin/sh\n# zaf description: land it\n# zaf 1: failed\n# changed: it now waits for the checks\npr=$1\ngh pr checks\ngh pr merge\n";
const DIFF: &str = "diff --git a/merge b/merge\n--- a/merge\n+++ b/merge\n@@ -4,2 +4,3 @@\n pr=$1\n-gh pr merge\n+gh pr checks\n+gh pr merge\n";
const MARKETPLACE: &str = "ways\n  Source: GitHub (ProgrammingCheetah/ways)\n";

/// A writer the test reads back.
#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().expect("lock").extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Buffer {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().expect("lock")).into_owned()
    }
}

/// One release behind: a zaf binary at 0.5.1, releases up to 0.5.2 (with notes), the plugin at
/// 2.1.4 installed and 2.1.5 declared, and two actions in the plugin, one of them differing from
/// the machine's copy and one new to it.
struct Scene {
    dir: tempfile::TempDir,
    releases: FakeReleases,
    commands: FakeCommands,
}

impl Scene {
    fn new() -> Self {
        Self::with_commands(
            FakeCommands::new()
                .with("claude", MARKETPLACE)
                .with("git", DIFF),
        )
    }

    fn with_commands(commands: FakeCommands) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        publish(&root.join("files"), "zaf-v0.5.2", "zaf 0.5.2");
        let releases = FakeReleases::default();
        releases.publish(&["zaf-v0.5.1", "zaf-v0.5.2"]);
        releases.write_notes(
            "zaf-v0.5.2",
            "## What is new\n- A question shows in the hub.\n- The merge action waits for checks.\n\nMore.\n",
        );
        releases.set_plugin_version("2.1.5");
        releases.serve_from(&root.join("files"));
        std::fs::create_dir_all(root.join("bin")).expect("bin");
        std::fs::write(root.join("bin/zaf"), "the old zaf").expect("binary");
        let pack = root.join("claude/plugins/cache/teams");
        std::fs::create_dir_all(pack.join("actions")).expect("pack");
        std::fs::write(pack.join("actions/merge"), NEW_MERGE).expect("merge");
        std::fs::write(pack.join("actions/ci-wait"), "#!/bin/sh\necho wait\n").expect("ci-wait");
        let installed = serde_json::json!({
            "plugins": {"teams@ways": [{"version": "2.1.4", "installPath": pack}]}
        });
        std::fs::write(
            root.join("claude/plugins/installed_plugins.json"),
            installed.to_string(),
        )
        .expect("installed");
        std::fs::create_dir_all(root.join("zaf-home/actions")).expect("home");
        std::fs::write(root.join("zaf-home/actions/merge"), OLD_MERGE).expect("old merge");
        Self {
            dir,
            releases,
            commands,
        }
    }

    fn exe(&self) -> PathBuf {
        self.dir.path().join("bin/zaf")
    }

    fn merge(&self) -> String {
        std::fs::read_to_string(self.dir.path().join("zaf-home/actions/merge")).expect("merge")
    }

    /// Runs the plain verb with `input` as the Engineer's side; its exit code and what it printed.
    fn run(&self, input: &str, interactive: bool) -> (u8, String) {
        self.run_on(&mut input.as_bytes(), interactive)
    }

    /// [`Scene::run`] over any reader as the Engineer's side.
    fn run_on<R: BufRead>(&self, input: &mut R, interactive: bool) -> (u8, String) {
        self.run_via(interactive, |machine, releases, screen, out| {
            run_plan(machine, releases, screen, input, out)
        })
    }

    /// Runs the plain verb with its answers from `answers`, standard input never touched.
    fn run_with_answers(&self, answers: &mut dyn UpdateAnswers) -> (u8, String) {
        self.run_via(true, |machine, releases, screen, out| {
            run_plan_with(machine, releases, screen, answers, out)
        })
    }

    fn run_via(
        &self,
        interactive: bool,
        go: impl FnOnce(&Machine<'_>, &dyn Releases, Screen, Out) -> u8,
    ) -> (u8, String) {
        let root = self.dir.path();
        let (claude, home, exe) = (root.join("claude"), root.join("zaf-home"), self.exe());
        let machine = Machine {
            current: Version::new(0, 5, 1),
            exe: &exe,
            target: Some(TARGET),
            claude_home: Some(&claude),
            zaf_home: Some(&home),
            project: None,
            commands: &self.commands,
            shell: None,
            interactive,
            now: SystemTime::now(),
        };
        let plain = Screen::from_environment(Environment {
            stdout_is_terminal: false,
            no_color: None,
            term: None,
        });
        let buffer = Buffer::default();
        let code = go(
            &machine,
            &self.releases,
            plain,
            Out::new(Box::new(buffer.clone())),
        );
        (code, buffer.text())
    }
}

/// Publishes `tag` as the release workflow would: a tarball of `<tag>-<target>/zaf` (a script
/// printing `says`) and its `.sha256`.
fn publish(files: &Path, tag: &str, says: &str) {
    let stage = files.join("stage");
    let inner = stage.join(format!("{tag}-{TARGET}"));
    std::fs::create_dir_all(&inner).expect("stage");
    std::fs::write(inner.join("zaf"), format!("#!/bin/sh\necho '{says}'\n")).expect("binary");
    let release = files.join(tag);
    std::fs::create_dir_all(&release).expect("release");
    let name = update::archive(tag, TARGET);
    let status = Command::new("tar")
        .arg("-czf")
        .arg(release.join(&name))
        .arg("-C")
        .arg(&stage)
        .arg(format!("{tag}-{TARGET}"))
        .status()
        .expect("tar");
    assert!(status.success());
    let sum = Command::new("sha256sum")
        .arg(release.join(&name))
        .output()
        .expect("sha256sum");
    let sum = String::from_utf8_lossy(&sum.stdout);
    let sum = sum.split_whitespace().next().expect("a sum");
    std::fs::write(
        release.join(format!("{name}.sha256")),
        format!("{sum}  {name}\n"),
    )
    .expect("sum");
    std::fs::remove_dir_all(&stage).expect("clean stage");
}

/// Asserts that each of `parts` is in `text`, each after the one before.
fn in_order(text: &str, parts: &[&str]) {
    let mut from = 0;
    for part in parts {
        match text[from..].find(part) {
            Some(at) => from += at + part.len(),
            None => panic!("`{part}` is not in the output after byte {from}:\n{text}"),
        }
    }
}

#[test]
fn update_reads_as_the_mock() {
    let scene = Scene::new();

    let (code, out) = scene.run("\n\n\n", true);

    assert_eq!(code, OK, "{out}");
    in_order(
        &out,
        &[
            "Newer versions are out",
            "1. Replace  zaf 0.5.1 → 0.5.2",
            "2. Update   teams plugin 2.1.4 → 2.1.5",
            "3. Update   2 of 2 actions in",
            "What is new",
            "A question shows in the hub.",
            "The merge action waits for checks.",
            "All of it: https://github.com/ProgrammingCheetah/ways/releases/tag/zaf-v0.5.2",
            "Continue? [Y/n]",
            "✓ Replace zaf 0.5.2",
            "✓ Update teams plugin updated",
            "merge will be replaced. Why: it now waits for the checks",
            "@@ -4,2 +4,3 @@",
            "Replace it? The old one is kept in",
            "[Y/n]",
            "✓ merge replaced",
            "✓ Update actions: 1 new, 1 replaced",
            "zaf is up to date.",
            "Running instances finish on the old version; new ones start on 0.5.2.",
        ],
    );
    assert!(!out.contains("More."), "only the What is new section");
    assert!(!out.contains('\u{1b}'), "a plain screen has no escape");
    assert_eq!(scene.merge(), NEW_MERGE);
    let replaced = scene.dir.path().join("zaf-home/actions-replaced");
    assert_eq!(std::fs::read_dir(replaced).expect("kept").count(), 1);
    let said = Command::new(scene.exe()).arg("--version").output();
    assert_eq!(
        String::from_utf8_lossy(&said.expect("runs").stdout).trim(),
        "zaf 0.5.2"
    );
}

#[test]
fn an_action_with_no_why_says_so() {
    let scene = Scene::new();
    let pack = scene
        .dir
        .path()
        .join("claude/plugins/cache/teams/actions/merge");
    std::fs::write(
        pack,
        "#!/bin/sh\n# zaf description: land it\n# zaf 1: failed\ngh pr checks\n",
    )
    .expect("merge");

    let (_, out) = scene.run("\n\n\n", true);

    assert!(
        out.contains("will be replaced. Why: not said; see the difference"),
        "{out}"
    );
}

#[test]
fn no_changes_before_continue() {
    let scene = Scene::new();

    let (code, out) = scene.run("n\n", true);

    assert_eq!(code, OK);
    assert!(out.contains("Continue? [Y/n]"), "{out}");
    assert!(out.trim_end().ends_with("Nothing changed."), "{out}");
    assert!(!out.contains("will be replaced"), "{out}");
    assert_eq!(
        std::fs::read_to_string(scene.exe()).expect("binary"),
        "the old zaf"
    );
    assert_eq!(scene.merge(), OLD_MERGE);
    let home = scene.dir.path().join("zaf-home");
    assert!(!home.join("actions/ci-wait").exists());
    assert!(!home.join("actions-replaced").exists());
    assert_eq!(scene.commands.calls(), Vec::<Vec<String>>::new());
    assert!(scene.releases.downloads().is_empty());
}

#[test]
fn closed_input_is_a_no() {
    let scene = Scene::new();

    let (code, out) = scene.run("", true);

    assert_eq!(code, OK);
    assert!(out.contains("Nothing changed."), "{out}");
    assert!(scene.commands.calls().is_empty());
}

#[test]
fn with_no_terminal_it_shows_the_plan_and_changes_nothing() {
    let scene = Scene::new();

    let (code, out) = scene.run("\n\n\n", false);

    assert_eq!(code, OK);
    in_order(
        &out,
        &[
            "Newer versions are out",
            "What is new",
            "run zaf update in a terminal",
        ],
    );
    assert!(!out.contains("Continue?"), "{out}");
    assert_eq!(
        std::fs::read_to_string(scene.exe()).expect("binary"),
        "the old zaf"
    );
    assert_eq!(scene.merge(), OLD_MERGE);
    assert!(scene.commands.calls().is_empty());
    assert!(scene.releases.downloads().is_empty());
}

#[test]
fn a_kept_action_is_counted() {
    let scene = Scene::new();

    let (code, out) = scene.run("\nn\n", true);

    assert_eq!(code, OK, "{out}");
    assert_eq!(scene.merge(), OLD_MERGE, "kept byte for byte");
    assert!(out.contains("kept yours"), "{out}");
    assert!(
        out.contains("1 action(s) kept as yours; zaf update shows them again."),
        "{out}"
    );
    let home = scene.dir.path().join("zaf-home");
    assert!(
        home.join("actions/ci-wait").exists(),
        "the new one is added"
    );
    assert!(!home.join("actions-replaced").exists());
}

#[test]
fn a_failed_step_does_not_stop_the_rest() {
    let scene = Scene::with_commands(
        FakeCommands::new()
            .failing("claude", "claude: not logged in")
            .with("git", DIFF),
    );

    let (code, out) = scene.run("\n\n\n", true);

    assert_eq!(code, BACKEND, "{out}");
    in_order(
        &out,
        &[
            "✓ Replace zaf 0.5.2",
            "✗ Update claude plugin marketplace list: claude: not logged in",
            "merge will be replaced. Why:",
            "Replace it?",
            "✓ Update actions: 1 new, 1 replaced",
            "1 step(s) failed",
        ],
    );
    assert!(!out.contains("zaf is up to date."), "{out}");
    assert_eq!(scene.merge(), NEW_MERGE);
}

#[test]
fn nothing_to_do_says_it_is_up_to_date() {
    let scene = Scene::new();
    scene.releases.publish(&["zaf-v0.5.1"]);
    scene.releases.set_plugin_version("2.1.4");
    std::fs::write(scene.dir.path().join("zaf-home/actions/merge"), NEW_MERGE).expect("merge");
    std::fs::write(
        scene.dir.path().join("zaf-home/actions/ci-wait"),
        "#!/bin/sh\necho wait\n",
    )
    .expect("ci-wait");

    let (code, out) = scene.run("\n", true);

    assert_eq!(code, OK);
    assert_eq!(out, "zaf is up to date.\n");
    assert!(scene.commands.calls().is_empty());
}

/// A pseudo-terminal pair nobody types into: reading the slave blocks for ever, because the master
/// is kept open and never written to.
struct SilentPty {
    _master: std::fs::File,
    _slave: std::fs::File,
}

impl SilentPty {
    fn open() -> Self {
        use std::os::fd::FromRawFd;
        let (mut master, mut slave) = (0, 0);
        // SAFETY: `openpty` fills the two descriptors, which this takes over; the other arguments
        // are null, which it allows.
        let made = unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        assert_eq!(made, 0, "openpty");
        // SAFETY: the descriptors are open and owned by nothing else.
        unsafe {
            Self {
                _master: std::fs::File::from_raw_fd(master),
                _slave: std::fs::File::from_raw_fd(slave),
            }
        }
    }
}

/// What 0.5.4 did with a terminal that no one types into (#1010): `zaf update` waits for the
/// Engineer for ever. The plan is drawn, then `run_plan` stops in `ask_yes` (`cli/screen.rs`) on
/// `Continue? [Y/n]`, in `read_line` on a pty with no input and no end of input, so it neither
/// answers nor gives up; seen as a timeout here. Driven with [`update::Assumed`], which answers
/// for itself and never reads the terminal, the run finishes and replaces the binary.
#[test]
fn update_hang_pty_without_input() {
    let pty = SilentPty::open();
    let scene = Arc::new(Scene::new());
    let running = Arc::clone(&scene);
    let (done, wait) = std::sync::mpsc::channel();
    // The thread is left behind when it hangs: the test process ends it.
    std::thread::spawn(move || {
        let _pty = pty;
        let result = running.run_with_answers(&mut update::Assumed::default());
        let _ = done.send(result);
    });

    let finished = wait.recv_timeout(Duration::from_secs(5));

    let (code, out) = finished.expect("run_plan did not finish in 5s: it waits for an answer");
    assert_eq!(code, OK, "{out}");
    assert_ne!(
        std::fs::read_to_string(scene.exe()).expect("binary"),
        "the old zaf",
        "the answers were assumed, so the binary is replaced"
    );
}

/// What 0.5.4 did with standard input closed after `Continue?` (#1010), one release behind with the
/// machine's `merge` edited by its owner: the action question reads the closed input as a no and
/// `merge` stays as the Engineer left it, and no output line names `merge` as kept or says why: the
/// question is followed by a bare "kept yours" and the closing line counts "1 action(s) kept as
/// yours" without saying which, though no one answered. Driven with [`update::Assumed`] there is no
/// input to close: the edited action is kept and the run names it by name (`Assumed::kept`, what
/// the last line reads), not as a count.
#[test]
fn update_hang_closed_stdin_keeps_action_silently() {
    let scene = Scene::new();
    let edited = format!("{OLD_MERGE}# my own line\n");
    std::fs::write(scene.dir.path().join("zaf-home/actions/merge"), &edited).expect("edit merge");
    let mut assumed = update::Assumed::default();

    let (code, out) = scene.run_with_answers(&mut assumed);

    assert_eq!(code, OK, "{out}");
    assert_eq!(scene.merge(), edited, "the edited action is kept");
    assert_eq!(assumed.kept(), ["merge"], "the kept action is named");
}

#[test]
fn check_and_json_are_unchanged() {
    // The scene's binary is 0.5.1, whatever version this crate is at.
    const AT: Version = Version::new(0, 5, 1);
    let scene = Scene::new();
    let exe = scene.exe();
    let cli = |args: &[&str]| Cli::try_parse_from(args).expect("parses");

    // A check says a newer release is out and replaces nothing.
    let check = cli(&["zaf", "update", "--check"]);
    assert_eq!(run_with(&check, true, &scene.releases, &exe, &AT), REFUSED);
    let json = cli(&["zaf", "--json", "update", "--check"]);
    assert_eq!(run_with(&json, true, &scene.releases, &exe, &AT), REFUSED);
    assert_eq!(
        std::fs::read_to_string(&exe).expect("binary"),
        "the old zaf"
    );
    assert!(scene.commands.calls().is_empty());

    // `--json` alone still replaces the binary at once, asking nothing and touching no plugin.
    let update = cli(&["zaf", "--json", "update"]);
    let code = run_with(&update, false, &scene.releases, &exe, &AT);
    assert_eq!(code, OK);
    assert_ne!(
        std::fs::read_to_string(&exe).expect("binary"),
        "the old zaf"
    );
    assert_eq!(scene.merge(), OLD_MERGE);
    assert!(scene.commands.calls().is_empty());
}

#[test]
fn update_answers_asked_matches_today() {
    let screen = Screen::from_environment(Environment {
        stdout_is_terminal: false,
        no_color: None,
        term: None,
    });
    let buffer = Buffer::default();
    let out = Out::new(Box::new(buffer.clone()));
    let change = PackChange {
        file: PathBuf::from("/project/.zaf.toml"),
        old: "tag = zaf-v0.5.1".to_owned(),
        new: "tag = zaf-v0.5.2".to_owned(),
    };
    let diff = ActionDiff {
        name: "merge".to_owned(),
        why: None,
        difference: DIFF.to_owned(),
    };
    let mut input = "\nn\n\nno\n".as_bytes();
    let mut asked = Asked::new(
        &screen,
        &mut input,
        out,
        Some(Path::new("/zaf-home")),
        Some(Path::new("/project")),
    );

    assert!(asked.proceed(), "Enter agrees");
    assert!(!asked.replace(&diff), "n declines");
    assert!(asked.raise_pack(&change), "Enter agrees");
    assert!(!asked.proceed(), "no declines");

    in_order(
        &buffer.text(),
        &[
            "Continue? [Y/n]",
            "merge will be replaced. Why: not said; see the difference",
            "/zaf-home/actions/merge",
            "@@ -4,2 +4,3 @@",
            "Replace it? The old one is kept in /zaf-home/",
            "kept yours",
            "pack will be raised in .zaf.toml.",
            "-tag = zaf-v0.5.1",
            "+tag = zaf-v0.5.2",
            "Raise it? Its profiles are made again. [Y/n]",
            "Continue? [Y/n]",
        ],
    );
}

#[test]
fn update_answers_assumed_never_reads_stdin() {
    let scene = Scene::new();

    let mut assumed = update::Assumed::default();

    let (code, out) = scene.run_with_answers(&mut assumed);

    assert_eq!(code, OK, "{out}");
    assert!(!out.contains("Continue?"), "{out}");
    assert!(!out.contains("Replace it?"), "{out}");
    // The scene's installed pack is the one it updates to, so the machine's `merge` differs from the
    // old pack copy: edited, and kept.
    assert_eq!(scene.merge(), OLD_MERGE, "the edited-actions rule");
    assert_eq!(assumed.kept(), ["merge"]);
    assert!(out.contains("zaf is up to date."), "{out}");
}

#[test]
fn update_answers_assumed_proceeds_and_raises_pack() {
    let change = PackChange {
        file: PathBuf::from("/project/.zaf.toml"),
        old: "old".to_owned(),
        new: "new".to_owned(),
    };
    let diff = ActionDiff {
        name: "merge".to_owned(),
        why: None,
        difference: DIFF.to_owned(),
    };

    let mut plain = update::Assumed::default();
    assert!(plain.proceed());
    assert!(plain.raise_pack(&change));
    assert!(plain.replace(&diff), "yes for every diff with no hook");

    let mut hooked = update::Assumed::with_replace(|diff| diff.name != "merge");
    assert!(hooked.proceed());
    assert!(hooked.raise_pack(&change));
    assert!(!hooked.replace(&diff), "the hook answers replace");
}

/// A `claude` that does what `plugin update` does to the pack: the installed plugin's path moves to
/// a new folder holding the release's actions, the old folder staying where it was.
struct Upgrading<'a> {
    inner: &'a FakeCommands,
    installed: PathBuf,
    new_pack: PathBuf,
}

impl Commands for Upgrading<'_> {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<crate::app::doctor::CommandOutput> {
        if program == "claude" && args.starts_with(&["plugin", "update"]) {
            let moved = serde_json::json!({
                "plugins": {"teams@ways": [{"version": "2.1.5", "installPath": self.new_pack}]}
            });
            std::fs::write(&self.installed, moved.to_string()).expect("installed");
        }
        self.inner.run(program, args)
    }
}

const OLD_CI_WAIT: &str = "#!/bin/sh\necho old\n";
const MY_CI_WAIT: &str = "#!/bin/sh\necho mine\n";

/// A scene whose installed pack is the old release's (`OLD_MERGE`, an old `ci-wait`) and whose new
/// release (`NEW_MERGE`, a new `ci-wait` and a `lint` the machine lacks) is put in place by the
/// plugin step; the machine holds `merge` and `ci-wait` as given.
fn upgrade_scene(merge: &str, ci_wait: &str) -> Scene {
    let scene = Scene::new();
    let root = scene.dir.path();
    let old_pack = root.join("claude/plugins/cache/teams");
    std::fs::write(old_pack.join("actions/merge"), OLD_MERGE).expect("old merge");
    std::fs::write(old_pack.join("actions/ci-wait"), OLD_CI_WAIT).expect("old ci-wait");
    let new_pack = root.join("claude/plugins/cache/teams-new");
    std::fs::create_dir_all(new_pack.join("actions")).expect("new pack");
    std::fs::write(new_pack.join("actions/merge"), NEW_MERGE).expect("new merge");
    std::fs::write(new_pack.join("actions/ci-wait"), "#!/bin/sh\necho new\n").expect("ci-wait");
    std::fs::write(new_pack.join("actions/lint"), "#!/bin/sh\necho lint\n").expect("lint");
    let home = root.join("zaf-home/actions");
    std::fs::write(home.join("merge"), merge).expect("machine merge");
    std::fs::write(home.join("ci-wait"), ci_wait).expect("machine ci-wait");
    scene
}

/// The plain verb on `scene` with `answers`, the plugin step moving the installed pack.
fn run_upgrading(scene: &Scene, answers: &mut update::Assumed) -> (u8, String) {
    run_upgrading_via(scene, |machine, releases, screen, out| {
        run_plan_with(machine, releases, screen, answers, out)
    })
}

/// [`run_upgrading`] over any way of running the plan.
fn run_upgrading_via(
    scene: &Scene,
    go: impl FnOnce(&Machine<'_>, &dyn Releases, Screen, Out) -> u8,
) -> (u8, String) {
    let root = scene.dir.path();
    let wrapped = Upgrading {
        inner: &scene.commands,
        installed: root.join("claude/plugins/installed_plugins.json"),
        new_pack: root.join("claude/plugins/cache/teams-new"),
    };
    let (claude, home, exe) = (root.join("claude"), root.join("zaf-home"), scene.exe());
    let machine = Machine {
        current: Version::new(0, 5, 1),
        exe: &exe,
        target: Some(TARGET),
        claude_home: Some(&claude),
        zaf_home: Some(&home),
        project: None,
        commands: &wrapped,
        shell: None,
        interactive: true,
        now: SystemTime::now(),
    };
    let plain = Screen::from_environment(Environment {
        stdout_is_terminal: false,
        no_color: None,
        term: None,
    });
    let buffer = Buffer::default();
    let code = go(
        &machine,
        &scene.releases,
        plain,
        Out::new(Box::new(buffer.clone())),
    );
    (code, buffer.text())
}

fn machine_action(scene: &Scene, name: &str) -> String {
    std::fs::read_to_string(scene.dir.path().join("zaf-home/actions").join(name)).expect("action")
}

#[test]
fn update_assumed_replaces_unedited_action() {
    // `merge` is as the old pack had it; `ci-wait` was edited by hand.
    let scene = upgrade_scene(OLD_MERGE, MY_CI_WAIT);
    let mut assumed = update::Assumed::default();

    let (code, out) = run_upgrading(&scene, &mut assumed);

    assert_eq!(code, OK, "{out}");
    assert_eq!(machine_action(&scene, "merge"), NEW_MERGE);
    assert!(!assumed.kept().contains(&"merge".to_owned()));
}

#[test]
fn update_assumed_keeps_edited_action() {
    let scene = upgrade_scene(OLD_MERGE, MY_CI_WAIT);
    let mut assumed = update::Assumed::default();

    let (code, out) = run_upgrading(&scene, &mut assumed);

    assert_eq!(code, OK, "{out}");
    assert_eq!(
        machine_action(&scene, "ci-wait"),
        MY_CI_WAIT,
        "byte for byte"
    );
    assert_eq!(assumed.kept(), ["ci-wait"]);
}

#[test]
fn update_assumed_keeps_an_action_with_no_old_copy() {
    // The old pack has no `ci-wait` to read (installed by hand, or the cache pruned).
    let scene = upgrade_scene(OLD_MERGE, OLD_CI_WAIT);
    let old = scene.dir.path().join("claude/plugins/cache/teams/actions");
    std::fs::remove_file(old.join("ci-wait")).expect("remove");
    let mut assumed = update::Assumed::default();

    let (code, out) = run_upgrading(&scene, &mut assumed);

    assert_eq!(code, OK, "{out}");
    assert_eq!(machine_action(&scene, "ci-wait"), OLD_CI_WAIT);
    assert_eq!(assumed.kept(), ["ci-wait"]);
}

#[test]
fn update_assumed_installs_new_action() {
    let scene = upgrade_scene(OLD_MERGE, MY_CI_WAIT);
    let mut assumed = update::Assumed::default();

    let (code, out) = run_upgrading(&scene, &mut assumed);

    assert_eq!(code, OK, "{out}");
    assert_eq!(machine_action(&scene, "lint"), "#!/bin/sh\necho lint\n");
    assert!(out.contains("1 new"), "{out}");
    // The rule writes nothing of its own: the home holds its two folders and no more.
    let mut names: Vec<String> = std::fs::read_dir(scene.dir.path().join("zaf-home"))
        .expect("home")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    assert_eq!(names, ["actions", "actions-replaced"]);
}

/// The last line of `out`, which `--yes` ends on.
fn last_line(out: &str) -> &str {
    out.trim_end().lines().last().expect("a line")
}

#[test]
fn update_yes_line_replaced_and_kept() {
    // `merge` is as the old pack had it, so it is replaced; `ci-wait` was edited, so it is kept.
    let scene = upgrade_scene(OLD_MERGE, MY_CI_WAIT);

    let (code, out) = run_upgrading_via(&scene, run_yes);

    assert_eq!(code, OK, "{out}");
    assert_eq!(
        last_line(&out),
        "zaf 0.5.1 → 0.5.2; replaced: plugin teams 2.1.4 → 2.1.5, actions lint, merge; kept: action ci-wait (yours)"
    );
    assert_eq!(
        out.matches("; kept:").count(),
        1,
        "one line names them: {out}"
    );
    assert!(!out.contains("Continue?"), "{out}");
    assert_eq!(machine_action(&scene, "merge"), NEW_MERGE);
    assert_eq!(machine_action(&scene, "ci-wait"), MY_CI_WAIT);
}

#[test]
fn update_yes_failed_step_exits_2() {
    let scene = Scene::with_commands(
        FakeCommands::new()
            .failing("claude", "claude: not logged in")
            .with("git", DIFF),
    );

    let (code, out) = scene.run_via(true, run_yes);

    assert_eq!(code, BACKEND, "{out}");
    let line = last_line(&out);
    assert!(
        line.contains("; failed: plugin (claude plugin marketplace list: claude: not logged in)"),
        "{out}"
    );
    assert!(
        !line.contains("plugin teams"),
        "a failed step is not replaced: {out}"
    );
}

#[test]
fn update_yes_inside_instance_changes_nothing() {
    let scene = Scene::new();

    // An instance is not interactive, `--yes` or not.
    let (code, out) = scene.run_via(false, run_yes);

    assert_eq!(code, OK, "{out}");
    assert!(out.contains("does not run inside an instance"), "{out}");
    assert!(!out.contains("; replaced:"), "{out}");
    assert_eq!(
        std::fs::read_to_string(scene.exe()).expect("binary"),
        "the old zaf"
    );
    assert_eq!(scene.merge(), OLD_MERGE);
    assert!(scene.commands.calls().is_empty());
    assert!(scene.releases.downloads().is_empty());
}

#[test]
fn update_help_lists_yes() {
    let help = Cli::try_parse_from(["zaf", "update", "--help"]).expect_err("help stops the parse");
    assert!(help.to_string().contains("--yes"), "{help}");
    assert!(Cli::try_parse_from(["zaf", "update", "--yes"]).is_ok());
    assert!(Cli::try_parse_from(["zaf", "update", "--yes", "--check"]).is_err());
}
