use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;
use crate::app::schedule::lock::Holding;
use crate::app::spawn::scheduler::Launch;
use crate::ports::Holder;

/// A machine that records what it was asked to start, with `running` holding the lock.
#[derive(Default)]
struct Fake {
    running: Option<Holding>,
    refuses: bool,
    launched: Mutex<Vec<Launch>>,
}

impl Processes for Fake {
    fn program(&self) -> Result<PathBuf, String> {
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
            return Err("it exited 1".to_owned());
        }
        Ok(5150)
    }

    fn ask_tick(&self, _state_dir: &Path) -> Result<Option<Holding>, String> {
        Ok(self.running.clone())
    }
}

fn holding() -> Holding {
    Holding {
        pid: 4242,
        start: "12345".to_owned(),
        since: "2026-11-19T03:00:00Z".to_owned(),
        zaf: "0.5.5".to_owned(),
        version: "0.5.5".to_owned(),
    }
}

const ROOT: &str = "/srv/ways";

#[test]
fn spawn_start_verb_prints_pid() {
    let fake = Fake::default();

    let said = start_said(Path::new(ROOT), "0.5.5", &fake, false);

    assert_eq!(said.code, OK);
    let out = said.out.expect("a line");
    assert!(out.contains("pid 5150"), "{out}");
    assert!(out.contains(".zaf/logs/"), "{out}");
    assert_eq!(fake.launched.lock().expect("not poisoned").len(), 1);

    let json = start_said(Path::new(ROOT), "0.5.5", &Fake::default(), true);
    let value: serde_json::Value =
        serde_json::from_str(&json.out.expect("json")).expect("valid json");
    assert_eq!(value["pid"], 5150);
    assert_eq!(value["started"], true);
    assert!(
        value["log"]
            .as_str()
            .expect("a log")
            .starts_with(".zaf/logs/")
    );
}

#[test]
fn spawn_start_verb_hands_back_running_pid() {
    let fake = Fake {
        running: Some(holding()),
        ..Fake::default()
    };

    let said = start_said(Path::new(ROOT), "0.5.5", &fake, false);

    assert_eq!(said.code, OK);
    let out = said.out.expect("a line");
    assert!(out.contains("already running"), "{out}");
    assert!(out.contains("pid 4242"), "{out}");
    assert!(fake.launched.lock().expect("not poisoned").is_empty());

    let json = start_said(Path::new(ROOT), "0.5.5", &fake, true);
    let value: serde_json::Value =
        serde_json::from_str(&json.out.expect("json")).expect("valid json");
    assert_eq!(value["pid"], 4242);
    assert_eq!(value["started"], false);
    assert!(value["log"].is_null());
}

#[test]
fn spawn_start_verb_refuses_bad_version() {
    for bad in ["", "-x", "0.5 5", "a/b"] {
        let fake = Fake::default();

        let said = start_said(Path::new(ROOT), bad, &fake, false);

        assert_eq!(said.code, REFUSED, "{bad:?}");
        assert!(said.out.is_none());
        assert!(fake.launched.lock().expect("not poisoned").is_empty());
    }
}

#[test]
fn spawn_start_verb_fails_when_the_process_does_not_stay() {
    let fake = Fake {
        refuses: true,
        ..Fake::default()
    };

    let said = start_said(Path::new(ROOT), "0.5.5", &fake, false);

    assert_eq!(said.code, BACKEND);
    assert!(said.err.expect("a reason").contains("exited 1"));
}
