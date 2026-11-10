//! The process table: whether a recorded [`Holder`] still runs.
//!
//! A holder is a pid plus the time the OS says that process started. On Linux the
//! start is field 22 of `/proc/<pid>/stat` (clock ticks since boot); elsewhere it is
//! `ps -o lstart=` under the C locale and UTC, so every reader formats it alike. A pid
//! the OS has handed to another process reads with another start, so it is not alive.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::ports::Holder;

/// The holder for a running process, or none when there is no such process.
pub fn holder_of(pid: u32) -> Option<Holder> {
    start_of(pid).map(|start| Holder { pid, start })
}

/// This process as a holder.
pub fn this() -> Option<Holder> {
    holder_of(std::process::id())
}

/// Whether the recorded process still runs: the pid exists and started when recorded.
pub fn alive(holder: &Holder) -> bool {
    start_of(holder.pid).is_some_and(|start| start == holder.start)
}

/// When the process started, as the OS reports it; none when there is no such process.
fn start_of(pid: u32) -> Option<String> {
    if Path::new("/proc/self/stat").exists() {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        return proc_start(&stat);
    }
    let output = Command::new("ps")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let start = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    (output.status.success() && !start.is_empty()).then_some(start)
}

/// Field 22 of a `/proc/<pid>/stat` line, counted after the command name, which may hold
/// spaces and parentheses and ends at the line's last `)`. A zombie (state `Z`) has exited,
/// so it has no start.
fn proc_start(stat: &str) -> Option<String> {
    let (_, after) = stat.rsplit_once(')')?;
    // After the name: field 3 (state) is the first, so field 22 is the twentieth.
    let fields: Vec<&str> = after.split_whitespace().collect();
    if fields.first() == Some(&"Z") {
        return None;
    }
    fields.get(19).map(|start| (*start).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn this_process_is_alive() {
        let holder = this().expect("this process has a start time");
        assert_eq!(holder.pid, std::process::id());
        assert!(alive(&holder));
    }

    #[test]
    fn the_same_pid_with_another_start_is_not_alive() {
        let mut holder = this().expect("this process has a start time");
        holder.start.push('0');
        assert!(!alive(&holder));
    }

    #[test]
    fn a_process_that_exited_is_not_alive() {
        let mut child = Command::new("true").spawn().expect("runs true");
        let holder = holder_of(child.id()).unwrap_or(Holder {
            pid: child.id(),
            start: "0".to_owned(),
        });
        child.wait().expect("waits");
        assert!(!alive(&holder));
    }

    #[test]
    fn the_start_is_read_past_a_name_holding_spaces_and_parentheses() {
        let stat =
            "4242 (a (b) c) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 10";
        assert_eq!(proc_start(stat).as_deref(), Some("987654"));
    }

    #[test]
    fn a_zombie_has_no_start() {
        let stat = "4242 (claude) Z 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 0 0";
        assert_eq!(proc_start(stat), None);
    }
}
