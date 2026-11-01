//! A faked [`Commands`]: canned output per program, and a record of every call.

use std::collections::BTreeMap;
use std::io;
use std::sync::{Mutex, PoisonError};

use super::{CommandOutput, Commands};

/// Programs that answer with fixed text; any other program is not installed.
#[derive(Debug, Default)]
pub struct FakeCommands {
    answers: BTreeMap<String, CommandOutput>,
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeCommands {
    /// A fake with nothing installed.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs a program that answers every call with `text` and success.
    pub fn with(mut self, program: &str, text: &str) -> Self {
        self.answers.insert(
            program.to_owned(),
            CommandOutput {
                text: text.to_owned(),
                success: true,
            },
        );
        self
    }

    /// Installs a program whose every call fails with `text`.
    pub fn failing(mut self, program: &str, text: &str) -> Self {
        self.answers.insert(
            program.to_owned(),
            CommandOutput {
                text: text.to_owned(),
                success: false,
            },
        );
        self
    }

    /// Every call so far, program first.
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Commands for FakeCommands {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CommandOutput> {
        let mut call = vec![program.to_owned()];
        call.extend(args.iter().map(|arg| (*arg).to_owned()));
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(call);
        self.answers.get(program).cloned().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("{program}: not installed"))
        })
    }
}
