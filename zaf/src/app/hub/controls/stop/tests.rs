use std::sync::{Arc, Mutex};

use super::*;
use crate::app::spawn::stop::StopError;

/// A zaf home that lists each of `names` as a folder holding a manifest, in a tree of its own;
/// the roots as the list holds them.
fn listed(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let home = tempfile::tempdir().expect("home");
    let tree = home.path().join("tree");
    let mut roots = Vec::new();
    for name in names {
        let folder = tree.join(name);
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join(".zaf.toml"), "").expect("manifest");
        Projects::add(home.path(), &folder).expect("listed");
        roots.push(std::fs::canonicalize(&folder).expect("root"));
    }
    (home, roots)
}

#[test]
fn a_body_names_a_project_and_a_task() {
    let (project, task) =
        parse(br#"{"project": "/a/b", "task": 17, "extra": true}"#).expect("a stop");
    assert_eq!(project, "/a/b");
    assert_eq!(task, TicketId::from(17));
}

#[test]
fn a_body_that_is_not_a_stop_is_a_bad_request() {
    let never: &Stopper = &|_, _| panic!("nothing is stopped");
    let (home, _) = listed(&["one"]);
    for body in [
        "",
        "not json",
        "[]",
        r#"{"project": "one"}"#,
        r#"{"task": 17}"#,
        r#"{"project": 1, "task": 17}"#,
        r#"{"project": "one", "task": "17"}"#,
        r#"{"project": "one", "task": 0}"#,
        r#"{"project": "one", "task": -3}"#,
        r#"{"project": "one", "task": 1.5}"#,
    ] {
        let answer = run(Some(home.path()), never, body.as_bytes());
        assert_eq!(answer.status, "400 Bad Request", "{body}");
        assert!(answer.body.contains("refused"), "{body}");
    }
}

#[test]
fn the_stop_runs_from_the_root_of_the_project_named() {
    let (home, roots) = listed(&["one", "two"]);
    let asked = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&asked);
    let stop: Arc<Stopper> = Arc::new(move |root, task| {
        sink.lock().unwrap().push((root.to_owned(), task));
        Err(Failure::Refused("nothing runs".to_owned()))
    });

    run(
        Some(home.path()),
        stop.as_ref(),
        br#"{"project": "two", "task": 5}"#,
    );
    let by_folder = serde_json::json!({ "project": roots[0], "task": 6 }).to_string();
    run(Some(home.path()), stop.as_ref(), by_folder.as_bytes());

    assert_eq!(
        *asked.lock().unwrap(),
        vec![
            (roots[1].clone(), TicketId::from(5)),
            (roots[0].clone(), TicketId::from(6))
        ]
    );
}

#[test]
fn a_project_the_list_does_not_hold_is_never_reached() {
    let never: &Stopper = &|_, _| panic!("nothing is stopped");
    let (home, _) = listed(&["one"]);
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    std::fs::write(elsewhere.path().join(".zaf.toml"), "").expect("manifest");
    let unlisted = serde_json::json!({ "project": elsewhere.path(), "task": 1 }).to_string();

    for body in [unlisted.as_str(), r#"{"project": "three", "task": 1}"#] {
        let answer = run(Some(home.path()), never, body.as_bytes());
        assert_eq!(answer.status, "404 Not Found", "{body}");
        assert!(
            answer.body.contains("not a listed project"),
            "{}",
            answer.body
        );
    }
    let answer = run(None, never, br#"{"project": "one", "task": 1}"#);
    assert_eq!(answer.status, "404 Not Found");
    assert!(answer.body.contains("no zaf home"), "{}", answer.body);
}

#[test]
fn a_name_two_projects_share_is_refused() {
    let never: &Stopper = &|_, _| panic!("nothing is stopped");
    let home = tempfile::tempdir().expect("home");
    for parent in ["x", "y"] {
        let folder = home.path().join(parent).join("same");
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join(".zaf.toml"), "").expect("manifest");
        Projects::add(home.path(), &folder).expect("listed");
    }

    let answer = run(
        Some(home.path()),
        never,
        br#"{"project": "same", "task": 1}"#,
    );

    assert_eq!(answer.status, "404 Not Found");
    assert!(answer.body.contains("more than one"), "{}", answer.body);
}

#[test]
fn a_missing_project_is_refused() {
    let never: &Stopper = &|_, _| panic!("nothing is stopped");
    let (home, roots) = listed(&["gone"]);
    std::fs::remove_dir_all(&roots[0]).expect("removed");

    let answer = run(
        Some(home.path()),
        never,
        br#"{"project": "gone", "task": 1}"#,
    );

    assert_eq!(answer.status, "404 Not Found");
    assert!(answer.body.contains("missing"), "{}", answer.body);
}

#[test]
fn a_refusal_is_a_conflict_and_a_failure_a_server_error() {
    let refused = answer(Err(StopError::NoInstance(TicketId::from(9)).into()));
    assert_eq!(refused.status, "409 Conflict");
    assert_eq!(
        refused.body, r#"{"refused":"no instance is running #9"}"#,
        "one field, in plain words"
    );

    let failed = answer(Err(StopError::Survived {
        task: TicketId::from(9),
        instance: "implement instance 3 on #1".to_owned(),
        pid: 700,
    }
    .into()));
    assert_eq!(failed.status, "500 Internal Server Error");
    assert!(failed.body.contains("did not end"), "{}", failed.body);
}
