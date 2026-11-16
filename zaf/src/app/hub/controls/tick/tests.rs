//! The tick control's own rules, with no server: which projects it ticks, which versions it takes
//! and what it says of a tick that was refused or has not ended.

use std::sync::Mutex;

use super::*;
use crate::app::projects::Projects;

/// A ticker that answers `answer` and keeps what it was asked.
struct Fake {
    answer: Ticked,
    asked: Mutex<Vec<(PathBuf, String)>>,
}

impl Fake {
    fn answering(answer: Ticked) -> Self {
        Self {
            answer,
            asked: Mutex::new(Vec::new()),
        }
    }

    fn asked(&self) -> Vec<(PathBuf, String)> {
        self.asked.lock().expect("lock").clone()
    }
}

impl Ticker for Fake {
    fn tick(&self, root: &Path, version: &str) -> Ticked {
        self.asked
            .lock()
            .expect("lock")
            .push((root.to_owned(), version.to_owned()));
        self.answer.clone()
    }
}

/// A zaf home listing one project, with its manifest.
fn listed() -> (tempfile::TempDir, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("home");
    let project = tempfile::tempdir().expect("project");
    std::fs::write(
        project.path().join(".zaf.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("manifest");
    Projects::add(home.path(), project.path()).expect("listed");
    (home, project)
}

fn body(project: &Path, version: Option<&str>) -> Vec<u8> {
    match version {
        Some(version) => serde_json::json!({ "project": project, "version": version }),
        None => serde_json::json!({ "project": project }),
    }
    .to_string()
    .into_bytes()
}

fn refusal(reply: &Reply) -> &str {
    reply.body["refused"].as_str().expect("a refusal")
}

#[test]
fn versions_are_milestones_and_never_an_argument() {
    for good in ["0.4.0", "1.0.0-rc.1", "2026_11", "0.4.0+build.5"] {
        assert!(valid_version(good), "{good}");
    }
    for bad in [
        "",
        "-0.4.0",
        "--help",
        "0.4 0",
        "0.4.0\n",
        "a/b",
        &"9".repeat(65),
    ] {
        assert!(!valid_version(bad), "{bad:?}");
    }
}

#[test]
fn only_a_listed_project_is_ticked() {
    let (home, _) = listed();
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let ticker = Fake::answering(Ticked::Ran(Vec::new()));

    let reply = handle(
        Some(home.path()),
        &ticker,
        &body(elsewhere.path(), Some("0.4.0")),
    );

    assert_eq!(reply.status, "404 Not Found");
    assert!(refusal(&reply).contains("not on this machine's project list"));
    assert!(ticker.asked().is_empty());
}

#[test]
fn a_project_gone_from_the_machine_is_not_ticked() {
    let (home, project) = listed();
    let root = project.path().to_owned();
    std::fs::remove_file(root.join(".zaf.toml")).expect("manifest gone");
    let ticker = Fake::answering(Ticked::Ran(Vec::new()));

    let reply = handle(Some(home.path()), &ticker, &body(&root, Some("0.4.0")));

    assert_eq!(reply.status, "409 Conflict");
    assert!(ticker.asked().is_empty());
}

#[test]
fn a_body_that_is_not_a_request_is_refused() {
    let (home, _) = listed();
    let ticker = Fake::answering(Ticked::Ran(Vec::new()));

    for bad in [&b"not json"[..], b"[]", b"{}", b""] {
        let reply = handle(Some(home.path()), &ticker, bad);
        assert_eq!(
            reply.status,
            "400 Bad Request",
            "{:?}",
            String::from_utf8_lossy(bad)
        );
    }
    assert!(ticker.asked().is_empty());
}

#[test]
fn a_version_that_is_not_one_is_refused_and_not_remembered() {
    let (home, project) = listed();
    let ticker = Fake::answering(Ticked::Ran(Vec::new()));

    let reply = handle(
        Some(home.path()),
        &ticker,
        &body(project.path(), Some("--help")),
    );

    assert_eq!(reply.status, "400 Bad Request");
    assert!(refusal(&reply).contains("is not a version"));
    assert!(ticker.asked().is_empty());
    assert_eq!(Projects::read(home.path())[0].version, None);
}

#[test]
fn no_zaf_home_is_said_in_words() {
    let ticker = Fake::answering(Ticked::Ran(Vec::new()));

    let reply = handle(None, &ticker, &body(Path::new("/x"), Some("0.4.0")));

    assert_eq!(reply.status, "503 Service Unavailable");
    assert!(refusal(&reply).contains("no zaf home"));
}

#[test]
fn a_tick_the_verb_refused_is_a_conflict_with_its_words() {
    let (home, project) = listed();
    let ticker = Fake::answering(Ticked::Refused("the profile is stale".to_owned()));

    let reply = handle(
        Some(home.path()),
        &ticker,
        &body(project.path(), Some("0.4.0")),
    );

    assert_eq!(reply.status, "409 Conflict");
    assert_eq!(refusal(&reply), "the profile is stale");
}

#[test]
fn a_tick_that_failed_is_a_bad_gateway_with_its_words() {
    let (home, project) = listed();
    let ticker = Fake::answering(Ticked::Failed("the forge cannot be reached".to_owned()));

    let reply = handle(
        Some(home.path()),
        &ticker,
        &body(project.path(), Some("0.4.0")),
    );

    assert_eq!(reply.status, "502 Bad Gateway");
    assert_eq!(refusal(&reply), "the forge cannot be reached");
}

#[test]
fn a_tick_still_running_says_the_hub_does_not_wait() {
    let (home, project) = listed();
    let ticker = Fake::answering(Ticked::Running(vec!["started implement #7".to_owned()]));

    let reply = handle(
        Some(home.path()),
        &ticker,
        &body(project.path(), Some("0.4.0")),
    );

    assert_eq!(reply.status, "200 OK");
    assert_eq!(reply.body["running"], true);
    let lines = reply.body["tick"].as_array().expect("lines");
    assert_eq!(lines[0], "started implement #7");
    assert!(
        lines[1].as_str().expect("a line").contains("does not wait"),
        "{lines:?}"
    );
}

#[test]
fn a_tick_with_nothing_to_say_still_says_it_ran() {
    let (home, project) = listed();
    let ticker = Fake::answering(Ticked::Ran(Vec::new()));

    let reply = handle(
        Some(home.path()),
        &ticker,
        &body(project.path(), Some("0.4.0")),
    );

    assert_eq!(reply.body["tick"][0], "the tick ran and said nothing");
}

#[test]
fn the_project_may_be_named_by_a_path_that_reaches_it_another_way() {
    let (home, project) = listed();
    let ticker = Fake::answering(Ticked::Ran(Vec::new()));
    // The list keeps the resolved folder; a request may name it with a `..` in it.
    let roundabout = project
        .path()
        .join("..")
        .join(project.path().file_name().expect("name"));

    let reply = handle(
        Some(home.path()),
        &ticker,
        &body(&roundabout, Some("0.4.0")),
    );

    assert_eq!(reply.status, "200 OK", "{:?}", reply.body);
    assert_eq!(ticker.asked().len(), 1);
}
