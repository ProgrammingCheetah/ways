//! `zaf spawn --watch` in a terminal, through the binary under a pseudo-terminal: one frame redrawn
//! in place instead of each tick's text, the tick's text and warnings in `.zaf/logs/scheduler.log`,
//! a failed tick as one line naming the log, and the terminal's settings put back after Ctrl-C.
//! With the output piped, or `--json`, the stream is as it always was.

mod common;

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{Project, Running};

const MANIFEST: &str = "[project]\nname = \"example\"\n\n\
    [board]\nkind = \"github-issues\"\nrepo = \"example/tracker\"\n\n\
    [runners.claude]\ncommand = \"true\"\n\n\
    [roles.team]\nmodel = \"opus\"\neffort = \"xhigh\"\nprofile = \"build\"\nturns = 5\nbatch = 2\n\n\
    [spawn]\ntick_minutes = 1\n";

/// What the view draws to go up over the last frame and clear below it.
const REDRAW: &str = "A\r\x1b[J";

/// A project with current profiles, an empty board and a `pre-tick` hook that marks each tick and
/// exits with `hook_exit`.
fn project(hook_exit: u8) -> Project {
    let mut project = Project::new(MANIFEST);
    let config = project.temp.path().join("claude-config");
    fs::create_dir_all(&config).expect("config");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project.on("*", "[]");
    project.provide_slice_actions();
    let output = project.zaf(&["profile", "init"]);
    assert!(output.status.success(), "profile init failed");
    let hooks = project.root.join(".zaf/actions/hooks");
    fs::create_dir_all(&hooks).expect("hooks");
    let file = hooks.join("pre-tick");
    fs::write(
        &file,
        format!(
            "#!/bin/sh\n# zaf description: before each tick\n# zaf 1: failed\n\
             echo tick >> ticked\nexit {hook_exit}\n"
        ),
    )
    .expect("hook");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    project.ready_to_build("0.3.0", true);
    project
}

fn watch_args(json: bool) -> Vec<&'static str> {
    let mut args = Vec::new();
    if json {
        args.push("--json");
    }
    args.extend(["spawn", "--watch", "--version", "0.3.0"]);
    args
}

/// Waits up to `within` for `done` to hold.
fn wait_until(within: Duration, done: impl Fn() -> bool) -> bool {
    let deadline = Instant::now() + within;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    done()
}

fn log_of(project: &Project) -> String {
    fs::read_to_string(project.root.join(".zaf/logs/scheduler.log")).unwrap_or_default()
}

/// A watch started on a pseudo-terminal, which it reads as its stdin and writes as its stdout and
/// stderr, with everything it drew gathered.
struct Terminal {
    child: Child,
    master: File,
    slave: File,
    /// The terminal's settings before the watch started.
    before: libc::termios,
    drawn: Arc<Mutex<Vec<u8>>>,
}

impl Terminal {
    fn start(project: &Project, args: &[&str]) -> Self {
        Self::start_command(project.command_in(&project.root, args))
    }

    /// Starts `command`, a `zaf` of the project, on a terminal of its own.
    fn start_command(mut command: Command) -> Self {
        let (master, slave) = open_pty();
        let before = settings_of(&slave);
        command
            .env("TERM", "xterm")
            .env_remove("NO_COLOR")
            .stdin(Stdio::from(slave.try_clone().expect("slave")))
            .stdout(Stdio::from(slave.try_clone().expect("slave")))
            .stderr(Stdio::from(slave.try_clone().expect("slave")));
        // SAFETY: only `setsid` and `ioctl` run between the fork and the exec, which are safe
        // there; the terminal becomes the child's own, so Ctrl-C reaches it as a signal.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                Ok(())
            });
        }
        let child = command.spawn().expect("start zaf");
        let drawn = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&drawn);
        let mut reader = master.try_clone().expect("master");
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                sink.lock()
                    .expect("sink")
                    .extend_from_slice(&buffer[..read]);
            }
        });
        Self {
            child,
            master,
            slave,
            before,
            drawn,
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.drawn.lock().expect("drawn")).into_owned()
    }

    /// Types Ctrl-C.
    fn interrupt(&mut self) {
        self.master.write_all(&[3]).expect("type Ctrl-C");
    }

    /// Types `text` and Enter.
    fn type_line(&mut self, text: &str) {
        self.master
            .write_all(format!("{text}\n").as_bytes())
            .expect("type a line");
    }

    /// Types `bytes` as they are, with no Enter after them.
    fn type_raw(&mut self, bytes: &[u8]) {
        self.master.write_all(bytes).expect("type bytes");
    }

    /// Waits up to `within` for what a person reads (the drawing without its codes) to hold `text`.
    fn shows(&self, within: Duration, text: &str) -> bool {
        wait_until(within, || plain(&self.text()).contains(text))
    }

    /// The exit code, once the watch ended within `within`.
    fn exit(&mut self, within: Duration) -> Option<i32> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return status.code();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    /// The terminal's settings now, as the slave side holds them.
    fn settings(&self) -> libc::termios {
        settings_of(&self.slave)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `text` without its escape codes (`ESC [ … letter`): the words a person reads.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// The settings of the terminal `slave` is one end of.
fn settings_of(slave: &File) -> libc::termios {
    // SAFETY: an all-zero `termios` is a valid value of that plain-data type, which `tcgetattr`
    // then fills in from the open descriptor.
    unsafe {
        let mut settings: libc::termios = std::mem::zeroed();
        assert_eq!(libc::tcgetattr(slave.as_raw_fd(), &raw mut settings), 0);
        settings
    }
}

/// The settings that `stty` shows, as the fields that hold them.
fn same_settings(a: &libc::termios, b: &libc::termios) -> bool {
    a.c_iflag == b.c_iflag
        && a.c_oflag == b.c_oflag
        && a.c_cflag == b.c_cflag
        && a.c_lflag == b.c_lflag
        && a.c_cc == b.c_cc
}

/// A pseudo-terminal 30 rows by 100 columns: its master and its slave.
fn open_pty() -> (File, File) {
    use std::os::fd::FromRawFd;
    let (mut master, mut slave) = (0, 0);
    let size = libc::winsize {
        ws_row: 30,
        ws_col: 100,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: `openpty` writes the two descriptors, takes no name and no settings, and only reads
    // `size`; each descriptor it hands back is ours alone.
    unsafe {
        let made = libc::openpty(
            &raw mut master,
            &raw mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            (&raw const size).cast_mut(),
        );
        assert_eq!(made, 0, "openpty");
        (File::from_raw_fd(master), File::from_raw_fd(slave))
    }
}

#[test]
fn piped_watch_streams_as_today() {
    let project = project(0);
    let watch = Running::start(project.command_in(&project.root, &watch_args(false)));
    // The tick has ended once its `next tick` line is out: a slow machine only waits longer.
    watch.wait_for_lines("next tick ", 1);
    assert!(project.root.join("ticked").exists());
    let stdout = watch.stdout();

    assert!(stdout.contains("a tick every 1 minutes"), "{stdout}");
    assert!(stdout.contains("nothing to do"), "{stdout}");
    assert!(stdout.contains("next tick "), "{stdout}");
    assert!(!stdout.contains('\x1b'), "no code in a pipe: {stdout:?}");
    assert!(
        !project.root.join(".zaf/logs/scheduler.log").exists(),
        "a pipe writes no scheduler log"
    );
}

#[test]
fn json_watch_streams_as_today() {
    let project = project(0);
    // A terminal on all three, and still `--json` streams.
    let mut terminal = Terminal::start(&project, &watch_args(true));
    let ticked = project.root.join("ticked");
    assert!(wait_until(Duration::from_secs(20), || ticked.exists()));
    assert!(wait_until(Duration::from_secs(10), || terminal
        .text()
        .contains("{\"scheduler\":\"scheduler up")));
    std::thread::sleep(Duration::from_millis(500));
    let text = terminal.text();
    terminal.child.kill().expect("kill");

    assert!(text.contains("{\"scheduler\":\"scheduler up"), "{text}");
    assert!(!text.contains('\x1b'), "no code under --json: {text:?}");
    assert!(
        !project.root.join(".zaf/logs/scheduler.log").exists(),
        "--json writes no scheduler log"
    );
}

#[test]
fn terminal_watch_redraws_in_place() {
    let project = project(0);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(
        wait_until(Duration::from_secs(20), || terminal
            .text()
            .matches(REDRAW)
            .count()
            >= 3),
        "no frame was redrawn: {:?}",
        terminal.text()
    );
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
    let text = terminal.text();

    // Painted: each meaning in its own code, so the words are read without them.
    assert!(text.contains("\x1b[1;38;5;201mzaf\x1b[0m"), "{text:?}");
    assert!(plain(&text).contains("zaf is building 0.3.0"), "{text:?}");
    // Frames, never a tick's text block: that is the log's.
    assert!(!text.contains("nothing to do"), "{text:?}");
    assert!(!text.contains("a tick every 1 minutes"), "{text:?}");
    assert!(!text.contains("next tick "), "{text:?}");
    // The normal screen: the alternate one is never entered.
    assert!(!text.contains("\x1b[?1049h"), "{text:?}");
    // What the watch says once the terminal is back.
    assert!(text.contains("scheduler stopped on SIGINT"), "{text:?}");
}

#[test]
fn tick_text_goes_to_scheduler_log() {
    let project = project(0);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(wait_until(Duration::from_secs(20), || log_of(&project)
        .contains("nothing to do")));
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
    let log = log_of(&project);

    assert!(log.contains("a tick every 1 minutes"), "{log}");
    assert!(log.contains("nothing to do"), "{log}");
    assert!(log.contains("next tick "), "{log}");
    assert!(!log.contains('\x1b'), "no drawing in the log: {log:?}");
    assert!(!terminal.text().contains("nothing to do"));
}

#[test]
fn failed_tick_shows_one_line() {
    // The pre-tick hook exits 1, which fails the tick with exit 2.
    let project = project(1);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(
        wait_until(Duration::from_secs(20), || terminal
            .text()
            .contains("tick 1 failed with exit 2")),
        "{:?}",
        terminal.text()
    );
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
    let text = terminal.text();

    assert!(
        text.contains("tick 1 failed with exit 2; the log is .zaf/logs/scheduler.log"),
        "{text:?}"
    );
    // One line in the view; the scheduler's own words are in the log.
    assert!(!text.contains("the watch goes on"), "{text:?}");
    assert!(
        log_of(&project).contains("tick 1 failed with exit 2; the watch goes on"),
        "{}",
        log_of(&project)
    );
}

#[test]
fn ctrl_c_restores_terminal() {
    let project = project(0);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(wait_until(Duration::from_secs(20), || plain(
        &terminal.text()
    )
    .contains("zaf is building")));
    // The view is up: keys are not echoed.
    assert!(
        wait_until(Duration::from_secs(5), || terminal.settings().c_lflag
            & libc::ECHO
            == 0),
        "echo is still on while the view is up"
    );
    terminal.interrupt();

    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
    let after = terminal.settings();
    assert!(
        same_settings(&terminal.before, &after),
        "the terminal's settings changed: {} {} {} {} before, {} {} {} {} after",
        terminal.before.c_iflag,
        terminal.before.c_oflag,
        terminal.before.c_cflag,
        terminal.before.c_lflag,
        after.c_iflag,
        after.c_oflag,
        after.c_cflag,
        after.c_lflag
    );
}

// Answering: a question open on the board is asked in the view.

/// Opens `questions` (number, text) on the project's board, each raised on task #6, which waits on
/// nothing, so a ruling only records and closes.
fn open_questions(project: &mut Project, questions: &[(u64, &str)]) {
    let row = |number: u64, text: &str| {
        serde_json::json!({
            "number": number,
            "title": text,
            "body": format!("Raised on #6.\n\n{text}"),
            "labels": [{"name": "question"}],
            "state": "OPEN",
            "comments": [],
        })
    };
    let rows: Vec<_> = questions.iter().map(|(n, text)| row(*n, text)).collect();
    project.on_first(
        "issue list --label question --state open *",
        &serde_json::to_string(&rows).expect("rows"),
    );
    // The more specific list, which must match before the one above.
    project.on_first("issue list --label question --label door *", "[]");
    for (number, text) in questions {
        project
            .on_first(
                &format!("issue view {number} *"),
                &row(*number, text).to_string(),
            )
            .on_first(&format!("issue close {number} *"), "")
            .on_first(
                &format!("issue comment {number} *"),
                "https://github.com/example/tracker/issues/1#issuecomment-1\n",
            );
    }
    project.on_first(
        "issue view 6 *",
        r#"{"number":6,"title":"Task 6","body":"","labels":[{"name":"task"},{"name":"ready"}],"state":"OPEN","comments":[]}"#,
    );
}

/// The calls the board was asked to write for question `number`: comments and closes.
fn writes_on(project: &Project, number: u64) -> Vec<String> {
    project
        .gh_calls()
        .into_iter()
        .filter(|call| {
            call.starts_with(&format!("issue comment {number} "))
                || call.starts_with(&format!("issue close {number}"))
        })
        .collect()
}

fn ticks(project: &Project) -> usize {
    fs::read_to_string(project.root.join("ticked")).map_or(0, |text| text.lines().count())
}

const QUESTION: &str = "Which columns should the export have?";

#[test]
fn question_answered_in_view_is_ruled() {
    let mut project = project(0);
    open_questions(&mut project, &[(7, QUESTION)]);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(
        terminal.shows(Duration::from_secs(20), "Your answer:"),
        "{:?}",
        terminal.text()
    );
    let seen = plain(&terminal.text());
    // The question in full with its own number, and how to answer from elsewhere.
    assert!(seen.contains(&format!("#7 asks: {QUESTION}")), "{seen}");
    assert!(
        seen.contains(
            "The rest keeps building while you think. Answer here, or from any terminal:"
        ),
        "{seen}"
    );
    assert!(seen.contains("zaf board ruling 7 'your answer'"), "{seen}");
    // Echo is on while asking.
    assert!(
        wait_until(Duration::from_secs(5), || terminal.settings().c_lflag
            & libc::ECHO
            != 0),
        "echo is off while asking"
    );
    let before = ticks(&project);

    terminal.type_line("Title and author.");

    assert!(
        terminal.shows(Duration::from_secs(20), "✓ recorded; #7 goes on."),
        "{:?}",
        terminal.text()
    );
    let writes = writes_on(&project, 7);
    assert!(
        writes
            .iter()
            .any(|call| call.starts_with("issue comment 7 ") && call.contains("Title and author.")),
        "{writes:?}"
    );
    assert!(
        writes.iter().any(|call| call.starts_with("issue close 7")),
        "{writes:?}"
    );
    // Echo is off again, a tick was asked for and the question is not asked again.
    assert!(
        wait_until(Duration::from_secs(5), || terminal.settings().c_lflag
            & libc::ECHO
            == 0),
        "echo is still on after the answer"
    );
    assert!(
        wait_until(Duration::from_secs(20), || ticks(&project) > before),
        "the ruling asked for no tick"
    );
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
    let seen = plain(&terminal.text());
    assert_eq!(seen.matches("#7 asks:").count(), 1, "{seen}");
    assert!(!seen.contains("stays open"), "{seen}");
}

#[test]
fn empty_line_skips_for_the_run() {
    let mut project = project(0);
    open_questions(&mut project, &[(7, QUESTION)]);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(terminal.shows(Duration::from_secs(20), "Your answer:"));
    let before = ticks(&project);

    terminal.type_line("");

    assert!(
        terminal.shows(Duration::from_secs(20), "skipped; #7 stays open"),
        "{:?}",
        terminal.text()
    );
    // A tick reads the board again, which still holds the question.
    let (code, stdout, stderr) =
        common::outcome(&project.zaf(&["spawn", "--once", "--version", "0.3.0"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(wait_until(Duration::from_secs(20), || ticks(&project) > before));
    std::thread::sleep(Duration::from_secs(1));
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));

    let seen = plain(&terminal.text());
    assert_eq!(seen.matches("#7 asks:").count(), 1, "asked again: {seen}");
    assert!(
        writes_on(&project, 7).is_empty(),
        "{:?}",
        writes_on(&project, 7)
    );
    assert!(!seen.contains("recorded"), "{seen}");
}

#[test]
fn questions_asked_oldest_first() {
    let mut project = project(0);
    open_questions(&mut project, &[(9, "Which rows?"), (7, QUESTION)]);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(
        terminal.shows(Duration::from_secs(20), "#7 asks:"),
        "{:?}",
        terminal.text()
    );
    assert!(
        !plain(&terminal.text()).contains("#9 asks:"),
        "one at a time: {:?}",
        terminal.text()
    );

    terminal.type_line("Title and author.");
    assert!(terminal.shows(Duration::from_secs(20), "recorded; #7 goes on."));
    assert!(
        terminal.shows(Duration::from_secs(20), "#9 asks: Which rows?"),
        "{:?}",
        terminal.text()
    );
    terminal.type_line("Header row only.");
    assert!(terminal.shows(Duration::from_secs(20), "recorded; #9 goes on."));
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));

    let seen = plain(&terminal.text());
    let at = |text: &str| {
        seen.find(text)
            .unwrap_or_else(|| panic!("no {text:?} in {seen}"))
    };
    assert!(at("#7 asks:") < at("recorded; #7 goes on."), "{seen}");
    assert!(at("recorded; #7 goes on.") < at("#9 asks:"), "{seen}");
    assert!(at("#9 asks:") < at("recorded; #9 goes on."), "{seen}");
}

#[test]
fn answer_skips_gap_check() {
    let mut project = project(0);
    open_questions(&mut project, &[(7, QUESTION)]);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(terminal.shows(Duration::from_secs(20), "Your answer:"));
    // An empty backtick pair and a gap: what `zaf board ruling` refuses as eaten by a shell.
    let typed = "Use `` for the name.  Then the rule.";
    assert!(zaf::domain::check_typed_ruling(typed, false).is_err());

    terminal.type_line(typed);

    assert!(
        terminal.shows(Duration::from_secs(20), "✓ recorded; #7 goes on."),
        "{:?}",
        terminal.text()
    );
    let writes = writes_on(&project, 7);
    assert!(
        writes
            .iter()
            .any(|call| call.starts_with("issue comment 7 ") && call.contains(typed)),
        "{writes:?}"
    );
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
}

#[test]
fn ctrl_c_while_asking_names_the_question() {
    let mut project = project(0);
    open_questions(&mut project, &[(7, QUESTION)]);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(terminal.shows(Duration::from_secs(20), "Your answer:"));

    terminal.interrupt();

    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
    let text = terminal.text();
    assert!(text.contains("scheduler stopped on SIGINT"), "{text:?}");
    assert!(
        plain(&text).contains("#7 stays open: answer it with `zaf board ruling 7 '…'`"),
        "{text:?}"
    );
    assert!(writes_on(&project, 7).is_empty());
    let after = terminal.settings();
    assert!(
        same_settings(&terminal.before, &after),
        "the terminal was not put back"
    );
}

/// What turns bracketed paste on, and off.
const PASTE_ON: &str = "\x1b[?2004h";
const PASTE_OFF: &str = "\x1b[?2004l";

#[test]
fn multi_line_paste_without_last_newline_is_refused() {
    let mut project = project(0);
    open_questions(&mut project, &[(7, QUESTION)]);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(terminal.shows(Duration::from_secs(20), "Your answer:"));

    // A bracketed paste of two lines, the second with no newline after it.
    terminal.type_raw(b"\x1b[200~line one\nline two \x1b[201~");

    assert!(
        terminal.shows(
            Duration::from_secs(20),
            "one line only; answer longer ones with `zaf board ruling`"
        ),
        "{:?}",
        terminal.text()
    );
    // The same question is asked again, and still answers a line.
    assert!(
        wait_until(Duration::from_secs(20), || plain(&terminal.text())
            .matches("#7 asks:")
            .count()
            == 2),
        "{:?}",
        terminal.text()
    );
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        writes_on(&project, 7).is_empty(),
        "{:?}",
        writes_on(&project, 7)
    );
    assert!(!plain(&terminal.text()).contains("recorded"));
    terminal.type_line("Title and author.");
    assert!(
        terminal.shows(Duration::from_secs(20), "✓ recorded; #7 goes on."),
        "{:?}",
        terminal.text()
    );
    // What was pasted never reached the ruling.
    let writes = writes_on(&project, 7);
    assert!(
        writes
            .iter()
            .any(|call| call.starts_with("issue comment 7 ") && call.contains("Title and author.")),
        "{writes:?}"
    );
    assert!(
        writes.iter().all(|call| !call.contains("line one")),
        "{writes:?}"
    );
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
}

#[test]
fn one_line_paste_is_taken_as_typed() {
    let mut project = project(0);
    open_questions(&mut project, &[(7, QUESTION)]);
    let mut terminal = Terminal::start(&project, &watch_args(false));
    assert!(terminal.shows(Duration::from_secs(20), "Your answer:"));

    terminal.type_raw(b"\x1b[200~one line \x1b[201~");
    terminal.type_raw(b"\n");

    assert!(
        terminal.shows(Duration::from_secs(20), "✓ recorded; #7 goes on."),
        "{:?}",
        terminal.text()
    );
    let writes = writes_on(&project, 7);
    assert!(
        writes
            .iter()
            .any(|call| call.starts_with("issue comment 7 ") && call.contains("one line")),
        "{writes:?}"
    );
    // The markers are not part of the ruling.
    assert!(
        writes
            .iter()
            .all(|call| !call.contains("[200~") && !call.contains("[201~")),
        "{writes:?}"
    );
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
}

#[test]
fn bracketed_paste_is_off_after_asking() {
    // The last on and the last off the terminal was sent, by where they stand in what it drew.
    let last = |terminal: &Terminal, code: &str| terminal.text().rfind(code);
    for way in ["answer", "skip", "ctrl-c"] {
        let mut project = project(0);
        open_questions(&mut project, &[(7, QUESTION)]);
        let mut terminal = Terminal::start(&project, &watch_args(false));
        assert!(terminal.shows(Duration::from_secs(20), "Your answer:"));

        // On before the question is asked, and not off while it is.
        let drawn = terminal.text();
        let on = drawn
            .find(PASTE_ON)
            .unwrap_or_else(|| panic!("{way}: {drawn:?}"));
        assert!(
            on < drawn.find("#7 asks:").expect("asked"),
            "{way}: {drawn:?}"
        );
        assert!(!drawn.contains(PASTE_OFF), "{way}: {drawn:?}");

        match way {
            "answer" => {
                terminal.type_line("Title and author.");
                assert!(terminal.shows(Duration::from_secs(20), "✓ recorded; #7 goes on."));
            }
            "skip" => {
                terminal.type_line("");
                assert!(terminal.shows(Duration::from_secs(20), "skipped; #7 stays open"));
            }
            _ => {
                terminal.interrupt();
                assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
            }
        }
        let on = last(&terminal, PASTE_ON).expect("on");
        let off = last(&terminal, PASTE_OFF).unwrap_or_else(|| panic!("{way}: never off"));
        assert!(on < off, "{way}: {:?}", terminal.text());
        if way != "ctrl-c" {
            terminal.interrupt();
            assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
        }
    }
}

#[test]
fn restart_waits_while_asking() {
    let mut project = project(0);
    open_questions(&mut project, &[(7, QUESTION)]);
    let bin = project.temp.path().join("bin");
    fs::create_dir_all(&bin).expect("bin");
    let install = || {
        let staged = bin.join("zaf.new");
        fs::copy(env!("CARGO_BIN_EXE_zaf"), &staged).expect("copy the binary");
        fs::rename(&staged, bin.join("zaf")).expect("into place");
    };
    install();
    let mut terminal = Terminal::start_command(project.command_of(
        &bin.join("zaf"),
        &project.root,
        &watch_args(false),
    ));
    assert!(terminal.shows(Duration::from_secs(20), "Your answer:"));

    // A new binary, seen at two looks and answering `--version`: the restart would be due.
    install();
    std::thread::sleep(Duration::from_secs(5));
    assert!(
        !log_of(&project).contains("restarting as"),
        "restarted while a question was asked: {}",
        log_of(&project)
    );

    terminal.type_line("Title and author.");
    assert!(
        wait_until(Duration::from_secs(30), || log_of(&project)
            .contains("restarting as zaf ")),
        "no restart once answered: {}",
        log_of(&project)
    );
    assert!(
        wait_until(Duration::from_secs(30), || log_of(&project)
            .contains("scheduler restarted from zaf ")),
        "{}",
        log_of(&project)
    );
    terminal.interrupt();
    assert_eq!(terminal.exit(Duration::from_secs(20)), Some(0));
}

/// The board of a built version: its one epic, signed and challenged, and its one task done.
fn built_board(project: &mut Project) {
    use zaf::domain::{BodyHash, TicketBody};
    let body = "Release: 0.3.0\n\nEpic 1.";
    let hash = BodyHash::of(&TicketBody::from(body));
    let epics = serde_json::json!([{
        "number": 1,
        "title": "Epic 1",
        "body": body,
        "labels": [{"name": "epic"}],
        "comments": [
            {
                "body": format!(
                    "<!-- zaf:signature sha256={} at=2026-11-01T00:00:00Z -->\nSigned.",
                    hash.as_str()
                ),
                "author": {"login": common::OWN_LOGIN},
            },
            {
                "body": "<!-- zaf:report challenger instance 9 on #1 -->\nEvery task is exact.",
                "author": {"login": common::OWN_LOGIN},
            },
        ],
    }]);
    let tasks = serde_json::json!([{
        "number": 2, "title": "t", "labels": [{"name": "task"}],
        "parent": {"number": 1}, "state": "CLOSED", "stateReason": "COMPLETED",
    }]);
    project
        .on_first("issue list --label task *", &tasks.to_string())
        .on_first("issue list --label epic *", &epics.to_string());
}

/// Gives the project a `release-open` action that prints pull request 12.
fn release_open_action(project: &Project) {
    let file = project.root.join(".zaf/actions/release-open");
    fs::write(
        &file,
        "#!/bin/sh\n# zaf description: open the release pull request\n# zaf 1: refused\n\
         echo 12\necho https://forge.example/example/tracker/pulls/12\n",
    )
    .expect("action");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
}

/// A project whose version is built as soon as the first tick has read the board.
fn built_project() -> Project {
    let mut project = project(0);
    built_board(&mut project);
    release_open_action(&project);
    project
}

#[test]
fn built_version_ends_view_with_release_pr() {
    let project = built_project();
    let mut terminal = Terminal::start(&project, &watch_args(false));

    assert_eq!(terminal.exit(Duration::from_secs(30)), Some(0));
    let text = plain(&terminal.text());
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .collect();
    let end = &lines[lines.len() - 3..];

    assert_eq!(
        end[0], "✓ zaf built 0.3.0: 1 ticket merged in 0 min.",
        "{text:?}"
    );
    assert_eq!(
        end[1], "Next: merge pull request #12 on the forge to ship 0.3.0.",
        "{text:?}"
    );
    assert_eq!(
        end[2], "Then start the next release: zaf begin --version 0.4.0",
        "{text:?}"
    );
    // The last frame stays above the end screen, and the scheduler's own line is the log's.
    assert!(text.contains("zaf is building 0.3.0"), "{text:?}");
    assert!(
        !text.contains("zaf built 0.3.0: release pull request"),
        "{text:?}"
    );
    assert!(log_of(&project).contains("zaf built 0.3.0: release pull request #12"));
}

#[test]
fn end_restores_terminal() {
    let project = built_project();
    let mut terminal = Terminal::start(&project, &watch_args(false));

    assert_eq!(terminal.exit(Duration::from_secs(30)), Some(0));
    let after = terminal.settings();

    assert!(
        same_settings(&terminal.before, &after),
        "the terminal's settings changed: {} {} {} {} before, {} {} {} {} after",
        terminal.before.c_iflag,
        terminal.before.c_oflag,
        terminal.before.c_cflag,
        terminal.before.c_lflag,
        after.c_iflag,
        after.c_oflag,
        after.c_cflag,
        after.c_lflag
    );
}

#[test]
fn piped_watch_does_not_end() {
    let project = built_project();
    let mut child = project
        .command_in(&project.root, &watch_args(false))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start zaf");
    let mut stdout = child.stdout.take().expect("stdout");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        while let Ok(read) = stdout.read(&mut buffer) {
            if read == 0 {
                break;
            }
            sink.lock()
                .expect("sink")
                .extend_from_slice(&buffer[..read]);
        }
    });
    let text = || String::from_utf8_lossy(&seen.lock().expect("seen")).into_owned();
    assert!(wait_until(Duration::from_secs(30), || text()
        .contains("zaf built 0.3.0: release pull request #12")));
    assert!(wait_until(Duration::from_secs(10), || text().contains("next tick ")));
    std::thread::sleep(Duration::from_secs(1));
    let ended = child.try_wait().expect("try_wait");
    let _ = child.kill();
    let _ = child.wait();
    let text = text();

    assert!(ended.is_none(), "the piped watch ended: {text}");
    assert!(!text.contains("Next:"), "{text}");
    assert!(!text.contains("zaf begin --version"), "{text}");
}
