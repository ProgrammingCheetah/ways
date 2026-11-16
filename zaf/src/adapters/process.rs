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
//! environment holds tokens; nothing it finds is written anywhere. Each instance also reads
//! the resident memory of its processes, summed: `VmRSS` of `/proc/<pid>/status` on Linux,
//! else `ps -o rss=`; a process whose memory cannot be read adds nothing.
//!
//! The machine read counts every live process carrying [`RUN_VAR`], which is what the budget
//! charges: an orphaned grandchild of an ended run still uses memory, and one of size E still
//! holds the one E. What `zaf status` shows ([`displayed`]) leaves out a run whose folder says it
//! has ended (an `exit` recorded, or the process its `pid` file names gone), whatever still
//! carries its [`RUN_VAR`]. Neither holds a slot: the slots come from the phase run folders.
//!
//! The scheduler catches SIGINT and SIGTERM ([`catch_stop`]) so it can end between two ticks: a
//! signal is only recorded, and [`stop_signal`] reads it. Across its restart it holds both back
//! ([`hold_stop`]) and starts the new image with [`exec`], which keeps them held and pending, so a
//! stop that lands then is let through ([`let_stop_through`]) once the new image catches it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};

use crate::domain::{Mebibytes, TaskSize};
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
    ancestors_of(std::process::id())
}

/// Process `pid` and every process above it, as [`ancestors`] reads this one's. Empty when the
/// process is gone, which a caller that must not guess reads as unknown.
pub fn ancestors_of(mut pid: u32) -> Vec<Holder> {
    let mut chain = Vec::new();
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

/// The process and user at the other end of a Unix socket, as the kernel recorded them when it
/// connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    /// The connecting process.
    pub pid: u32,
    /// The user it runs as.
    pub uid: u32,
}

/// Who connected on `stream`: `SO_PEERCRED` on Linux. On a platform that has no such read, and
/// when the kernel does not say, none; a caller that needs to know who asks treats none as a refusal.
#[cfg(target_os = "linux")]
pub fn peer_of(stream: &std::os::unix::net::UnixStream) -> Option<PeerCredentials> {
    use std::os::fd::AsRawFd;
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut size = libc::socklen_t::try_from(std::mem::size_of::<libc::ucred>()).ok()?;
    // SAFETY: `cred` and `size` live for the call and `size` is the size of `cred`.
    let status = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            std::ptr::from_mut(&mut cred).cast(),
            &raw mut size,
        )
    };
    if status != 0 {
        return None;
    }
    Some(PeerCredentials {
        pid: u32::try_from(cred.pid).ok()?,
        uid: cred.uid,
    })
}

/// Who connected on `stream`, on macOS: the user from `getpeereid` (`LOCAL_PEERCRED` underneath)
/// and the process from `LOCAL_PEERPID`, an option at level `SOL_LOCAL`. None when the kernel does
/// not say, as on Linux.
#[cfg(target_os = "macos")]
pub fn peer_of(stream: &std::os::unix::net::UnixStream) -> Option<PeerCredentials> {
    use std::os::fd::AsRawFd;
    let fd = stream.as_raw_fd();
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: `uid` and `gid` live for the call, which only writes into them.
    if unsafe { libc::getpeereid(fd, &raw mut uid, &raw mut gid) } != 0 {
        return None;
    }
    let mut pid: libc::pid_t = 0;
    let mut size = libc::socklen_t::try_from(std::mem::size_of::<libc::pid_t>()).ok()?;
    // SAFETY: `pid` and `size` live for the call and `size` is the size of `pid`.
    let status = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            std::ptr::from_mut(&mut pid).cast(),
            &raw mut size,
        )
    };
    if status != 0 {
        return None;
    }
    Some(PeerCredentials {
        pid: u32::try_from(pid).ok().filter(|pid| *pid > 0)?,
        uid,
    })
}

/// Who connected on `stream`; this platform does not say.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn peer_of(_stream: &std::os::unix::net::UnixStream) -> Option<PeerCredentials> {
    None
}

/// The user this process runs as.
pub fn effective_uid() -> u32 {
    // SAFETY: `geteuid` takes no arguments and cannot fail.
    unsafe { libc::geteuid() }
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

/// A signal [`signal`] sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// `SIGTERM`: asks the process to end.
    Terminate,
    /// `SIGKILL`: ends it.
    Kill,
}

impl Signal {
    fn number(self) -> libc::c_int {
        match self {
            Self::Terminate => libc::SIGTERM,
            Self::Kill => libc::SIGKILL,
        }
    }
}

/// The stop signal that arrived since [`catch_stop`], by number; 0 while none has.
static STOP_SIGNAL: AtomicI32 = AtomicI32::new(0);

/// What SIGINT and SIGTERM run once [`catch_stop`] is in place: it records which one arrived and
/// does nothing else, the one thing a handler may safely do.
extern "C" fn record_stop(signal: libc::c_int) {
    STOP_SIGNAL.store(signal, Ordering::SeqCst);
}

/// From now on SIGINT and SIGTERM end nothing at once: each is recorded, for [`stop_signal`] to
/// read, and the process ends when it chooses to (the scheduler, between two ticks). A SIGINT this
/// process was started ignoring (a job put in the background by a shell) stays ignored. An `exec`
/// puts both back to the default, so a child, which runs a program of its own, never inherits the
/// handler. An error names the signal that could not be caught.
pub fn catch_stop() -> Result<(), String> {
    let handler = record_stop as extern "C" fn(libc::c_int) as libc::sighandler_t;
    for (signal, name) in [(libc::SIGINT, "SIGINT"), (libc::SIGTERM, "SIGTERM")] {
        // SAFETY: the handler only stores into an atomic, which is async-signal-safe, and touches
        // no other state; the call only changes how this process takes `signal`.
        let before = unsafe { libc::signal(signal, handler) };
        if before == libc::SIG_ERR {
            return Err(format!("cannot catch {name}"));
        }
        if signal == libc::SIGINT && before == libc::SIG_IGN {
            // SAFETY: as above; `SIG_IGN` runs no code at all.
            unsafe { libc::signal(signal, libc::SIG_IGN) };
        }
    }
    Ok(())
}

/// SIGINT and SIGTERM, the stop signals, as a set.
fn stop_signals() -> libc::sigset_t {
    // SAFETY: an all-zero `sigset_t` is a valid value of that plain-data type, and `sigemptyset`
    // and `sigaddset` only write into the set this function owns, with signals that exist.
    unsafe {
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&raw mut set);
        libc::sigaddset(&raw mut set, libc::SIGINT);
        libc::sigaddset(&raw mut set, libc::SIGTERM);
        set
    }
}

/// Changes this thread's mask of the stop signals: `how` is `SIG_BLOCK` or `SIG_UNBLOCK`.
fn mask_stop(how: libc::c_int) -> Result<(), String> {
    let set = stop_signals();
    // SAFETY: `pthread_sigmask` only reads `set`, which lives for the call, and changes the
    // calling thread's mask; the old mask is not asked for.
    let failed = unsafe { libc::pthread_sigmask(how, &raw const set, std::ptr::null_mut()) };
    if failed == 0 {
        Ok(())
    } else {
        Err(format!(
            "cannot change the mask of SIGINT and SIGTERM: {}",
            std::io::Error::from_raw_os_error(failed)
        ))
    }
}

/// From now on SIGINT and SIGTERM are held back: one that arrives is kept pending and changes
/// nothing until [`let_stop_through`]. An [`exec`] keeps both the mask and what is pending, so the
/// new image gets a stop that landed in between once it lets it through. The scheduler holds them
/// from before its last look for a stop to its restart; it runs one thread then, whose mask this
/// is.
pub fn hold_stop() -> Result<(), String> {
    mask_stop(libc::SIG_BLOCK)
}

/// Lets SIGINT and SIGTERM through again, after [`hold_stop`] here or before the `exec` that
/// started this image: one held back is delivered now, recorded once [`catch_stop`] is in place.
pub fn let_stop_through() -> Result<(), String> {
    mask_stop(libc::SIG_UNBLOCK)
}

/// Starts `program` in place of this process, with `args` after it (its own path is the first
/// argument), in the same environment: the same pid, so the same start. Unlike `std`'s `exec`,
/// which clears the signal mask first, it keeps the mask and the signals pending, so a stop held
/// back ([`hold_stop`]) reaches the new image instead of a handler the exec wipes. Returns only
/// when it failed, with why.
pub fn exec(program: &Path, args: &[std::ffi::OsString]) -> std::io::Error {
    use std::os::unix::ffi::OsStrExt;
    let nul = || std::io::Error::new(std::io::ErrorKind::InvalidInput, "an argument holds a NUL");
    let Ok(path) = std::ffi::CString::new(program.as_os_str().as_bytes()) else {
        return nul();
    };
    let mut argv = vec![path.clone()];
    for arg in args {
        let Ok(arg) = std::ffi::CString::new(arg.as_bytes()) else {
            return nul();
        };
        argv.push(arg);
    }
    let mut pointers: Vec<*const libc::c_char> = argv.iter().map(|arg| arg.as_ptr()).collect();
    pointers.push(std::ptr::null());
    // SAFETY: `path` and every pointer of `pointers` point into `CString`s, each ended by a NUL,
    // that live past the call, and `pointers` ends with the null pointer `execv` looks for. It
    // returns only when it failed, and changes nothing then.
    unsafe { libc::execv(path.as_ptr(), pointers.as_ptr()) };
    std::io::Error::last_os_error()
}

/// The stop signal that arrived since [`catch_stop`], by name (`SIGINT`, `SIGTERM`); none while
/// none has.
pub fn stop_signal() -> Option<&'static str> {
    match STOP_SIGNAL.load(Ordering::SeqCst) {
        0 => None,
        libc::SIGINT => Some("SIGINT"),
        libc::SIGTERM => Some("SIGTERM"),
        _ => Some("a signal"),
    }
}

/// The process group a live holder belongs to; none when it is not alive, or when the group is
/// this process's own (or init's), which no verb may signal as a whole.
pub fn group_of(holder: &Holder) -> Option<u32> {
    if !alive(holder) {
        return None;
    }
    let pid = libc::pid_t::try_from(holder.pid).ok()?;
    // SAFETY: `getpgid` and `getpgrp` take and return plain values and touch no memory.
    let group = unsafe { libc::getpgid(pid) };
    let own = unsafe { libc::getpgrp() };
    (group > 1 && group != own)
        .then(|| u32::try_from(group).ok())
        .flatten()
}

/// Sends `signal` to the holder, if it is still the process recorded (a pid the OS has since
/// handed to another is left alone), and to every process of `group`, read earlier with
/// [`group_of`]. A process or group already gone is not an error.
pub fn signal(holder: &Holder, group: Option<u32>, signal: Signal) {
    if let Some(group) = group.and_then(|group| libc::pid_t::try_from(group).ok())
        && group > 1
    {
        // SAFETY: `killpg` takes plain values; a group that is gone answers `ESRCH`, ignored.
        unsafe { libc::killpg(group, signal.number()) };
    }
    if let Ok(pid) = libc::pid_t::try_from(holder.pid)
        && pid > 1
        && alive(holder)
    {
        // SAFETY: `kill` takes plain values; a process that is gone answers `ESRCH`, ignored.
        unsafe { libc::kill(pid, signal.number()) };
    }
}

/// When the process started, as the OS reports it; none when there is no such process.
fn start_of(pid: u32) -> Option<String> {
    if Path::new("/proc/self/stat").exists() {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        return proc_start(&stat);
    }
    let output = Command::new("ps")
        // Two `-o`: BSD's ps reads everything after a `=` as that column's header, commas included.
        .args(["-o", "stat=", "-o", "lstart=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    ps_start(&String::from_utf8_lossy(&output.stdout))
}

/// The start of a `ps -o stat= -o lstart=` line: its state word, then the start. A zombie (a state
/// that opens with `Z`) has exited, though `ps` still prints when it started, and has no start,
/// as on Linux ([`proc_start`]).
fn ps_start(line: &str) -> Option<String> {
    let (state, start) = line.trim().split_once(char::is_whitespace)?;
    let start = start.trim();
    (!state.starts_with('Z') && !start.is_empty()).then(|| start.to_owned())
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
/// whose environment carries [`RUN_VAR`], grouped by it, ordered by run folder. A run whose
/// folder says it has ended is kept while a process still carries it: this is what the budget's
/// live use and its one-E rule read, and an orphan still uses the machine.
///
/// A process whose environment cannot be read (another user's, or one that exits during
/// the read) is skipped. Each call reads the process table afresh; nothing is kept.
pub fn machine() -> Vec<MachineInstance> {
    group(environments(), &resident_kibibytes)
}

/// The live zaf instances as `zaf status` shows them: the [`machine`] read less the runs whose
/// folder says they have ended ([`run_ended`]), so an orphan of an ended run is not shown as an
/// instance. Only for what is shown; the budget reads [`machine`].
pub fn displayed() -> Vec<MachineInstance> {
    let mut instances = machine();
    instances.retain(|instance| !run_ended(&instance.run, &alive));
    instances
}

/// The file a run folder records its exit in, a launch's and a phase's alike.
const EXIT_FILE: &str = "exit";

/// The file a run folder names the run's own process in, a launch's and a phase's alike.
const PID_FILE: &str = "pid";

/// Whether the run folder `run` says its run has ended: it records an exit, or the process its
/// `pid` file names is gone (`alive` says). A folder that records neither, or is not there, has
/// not: a process carrying [`RUN_VAR`] then still counts.
fn run_ended(run: &Path, alive: &dyn Fn(&Holder) -> bool) -> bool {
    if run.join(EXIT_FILE).exists() {
        return true;
    }
    std::fs::read_to_string(run.join(PID_FILE))
        .ok()
        .and_then(|text| Holder::parse(&text))
        .is_some_and(|holder| !alive(&holder))
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

/// The pid and kept variables of one line of `ps` with the environment
/// ([`PS_ENVIRONMENT_FLAGS`], `-o pid=,command=`): the pid, then the
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

/// The name of a `NAME=value` word, a name being letters of either case, digits and `_`, not led
/// by a digit: a lowercase name (`api_token=...`) ends the entry before it as a capital one does,
/// so its value is never taken for the rest of a kept one.
fn assigned_name(word: &[u8]) -> Option<&str> {
    let split = word.iter().position(|byte| *byte == b'=')?;
    let name = &word[..split];
    let first = *name.first()?;
    let valid = !first.is_ascii_digit()
        && name
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_');
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

/// The `ps` flags that list every process, unclipped, with its environment after its
/// command. macOS's `ps` takes `-e` as `-A` and shows the environment with `-E`; the other
/// BSDs show it with `-e`.
#[cfg(target_os = "macos")]
const PS_ENVIRONMENT_FLAGS: &str = "-AEww";
#[cfg(not(target_os = "macos"))]
const PS_ENVIRONMENT_FLAGS: &str = "-Aeww";

/// Elsewhere: `ps` with the environment of every process it may show.
fn ps_environments() -> Vec<(u32, Kept)> {
    let Ok(output) = Command::new("ps")
        .args([PS_ENVIRONMENT_FLAGS, "-o", "pid=,command="])
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

/// The resident memory of a process in kibibytes: the `VmRSS` line of `/proc/<pid>/status`
/// on Linux, else `ps -o rss=`. None when it cannot be read: the process is gone, a zombie
/// (which holds none) or another user's.
fn resident_kibibytes(pid: u32) -> Option<u64> {
    if Path::new("/proc/self/status").exists() {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        return vm_rss(&status);
    }
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// The kibibytes of the `VmRSS:` line of a `/proc/<pid>/status` file, as `VmRSS:  1234 kB`.
fn vm_rss(status: &str) -> Option<u64> {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?;
    line.split_whitespace().next()?.parse().ok()
}

/// The processes carrying [`RUN_VAR`], one instance per run folder. The instance and size
/// come from its lowest pid that carries them; a size that does not parse is none. Its memory
/// is the sum of what `resident` reads, in kibibytes, of each of its processes; one it cannot
/// read adds nothing. The sum is rounded to mebibytes once, so many small processes lose nothing.
fn group(
    mut processes: Vec<(u32, Kept)>,
    resident: &dyn Fn(u32) -> Option<u64>,
) -> Vec<MachineInstance> {
    processes.sort_by_key(|(pid, _)| *pid);
    let mut instances: BTreeMap<PathBuf, (MachineInstance, u64)> = BTreeMap::new();
    for (pid, kept) in processes {
        let Some(run) = kept.run.map(PathBuf::from) else {
            continue;
        };
        let (instance, kibibytes) = instances.entry(run.clone()).or_insert_with(|| {
            (
                MachineInstance {
                    root: root_of(&run),
                    run,
                    instance: None,
                    size: None,
                    pids: Vec::new(),
                    memory: Mebibytes::default(),
                },
                0,
            )
        });
        instance.pids.push(pid);
        if instance.instance.is_none() {
            instance.instance = kept.instance;
        }
        if instance.size.is_none() {
            instance.size = kept.size.and_then(|size| size.parse::<TaskSize>().ok());
        }
        *kibibytes = kibibytes.saturating_add(resident(pid).unwrap_or(0));
    }
    instances
        .into_values()
        .map(|(mut instance, kibibytes)| {
            instance.memory = Mebibytes::from_kibibytes(kibibytes);
            instance
        })
        .collect()
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
            let stub = Self(command.spawn().expect("runs sleep"));
            stub.await_environment(env);
            stub
        }

        /// Waits, where `/proc` is read, until the stub's own environment shows there: a read made
        /// straight after the start has been seen to miss it. Bounded; a test that still misses it
        /// fails on its own assertion.
        fn await_environment(&self, env: &[(&str, &str)]) {
            let file = format!("/proc/{}/environ", self.0.id());
            let Some((name, value)) = env.first() else {
                return;
            };
            let wanted = format!("{name}={value}");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while Path::new(&file).exists() && std::time::Instant::now() < deadline {
                let shown = std::fs::read(&file)
                    .is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(&wanted));
                if shown {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        /// A stub that holds `mebibytes` of resident memory: python fills a bytes object, which
        /// it must touch to build. Returns once the memory shows in the process table.
        fn holding(env: &[(&str, &str)], mebibytes: u64) -> Self {
            let script = format!("import time\nblock = b'x' * ({mebibytes} << 20)\ntime.sleep(30)");
            let mut command = Command::new("python3");
            command
                .args(["-c", &script])
                .env_remove(RUN_VAR)
                .env_remove(SIZE_VAR)
                .env_remove(INSTANCE_VAR)
                .stdin(Stdio::null());
            for (name, value) in env {
                command.env(name, value);
            }
            let stub = Self(command.spawn().expect("runs python3"));
            let wanted = mebibytes * 1024;
            for _ in 0..200 {
                if resident_kibibytes(stub.pid()).is_some_and(|held| held >= wanted) {
                    return stub;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            panic!("the stub never held {mebibytes} MiB");
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

    /// The instances `zaf status` shows whose run folder is under `dir`.
    fn shown_under(dir: &Path) -> Vec<MachineInstance> {
        displayed()
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
                memory: read[0].memory,
            }]
        );
    }

    /// The memory the machine read gives the instance of `run`, under `dir`.
    fn memory_of(dir: &Path, run: &Path) -> Mebibytes {
        let read = read_under(dir);
        let found = read.iter().find(|instance| instance.run == run);
        found.expect("the instance is read").memory
    }

    #[test]
    fn an_instance_sums_its_processes_memory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let run = run_in(dir.path(), "bookshelf", "team-instance-1-on-2");
        let env = [(RUN_VAR, run.to_str().expect("utf-8"))];
        let _instance = Stub::holding(&env, 200);
        let _child = Stub::holding(&env, 100);

        let memory = memory_of(dir.path(), &run).get();

        // The two hold 300 MiB between them, and python itself about 10 MiB each.
        assert!(
            (300..=360).contains(&memory),
            "one instance of two processes reads their sum, not one of them: {memory} MiB"
        );
    }

    #[test]
    fn memory_reads_in_units() {
        let dir = tempfile::tempdir().expect("temp dir");
        let run = run_in(dir.path(), "bookshelf", "team-instance-1-on-2");
        let _stub = Stub::holding(&[(RUN_VAR, run.to_str().expect("utf-8"))], 512);

        let read = read_under(dir.path());

        let units = crate::ports::live_use(&read);
        assert!(
            (0.5..0.55).contains(&units.as_f64()),
            "512 MiB and a little reads as about 0.5 units, not {units}"
        );
        assert_eq!(units.to_string(), "0.5");
    }

    #[test]
    fn an_exited_process_frees_its_memory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let run = run_in(dir.path(), "bookshelf", "team-instance-1-on-2");
        let env = [(RUN_VAR, run.to_str().expect("utf-8"))];
        let _kept = Stub::holding(&env, 100);
        let mut gone = Stub::holding(&env, 300);
        assert!(memory_of(dir.path(), &run).get() >= 400);

        gone.stop();

        let memory = memory_of(dir.path(), &run).get();
        assert!(
            (100..200).contains(&memory),
            "the next read no longer counts the exited process: {memory} MiB"
        );
    }

    #[test]
    fn an_orphan_of_an_ended_run_is_no_instance() {
        let dir = tempfile::tempdir().expect("temp dir");
        // Three runs of one project, each with a process still carrying its `ZAF_RUN`: one whose
        // folder records its exit, one whose recorded process is gone (the orphan's parent),
        // and one whose recorded process is the live stub itself.
        let ended = run_in(dir.path(), "bookshelf", "implement-1-task-3");
        let orphaned = run_in(dir.path(), "bookshelf", "implement-2-task-4");
        let live = run_in(dir.path(), "bookshelf", "implement-3-task-5");
        for run in [&ended, &orphaned, &live] {
            std::fs::create_dir_all(run).expect("run folder");
        }
        std::fs::write(ended.join(EXIT_FILE), "0\n").expect("exit");
        let gone = Holder {
            pid: u32::MAX - 1,
            start: "1".to_owned(),
        };
        std::fs::write(orphaned.join(PID_FILE), gone.to_string()).expect("pid");
        let _ended = Stub::start(&[(RUN_VAR, ended.to_str().expect("utf-8"))]);
        let _orphan = Stub::start(&[(RUN_VAR, orphaned.to_str().expect("utf-8"))]);
        let running = Stub::start(&[(RUN_VAR, live.to_str().expect("utf-8"))]);
        let holder = holder_of(running.pid()).expect("the stub's holder");
        std::fs::write(live.join(PID_FILE), holder.to_string()).expect("pid");

        let shown = shown_under(dir.path());

        let runs: Vec<&Path> = shown
            .iter()
            .map(|instance| instance.run.as_path())
            .collect();
        assert_eq!(runs, [live.as_path()], "only the live run is shown");
        // The machine read the budget charges still sees all three.
        assert_eq!(read_under(dir.path()).len(), 3);
        assert!(run_ended(&ended, &|_| true));
        assert!(run_ended(&orphaned, &alive));
        assert!(!run_ended(&live, &alive));
        assert!(
            !run_ended(&dir.path().join("nowhere"), &alive),
            "no folder, nothing ended"
        );
    }

    #[test]
    fn a_process_whose_memory_cannot_be_read_adds_nothing() {
        let env = |run: &str| Kept {
            run: Some(run.to_owned()),
            ..Kept::default()
        };
        let read = group(
            vec![(7, env("/p/.zaf/runs/x")), (8, env("/p/.zaf/runs/x"))],
            &|pid| (pid == 7).then_some(2048),
        );

        assert_eq!(read.len(), 1);
        assert_eq!(read[0].memory, Mebibytes::new(2));
        assert_eq!(
            read[0].pids,
            [7, 8],
            "it still counts as a process of the instance"
        );
    }

    #[test]
    fn processes_are_summed_before_rounding_to_mebibytes() {
        let env = |run: &str| Kept {
            run: Some(run.to_owned()),
            ..Kept::default()
        };
        let processes = (1..=4).map(|pid| (pid, env("/p/.zaf/runs/x"))).collect();

        let read = group(processes, &|_| Some(400));

        assert_eq!(
            read[0].memory,
            Mebibytes::new(2),
            "1600 KiB, not four rounds of 0"
        );
    }

    #[test]
    fn the_status_file_gives_its_resident_kibibytes() {
        let status = "Name:\tclaude\nVmPeak:\t  900000 kB\nVmRSS:\t  524288 kB\nRssAnon:\t 1 kB\n";
        assert_eq!(vm_rss(status), Some(524_288));
        assert_eq!(vm_rss("Name:\tkthreadd\nState:\tS (sleeping)\n"), None);
        assert_eq!(vm_rss("VmRSS:\tmany kB\n"), None);
    }

    #[test]
    fn this_process_reads_some_memory() {
        let held = resident_kibibytes(std::process::id()).expect("this process has memory");
        assert!(held > 0);
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
    fn a_lowercase_variable_ends_the_entry_before_it() {
        let line =
            b"4242 claude ZAF_RUN=/p/.zaf/runs/x api_token=secret http_proxy=http://h ZAF_SIZE=S";
        let (_, kept) = kept_of_ps_line(line).expect("a line");
        assert_eq!(kept.run.as_deref(), Some("/p/.zaf/runs/x"));
        assert_eq!(kept.size.as_deref(), Some("S"));
    }

    #[test]
    fn a_process_without_a_run_is_no_instance() {
        let kept = Kept {
            instance: Some("team instance 1 on #2".to_owned()),
            ..Kept::default()
        };
        assert!(group(vec![(7, kept)], &|_| Some(1024)).is_empty());
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
    fn a_zombie_in_a_ps_line_has_no_start() {
        assert_eq!(
            ps_start("S+   Mon Nov 16 01:02:03 2026\n").as_deref(),
            Some("Mon Nov 16 01:02:03 2026")
        );
        assert_eq!(ps_start("Z    Mon Nov 16 01:02:03 2026\n"), None);
        assert_eq!(ps_start("Z+   Mon Nov 16 01:02:03 2026\n"), None);
        assert_eq!(ps_start(""), None);
        assert_eq!(ps_start("S\n"), None);
    }

    #[test]
    fn a_zombie_has_no_start() {
        let stat = "4242 (claude) Z 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 0 0";
        assert_eq!(proc_start(stat), None);
    }
}
