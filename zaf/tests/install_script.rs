//! `zaf/install.sh`: its first screen and the refusals of its step 1, run under `sh` with fake
//! `uname`, `sysctl`, `git`, `curl`, `tar` and `claude` on a PATH of their own, a temporary HOME
//! and a temporary TMPDIR. Nothing here touches the network or the real HOME.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The utilities the script may use, linked from the machine into the fake PATH. `git`, `curl`,
/// `tar`, the sum tools, `claude`, `uname` and `sysctl` are not among them: those are faked.
const REAL_TOOLS: &[&str] = &[
    "mktemp", "rm", "tail", "sleep", "mkdir", "cat", "tr", "head", "chmod", "mv", "ln", "cp",
    "dirname", "basename", "grep", "date", "kill",
];

/// What the install step runs for real: the archive is unpacked and summed by the machine's own
/// tools, so that a sum or an archive that is wrong is caught by the real thing.
const STEP_TOOLS: &[&str] = &["tar", "gzip", "sha256sum"];

/// How the script is run: on a controlling terminal that answers with the given text, or on none.
enum Terminal<'a> {
    Answers(&'a str),
    None,
}

struct Install {
    temp: tempfile::TempDir,
    home: PathBuf,
    folder: PathBuf,
    bin: PathBuf,
    tmp: PathBuf,
    env: Vec<(String, String)>,
}

impl Install {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp dir");
        let home = temp.path().join("home");
        let folder = temp.path().join("bin-folder");
        let bin = temp.path().join("fake-path");
        let tmp = temp.path().join("tmp");
        for dir in [&home, &folder, &bin, &tmp] {
            fs::create_dir_all(dir).expect("dir");
        }
        let install = Self {
            temp,
            home,
            folder,
            bin,
            tmp,
            env: Vec::new(),
        };
        for tool in REAL_TOOLS {
            if let Some(path) = host_tool(tool) {
                let _ = std::os::unix::fs::symlink(path, install.bin.join(tool));
            }
        }
        install.fake("uname", "case \"$1\" in -s) echo \"${FAKE_UNAME_S:-Linux}\";; -m) echo \"${FAKE_UNAME_M:-x86_64}\";; *) echo \"${FAKE_UNAME_S:-Linux}\";; esac");
        install.fake("sysctl", "echo \"${FAKE_SYSCTL_ARM64:-0}\"");
        // The script asks `id -u` who runs it; the fake says an ordinary user unless FAKE_ID_U is set.
        install.fake("id", "echo \"${FAKE_ID_U:-1000}\"");
        install.fake("git", "echo 'git version 2.47.0'");
        install.fake("curl", "echo called >> \"$FAKE_CALLS\"; exit 1");
        install.fake("tar", "echo called >> \"$FAKE_CALLS\"; exit 1");
        install.fake("sha256sum", "echo called >> \"$FAKE_CALLS\"; exit 1");
        // `plugin` calls are recorded in `claude_calls`; the marketplaces and plugins it lists come
        // from FAKE_CLAUDE_MARKETS and FAKE_CLAUDE_PLUGINS, and the call named in FAKE_CLAUDE_FAIL
        // (as `plugin install teams@ways`) prints seven lines and fails.
        install.fake(
            "claude",
            r#"case "$1" in
  --version) echo "${FAKE_CLAUDE_VERSION:-2.1.328 (Claude Code)}"; exit 0;;
esac
echo "$*" >> "$FAKE_CALLS.claude"
if [ "$*" = "${FAKE_CLAUDE_FAIL:-}" ]; then
  for n in 1 2 3 4 5 6 7; do echo "claude: failure line $n"; done
  exit 1
fi
case "$*" in
  "plugin marketplace list") echo "${FAKE_CLAUDE_MARKETS:-No marketplaces configured}";;
  "plugin list") echo "${FAKE_CLAUDE_PLUGINS:-No plugins installed}";;
  *) echo "ok";;
esac"#,
        );
        install
    }

    /// The `claude plugin ...` calls the script made, one per line, as the fake `claude` got them.
    fn claude_calls(&self) -> Vec<String> {
        fs::read_to_string(self.temp.path().join("calls.claude"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn with_env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.to_string(), value.to_string()));
        self
    }

    fn fake(&self, name: &str, body: &str) {
        let path = self.bin.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("fake");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("mode");
    }

    fn remove(&self, name: &str) {
        let _ = fs::remove_file(self.bin.join(name));
    }

    fn calls(&self) -> PathBuf {
        self.temp.path().join("calls")
    }

    fn script() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh")
    }

    /// Runs `sh <script> <args>`. Without a terminal the script gets a session of its own, so
    /// `/dev/tty` cannot be opened even when the tests are run from a terminal.
    fn run_script(&self, script: &Path, args: &[&str], terminal: Terminal) -> Output {
        let mut command = Command::new("/bin/sh");
        command
            .arg(script)
            .args(args)
            .env_clear()
            .env("PATH", &self.bin)
            .env("HOME", &self.home)
            .env("TMPDIR", &self.tmp)
            .env("SHELL", "/bin/zsh")
            .env("FAKE_CALLS", self.calls())
            .stdin(Stdio::null());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        match terminal {
            Terminal::Answers(text) => {
                let answers = self.temp.path().join("answers");
                fs::write(&answers, text).expect("answers");
                command.env("ZAF_INSTALL_TTY", answers);
            }
            Terminal::None => {
                // SAFETY: setsid is async-signal-safe and allowed between fork and exec.
                unsafe {
                    command.pre_exec(|| {
                        libc::setsid();
                        Ok(())
                    });
                }
            }
        }
        command.output().expect("run sh")
    }

    fn run(&self, args: &[&str], terminal: Terminal) -> Output {
        self.run_script(&Self::script(), args, terminal)
    }

    /// Every path under HOME and the folder, so a run can be compared with what it started from.
    fn snapshot(&self) -> Vec<String> {
        let mut paths = Vec::new();
        for root in [&self.home, &self.folder] {
            walk(root, root, &mut paths);
        }
        paths.sort();
        paths
    }

    fn folder_arg(&self) -> String {
        self.folder.to_string_lossy().into_owned()
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("read dir").flatten() {
        let path = entry.path();
        out.push(format!("{}:{}", root.display(), path.display()));
        if path.is_dir() {
            walk(root, &path, out);
        }
    }
}

fn host_tool(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn install_refuses_without_terminal() {
    let install = Install::new();
    let before = install.snapshot();
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder], Terminal::None);
    assert!(!output.status.success(), "{}", text(&output));
    assert!(text(&output).contains("--yes"), "{}", text(&output));
    assert_eq!(install.snapshot(), before);
    assert!(!install.calls().exists());

    // --yes takes every default and needs no terminal.
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    // Step 1 passes; step 2 then has no release to read, which is not this test's business.
    assert!(text(&output).contains("✓ Check"), "{}", text(&output));
    assert_eq!(install.snapshot(), before);
}

#[test]
fn root_is_refused_before_any_change() {
    let install = Install::new().with_env("FAKE_ID_U", "0");
    let before = install.snapshot();
    let folder = install.folder_arg();
    for args in [vec!["--dir", &folder, "--yes"], vec![]] {
        let output = install.run(&args, Terminal::Answers("\ny\n"));
        let shown = text(&output);
        assert_eq!(output.status.code(), Some(1), "{shown}");
        assert!(shown.contains("runs as you"), "{shown}");
        assert!(shown.contains("never uses sudo"), "{shown}");
        assert_eq!(shown.trim_end().lines().count(), 1, "{shown}");
        assert!(!shown.contains('✓'), "{shown}");
        assert_eq!(install.snapshot(), before);
        assert!(!install.calls().exists());
        assert!(install.claude_calls().is_empty());
    }

    // An ordinary user is not refused: the run goes on to its checks.
    let install = Install::new().with_env("FAKE_ID_U", "1000");
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    assert!(text(&output).contains("✓ Check"), "{}", text(&output));
}

#[test]
fn install_refuses_missing_tool() {
    for (tool, gone) in [
        ("git", vec!["git"]),
        ("curl", vec!["curl"]),
        ("tar", vec!["tar"]),
        ("sha256sum", vec!["sha256sum", "shasum"]),
        ("claude", vec!["claude"]),
    ] {
        let install = Install::new();
        for name in gone {
            install.remove(name);
        }
        let before = install.snapshot();
        let folder = install.folder_arg();
        let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
        let shown = text(&output);
        assert!(!output.status.success(), "{tool}: {shown}");
        assert!(shown.contains("✗"), "{tool}: {shown}");
        assert!(
            shown.contains(&format!("Install {tool}")),
            "{tool}: {shown}"
        );
        assert_eq!(install.snapshot(), before, "{tool}");
        assert!(!install.calls().exists(), "{tool}");
    }
}

#[test]
fn install_refuses_old_claude() {
    let install = Install::new().with_env("FAKE_CLAUDE_VERSION", "2.1.200 (Claude Code)");
    let before = install.snapshot();
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert!(!output.status.success(), "{shown}");
    assert!(shown.contains("2.1.200"), "{shown}");
    assert!(shown.contains("2.1.296"), "{shown}");
    assert_eq!(install.snapshot(), before);

    // The minimum itself, and a later one, pass.
    for version in [
        "2.1.296 (Claude Code)",
        "2.2.0 (Claude Code)",
        "3.0.1 (Claude Code)",
    ] {
        let install = Install::new().with_env("FAKE_CLAUDE_VERSION", version);
        let folder = install.folder_arg();
        let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
        assert!(
            text(&output).contains("✓ Check"),
            "{version}: {}",
            text(&output)
        );
    }
}

#[test]
fn install_refuses_unknown_platform() {
    let cargo = "cargo install --git https://github.com/ProgrammingCheetah/ways --tag zaf-v0.0.0-dev --locked zaf";
    for (os, arch, sysctl) in [
        ("FreeBSD", "amd64", "0"),
        ("Linux", "aarch64", "0"),
        ("Darwin", "x86_64", "0"),
    ] {
        let install = Install::new()
            .with_env("FAKE_UNAME_S", os)
            .with_env("FAKE_UNAME_M", arch)
            .with_env("FAKE_SYSCTL_ARM64", sysctl);
        let before = install.snapshot();
        let folder = install.folder_arg();
        let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
        let shown = text(&output);
        assert!(!output.status.success(), "{os} {arch}: {shown}");
        assert!(shown.contains("Linux x86_64"), "{os} {arch}: {shown}");
        assert!(
            shown.contains("macOS Apple silicon"),
            "{os} {arch}: {shown}"
        );
        assert!(shown.contains(cargo), "{os} {arch}: {shown}");
        assert_eq!(install.snapshot(), before, "{os} {arch}");
    }
}

#[test]
fn install_rosetta_is_arm64() {
    for (os, arch, sysctl, target) in [
        ("Darwin", "x86_64", "1", "aarch64-apple-darwin"),
        ("Darwin", "arm64", "1", "aarch64-apple-darwin"),
        ("Linux", "x86_64", "0", "x86_64-unknown-linux-musl"),
    ] {
        let install = Install::new()
            .with_env("FAKE_UNAME_S", os)
            .with_env("FAKE_UNAME_M", arch)
            .with_env("FAKE_SYSCTL_ARM64", sysctl);
        let folder = install.folder_arg();
        let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
        let shown = text(&output);
        assert!(shown.contains("✓ Check"), "{os} {arch}: {shown}");
        assert!(shown.contains(target), "{os} {arch}: {shown}");
    }
}

#[test]
fn install_refuses_unwritable_folder() {
    // A superuser can write anywhere, so there is no folder to refuse.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: running as root");
        return;
    }
    let install = Install::new();
    // Under HOME, so the folder is named as the default is: ~/.local/bin.
    let folder = install.home.join("locked");
    fs::create_dir(&folder).expect("folder");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).expect("mode");
    let before = install.snapshot();
    let output = install.run(
        &["--dir", folder.to_str().unwrap(), "--yes"],
        Terminal::None,
    );
    let shown = text(&output);
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).expect("mode");
    assert!(!output.status.success(), "{shown}");
    assert!(shown.contains("cannot be written"), "{shown}");
    assert!(shown.contains("~/.local/bin"), "{shown}");
    assert!(shown.contains("never uses sudo"), "{shown}");
    assert_eq!(install.snapshot(), before);
}

#[test]
fn relative_folder_is_refused_and_asked_again() {
    let install = Install::new();
    let before = install.snapshot();
    // Three refused answers, then Enter for the default, then "n" at Continue.
    let output = install.run(&[], Terminal::Answers("bin\n.local/bin\n~user/x\n\nn\n"));
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert_eq!(shown.matches("is not a full folder").count(), 3, "{shown}");
    for typed in ["bin is not", ".local/bin is not", "~user/x is not"] {
        assert!(shown.contains(typed), "{typed}: {shown}");
    }
    assert!(shown.contains("such as ~/.local/bin"), "{shown}");
    // The question is asked once at first and once after each refusal.
    assert_eq!(shown.matches("be installed?").count(), 4, "{shown}");
    assert!(shown.contains("zaf 0.0.0-dev in ~/.local/bin"), "{shown}");
    assert!(shown.contains("Nothing changed."), "{shown}");
    assert_eq!(install.snapshot(), before);
    assert!(!install.calls().exists());
}

#[test]
fn relative_dir_flag_is_refused() {
    for dir in ["bin", ".local/bin", "~user/x"] {
        let install = Install::new();
        let before = install.snapshot();
        let output = install.run(&["--dir", dir, "--yes"], Terminal::None);
        let shown = text(&output);
        assert_eq!(output.status.code(), Some(1), "{dir}: {shown}");
        assert!(shown.contains("~/.local/bin"), "{dir}: {shown}");
        assert!(!shown.contains('✓'), "{dir}: {shown}");
        assert_eq!(install.snapshot(), before, "{dir}");
        assert_eq!(fs::read_dir(&install.tmp).unwrap().count(), 0, "{dir}");
        assert!(!install.calls().exists(), "{dir}");
    }
    // The same through `--dir=<folder>`.
    let install = Install::new();
    let output = install.run(&["--dir=bin", "--yes"], Terminal::None);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
}

#[test]
fn colon_folder_is_refused_and_asked_again() {
    let mut install = Install::new();
    install.serve("zaf 0.0.0-dev", true);
    let before = install.snapshot();
    // A refused answer, `~/abin` accepted, then "n" at Continue.
    let output = install.run(&[], Terminal::Answers("~/a:bin\n~/abin\nn\n"));
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert_eq!(
        shown.matches("a folder in PATH cannot hold `:`").count(),
        1,
        "{shown}"
    );
    // The question is asked once at first and once after the refusal.
    assert_eq!(shown.matches("be installed?").count(), 2, "{shown}");
    assert!(shown.contains("zaf 0.0.0-dev in ~/abin"), "{shown}");
    assert!(shown.contains("Nothing changed."), "{shown}");
    assert_eq!(install.snapshot(), before);
    assert!(!install.home.join(".zshrc").exists());
}

#[test]
fn colon_dir_flag_is_refused() {
    let forms: [&[&str]; 2] = [&["--dir", "~/a:bin", "--yes"], &["--dir=~/a:bin", "--yes"]];
    for args in forms {
        let install = Install::new();
        let before = install.snapshot();
        let output = install.run(args, Terminal::None);
        let shown = text(&output);
        assert_eq!(output.status.code(), Some(1), "{shown}");
        assert!(
            shown.contains("a folder in PATH cannot hold `:`"),
            "{shown}"
        );
        assert!(!shown.contains('✓'), "{shown}");
        assert_eq!(install.snapshot(), before);
        assert!(!install.home.join(".zshrc").exists());
        assert!(!install.calls().exists());
    }
    // An absolute folder with a colon, not typed with ~.
    let install = Install::new();
    let dir = format!("{}/a:bin", install.home.display());
    let output = install.run(&["--dir", &dir, "--yes"], Terminal::None);
    assert_eq!(output.status.code(), Some(1), "{}", text(&output));
}

#[test]
fn tilde_folder_is_expanded() {
    let mut install = Install::new();
    install.serve("zaf 0.0.0-dev", true);
    // `~/x` at the folder question, then Enter at Continue.
    let output = install.run(&[], Terminal::Answers("~/x\n\n"));
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert!(!shown.contains("is not a full folder"), "{shown}");
    assert!(install.home.join("x").join("zaf").is_file(), "{shown}");
    assert!(shown.contains("zaf 0.0.0-dev in ~/x"), "{shown}");
    // The PATH block holds the expanded folder (written as $HOME/x), never `~/x` or a relative one.
    let rc = fs::read_to_string(install.home.join(".zshrc")).expect("rc");
    assert!(rc.contains("export PATH=\"$HOME/x:$PATH\""), "{rc}");
}

#[test]
fn install_answer_no_changes_nothing() {
    for answer in ["n", "N", "no"] {
        let install = Install::new();
        let before = install.snapshot();
        // The first line is the folder question: Enter takes the default, ~/.local/bin.
        let output = install.run(&[], Terminal::Answers(&format!("\n{answer}\n")));
        let shown = text(&output);
        assert!(output.status.success(), "{answer}: {shown}");
        assert!(shown.contains("Nothing changed."), "{answer}: {shown}");
        assert!(!shown.contains('✓'), "{answer}: {shown}");
        assert_eq!(install.snapshot(), before, "{answer}");
        assert_eq!(fs::read_dir(&install.tmp).unwrap().count(), 0, "{answer}");
        assert!(!install.calls().exists(), "{answer}");
    }
}

#[test]
fn install_screen_reads_as_mock() {
    let install = Install::new().with_env("NO_COLOR", "1");
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder], Terminal::Answers("n\n"));
    let shown = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "{shown}");
    assert_eq!(
        shown.lines().next(),
        Some("zaf: Zuri's Amazing Framework"),
        "{shown}"
    );
    assert!(!shown.contains('\u{1b}'), "{shown:?}");
    let rows = [
        "1. Check   git, Claude Code 2.1.296+",
        "2. Install zaf 0.0.0-dev in ",
        "3. Add     ",
        "4. Add     the teams plugin to Claude Code",
        "5. Show    your first three commands",
    ];
    let mut at = 0;
    for row in rows {
        let found = shown[at..]
            .find(row)
            .unwrap_or_else(|| panic!("row {row:?} not in order in:\n{shown}"));
        at += found + row.len();
    }
    // Row 3 names the shell file from $SHELL (zsh here); row 2 and 3 the folder.
    assert!(shown.contains(&format!("{folder} to PATH (")), "{shown}");
    assert!(shown.contains(".zshrc)"), "{shown}");
    assert!(shown.contains("It changes nothing else"), "{shown}");
    assert!(shown.contains("Continue? [Y/n]"), "{shown}");
    assert!(shown.contains("Nothing changed."), "{shown}");
}

#[test]
fn install_cut_in_half_runs_nothing() {
    let whole = fs::read_to_string(Install::script()).expect("script");
    let lines: Vec<&str> = whole.lines().collect();
    assert_eq!(
        lines.last().copied(),
        Some("main \"$@\" || exit 1"),
        "the call is the last line"
    );
    assert_eq!(
        lines
            .iter()
            .filter(|line| **line == "ZAF_VERSION=\"0.0.0-dev\"")
            .count(),
        1,
        "the version is on one line"
    );
    let install = Install::new();
    let before = install.snapshot();
    let half = install.temp.path().join("half.sh");
    fs::write(&half, format!("{}\n", lines[..lines.len() / 2].join("\n"))).expect("half");
    // A terminal that would answer yes to everything, so that anything that ran would act.
    let output = install.run_script(&half, &[], Terminal::Answers("\n\n\n"));
    assert_eq!(output.stdout, b"", "{}", text(&output));
    assert!(!text(&output).contains("Zuri"), "{}", text(&output));
    assert_eq!(install.snapshot(), before);
    assert_eq!(fs::read_dir(&install.tmp).unwrap().count(), 0);
    assert!(!install.calls().exists());
}

const TARGET: &str = "x86_64-unknown-linux-musl";

impl Install {
    /// Puts a release in a folder of its own and points `ZAF_INSTALL_FROM` at it: an archive whose
    /// `zaf` prints `said`, and a `.sha256` that matches it or does not. The step's tools (`tar`,
    /// `gzip`, `sha256sum`) are the machine's own.
    fn serve(&mut self, said: &str, matching_sum: bool) -> PathBuf {
        self.serve_for(TARGET, said, matching_sum)
    }

    /// `serve` for the release of another target, such as the macOS one.
    fn serve_for(&mut self, target: &str, said: &str, matching_sum: bool) -> PathBuf {
        for tool in STEP_TOOLS {
            self.remove(tool);
            let path = host_tool(tool).unwrap_or_else(|| panic!("{tool} on the machine"));
            std::os::unix::fs::symlink(path, self.bin.join(tool)).expect("link");
        }
        let release = self.temp.path().join("release");
        let name = format!("zaf-v0.0.0-dev-{target}");
        let content = release.join("content").join(&name);
        fs::create_dir_all(&content).expect("content");
        let zaf = content.join("zaf");
        fs::write(&zaf, format!("#!/bin/sh\necho '{said}'\n")).expect("zaf");
        // Not executable in the archive: the script makes it so, as `zaf update` does.
        fs::set_permissions(&zaf, fs::Permissions::from_mode(0o644)).expect("mode");
        let archive = release.join(format!("{name}.tar.gz"));
        let status = Command::new(host_tool("tar").expect("tar"))
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(release.join("content"))
            .arg(&name)
            .status()
            .expect("tar");
        assert!(status.success());
        let sum = if matching_sum {
            let output = Command::new(host_tool("sha256sum").expect("sha256sum"))
                .arg(&archive)
                .output()
                .expect("sha256sum");
            String::from_utf8_lossy(&output.stdout).into_owned()
        } else {
            format!("{}  {name}.tar.gz\n", "0".repeat(64))
        };
        fs::write(release.join(format!("{name}.tar.gz.sha256")), sum).expect("sum");
        self.env.push((
            "ZAF_INSTALL_FROM".to_string(),
            release.to_string_lossy().into_owned(),
        ));
        release
    }
}

#[test]
fn install_clean_places_zaf() {
    let mut install = Install::new();
    install.serve("zaf 0.0.0-dev", true);
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert!(
        shown.contains(&format!("zaf 0.0.0-dev in {folder}")),
        "{shown}"
    );
    let zaf = install.folder.join("zaf");
    let mode = fs::metadata(&zaf).expect("zaf").permissions().mode();
    assert_eq!(mode & 0o111, 0o111, "executable: {mode:o}");
    let said = Command::new(&zaf).arg("--version").output().expect("run");
    assert_eq!(
        String::from_utf8_lossy(&said.stdout).trim(),
        "zaf 0.0.0-dev"
    );
    assert_eq!(fs::read_dir(&install.folder).unwrap().count(), 1);
    // Nothing of the run is left in the temporary folder.
    assert_eq!(fs::read_dir(&install.tmp).unwrap().count(), 0);
}

#[test]
fn http_source_is_refused() {
    let install = Install::new().with_env("ZAF_INSTALL_FROM", "http://example.invalid/release");
    let before = install.snapshot();
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert_eq!(output.status.code(), Some(1), "{shown}");
    assert!(shown.contains("https"), "{shown}");
    // The fake curl records every call: none was made, and nothing was installed.
    assert!(!install.calls().exists(), "{shown}");
    assert_eq!(install.snapshot(), before);
}

#[test]
fn https_source_uses_the_same_curl_flags() {
    let install = Install::new().with_env("ZAF_INSTALL_FROM", "https://example.invalid/release/");
    install.fake("curl", "echo \"$*\" >> \"$FAKE_CALLS.curl\"; exit 1");
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert!(!output.status.success(), "{shown}");
    let seen = fs::read_to_string(install.temp.path().join("calls.curl")).expect("curl was called");
    assert!(
        seen.contains("--proto =https --tlsv1.2"),
        "curl got: {seen}"
    );
    assert!(
        seen.contains("https://example.invalid/release/zaf-v"),
        "curl got: {seen}"
    );
}

#[test]
fn local_source_still_installs() {
    let mut install = Install::new();
    install.serve("zaf 0.0.0-dev", true);
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    assert!(output.status.success(), "{}", text(&output));
    assert!(install.folder.join("zaf").is_file());
    // A folder source never reaches curl.
    assert!(!install.calls().exists());
}

#[test]
fn install_makes_missing_folder() {
    let mut install = Install::new();
    install.serve("zaf 0.0.0-dev", true);
    let folder = install.temp.path().join("not").join("yet");
    let output = install.run(
        &["--dir", folder.to_str().unwrap(), "--yes"],
        Terminal::None,
    );
    assert!(output.status.success(), "{}", text(&output));
    assert!(folder.join("zaf").is_file());
}

#[test]
fn install_wrong_sum_refuses() {
    let mut install = Install::new();
    install.serve("zaf 0.0.0-dev", false);
    let before = install.snapshot();
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert!(!output.status.success(), "{shown}");
    assert!(shown.contains('✗'), "{shown}");
    assert!(shown.contains("sha256"), "{shown}");
    assert!(!install.folder.join("zaf").exists(), "{shown}");
    assert_eq!(install.snapshot(), before);
    // The log is kept, and its path is named.
    let kept: Vec<_> = fs::read_dir(&install.tmp).unwrap().flatten().collect();
    assert_eq!(kept.len(), 1, "{shown}");
    assert!(
        shown.contains("The log is kept:") && shown.contains(&*kept[0].path().to_string_lossy()),
        "{shown}"
    );
}

#[test]
fn install_wrong_version_refuses() {
    let mut install = Install::new();
    install.serve("zaf 9.9.9", true);
    let before = install.snapshot();
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert!(!output.status.success(), "{shown}");
    assert!(shown.contains('✗'), "{shown}");
    assert!(shown.contains("zaf 9.9.9"), "{shown}");
    assert_eq!(install.snapshot(), before);
}

#[test]
fn install_replaces_by_rename() {
    use std::os::unix::fs::MetadataExt;
    let mut install = Install::new();
    install.serve("zaf 0.0.0-dev", true);
    let folder = install.folder_arg();
    let first = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    assert!(first.status.success(), "{}", text(&first));
    let zaf = install.folder.join("zaf");
    let inode = fs::metadata(&zaf).expect("zaf").ino();
    let second = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    assert!(second.status.success(), "{}", text(&second));
    let names: Vec<_> = fs::read_dir(&install.folder)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["zaf"]);
    // A new file renamed over the old one, never the old one written into.
    assert_ne!(fs::metadata(&zaf).expect("zaf").ino(), inode);
}

const MARK: &str = "# added by zaf's install script";

impl Install {
    /// A release served and the script run for the folder, with `shell` as `$SHELL`.
    fn run_for_shell(&mut self, shell: &str) -> Output {
        self.serve("zaf 0.0.0-dev", true);
        self.env.push(("SHELL".to_string(), shell.to_string()));
        let folder = self.folder_arg();
        self.run(&["--dir", &folder, "--yes"], Terminal::None)
    }

    /// The same, as bash on macOS on Apple silicon.
    fn run_for_macos_bash(&mut self) -> Output {
        self.serve_for("aarch64-apple-darwin", "zaf 0.0.0-dev", true);
        self.env
            .push(("SHELL".to_string(), "/bin/bash".to_string()));
        self.env
            .push(("FAKE_UNAME_S".to_string(), "Darwin".to_string()));
        self.env
            .push(("FAKE_UNAME_M".to_string(), "arm64".to_string()));
        let folder = self.folder_arg();
        self.run(&["--dir", &folder, "--yes"], Terminal::None)
    }

    fn block_line(&self) -> String {
        format!("export PATH=\"{}:$PATH\"", self.folder.display())
    }

    /// Runs with PATH set to the fake folder and the install folder, in the order given.
    fn run_with_path(&mut self, folder_first: bool) -> Output {
        self.serve("zaf 0.0.0-dev", true);
        let (bin, folder) = (self.bin.display(), self.folder.display());
        let path = if folder_first {
            format!("{folder}:{bin}")
        } else {
            format!("{bin}:{folder}")
        };
        self.env.push(("PATH".to_string(), path));
        let folder = self.folder_arg();
        self.run(&["--dir", &folder, "--yes"], Terminal::None)
    }
}

fn marks(file: &Path) -> usize {
    fs::read_to_string(file)
        .expect("shell file")
        .lines()
        .filter(|line| *line == MARK)
        .count()
}

#[test]
fn install_path_zsh_block() {
    let mut install = Install::new();
    let zdotdir = install.temp.path().join("zdot");
    fs::create_dir_all(&zdotdir).expect("zdotdir");
    install.env.push((
        "ZDOTDIR".to_string(),
        zdotdir.to_string_lossy().into_owned(),
    ));
    let output = install.run_for_shell("/bin/zsh");
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    let rc = fs::read_to_string(zdotdir.join(".zshrc")).expect(".zshrc");
    assert_eq!(marks(&zdotdir.join(".zshrc")), 1, "{rc}");
    assert!(rc.contains(&install.block_line()), "{rc}");
    assert!(!install.home.join(".zshrc").exists());
    assert!(shown.contains("added to PATH in"), "{shown}");
}

#[test]
fn install_path_bash_linux() {
    let mut install = Install::new();
    let output = install.run_for_shell("/bin/bash");
    assert!(output.status.success(), "{}", text(&output));
    let rc = install.home.join(".bashrc");
    assert_eq!(marks(&rc), 1);
    assert!(
        fs::read_to_string(&rc)
            .unwrap()
            .contains(&install.block_line())
    );
    assert!(!install.home.join(".bash_profile").exists());
    assert!(!install.home.join(".profile").exists());
}

#[test]
fn install_path_bash_macos_existing_file() {
    // With ~/.profile alone, the block goes there.
    let mut install = Install::new();
    fs::write(install.home.join(".profile"), "export A=1").expect("profile");
    let output = install.run_for_macos_bash();
    assert!(output.status.success(), "{}", text(&output));
    let profile = install.home.join(".profile");
    assert_eq!(marks(&profile), 1);
    let content = fs::read_to_string(&profile).unwrap();
    // The line it had stays a line of its own, and the block starts a new one.
    assert!(content.starts_with("export A=1\n\n"), "{content:?}");
    assert!(!install.home.join(".bash_profile").exists());
    assert!(!install.home.join(".bash_login").exists());

    // With none, a new ~/.bash_profile.
    let mut install = Install::new();
    let output = install.run_for_macos_bash();
    assert!(output.status.success(), "{}", text(&output));
    let created = install.home.join(".bash_profile");
    assert_eq!(marks(&created), 1);
    assert!(
        fs::read_to_string(&created)
            .unwrap()
            .contains(&install.block_line())
    );
    assert!(!install.home.join(".profile").exists());

    // The first of the three that exists wins: .bash_login over .profile.
    let mut install = Install::new();
    fs::write(install.home.join(".bash_login"), "").expect("login");
    fs::write(install.home.join(".profile"), "").expect("profile");
    let output = install.run_for_macos_bash();
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(marks(&install.home.join(".bash_login")), 1);
    assert_eq!(
        fs::read_to_string(install.home.join(".profile")).unwrap(),
        ""
    );
}

#[test]
fn install_path_fish_conf_d() {
    let mut install = Install::new();
    let output = install.run_for_shell("/usr/bin/fish");
    assert!(output.status.success(), "{}", text(&output));
    let file = install.home.join(".config/fish/conf.d/zaf.fish");
    assert_eq!(marks(&file), 1);
    let content = fs::read_to_string(&file).unwrap();
    assert!(
        content.contains(&format!(
            "fish_add_path -- \"{}\"",
            install.folder.display()
        )),
        "{content}"
    );
    assert!(!content.contains("export"), "{content}");
}

#[test]
fn install_path_other_shell_profile() {
    let mut install = Install::new();
    let output = install.run_for_shell("/bin/tcsh");
    assert!(output.status.success(), "{}", text(&output));
    let profile = install.home.join(".profile");
    assert_eq!(marks(&profile), 1);
    assert!(
        fs::read_to_string(&profile)
            .unwrap()
            .contains(&install.block_line())
    );
}

#[test]
fn install_path_writes_home_as_home() {
    let mut install = Install::new();
    let folder = install.home.join(".local/bin");
    install.serve("zaf 0.0.0-dev", true);
    let output = install.run(
        &["--dir", folder.to_str().unwrap(), "--yes"],
        Terminal::None,
    );
    assert!(output.status.success(), "{}", text(&output));
    let rc = fs::read_to_string(install.home.join(".zshrc")).unwrap();
    assert!(
        rc.contains("export PATH=\"$HOME/.local/bin:$PATH\""),
        "{rc}"
    );
}

#[test]
fn install_second_run_adds_nothing() {
    let mut install = Install::new();
    // A file with no newline at its end, so the first run has to mend it.
    fs::write(install.home.join(".zshrc"), "alias a=b").expect("zshrc");
    install.serve("zaf 0.0.0-dev", true);
    let folder = install.folder_arg();
    let first = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    assert!(first.status.success(), "{}", text(&first));
    let rc = install.home.join(".zshrc");
    let after_first = fs::read(&rc).expect("zshrc");
    assert!(
        String::from_utf8_lossy(&after_first).starts_with("alias a=b\n\n"),
        "{}",
        String::from_utf8_lossy(&after_first)
    );
    let second = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&second);
    assert!(second.status.success(), "{shown}");
    assert_eq!(fs::read(&rc).expect("zshrc"), after_first);
    assert_eq!(marks(&rc), 1);
    assert!(shown.contains("already has the block"), "{shown}");

    // A folder already on PATH writes nothing at all.
    let mut install = Install::new();
    let output = install.run_with_path(false);
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert!(shown.contains("already on PATH"), "{shown}");
    assert!(!install.home.join(".zshrc").exists());
}

#[test]
fn second_folder_updates_the_path_block() {
    let mut install = Install::new();
    let rc = install.home.join(".zshrc");
    let first = install.temp.path().join("first-bin");
    let before = format!(
        "alias a=b\n\n{MARK}\nexport PATH=\"{}:$PATH\"\n",
        first.display()
    );
    fs::write(&rc, &before).expect("zshrc");
    install.serve("zaf 0.0.0-dev", true);
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    let after = fs::read_to_string(&rc).expect("zshrc");
    assert_eq!(
        after,
        format!("alias a=b\n\n{MARK}\n{}\n", install.block_line())
    );
    assert_eq!(marks(&rc), 1, "{after}");
    assert!(!after.contains("first-bin"), "{after}");
    assert!(shown.contains(&first.display().to_string()), "{shown}");

    // A block that names $HOME is shown with ~.
    fs::write(
        &rc,
        format!("{MARK}\nexport PATH=\"$HOME/.local/bin:$PATH\"\n"),
    )
    .expect("zshrc");
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert!(shown.contains("~/.local/bin"), "{shown}");
    assert_eq!(marks(&rc), 1);
}

#[test]
fn same_folder_writes_nothing() {
    let mut install = Install::new();
    let rc = install.home.join(".zshrc");
    fs::write(
        &rc,
        format!("alias a=b\n{MARK}\n{}\nalias c=d", install.block_line()),
    )
    .expect("zshrc");
    let before = fs::read(&rc).expect("zshrc");
    install.serve("zaf 0.0.0-dev", true);
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert!(shown.contains("already has the block"), "{shown}");
    assert_eq!(fs::read(&rc).expect("zshrc"), before);
}

#[test]
fn other_rc_lines_stay() {
    let mut install = Install::new();
    let rc = install.home.join(".zshrc");
    let other = install.temp.path().join("first-bin");
    // Lines around the block, odd ones among them, and no newline at the end.
    let before = format!(
        "  indented \\ back\\slash $X \"q\"\n\n{MARK}\nexport PATH=\"{}:$PATH\"\n# export PATH=\"/old:$PATH\"\nalias c=d",
        other.display()
    );
    fs::write(&rc, &before).expect("zshrc");
    install.serve("zaf 0.0.0-dev", true);
    let folder = install.folder_arg();
    let output = install.run(&["--dir", &folder, "--yes"], Terminal::None);
    assert!(output.status.success(), "{}", text(&output));
    assert_eq!(
        fs::read_to_string(&rc).expect("zshrc"),
        format!(
            "  indented \\ back\\slash $X \"q\"\n\n{MARK}\n{}\n# export PATH=\"/old:$PATH\"\nalias c=d",
            install.block_line()
        )
    );
}

#[test]
fn path_rewrite_removes_the_backup() {
    let mut install = Install::new();
    let rc = install.home.join(".zshrc");
    let other = install.temp.path().join("first-bin");
    fs::write(
        &rc,
        format!(
            "alias a=b\n{MARK}\nexport PATH=\"{}:$PATH\"\nalias c=d\n",
            other.display()
        ),
    )
    .expect("zshrc");
    let output = install.run_whole();
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert!(!install.home.join(".zshrc.zaf-backup").exists(), "{shown}");
    assert_eq!(marks(&rc), 1);
    let after = fs::read_to_string(&rc).expect("zshrc");
    assert_eq!(after.matches(&install.block_line()).count(), 1, "{after}");
    assert!(!after.contains("first-bin"), "{after}");
}

#[test]
fn failed_path_rewrite_keeps_the_backup() {
    // A superuser writes a read-only file, so the rewrite cannot fail this way.
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("skipped: running as root");
        return;
    }
    let mut install = Install::new();
    let rc = install.home.join(".zshrc");
    let other = install.temp.path().join("first-bin");
    let before = format!(
        "alias a=b\n{MARK}\nexport PATH=\"{}:$PATH\"\nalias c=d\n",
        other.display()
    );
    fs::write(&rc, &before).expect("zshrc");
    // Read-only: the copy still reads it, the rewrite cannot write it.
    fs::set_permissions(&rc, fs::Permissions::from_mode(0o444)).expect("chmod");
    let output = install.run_whole();
    let shown = text(&output);
    assert!(!output.status.success(), "{shown}");
    let backup = install.home.join(".zshrc.zaf-backup");
    assert_eq!(fs::read_to_string(&backup).expect("backup"), before);
    assert!(shown.contains(&backup.display().to_string()), "{shown}");
    // The file itself is as it was, and the temporary copy was not deleted.
    assert_eq!(fs::read_to_string(&rc).expect("zshrc"), before);
    assert!(shown.contains("zaf-rc."), "{shown}");
}

#[test]
fn install_names_shadowing_zaf() {
    // Another zaf in the fake PATH, found before a folder that is not on PATH yet.
    let mut install = Install::new();
    install.fake("zaf", "echo other");
    let output = install.run_for_shell("/bin/zsh");
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    let other = install.bin.join("zaf");
    assert!(
        shown.contains(&format!("another zaf first: {}", other.display())),
        "{shown}"
    );

    // The folder on PATH after it: the other comes first.
    let mut install = Install::new();
    install.fake("zaf", "echo other");
    let output = install.run_with_path(false);
    let shown = text(&output);
    assert!(
        shown.contains(&format!(
            "Another zaf comes first on PATH: {}",
            install.bin.join("zaf").display()
        )),
        "{shown}"
    );

    // The folder first: nothing to name.
    let mut install = Install::new();
    install.fake("zaf", "echo other");
    let output = install.run_with_path(true);
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert!(
        !shown.contains("another zaf") && !shown.contains("Another zaf"),
        "{shown}"
    );
}

impl Install {
    /// A release served and the whole script run for the folder.
    fn run_whole(&mut self) -> Output {
        self.serve("zaf 0.0.0-dev", true);
        let folder = self.folder_arg();
        self.run(&["--dir", &folder, "--yes"], Terminal::None)
    }
}

#[test]
fn install_plugin_adds_marketplace() {
    let mut install = Install::new();
    let output = install.run_whole();
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert_eq!(
        install.claude_calls(),
        [
            "plugin marketplace list",
            "plugin marketplace add ProgrammingCheetah/ways",
            "plugin list",
            "plugin install teams@ways",
        ],
        "{shown}"
    );
    let line = shown
        .lines()
        .find(|line| line.contains("✓ Add") && line.contains("teams plugin"))
        .unwrap_or_else(|| panic!("no plugin line in:\n{shown}"));
    assert!(line.ends_with("teams@ways)"), "{line}");
}

#[test]
fn install_plugin_updates_when_present() {
    let mut install = Install::new()
        .with_env(
            "FAKE_CLAUDE_MARKETS",
            "  ❯ ways\n    Source: GitHub (ProgrammingCheetah/ways)",
        )
        .with_env("FAKE_CLAUDE_PLUGINS", "  ❯ teams@ways\n    Version: 2.1.4");
    let output = install.run_whole();
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert_eq!(
        install.claude_calls(),
        [
            "plugin marketplace list",
            "plugin list",
            "plugin update teams@ways"
        ],
        "{shown}"
    );
    assert!(
        shown.contains("updated in Claude Code (teams@ways)"),
        "{shown}"
    );
}

#[test]
fn install_plugin_failure_shows_log() {
    let mut install = Install::new().with_env("FAKE_CLAUDE_FAIL", "plugin install teams@ways");
    let output = install.run_whole();
    let shown = text(&output);
    assert!(!output.status.success(), "{shown}");
    assert!(shown.contains("✗ Add"), "{shown}");
    // The last five of its seven lines, and not the first two.
    for n in 3..=7 {
        assert!(
            shown.contains(&format!("    claude: failure line {n}")),
            "{shown}"
        );
    }
    assert!(!shown.contains("failure line 2"), "{shown}");
    let kept: Vec<_> = fs::read_dir(&install.tmp).unwrap().flatten().collect();
    assert_eq!(kept.len(), 1, "{shown}");
    assert!(
        shown.contains("The log is kept:") && shown.contains(&*kept[0].path().to_string_lossy()),
        "{shown}"
    );
    // The closing commands are not printed after a failure.
    assert!(!shown.contains("is installed."), "{shown}");
}

#[test]
fn install_closing_commands() {
    let mut install = Install::new();
    let output = install.run_whole();
    let shown = text(&output);
    assert!(output.status.success(), "{shown}");
    assert!(
        shown.contains("zaf is installed. Open a new terminal first, so PATH picks it up."),
        "{shown}"
    );
    let words = [
        "zaf setup",
        "zaf begin --version 0.1.0",
        "zaf spawn --watch --version 0.1.0",
        "zaf hub",
        "zaf status",
        "zaf help",
    ];
    let start = shown
        .find("Start working on anything in 3 commands")
        .expect("heading");
    let tail = &shown[start..];
    let mut at = 0;
    for word in words {
        let found = tail[at..]
            .find(&format!("{word} "))
            .unwrap_or_else(|| panic!("{word:?} not in order in:\n{tail}"));
        at += found + word.len();
    }
    assert!(tail.contains("connects zaf to your project"), "{tail}");
    assert!(
        shown.trim_end().ends_with(
            "The teams plugin is what zaf starts inside Claude Code; you never call it yourself."
        ),
        "{shown}"
    );
}
