use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use super::{Launch, Processes, Start, dropped, start, start_with};
use crate::app::chair::ReleaseVersion;
use crate::app::hub::controls::Failure;
use crate::app::schedule::lock::Holding;
use crate::ports::Holder;

fn holding(pid: u32) -> Holding {
    Holding {
        pid,
        start: "12345".to_owned(),
        since: "2026-11-19T03:00:00Z".to_owned(),
        zaf: "0.5.1".to_owned(),
        version: "0.5.5".to_owned(),
    }
}

/// A machine that records what it was asked to start, with `running` holding the lock.
#[derive(Default)]
struct Fake {
    running: Option<Holding>,
    no_binary: bool,
    refuses: bool,
    launched: Mutex<Vec<Launch>>,
}

impl Processes for Fake {
    fn program(&self) -> Result<PathBuf, String> {
        if self.no_binary {
            return Err("no binary".to_owned());
        }
        Ok(PathBuf::from("/usr/local/bin/zaf"))
    }

    fn scheduler(&self, _state_dir: &Path) -> Option<Holding> {
        self.running.clone()
    }

    fn terminate(&self, _holder: &Holder) {}

    fn detach(&self, launch: &Launch) -> Result<u32, String> {
        self.launched
            .lock()
            .expect("not poisoned")
            .push(launch.clone());
        if self.refuses {
            return Err("it exited 1: zaf spawn: refused".to_owned());
        }
        Ok(5150)
    }

    fn ask_tick(&self, _state_dir: &Path) -> Result<Option<Holding>, String> {
        Ok(self.running.clone())
    }
}

fn version() -> ReleaseVersion {
    ReleaseVersion::from("0.5.5")
}

#[test]
fn spawn_start_starts_detached() {
    let fake = Fake::default();
    let root = Path::new("/srv/ways");

    let started = start(root, &version(), &fake).expect("started");

    let Start::Started { pid, log } = &started else {
        panic!("a new scheduler: {started:?}");
    };
    assert_eq!(*pid, 5150);
    assert_eq!(started.pid(), 5150);
    let launched = fake.launched.lock().expect("not poisoned");
    let [launch] = launched.as_slice() else {
        panic!("one launch: {launched:?}");
    };
    assert_eq!(launch.program, PathBuf::from("/usr/local/bin/zaf"));
    assert_eq!(launch.args, ["spawn", "--watch", "--version", "0.5.5"]);
    assert_eq!(launch.folder, root);
    assert_eq!(&launch.log, log);
    assert_eq!(log.parent(), Some(Path::new("/srv/ways/.zaf/logs")));
    let name = log.file_name().and_then(OsStr::to_str).expect("a name");
    assert!(name.ends_with("_spawn-watch-0.5.5.log"), "{name}");
}

#[test]
fn spawn_start_hands_back_running() {
    let fake = Fake {
        running: Some(holding(4242)),
        ..Fake::default()
    };

    let said = start(Path::new("/srv/ways"), &version(), &fake).expect("handed back");

    assert_eq!(said, Start::Running(holding(4242)));
    assert_eq!(said.pid(), 4242);
    assert!(fake.launched.lock().expect("not poisoned").is_empty());
}

#[test]
fn spawn_start_says_why_it_could_not() {
    let root = Path::new("/srv/ways");
    let no_binary = Fake {
        no_binary: true,
        ..Fake::default()
    };
    assert_eq!(
        start(root, &version(), &no_binary),
        Err(Failure::Failed("no binary".to_owned()))
    );
    let refused = Fake {
        refuses: true,
        ..Fake::default()
    };
    assert_eq!(
        start(root, &version(), &refused),
        Err(Failure::Failed(
            "it exited 1: zaf spawn: refused".to_owned()
        ))
    );
}

#[test]
fn spawn_start_drops_claude_and_zaf_variables() {
    let names: Vec<OsString> = [
        "PATH",
        "HOME",
        "ZAF_HOME",
        "ZAF_DEPTH",
        "ZAF_INSTANCE",
        "CLAUDECODE",
        "CLAUDE_CONFIG_DIR",
        "CLAUDE_PROJECT_DIR",
        "NOT_CLAUDE",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let fake = Fake::default();

    start_with(
        Path::new("/srv/ways"),
        &version(),
        &fake,
        &names,
        SystemTime::UNIX_EPOCH,
    )
    .expect("started");

    let launched = fake.launched.lock().expect("not poisoned");
    let dropped: Vec<&str> = launched[0]
        .dropped
        .iter()
        .filter_map(|name| name.to_str())
        .collect();
    assert_eq!(
        dropped,
        [
            "ZAF_DEPTH",
            "ZAF_INSTANCE",
            "CLAUDECODE",
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_PROJECT_DIR"
        ]
    );
}

#[test]
fn a_variable_that_only_holds_claude_in_its_name_stays() {
    assert!(dropped(["MY_CLAUDE", "claude_lower"].iter().map(OsStr::new)).is_empty());
}
