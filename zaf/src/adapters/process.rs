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

/// The most parents [`ancestors`] walks; a deeper chain is cut there.
const MAX_ANCESTORS: usize = 64;

/// This process and every process above it, nearest first, each as a holder, up to the
/// first process (pid 1) or one the OS no longer reports.
pub fn ancestors() -> Vec<Holder> {
    let mut chain = Vec::new();
    let mut pid = std::process::id();
    while chain.len() < MAX_ANCESTORS {
        let Some(holder) = holder_of(pid) else {
            break;
        };
        chain.push(holder);
        match parent_of(pid) {
            Some(parent) if parent > 1 && parent != pid => pid = parent,
            _ => break,
        }
    }
    chain
}

/// The pid of a process's parent: field 4 of `/proc/<pid>/stat` on Linux, else
/// `ps -o ppid=`.
fn parent_of(pid: u32) -> Option<u32> {
    if Path::new("/proc/self/stat").exists() {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, after) = stat.rsplit_once(')')?;
        return after.split_whitespace().nth(1)?.parse().ok();
    }
    let output = Command::new("ps")
        .args(["-o", "ppid=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
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
    fn the_ancestors_start_with_this_process_and_hold_its_parent() {
        let chain = ancestors();
        assert_eq!(chain.first(), this().as_ref());
        let parent = parent_of(std::process::id()).expect("this process has a parent");
        if parent > 1 {
            assert_eq!(chain.get(1).map(|holder| holder.pid), Some(parent));
        }
        assert!(chain.iter().all(alive));
    }

    #[test]
    fn a_child_sees_this_process_above_it() {
        let me = this().expect("this process has a start time");
        let mut child = Command::new("sleep").arg("5").spawn().expect("runs sleep");
        assert_eq!(parent_of(child.id()), Some(me.pid));
        let _ = child.kill();
        let _ = child.wait();
    }

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
