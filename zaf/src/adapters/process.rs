//! The process table: whether a recorded [`Holder`] still runs.
//!
//! A holder is a pid plus the time the OS says that process started. On Linux the
//! start is field 22 of `/proc/<pid>/stat` (clock ticks since boot); elsewhere it is
//! `ps -o lstart=` under the C locale and UTC, so every reader formats it alike. A pid
//! the OS has handed to another process reads with another start, so it is not alive.
//!
//! The machine read ([`machine`]) finds every live zaf instance on the machine, from any
//! project, by the [`RUN_VAR`] in each process's environment: `/proc/<pid>/environ` on
//! Linux, else `ps` with the environment. It keeps [`RUN_VAR`], [`SIZE_VAR`] and
//! [`INSTANCE_VAR`] and drops every other variable as it reads, since an instance's
//! environment holds tokens; nothing it finds is written anywhere.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::domain::TaskSize;
use crate::ports::{Holder, INSTANCE_VAR, MachineInstance, RUN_VAR, SIZE_VAR};

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

/// Every live zaf instance on the machine, from any project, each once: the live processes
/// whose environment carries [`RUN_VAR`], grouped by it, ordered by run folder.
///
/// A process whose environment cannot be read (another user's, or one that exits during
/// the read) is skipped. Each call reads the process table afresh; nothing is kept.
pub fn machine() -> Vec<MachineInstance> {
    group(environments())
}

/// The three variables of one process's environment the machine read keeps; every other
/// variable is dropped as it is read, so none can leave this module.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Kept {
    run: Option<String>,
    size: Option<String>,
    instance: Option<String>,
}

impl Kept {
    /// Keeps a `NAME=value` entry when its name is one of the three, ignoring it otherwise.
    fn take(&mut self, entry: &[u8]) {
        let Some(split) = entry.iter().position(|byte| *byte == b'=') else {
            return;
        };
        let (name, value) = (&entry[..split], &entry[split + 1..]);
        let slot = if name == RUN_VAR.as_bytes() {
            &mut self.run
        } else if name == SIZE_VAR.as_bytes() {
            &mut self.size
        } else if name == INSTANCE_VAR.as_bytes() {
            &mut self.instance
        } else {
            return;
        };
        let value = String::from_utf8_lossy(value).trim().to_owned();
        *slot = (!value.is_empty()).then_some(value);
    }
}

/// The kept variables of a `/proc/<pid>/environ` file: `NAME=value` entries, each ended by a NUL.
fn kept_of_environ(environ: &[u8]) -> Kept {
    let mut kept = Kept::default();
    for entry in environ.split(|byte| *byte == 0) {
        kept.take(entry);
    }
    kept
}

/// The pid and kept variables of one line of `ps -Aeww -o pid=,command=`: the pid, then the
/// command and its environment as words, the environment last, so a later entry wins. `ps`
/// joins entries with spaces, so a kept value runs on to the next `NAME=` word.
fn kept_of_ps_line(line: &[u8]) -> Option<(u32, Kept)> {
    let mut words = line
        .split(u8::is_ascii_whitespace)
        .filter(|word| !word.is_empty());
    let pid = std::str::from_utf8(words.next()?).ok()?.parse().ok()?;
    let mut kept = Kept::default();
    let mut open: Option<Vec<u8>> = None;
    for word in words {
        if let Some(name) = assigned_name(word) {
            if let Some(entry) = open.take() {
                kept.take(&entry);
            }
            // Only the kept names are gathered; any other entry is passed over.
            if [RUN_VAR, SIZE_VAR, INSTANCE_VAR].contains(&name) {
                open = Some(word.to_vec());
            }
        } else if let Some(entry) = open.as_mut() {
            entry.push(b' ');
            entry.extend_from_slice(word);
        }
    }
    if let Some(entry) = open {
        kept.take(&entry);
    }
    Some((pid, kept))
}

/// The name of a `NAME=value` word, a name being capitals, digits and `_`, not led by a digit.
fn assigned_name(word: &[u8]) -> Option<&str> {
    let split = word.iter().position(|byte| *byte == b'=')?;
    let name = &word[..split];
    let first = *name.first()?;
    let valid = !first.is_ascii_digit()
        && name
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_');
    valid.then(|| std::str::from_utf8(name).ok()).flatten()
}

/// Each readable live process with its kept variables.
fn environments() -> Vec<(u32, Kept)> {
    if Path::new("/proc/self/environ").exists() {
        proc_environments()
    } else {
        ps_environments()
    }
}

/// Linux: `/proc/<pid>/environ` of every pid. A process that carries [`RUN_VAR`] but has no
/// start (a zombie, or one gone since) has exited and is left out.
fn proc_environments() -> Vec<(u32, Kept)> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let pid: u32 = entry.file_name().to_str()?.parse().ok()?;
            let kept = kept_of_environ(&std::fs::read(entry.path().join("environ")).ok()?);
            (kept.run.is_none() || start_of(pid).is_some()).then_some((pid, kept))
        })
        .collect()
}

/// Elsewhere: `ps` with the environment of every process it may show.
fn ps_environments() -> Vec<(u32, Kept)> {
    let Ok(output) = Command::new("ps")
        .args(["-Aeww", "-o", "pid=,command="])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter_map(kept_of_ps_line)
        .collect()
}

/// The processes carrying [`RUN_VAR`], one instance per run folder. The instance and size
/// come from its lowest pid that carries them; a size that does not parse is none.
fn group(mut processes: Vec<(u32, Kept)>) -> Vec<MachineInstance> {
    processes.sort_by_key(|(pid, _)| *pid);
    let mut instances: BTreeMap<PathBuf, MachineInstance> = BTreeMap::new();
    for (pid, kept) in processes {
        let Some(run) = kept.run.map(PathBuf::from) else {
            continue;
        };
        let instance = instances
            .entry(run.clone())
            .or_insert_with(|| MachineInstance {
                root: root_of(&run),
                run,
                instance: None,
                size: None,
                pids: Vec::new(),
            });
        instance.pids.push(pid);
        if instance.instance.is_none() {
            instance.instance = kept.instance;
        }
        if instance.size.is_none() {
            instance.size = kept.size.and_then(|size| size.parse::<TaskSize>().ok());
        }
    }
    instances.into_values().collect()
}

/// The project of a run folder `<root>/.zaf/runs/<slug>`: three folders up, or the run
/// folder itself when it is not that deep.
fn root_of(run: &Path) -> PathBuf {
    run.ancestors().nth(3).unwrap_or(run).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stub instance process: `sleep` carrying only the zaf variables given, killed when dropped.
    struct Stub(std::process::Child);

    impl Stub {
        fn start(env: &[(&str, &str)]) -> Self {
            let mut command = Command::new("sleep");
            command
                .arg("30")
                .env_remove(RUN_VAR)
                .env_remove(SIZE_VAR)
                .env_remove(INSTANCE_VAR)
                .stdin(Stdio::null());
            for (name, value) in env {
                command.env(name, value);
            }
            Self(command.spawn().expect("runs sleep"))
        }

        fn pid(&self) -> u32 {
            self.0.id()
        }

        fn stop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Drop for Stub {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// A run folder of a project under `dir`.
    fn run_in(dir: &Path, project: &str, slug: &str) -> PathBuf {
        dir.join(project).join(".zaf/runs").join(slug)
    }

    /// The machine read's instances whose run folder is under `dir`, so other instances
    /// live on this machine do not count.
    fn read_under(dir: &Path) -> Vec<MachineInstance> {
        machine()
            .into_iter()
            .filter(|instance| instance.run.starts_with(dir))
            .collect()
    }

    #[test]
    fn an_instance_and_its_children_count_once() {
        let dir = tempfile::tempdir().expect("temp dir");
        let run = run_in(dir.path(), "bookshelf", "planning-instance-1-on-2");
        let env = [
            (RUN_VAR, run.to_str().expect("utf-8")),
            (SIZE_VAR, "XS"),
            (INSTANCE_VAR, "planning instance 1 on #2"),
        ];
        let instance = Stub::start(&env);
        let child = Stub::start(&env);

        let read = read_under(dir.path());

        let mut pids = vec![instance.pid(), child.pid()];
        pids.sort_unstable();
        assert_eq!(
            read,
            [MachineInstance {
                run: run.clone(),
                root: dir.path().join("bookshelf"),
                instance: Some("planning instance 1 on #2".to_owned()),
                size: Some(TaskSize::XS),
                pids,
            }]
        );
    }

    #[test]
    fn an_instance_of_another_project_is_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let here = run_in(dir.path(), "bookshelf", "team-instance-1-on-2");
        let there = run_in(dir.path(), "ledger", "planning-instance-4-on-9");
        let _here = Stub::start(&[(RUN_VAR, here.to_str().expect("utf-8"))]);
        let _there = Stub::start(&[(RUN_VAR, there.to_str().expect("utf-8")), (SIZE_VAR, "XS")]);

        let read = read_under(dir.path());

        let roots: Vec<&Path> = read
            .iter()
            .map(|instance| instance.root.as_path())
            .collect();
        assert_eq!(
            roots,
            [dir.path().join("bookshelf"), dir.path().join("ledger")]
        );
        assert_eq!(read[0].run, here);
        assert_eq!(read[1].run, there);
        assert_eq!(read[1].size, Some(TaskSize::XS));
    }

    #[test]
    fn an_exited_instance_leaves_the_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let run = run_in(dir.path(), "bookshelf", "challenger-instance-3-on-2");
        let mut stub = Stub::start(&[(RUN_VAR, run.to_str().expect("utf-8"))]);
        assert_eq!(read_under(dir.path()).len(), 1);

        stub.stop();

        assert!(
            read_under(dir.path()).is_empty(),
            "the next read no longer sees it, with nothing to clean up"
        );
    }

    #[test]
    fn an_instance_without_a_size_reads_none() {
        let dir = tempfile::tempdir().expect("temp dir");
        let sizeless = run_in(dir.path(), "bookshelf", "team-instance-1-on-2");
        let garbled = run_in(dir.path(), "bookshelf", "team-instance-2-on-3");
        let _sizeless = Stub::start(&[(RUN_VAR, sizeless.to_str().expect("utf-8"))]);
        let _garbled = Stub::start(&[
            (RUN_VAR, garbled.to_str().expect("utf-8")),
            (SIZE_VAR, "huge"),
        ]);

        let read = read_under(dir.path());

        assert_eq!(read.len(), 2);
        assert!(
            read.iter().all(|instance| instance.size.is_none()),
            "{read:?}"
        );
    }

    #[test]
    fn the_read_keeps_no_other_variable() {
        let dir = tempfile::tempdir().expect("temp dir");
        let run = run_in(dir.path(), "bookshelf", "team-instance-1-on-2");
        let _stub = Stub::start(&[
            (RUN_VAR, run.to_str().expect("utf-8")),
            ("GH_TOKEN", "ghp_not_a_real_token_413"),
        ]);

        let read = format!("{:?}", machine());

        assert!(read.contains(run.to_str().expect("utf-8")), "{read}");
        assert!(!read.contains("ghp_not_a_real_token_413"), "{read}");
        assert!(!read.contains("GH_TOKEN"), "{read}");
    }

    #[test]
    fn an_environment_file_yields_only_the_three_variables() {
        let environ = b"GH_TOKEN=ghp_secret\0ZAF_RUN=/p/.zaf/runs/x\0HOME=/home/u\0ZAF_SIZE=S\0\
                        ZAF_INSTANCE=team instance 1 on #2\0ZAF_VERSION=0.3.0\0";
        assert_eq!(
            kept_of_environ(environ),
            Kept {
                run: Some("/p/.zaf/runs/x".to_owned()),
                size: Some("S".to_owned()),
                instance: Some("team instance 1 on #2".to_owned()),
            }
        );
    }

    #[test]
    fn a_ps_line_yields_its_pid_and_only_the_three_variables() {
        let line = b"  4242 claude -p hello GH_TOKEN=ghp_secret ZAF_RUN=/p/.zaf/runs/x \
                     ZAF_INSTANCE=team instance 1 on #2 HOME=/home/u ZAF_SIZE=XS";
        assert_eq!(
            kept_of_ps_line(line),
            Some((
                4242,
                Kept {
                    run: Some("/p/.zaf/runs/x".to_owned()),
                    size: Some("XS".to_owned()),
                    instance: Some("team instance 1 on #2".to_owned()),
                }
            ))
        );
        assert_eq!(kept_of_ps_line(b"   "), None);
    }

    #[test]
    fn a_process_without_a_run_is_no_instance() {
        let kept = Kept {
            instance: Some("team instance 1 on #2".to_owned()),
            ..Kept::default()
        };
        assert!(group(vec![(7, kept)]).is_empty());
    }

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
