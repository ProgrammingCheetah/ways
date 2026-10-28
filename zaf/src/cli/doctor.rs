//! `zaf doctor`: validate the manifest and verify the required tools.

use std::io;

use super::Cli;
use super::exit::{self, OK, REFUSED};

use crate::app::doctor::{CommandOutput, Commands, doctor};
use crate::composition::Composition;
use crate::manifest::CommandLine;
use crate::ports::{GateVerdict, Shell};

/// The shell exit codes for a program that is not there or cannot be run.
const NOT_FOUND: [&str; 2] = ["126", "127"];

/// [`Commands`] over the [`Shell`] port: a program and its arguments become one quoted line.
///
/// A program the shell cannot find is an `io::Error`, as a process that cannot start is.
pub struct ShellCommands {
    shell: Box<dyn Shell>,
}

impl ShellCommands {
    /// Runs commands through `shell`.
    ///
    /// An inherent constructor: it wraps a boxed port, which no std conversion names well.
    pub fn new(shell: Box<dyn Shell>) -> Self {
        Self { shell }
    }
}

impl Commands for ShellCommands {
    /// Output is stdout and stderr together, because some tools print their version on stderr.
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CommandOutput> {
        let words: Vec<String> = std::iter::once(program)
            .chain(args.iter().copied())
            .map(quote)
            .collect();
        // The status line is how a missing program shows through `sh -c`; it never reaches the caller.
        let line = CommandLine::from(format!(
            "{}; status=$?; echo \"zaf-status:$status\"; exit $status",
            words.join(" ")
        ));
        let run = self
            .shell
            .run(&line)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let (text, status) = run
            .output
            .rsplit_once("zaf-status:")
            .unwrap_or((run.output.as_str(), ""));
        if NOT_FOUND.contains(&status.trim()) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("{program}: not installed"),
            ));
        }
        Ok(CommandOutput {
            text: text.to_owned(),
            success: run.verdict == GateVerdict::Passed,
        })
    }
}

/// Wraps text in single quotes for a POSIX shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// Loads the manifest (an error is printed and exits 1), checks the tools and folders,
/// prints one line per check, or the table with `--json`; exits 1 when anything fails.
pub fn run(cli: &Cli) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => {
            eprintln!("zaf {path}: {error}");
            return REFUSED;
        }
    };
    let commands = ShellCommands::new(composition.shell());
    let report = doctor(composition.root(), composition.manifest(), &commands);
    if cli.json {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(error) => {
                eprintln!("zaf {path}: {error}");
                return exit::BACKEND;
            }
        }
    } else {
        for line in report.lines() {
            println!("{line}");
        }
    }
    if report.passed() { OK } else { REFUSED }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::adapters::shell::SystemShell;

    fn commands() -> ShellCommands {
        let dir = std::env::temp_dir();
        ShellCommands::new(Box::new(SystemShell::new(&dir, Duration::from_secs(30))))
    }

    #[test]
    fn a_program_and_its_arguments_run_quoted_and_the_output_comes_back() {
        let output = commands()
            .run("echo", &["a b", "it's"])
            .expect("echo is installed");
        assert_eq!(output.text, "a b it's\n");
        assert!(output.success);
    }

    #[test]
    fn a_failing_program_is_a_finished_run_with_its_output() {
        let output = commands()
            .run("sh", &["-c", "echo oops >&2; exit 3"])
            .expect("sh is installed");
        assert_eq!(output.text, "oops\n");
        assert!(!output.success);
    }

    #[test]
    fn a_program_that_is_not_installed_is_not_found() {
        let error = commands()
            .run("zaf-no-such-program", &["--version"])
            .expect_err("not installed");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }
}
