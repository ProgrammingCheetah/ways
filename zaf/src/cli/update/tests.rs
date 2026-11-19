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
        let code = run_plan(
            &machine,
            &self.releases,
            plain,
            &mut input.as_bytes(),
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
