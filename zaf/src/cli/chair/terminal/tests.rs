use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::process::{Command, Stdio};

use portable_pty::{PtySize, native_pty_system};

use super::{catch_closing_signals, divert, hands_back, release_closing_signals, said, signalled};

#[cfg(target_os = "linux")]
#[test]
fn a_child_started_while_stderr_goes_to_the_log_holds_no_copy_of_the_terminal() {
    let pair = native_pty_system()
        .openpty(PtySize::default())
        .expect("a pseudo-terminal");
    let name = pair.master.tty_name().expect("its name");
    // The terminal the chair takes, as its stderr would be.
    let terminal = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOCTTY)
        .open(&name)
        .expect("its side opens");
    let folder = tempfile::tempdir().expect("a folder");
    let log = File::create(folder.path().join("chair.log")).expect("a log");

    let saved = divert(terminal.as_raw_fd(), &log).expect("diverted");
    let output = Command::new("/bin/sh")
        .args(["-c", "ls -l /proc/$$/fd"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .expect("runs");
    // SAFETY: `saved` is the copy divert returned, closed once.
    unsafe { libc::close(saved) };

    let listing = String::from_utf8_lossy(&output.stdout);
    assert!(
        listing.contains(" 0 -> "),
        "the listing is there: {listing}"
    );
    let tty = name.display().to_string();
    let held: Vec<&str> = listing
        .lines()
        .filter(|line| line.ends_with(&format!("-> {tty}")))
        .collect();
    assert!(
        held.is_empty(),
        "the child holds the terminal beyond 0, 1 and 2 (the copy was {saved}): {held:?}"
    );
}

#[test]
fn only_a_panic_on_the_chairs_own_thread_hands_the_terminal_back() {
    assert!(hands_back(true, true));
    assert!(
        !hands_back(false, true),
        "the writer's panic leaves the screen drawn"
    );
    assert!(hands_back(false, false), "no screen to keep");
}

#[test]
fn a_panic_says_its_message() {
    let text: Box<dyn std::any::Any + Send> = Box::new("index out of bounds");
    assert_eq!(said(text.as_ref()), "index out of bounds");
    let owned: Box<dyn std::any::Any + Send> = Box::new(String::from("gone"));
    assert_eq!(said(owned.as_ref()), "gone");
    let other: Box<dyn std::any::Any + Send> = Box::new(7_u8);
    assert_eq!(said(other.as_ref()), "no message");
}

#[test]
fn a_closing_signal_is_noted_for_the_loop_and_given_back_after() {
    catch_closing_signals();
    // SAFETY: raise sends a signal to this process; SIGHUP is caught by the handler just set,
    // which only stores into an atomic.
    unsafe { libc::raise(libc::SIGHUP) };
    let noted = signalled();
    // SAFETY: an all-zero sigaction is a valid value; sigaction with no new action only reads.
    let after_one = unsafe {
        let mut now: libc::sigaction = std::mem::zeroed();
        libc::sigaction(libc::SIGHUP, std::ptr::null(), &raw mut now);
        now.sa_sigaction
    };
    release_closing_signals();
    assert_eq!(
        after_one,
        libc::SIG_DFL,
        "caught once: a second signal ends zaf during a slow close"
    );

    assert_eq!(
        noted,
        Some(libc::SIGHUP),
        "the process lives on to hand the terminal back"
    );
    // SAFETY: an all-zero sigaction is a valid value; sigaction with no new action only reads.
    let handler = unsafe {
        let mut now: libc::sigaction = std::mem::zeroed();
        libc::sigaction(libc::SIGHUP, std::ptr::null(), &raw mut now);
        now.sa_sigaction
    };
    assert_ne!(
        handler,
        super::note_signal as extern "C" fn(libc::c_int) as libc::sighandler_t,
        "given back"
    );
}
