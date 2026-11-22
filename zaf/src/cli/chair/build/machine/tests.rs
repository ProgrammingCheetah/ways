use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use super::{Launch, Machine, Processes, detach, dropped, signal_target};
use crate::app::chair::ReleaseVersion;
use crate::app::hub::controls::Failure;
use crate::app::schedule::lock::Holding;
use crate::cli::chair::build::Controls;
use crate::ports::Holder;

fn holding(pid: u32) -> Holding {
    Holding {
        pid,
        start: "12345".to_owned(),
        since: "2026-11-19T03:00:00Z".to_owned(),
        zaf: "0.5.1".to_owned(),
        version: "0.5.2".to_owned(),
    }
}

/// A machine that records what it was asked to start and signal, with `running` holding the lock.
#[derive(Default)]
struct Fake {
    running: Option<Holding>,
    launched: Mutex<Vec<Launch>>,
    terminated: Mutex<Vec<Holder>>,
    asked: Mutex<Vec<PathBuf>>,
}

impl Processes for Fake {
    fn program(&self) -> Result<PathBuf, String> {
        Ok(PathBuf::from("/usr/local/bin/zaf"))
    }

    fn scheduler(&self, _state_dir: &Path) -> Option<Holding> {
        self.running.clone()
    }

    fn terminate(&self, holder: &Holder) {
        self.terminated
            .lock()
            .expect("not poisoned")
            .push(holder.clone());
    }

    fn detach(&self, launch: &Launch) -> Result<u32, String> {
        self.launched
            .lock()
            .expect("not poisoned")
            .push(launch.clone());
        Ok(5150)
    }

    fn ask_tick(&self, state_dir: &Path) -> Result<Option<Holding>, String> {
        self.asked
            .lock()
            .expect("not poisoned")
            .push(state_dir.to_owned());
        Ok(self.running.clone())
    }
}

#[test]
fn the_sigterm_goes_to_the_scheduler_the_lock_names_and_no_other() {
    assert_eq!(
        signal_target(Some(&holding(4242)), 4242),
        Ok(Holder {
            pid: 4242,
            start: "12345".to_owned(),
        })
    );
    assert!(matches!(
        signal_target(Some(&holding(4343)), 4242),
        Err(Failure::Refused(why)) if why.starts_with("another scheduler holds the lock now (pid 4343")
    ));
    assert_eq!(
        signal_target(None, 4242),
        Err(Failure::Refused("no scheduler runs now".to_owned()))
    );
}

#[test]
fn stopping_signals_the_lock_holder_and_a_changed_lock_signals_nothing() {
    let machine = Machine::with(
        None,
        Fake {
            running: Some(holding(4242)),
            ..Fake::default()
        },
    );
    assert_eq!(
        machine.stop(Path::new("/srv/ways"), 4242),
        Ok("the scheduler (pid 4242) stops after its tick".to_owned())
    );
    assert!(machine.stop(Path::new("/srv/ways"), 999).is_err());
    assert_eq!(
        *machine.processes.terminated.lock().expect("not poisoned"),
        [Holder {
            pid: 4242,
            start: "12345".to_owned(),
        }],
        "one signal, to the pid asked about"
    );
}

#[test]
fn starting_runs_the_verb_detached_in_the_project_with_its_log_under_zaf_logs() {
    let machine = Machine::with(None, Fake::default());
    let root = Path::new("/srv/ways");

    let said = machine
        .start(root, &ReleaseVersion::from("0.5.2"))
        .expect("started");

    let launched = machine.processes.launched.lock().expect("not poisoned");
    let [launch] = launched.as_slice() else {
        panic!("one launch: {launched:?}");
    };
    assert_eq!(launch.program, PathBuf::from("/usr/local/bin/zaf"));
    assert_eq!(launch.args, ["spawn", "--watch", "--version", "0.5.2"]);
    assert_eq!(launch.folder, root);
    assert_eq!(launch.log.parent(), Some(Path::new("/srv/ways/.zaf/logs")));
    let name = launch
        .log
        .file_name()
        .and_then(OsStr::to_str)
        .expect("a name");
    assert!(name.ends_with("_spawn-watch-0.5.2.log"), "{name}");
    assert!(
        said.starts_with("the build of 0.5.2 started (pid 5150); its log is .zaf/logs/"),
        "{said}"
    );
}

#[test]
fn starting_is_refused_while_a_scheduler_runs() {
    let machine = Machine::with(
        None,
        Fake {
            running: Some(holding(4242)),
            ..Fake::default()
        },
    );
    let refused = machine.start(Path::new("/srv/ways"), &ReleaseVersion::from("0.5.2"));
    assert!(
        matches!(&refused, Err(Failure::Refused(why)) if why.starts_with("a scheduler already runs")),
        "{refused:?}"
    );
    assert!(
        machine
            .processes
            .launched
            .lock()
            .expect("not poisoned")
            .is_empty()
    );
}

#[test]
fn a_tick_is_asked_of_the_project_scheduler() {
    let machine = Machine::with(
        None,
        Fake {
            running: Some(holding(4242)),
            ..Fake::default()
        },
    );
    let said = machine.tick(Path::new("/srv/ways")).expect("asked");
    assert!(
        said.starts_with("asked the running scheduler for a tick: pid 4242"),
        "{said}"
    );
    assert_eq!(
        *machine.processes.asked.lock().expect("not poisoned"),
        [PathBuf::from("/srv/ways/.zaf")]
    );

    let none = Machine::with(None, Fake::default());
    assert_eq!(
        none.tick(Path::new("/srv/ways")),
        Err(Failure::Refused(
            "no scheduler runs now; b starts one".to_owned()
        ))
    );
}

#[test]
fn only_zaf_home_passes_to_the_scheduler() {
    let names = [
        "PATH",
        "ZAF_HOME",
        "ZAF_INSTANCE",
        "ZAF_RUN",
        "ZAF_SCHEDULED_BY",
        "HOME",
    ];
    assert_eq!(
        dropped(names.iter().map(OsStr::new)),
        ["ZAF_INSTANCE", "ZAF_RUN", "ZAF_SCHEDULED_BY"]
    );
}

fn launch(folder: &Path, script: &str) -> Launch {
    Launch {
        program: PathBuf::from("/bin/sh"),
        args: vec!["-c".to_owned(), script.to_owned()],
        folder: folder.to_owned(),
        log: folder.join(".zaf").join("logs").join("run.log"),
        dropped: vec!["ZAF_INSTANCE".into()],
    }
}

#[test]
fn a_detached_process_leads_its_own_session_and_writes_its_log() {
    let folder = tempfile::tempdir().expect("a folder");
    let started = launch(folder.path(), "echo started; exec sleep 30");

    let pid = detach(&started, Duration::from_millis(200)).expect("it runs");

    let pid_t = libc::pid_t::try_from(pid).expect("a pid");
    // SAFETY: `getsid` and `kill` take plain values and touch no memory.
    let session = unsafe { libc::getsid(pid_t) };
    let until = std::time::Instant::now() + Duration::from_secs(5);
    let mut log = String::new();
    while log.is_empty() && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
        log = std::fs::read_to_string(&started.log).expect("its log");
    }
    // SAFETY: as above.
    unsafe { libc::kill(pid_t, libc::SIGKILL) };
    assert_eq!(session, pid_t, "it leads a session of its own");
    assert_eq!(log, "started\n");
}

#[test]
fn a_process_that_ends_at_once_says_its_last_line() {
    let folder = tempfile::tempdir().expect("a folder");
    let refused = launch(folder.path(), "echo 'zaf spawn: refused' >&2; exit 1");

    let why = detach(&refused, Duration::from_secs(5)).expect_err("it ended");

    assert_eq!(why, "it exited 1: zaf spawn: refused");
}

#[test]
fn a_detached_process_inherits_no_descriptor_beyond_its_stdio() {
    use std::os::fd::AsRawFd;
    let folder = tempfile::tempdir().expect("a folder");
    let held = folder.path().join("held-by-the-chair");
    let file = std::fs::File::create(&held).expect("a file");
    // SAFETY: dup takes a plain descriptor that is open; the copy is closed below. A plain dup
    // is not close-on-exec: as a library's leaked descriptor would be.
    let leaked = unsafe { libc::dup(file.as_raw_fd()) };
    assert!(leaked >= 0);
    let listing = launch(folder.path(), "ls -l /proc/$$/fd; exit 3");

    let why = detach(&listing, Duration::from_secs(5)).expect_err("it ended");

    // SAFETY: `leaked` is the copy dup returned.
    unsafe { libc::close(leaked) };
    assert!(why.starts_with("it exited 3"), "{why}");
    let log = std::fs::read_to_string(&listing.log).expect("its log");
    if cfg!(target_os = "linux") {
        assert!(log.contains(" 0 -> "), "the listing is there: {log}");
        assert!(
            !log.contains("held-by-the-chair"),
            "the parent's descriptor {leaked} is not inherited:\n{log}"
        );
    }
}
