//! The terminal's step report: a step's line spins while it runs and ends with its mark.
//!
//! On a live screen the line is redrawn in place every 80 ms by a small thread, then replaced
//! by `✓` or `✗` and the end's text. On any other screen only the final line is printed, with
//! no `\r`, and no escape byte when colour is off.

use std::io::Write;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::app::steps::StepReport;
use crate::cli::screen::Screen;
use crate::domain::screen::{Meaning, PlanRow, StepEnd};

/// The spinner's frames, from the mocks.
const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The time between two frames.
const TICK: Duration = Duration::from_millis(80);

/// Back to the line's start and clear it.
const REDRAW: &str = "\r\x1b[K";

/// A step's spinner thread: it stops when sent to or when its sender is dropped.
struct Spinner {
    stop: Sender<()>,
    thread: JoinHandle<()>,
}

/// A [`StepReport`] that draws on a writer, the terminal's stdout in the CLI.
pub struct ScreenSteps<W: Write + Send + 'static> {
    screen: Screen,
    rows: Vec<PlanRow>,
    out: Arc<Mutex<W>>,
    spinner: Option<Spinner>,
}

impl ScreenSteps<std::io::Stdout> {
    /// A reporter for `rows` that draws on stdout.
    pub fn stdout(screen: Screen, rows: Vec<PlanRow>) -> Self {
        Self::new(screen, rows, std::io::stdout())
    }
}

/// The line of a step: its mark (or frame), its verb and its noun.
fn line(screen: Screen, mark: (&str, Meaning), verb: &str, noun: &str) -> String {
    format!(
        "  {} {} {}",
        screen.paint(mark.1, mark.0),
        screen.paint(Meaning::Verb, verb),
        screen.paint(Meaning::Noun, noun),
    )
}

impl<W: Write + Send + 'static> ScreenSteps<W> {
    /// A reporter for `rows` that draws on `out`.
    pub fn new(screen: Screen, rows: Vec<PlanRow>, out: W) -> Self {
        Self {
            screen,
            rows,
            out: Arc::new(Mutex::new(out)),
            spinner: None,
        }
    }

    /// Stop the spinner, if one runs, and wait for its thread; whether one ran.
    fn stop_spinner(&mut self) -> bool {
        let Some(spinner) = self.spinner.take() else {
            return false;
        };
        let _ = spinner.stop.send(());
        let _ = spinner.thread.join();
        true
    }
}

impl<W: Write + Send + 'static> StepReport for ScreenSteps<W> {
    fn started(&mut self, row: usize) {
        self.stop_spinner();
        if !self.screen.live {
            return;
        }
        let Some(plan) = self.rows.get(row) else {
            return;
        };
        let screen = self.screen;
        let verb = plan.verb.to_string();
        let noun = plan.noun.to_string();
        let out = Arc::clone(&self.out);
        let (stop, wake) = mpsc::channel::<()>();
        let thread = thread::spawn(move || {
            let mut frame = 0;
            loop {
                let drawn = line(
                    screen,
                    (FRAMES[frame % FRAMES.len()], Meaning::Verb),
                    &verb,
                    &noun,
                );
                if let Ok(mut out) = out.lock() {
                    let _ = write!(out, "{REDRAW}{drawn}");
                    let _ = out.flush();
                }
                frame += 1;
                if !matches!(wake.recv_timeout(TICK), Err(RecvTimeoutError::Timeout)) {
                    break;
                }
            }
        });
        self.spinner = Some(Spinner { stop, thread });
    }

    fn ended(&mut self, row: usize, end: &StepEnd) {
        self.stop_spinner();
        let Some(plan) = self.rows.get(row) else {
            return;
        };
        let (mark, text) = match end {
            StepEnd::Done(text) => (("✓", Meaning::Done), text),
            StepEnd::Failed(text) => (("✗", Meaning::Failed), text),
        };
        let drawn = line(self.screen, mark, plan.verb.as_str(), text.as_str());
        let lead = if self.screen.live { REDRAW } else { "" };
        if let Ok(mut out) = self.out.lock() {
            let _ = writeln!(out, "{lead}{drawn}");
            let _ = out.flush();
        }
    }
}

impl<W: Write + Send + 'static> Drop for ScreenSteps<W> {
    /// A step left running is stopped, its thread joined and its line cleared.
    fn drop(&mut self) {
        if self.stop_spinner()
            && let Ok(mut out) = self.out.lock()
        {
            let _ = write!(out, "{REDRAW}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests;
