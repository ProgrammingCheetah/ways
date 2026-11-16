//! The watch control's own rules, with no server and no loop: which projects it reaches, which
//! versions it takes and what it says of a start and a stop the watcher refused or has not ended.

use std::sync::Mutex;

use super::*;
use crate::app::projects::Projects;
use crate::app::spawn::watch::{Started, Stopped};

/// A watcher that answers what it is told to and keeps what it was asked.
struct Fake {
    start: Result<Started, Failure>,
    stop: Result<Stopped, Failure>,
    asked: Mutex<Vec<String>>,
}

impl Fake {
    fn new() -> Self {
        Self {
            start: Ok(Started {
                version: "0.4.0".to_owned(),
                since: "2026-11-13T12:00:00Z".to_owned(),
            }),
            stop: Ok(Stopped::Ended),
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}

impl Watcher for Fake {
    fn start(&self, root: &Path, version: &str) -> Result<Started, Failure> {
        self.asked
            .lock()
            .unwrap()
            .push(format!("start {} {version}", root.display()));
        self.start.clone()
    }

    fn stop(&self, root: &Path) -> Result<Stopped, Failure> {
        self.asked
            .lock()
            .unwrap()
            .push(format!("stop {}", root.display()));
        self.stop.clone()
    }
}

fn home_with_project() -> (tempfile::TempDir, tempfile::TempDir, String) {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    std::fs::write(project.path().join(".zaf.toml"), "").expect("manifest");
    Projects::add(home.path(), project.path()).expect("listed");
    let root = std::fs::canonicalize(project.path())
        .expect("root")
        .display()
        .to_string();
    (home, project, root)
}

fn post(home: &Path, watcher: &Fake, body: serde_json::Value) -> (&'static str, serde_json::Value) {
    let answer = run(Some(home), watcher, body.to_string().as_bytes());
    (
        answer.status,
        serde_json::from_str(&answer.body).expect("JSON"),
    )
}

#[test]
fn a_start_runs_the_watch_from_the_projects_root_on_the_given_version() {
    let (home, _project, root) = home_with_project();
    let watcher = Fake::new();

    let (status, body) = post(
        home.path(),
        &watcher,
        serde_json::json!({ "project": root, "action": "start", "version": "0.4.0" }),
    );

    assert_eq!(status, "200 OK", "{body}");
    assert_eq!(body["watching"], true);
    assert_eq!(body["action"], "start");
    assert_eq!(body["version"], "0.4.0");
    assert_eq!(body["since"], "2026-11-13T12:00:00Z");
    assert_eq!(watcher.asked(), [format!("start {root} 0.4.0")]);
    assert_eq!(
        Projects::read(home.path())[0].version.as_deref(),
        Some("0.4.0"),
        "the version is remembered, as for a tick"
    );
}

#[test]
fn a_start_with_no_version_takes_the_remembered_one_and_asks_for_one_when_none() {
    let (home, project, root) = home_with_project();
    let watcher = Fake::new();
    let start = serde_json::json!({ "project": root, "action": "start" });

    let (status, body) = post(home.path(), &watcher, start.clone());
    assert_eq!(status, "400 Bad Request", "{body}");
    assert!(
        body["refused"]
            .as_str()
            .unwrap()
            .contains("a version is needed"),
        "{body}"
    );
    assert!(watcher.asked().is_empty(), "nothing was started");

    Projects::remember_version(home.path(), project.path(), "0.3.0").expect("remembered");
    let (status, body) = post(home.path(), &watcher, start);
    assert_eq!(status, "200 OK", "{body}");
    assert_eq!(watcher.asked(), [format!("start {root} 0.3.0")]);
}

#[test]
fn a_refused_start_is_a_conflict_with_its_reason_and_a_failed_one_is_a_failure() {
    let (home, _project, root) = home_with_project();
    let mut watcher = Fake::new();
    let start = serde_json::json!({ "project": root, "action": "start", "version": "0.4.0" });

    watcher.start = Err(Failure::Refused("a scheduler already runs here".to_owned()));
    let (status, body) = post(home.path(), &watcher, start.clone());
    assert_eq!(status, "409 Conflict");
    assert_eq!(
        body,
        serde_json::json!({ "refused": "a scheduler already runs here" })
    );

    watcher.start = Err(Failure::Failed("no thread".to_owned()));
    let (status, body) = post(home.path(), &watcher, start);
    assert_eq!(status, "500 Internal Server Error");
    assert_eq!(body, serde_json::json!({ "refused": "no thread" }));
}

#[test]
fn a_stop_says_the_watch_has_ended_or_is_ending() {
    let (home, _project, root) = home_with_project();
    let mut watcher = Fake::new();
    let stop = serde_json::json!({ "project": root, "action": "stop" });

    let (status, body) = post(home.path(), &watcher, stop.clone());
    assert_eq!(status, "200 OK", "{body}");
    assert_eq!(body["watching"], false);
    assert_eq!(watcher.asked(), [format!("stop {root}")]);

    watcher.stop = Ok(Stopped::Ending);
    let (status, body) = post(home.path(), &watcher, stop.clone());
    assert_eq!(status, "202 Accepted", "{body}");
    assert_eq!(body["watching"], true);
    assert!(body["stopping"].as_str().unwrap().contains("current tick"));

    watcher.stop = Err(Failure::Refused("no watch runs here".to_owned()));
    let (status, body) = post(home.path(), &watcher, stop);
    assert_eq!(status, "409 Conflict");
    assert_eq!(body["refused"], "no watch runs here");
}

#[test]
fn a_body_that_is_not_a_watch_is_refused_and_nothing_runs() {
    let (home, _project, root) = home_with_project();
    let watcher = Fake::new();
    let outside = serde_json::json!({ "project": "/etc", "action": "start", "version": "0.4.0" });
    let asks = [
        ("not json".to_owned(), "400 Bad Request"),
        (r#"{"project":"x"}"#.to_owned(), "400 Bad Request"),
        (
            serde_json::json!({ "project": root, "action": "restart" }).to_string(),
            "400 Bad Request",
        ),
        (
            serde_json::json!({ "project": root, "action": "start", "version": 4 }).to_string(),
            "400 Bad Request",
        ),
        (
            serde_json::json!({ "project": root, "action": "start", "version": "-x" }).to_string(),
            "400 Bad Request",
        ),
        (outside.to_string(), "404 Not Found"),
    ];
    for (ask, status) in asks {
        let answer = run(Some(home.path()), &watcher, ask.as_bytes());
        assert_eq!(answer.status, status, "{ask}");
        let body: serde_json::Value = serde_json::from_str(&answer.body).expect("JSON");
        assert!(body["refused"].is_string(), "{ask}: {body}");
    }
    assert!(watcher.asked().is_empty(), "the watcher was never asked");
}

#[test]
fn a_project_gone_from_the_machine_has_no_watch_started_but_may_be_stopped() {
    let (home, project, root) = home_with_project();
    let watcher = Fake::new();
    drop(project);

    let (status, body) = post(
        home.path(),
        &watcher,
        serde_json::json!({ "project": root, "action": "start", "version": "0.4.0" }),
    );
    assert_eq!(status, "409 Conflict", "{body}");
    assert!(body["refused"].as_str().unwrap().contains("gone"), "{body}");
    assert!(watcher.asked().is_empty());

    let (status, _) = post(
        home.path(),
        &watcher,
        serde_json::json!({ "project": root, "action": "stop" }),
    );
    assert_eq!(status, "200 OK");
}
