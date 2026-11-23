//! `zaf update --yes` from one release behind, with no terminal (O3): it ends on its own, its last
//! line names the action the Engineer edited as kept, and `zaf doctor` is green afterwards.
//!
//! No network and no `claude`: the releases are [`FakeReleases`] and the plugin step's shell is
//! [`FakeCommands`] (wrapped to move the installed pack, as `claude plugin update` does). Standard
//! input is closed for the whole run, so any question the update asked would read the end of input.

mod common;

use std::fs;
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use common::{Project, outcome, write};
use semver::Version;
use zaf::app::doctor::fake::FakeCommands;
use zaf::app::doctor::{CommandOutput, Commands};
use zaf::app::update;
use zaf::cli::screen::{Environment, Screen};
use zaf::cli::update::{Machine, Out, run_yes};
use zaf::fakes::FakeReleases;

const TARGET: &str = "x86_64-unknown-linux-musl";
const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "github-issues"
repo = "example/tracker"
"#;
const OLD_MERGE: &str =
    "#!/bin/sh\n# zaf description: land it\n# zaf 1: failed\npr=$1\ngh pr merge\n";
const NEW_MERGE: &str = "#!/bin/sh\n# zaf description: land it\n# zaf 1: failed\n# changed: it now waits for the checks\npr=$1\ngh pr checks\ngh pr merge\n";
const OLD_CI_WAIT: &str =
    "#!/bin/sh\n# zaf description: wait for checks\n# zaf 1: failed\necho old\n";
const NEW_CI_WAIT: &str =
    "#!/bin/sh\n# zaf description: wait for checks\n# zaf 1: failed\necho new\n";
const LINT: &str = "#!/bin/sh\n# zaf description: lint it\n# zaf 1: failed\necho lint\n";

/// What `git diff --no-index` prints for an action that differs; without it an action counts as
/// not shown and is kept.
const DIFF: &str = "diff --git a/merge b/merge\n--- a/merge\n+++ b/merge\n@@ -4,2 +4,3 @@\n pr=$1\n-gh pr merge\n+gh pr checks\n+gh pr merge\n";

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

/// A `claude` that does what `plugin update` does to the pack: the installed plugin's path moves to
/// the new release's folder.
struct Upgrading {
    inner: FakeCommands,
    installed: PathBuf,
    new_pack: PathBuf,
}

impl Commands for Upgrading {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CommandOutput> {
        if program == "claude" && args.starts_with(&["plugin", "update"]) {
            let moved = serde_json::json!({
                "plugins": {"teams@ways": [{"version": "2.1.5", "installPath": self.new_pack}]}
            });
            fs::write(&self.installed, moved.to_string())?;
        }
        self.inner.run(program, args)
    }
}

/// Publishes `tag` as the release workflow would: a tarball of `<tag>-<target>/zaf` and its sum.
fn publish(files: &Path, tag: &str) {
    let stage = files.join("stage");
    let inner = stage.join(format!("{tag}-{TARGET}"));
    fs::create_dir_all(&inner).expect("stage");
    fs::write(inner.join("zaf"), "#!/bin/sh\necho 'zaf 0.5.2'\n").expect("binary");
    let release = files.join(tag);
    fs::create_dir_all(&release).expect("release");
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
    fs::write(
        release.join(format!("{name}.sha256")),
        format!("{sum}  {name}\n"),
    )
    .expect("sum");
    fs::remove_dir_all(&stage).expect("clean stage");
}

/// Writes an executable action file.
fn action(dir: &Path, name: &str, text: &str) {
    write(dir, name, text);
    fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o755)).expect("mode");
}

/// Closes standard input for this process: it now reads the end of input at once, and is no terminal.
fn close_stdin() {
    let null = fs::File::open("/dev/null").expect("/dev/null");
    // SAFETY: both descriptors are open; `dup2` makes 0 a copy of the second and leaves it open.
    let made = unsafe { libc::dup2(null.as_raw_fd(), 0) };
    assert_eq!(made, 0, "dup2");
    assert!(
        !std::io::IsTerminal::is_terminal(&io::stdin()),
        "standard input is no terminal"
    );
}

#[test]
fn update_yes_no_terminal_one_release_behind() {
    let mut project = Project::new(MANIFEST);
    project.provide_slice_actions();
    let temp = project.temp.path().to_owned();
    // The Claude home the doctor reads is `$HOME/.claude`.
    let user = temp.join("user");
    project.env("HOME", &user.to_string_lossy());
    let claude = user.join(".claude");
    let home = project.home.clone();

    // One release behind: zaf 0.5.1 with 0.5.2 out, the plugin at 2.1.4 and 2.1.5 declared.
    let files = temp.join("files");
    publish(&files, "zaf-v0.5.2");
    let releases = FakeReleases::default();
    releases.publish(&["zaf-v0.5.1", "zaf-v0.5.2"]);
    releases.write_notes("zaf-v0.5.2", "## What is new\n- The merge action waits.\n");
    releases.set_plugin_version("2.1.5");
    releases.serve_from(&files);
    let exe = temp.join("bin/zaf");
    fs::create_dir_all(exe.parent().expect("bin")).expect("bin");
    fs::write(&exe, "the old zaf").expect("binary");
    let old_pack = claude.join("plugins/cache/teams");
    let new_pack = claude.join("plugins/cache/teams-new");
    action(&old_pack.join("actions"), "merge", OLD_MERGE);
    action(&old_pack.join("actions"), "ci-wait", OLD_CI_WAIT);
    action(&new_pack.join("actions"), "merge", NEW_MERGE);
    action(&new_pack.join("actions"), "ci-wait", NEW_CI_WAIT);
    action(&new_pack.join("actions"), "lint", LINT);
    let installed = claude.join("plugins/installed_plugins.json");
    fs::write(
        &installed,
        serde_json::json!({
            "plugins": {"teams@ways": [{"version": "2.1.4", "installPath": old_pack}]}
        })
        .to_string(),
    )
    .expect("installed");
    // The machine holds `merge` as the old pack had it and `ci-wait` edited by hand.
    let edited = format!("{OLD_CI_WAIT}# my own line\n");
    action(&home.join("actions"), "merge", OLD_MERGE);
    action(&home.join("actions"), "ci-wait", &edited);

    let commands = Upgrading {
        inner: FakeCommands::new()
            .with(
                "claude",
                "ways\n  Source: GitHub (ProgrammingCheetah/ways)\n",
            )
            .with("git", DIFF),
        installed,
        new_pack,
    };
    let machine = Machine {
        current: Version::new(0, 5, 1),
        exe: &exe,
        target: Some(TARGET),
        claude_home: Some(&claude),
        zaf_home: Some(&home),
        project: None,
        commands: &commands,
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

    close_stdin();
    let code = run_yes(
        &machine,
        &releases,
        plain,
        Out::new(Box::new(buffer.clone())),
    );

    let out = String::from_utf8_lossy(&buffer.0.lock().expect("lock")).into_owned();
    assert_eq!(code, 0, "{out}");
    let last = out.trim_end().lines().last().expect("a line");
    assert_eq!(
        last,
        "zaf 0.5.1 → 0.5.2; replaced: plugin teams 2.1.4 → 2.1.5, actions lint, merge; kept: action ci-wait (yours)",
        "{out}"
    );
    assert_ne!(fs::read_to_string(&exe).expect("binary"), "the old zaf");
    assert_eq!(
        fs::read_to_string(home.join("actions/ci-wait")).expect("ci-wait"),
        edited,
        "the edited action is as the Engineer left it"
    );
    assert_eq!(
        fs::read_to_string(home.join("actions/merge")).expect("merge"),
        NEW_MERGE
    );

    let (code, stdout, stderr) = outcome(&project.zaf(&["doctor"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
}
