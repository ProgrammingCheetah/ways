//! `zaf spawn stop <task>`: ends the instance running the task and releases its claim.
//!
//! Exit `0` once the instance is gone and the claim released, `1` when no instance runs the
//! task (nothing is changed), `2` when a run folder or the board cannot be read, the claim cannot
//! be released, or the instance would not end.

use std::path::Path;
use std::time::Duration;

use super::Cli;
use super::exit::{self, BACKEND, OK, REFUSED};
use crate::adapters::process::{self, Signal as Sent};
use crate::app::hub::controls::Failure;
use crate::app::spawn::stop::{self, GRACE, Processes, Signal, StopError};
use crate::composition::Composition;
use crate::domain::TicketId;
use crate::ports::Holder;

/// The process table and the signals of this machine.
pub struct Machine;

impl Processes for Machine {
    fn alive(&self, holder: &Holder) -> bool {
        process::alive(holder)
    }

    fn group_of(&self, holder: &Holder) -> Option<u32> {
        process::group_of(holder)
    }

    fn signal(&self, holder: &Holder, group: Option<u32>, signal: Signal) {
        process::signal(
            holder,
            group,
            match signal {
                Signal::Terminate => Sent::Terminate,
                Signal::Kill => Sent::Kill,
            },
        );
    }

    fn pause(&self, time: Duration) {
        std::thread::sleep(time);
    }
}

/// What `POST /api/stop` runs: the stop of `zaf spawn stop`, from the project at `root`, over this
/// machine's processes. A project whose manifest or board cannot be composed is a failure of a
/// backend, as for the verb.
pub fn stop_in(root: &Path, home: Option<&Path>, task: TicketId) -> Result<stop::Stopped, Failure> {
    let composition = Composition::discover_with_home(root, home)
        .map_err(|error| Failure::Failed(error.to_string()))?;
    let board = composition
        .board()
        .map_err(|error| Failure::Failed(error.to_string()))?;
    stop::stop(
        task,
        &composition.state_dir(),
        board.as_ref(),
        &Machine,
        GRACE,
    )
    .map_err(Failure::from)
}

/// Stops `task`'s instance and releases its claim.
pub fn run(cli: &Cli, task: TicketId) -> u8 {
    let path = cli.verb.path();
    let composition = match Composition::discover_here() {
        Ok(composition) => composition,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    let board = match composition.board() {
        Ok(board) => board,
        Err(error) => return exit::fail_composition(&path, &error),
    };
    match stop::stop(
        task,
        &composition.state_dir(),
        board.as_ref(),
        &Machine,
        GRACE,
    ) {
        Ok(stopped) => {
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({
                        "task": task.get(),
                        "stopped": stopped.runs.iter().map(|run| serde_json::json!({
                            "instance": run.instance,
                            "pid": run.pid,
                            "run": run.run_dir,
                            "killed": run.killed,
                        })).collect::<Vec<_>>(),
                        "released": stopped.released.iter().map(|released| {
                            released.instance.to_string()
                        }).collect::<Vec<_>>(),
                    })
                );
            } else {
                for run in &stopped.runs {
                    let how = if run.killed { ", killed" } else { "" };
                    println!("stopped {} (pid {}{how})", run.instance, run.pid);
                }
                if stopped.released.is_empty() {
                    println!("{task} held no claim");
                } else {
                    println!("released {task}");
                }
            }
            OK
        }
        Err(error @ StopError::NoInstance(_)) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            REFUSED
        }
        Err(StopError::Port(error)) => exit::fail(&path, &error),
        Err(error) => {
            crate::eprintln_zaf!("zaf {path}: {error}");
            BACKEND
        }
    }
}
