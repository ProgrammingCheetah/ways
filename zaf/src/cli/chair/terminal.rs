//! The real terminal: raw mode, the alternate screen, mouse reports, bracketed paste and a hidden
//! cursor while the chair runs, and all of it put back when the chair closes, returns early or
//! panics. Bracketed paste makes a paste one event of text, never a run of keys: a newline in it
//! is no Enter.
//!
//! While the chair runs, stderr goes to a log file: a line a child process or a library writes
//! there would land on the drawn screen. It comes back before the terminal is handed back, so a
//! panic's message is printed where the Engineer sees it. Only a panic on the chair's own thread
//! hands the terminal back: one on another thread (the writer, a gatherer) goes to the log, and
//! the loop says on the status line what stopped ([`deaths`]) while it goes on drawing.

use std::fs::{File, OpenOptions};
use std::io::{self, Stdout};
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::thread::ThreadId;

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// The terminal ratatui draws on.
pub type Screen = Terminal<CrosstermBackend<Stdout>>;

/// Whether the terminal is in the chair's modes.
static ENTERED: AtomicBool = AtomicBool::new(false);
/// The real stderr while it goes to the log; -1 when it does not.
static SAVED_STDERR: AtomicI32 = AtomicI32::new(-1);
/// The thread that took the terminal: a panic there hands it back, one elsewhere does not.
static MAIN: OnceLock<ThreadId> = OnceLock::new();
/// What the other threads said as they panicked, for the loop to say.
static DEATHS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The signal that asked the chair to close (SIGTERM, SIGINT, SIGHUP); 0 while none has.
static SIGNALLED: AtomicI32 = AtomicI32::new(0);
/// How the process took those signals before the chair caught them, to give back.
static BEFORE: Mutex<Vec<(libc::c_int, libc::sigaction)>> = Mutex::new(Vec::new());

/// The signals that close the chair as `q` does, the terminal handed back first.
const CLOSING: [libc::c_int; 3] = [libc::SIGTERM, libc::SIGINT, libc::SIGHUP];

/// The terminal taken over; dropping it hands it back.
pub struct Session {
    /// What ratatui draws on.
    pub screen: Screen,
}

impl Session {
    /// Takes the terminal over, stderr going to `log` meanwhile.
    pub fn enter(log: &Path) -> io::Result<Self> {
        let _ = MAIN.set(std::thread::current().id());
        install_panic_hook();
        catch_closing_signals();
        redirect_stderr(log);
        let entered = (|| {
            enable_raw_mode()?;
            ENTERED.store(true, Ordering::SeqCst);
            // Cleared too: a terminal without an alternate screen would show what was there
            // through the cells the first frame leaves blank.
            execute!(
                io::stdout(),
                EnterAlternateScreen,
                Clear(ClearType::All),
                EnableMouseCapture,
                EnableBracketedPaste,
                Hide
            )?;
            Terminal::new(CrosstermBackend::new(io::stdout()))
        })();
        match entered {
            Ok(screen) => Ok(Self { screen }),
            Err(error) => {
                restore();
                Err(error)
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        restore();
    }
}

/// Hands the terminal back as it was: the main screen, cooked mode, the cursor shown, no mouse
/// reports, no bracketed paste, stderr its own. Safe to call twice.
pub fn restore() {
    if ENTERED.swap(false, Ordering::SeqCst) {
        let _ = execute!(
            io::stdout(),
            DisableMouseCapture,
            DisableBracketedPaste,
            LeaveAlternateScreen,
            Show
        );
        let _ = disable_raw_mode();
    }
    restore_stderr();
    release_closing_signals();
}

/// What a closing signal runs while the chair holds the terminal: it notes which arrived and does
/// nothing else, the one thing a handler may safely do; the loop sees it ([`signalled`]).
extern "C" fn note_signal(signal: libc::c_int) {
    SIGNALLED.store(signal, Ordering::SeqCst);
}

/// From now on SIGTERM, SIGINT and SIGHUP close the chair as `q` does, the terminal handed back
/// before zaf ends, instead of ending zaf with the terminal in raw mode on the alternate screen.
/// The first only: the handler is reset as it runs (`SA_RESETHAND`), so a second ends zaf as it
/// would have. A signal the process was started ignoring (a SIGHUP under `nohup`) stays ignored.
fn catch_closing_signals() {
    let Ok(mut before) = BEFORE.lock() else {
        return;
    };
    if !before.is_empty() {
        return;
    }
    for signal in CLOSING {
        // SAFETY: an all-zero `sigaction` is a valid value of that plain-data type; the handler
        // only stores into an atomic, which is async-signal-safe; sigaction reads and writes only
        // the two values given, which live for the call.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = note_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
            libc::sigemptyset(&raw mut action.sa_mask);
            // Once only: a second signal while the chair closes slowly ends zaf at once.
            action.sa_flags = libc::SA_RESTART | libc::SA_RESETHAND;
            let mut was: libc::sigaction = std::mem::zeroed();
            if libc::sigaction(signal, &raw const action, &raw mut was) != 0 {
                continue;
            }
            if was.sa_sigaction == libc::SIG_IGN {
                libc::sigaction(signal, &raw const was, std::ptr::null_mut());
                continue;
            }
            before.push((signal, was));
        }
    }
}

/// Gives the closing signals back as they were before the chair caught them.
fn release_closing_signals() {
    let Ok(mut before) = BEFORE.lock() else {
        return;
    };
    for (signal, was) in before.drain(..) {
        // SAFETY: `was` is what sigaction gave for `signal`; it is put back as it was.
        unsafe { libc::sigaction(signal, &raw const was, std::ptr::null_mut()) };
    }
}

/// The closing signal that arrived while the chair held the terminal, when one has.
pub fn signalled() -> Option<libc::c_int> {
    match SIGNALLED.load(Ordering::SeqCst) {
        0 => None,
        signal => Some(signal),
    }
}

/// Puts the terminal back before a panic's message is printed, when the panic is the chair's own
/// thread's; another thread's panic leaves the screen to the chair, its message going to the log
/// and a line about it to [`deaths`].
fn install_panic_hook() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let main = MAIN
                .get()
                .is_none_or(|main| *main == std::thread::current().id());
            if hands_back(main, ENTERED.load(Ordering::SeqCst)) {
                restore();
                previous(info);
                return;
            }
            // Printed only where it lands in the log, never over the drawn screen.
            if SAVED_STDERR.load(Ordering::SeqCst) >= 0 {
                previous(info);
            }
            let name = std::thread::current()
                .name()
                .unwrap_or("a thread")
                .to_owned();
            if let Ok(mut deaths) = DEATHS.lock() {
                deaths.push(format!("{name} panicked: {}", said(info.payload())));
            }
        }));
    });
}

/// Whether a panic hands the terminal back: on the chair's own thread, or whenever the terminal
/// is not the chair's.
fn hands_back(main: bool, entered: bool) -> bool {
    main || !entered
}

/// What a panic said: its message, when it is text.
fn said(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|said| (*said).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_owned())
}

/// What the threads other than the chair's own said as they panicked since the last call, a line
/// each: `chair-write panicked: …`.
pub fn deaths() -> Vec<String> {
    DEATHS
        .lock()
        .map(|mut deaths| std::mem::take(&mut *deaths))
        .unwrap_or_default()
}

/// Sends stderr to `log`, keeping the real one to come back to; left as it is when the log
/// cannot be opened.
fn redirect_stderr(log: &Path) {
    if let Some(folder) = log.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let Ok(file) = OpenOptions::new().create(true).append(true).open(log) else {
        return;
    };
    if let Some(saved) = divert(2, &file) {
        SAVED_STDERR.store(saved, Ordering::SeqCst);
    }
}

/// Points `descriptor` at `file`, keeping a copy of what it was: the copy, none when either step
/// failed (and `descriptor` is then left alone). The copy is close-on-exec: it is the real
/// terminal, and no child (Claude in the phase pane, the scheduler `b` starts, a fix) may inherit
/// a handle that reads and writes it.
fn divert(descriptor: RawFd, file: &File) -> Option<RawFd> {
    // SAFETY: fcntl and dup2 take plain descriptors: `descriptor` is open, `file`'s is open for
    // the call.
    let saved = unsafe { libc::fcntl(descriptor, libc::F_DUPFD_CLOEXEC, 0) };
    if saved < 0 {
        return None;
    }
    // SAFETY: as above; on failure the copy is closed and `descriptor` is left alone.
    if unsafe { libc::dup2(file.as_raw_fd(), descriptor) } < 0 {
        // SAFETY: `saved` is the descriptor fcntl just returned.
        unsafe { libc::close(saved) };
        return None;
    }
    Some(saved)
}

/// Gives stderr back, when it was sent to the log.
fn restore_stderr() {
    let saved = SAVED_STDERR.swap(-1, Ordering::SeqCst);
    if saved >= 0 {
        // SAFETY: `saved` is the real stderr dup kept; it is put back on 2 and its copy closed.
        unsafe {
            libc::dup2(saved, 2);
            libc::close(saved);
        }
    }
}

#[cfg(test)]
mod tests;
