use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::super::dashboard::{Dashboard, Page, StateReader};
use super::super::state::gather_machine;
use super::*;
use crate::app::projects::Projects;

/// A hub serving on a free port with a guard of its own, over a zaf home whose list holds one
/// project folder, `proj`, with the folders a project keeps. Returns the page, the port and the
/// key, which the test takes before the guard moves into the server.
struct Hub {
    page: Page,
    port: u16,
    key: String,
    home: tempfile::TempDir,
}

fn start_hub(read_only: bool) -> Hub {
    let home = tempfile::tempdir().expect("home");
    let project = home.path().join("proj");
    for folder in [".zaf/logs", ".zaf/runs", ".zaf/hub"] {
        std::fs::create_dir_all(project.join(folder)).expect("folder");
    }
    std::fs::write(project.join(".zaf/runs/note"), "a run folder\n").expect("file");
    Projects::add(home.path(), &project).expect("listed");
    let root = home.path().to_owned();
    let reader: Arc<StateReader> = Arc::new(move || Ok(gather_machine(&root, SystemTime::now())));
    let guard = Guard::start(read_only).expect("a key");
    let key = guard.key.reveal().to_owned();
    let dashboard = Dashboard::bind(0).expect("binds");
    let port = dashboard.port();
    Hub {
        page: dashboard.start("frame\n", reader, guard),
        port,
        key,
        home,
    }
}

impl Hub {
    /// Sends `request` as it is and reads the whole answer.
    fn ask(&self, request: &str) -> String {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).expect("connects");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("timeout");
        stream.write_all(request.as_bytes()).expect("writes");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("reads");
        answer
    }

    fn get(&self, target: &str) -> String {
        self.ask(&format!(
            "GET {target} HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n"
        ))
    }

    /// A `POST` to `target` with a small body, carrying `key` in the key header when there is one.
    fn post(&self, target: &str, key: Option<&str>) -> String {
        let header = key.map_or_else(String::new, |key| format!("{KEY_HEADER}: {key}\r\n"));
        self.ask(&format!(
            "POST {target} HTTP/1.1\r\nHost: zaf\r\n{header}Content-Length: 2\r\n\
             Connection: close\r\n\r\n{{}}"
        ))
    }
}

fn status_of(answer: &str) -> &str {
    answer.lines().next().expect("a status line")
}

fn json_of(answer: &str) -> serde_json::Value {
    let (_, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
    serde_json::from_str(body).expect("JSON")
}

/// Every file under `dir` with what it holds.
fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).expect("reads the folder").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, into);
            } else {
                into.insert(
                    path.display().to_string(),
                    std::fs::read(&path).unwrap_or_default(),
                );
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(dir, &mut files);
    files
}

const CONTROLS: [&str; 4] = [
    "/api/answer",
    "/api/tick",
    "/api/stop",
    "/api/nothing-built-yet",
];

#[test]
fn right_key_passes() {
    let key = Key::generate().expect("a key");
    assert!(key.check(Some(key.reveal())));
    let guard = Guard::new(key, false);
    let right = guard.key.reveal().to_owned();
    assert_eq!(guard.admit(Some(&right)), Ok(()));

    // Through the server: the guard lets the request on to its route, and no control is built yet,
    // so it is an unknown route, as any path under /api/ that nobody listed.
    let hub = start_hub(false);
    for control in CONTROLS {
        let answer = hub.post(control, Some(&hub.key));
        assert_eq!(status_of(&answer), "HTTP/1.1 404 Not Found", "{control}");
        assert_eq!(
            json_of(&answer),
            serde_json::json!({ "refused": "not found" })
        );
    }
    // The header's name is not case-sensitive, as no header's is.
    let lower = hub.ask(&format!(
        "POST /api/tick HTTP/1.1\r\nHost: zaf\r\nx-zaf-key:   {}  \r\nContent-Length: 0\r\n\
         Connection: close\r\n\r\n",
        hub.key
    ));
    assert_eq!(status_of(&lower), "HTTP/1.1 404 Not Found", "{lower}");
}

#[test]
fn wrong_key_refused() {
    let key = Key::generate().expect("a key");
    let right = key.reveal().to_owned();
    let mut off_by_one = right.clone();
    off_by_one.pop();
    off_by_one.push(if right.ends_with('0') { '1' } else { '0' });
    for wrong in [
        "",
        "wrong",
        &off_by_one,
        &right[..right.len() - 1],
        &format!("{right}0"),
        &right.to_uppercase(),
        &format!(" {right}x"),
    ] {
        assert!(!key.check(Some(wrong)), "{wrong:?}");
    }
    let guard = Guard::new(key, false);
    assert_eq!(guard.admit(Some("wrong")), Err(Refusal::Key));
    // Two keys are two keys: another hub's is not this one's.
    assert!(
        !guard
            .key
            .check(Some(Key::generate().expect("another").reveal()))
    );

    // Through the server: 403 in the refusal's shape, for a control that exists in no list, and
    // the folders are as they were.
    let hub = start_hub(false);
    let before = tree(hub.home.path());
    for control in CONTROLS {
        for wrong in ["wrong", off_by_one.as_str(), ""] {
            let answer = hub.post(control, Some(wrong));
            assert_eq!(status_of(&answer), "HTTP/1.1 403 Forbidden", "{control}");
            assert!(
                answer.contains("Content-Type: application/json"),
                "{answer}"
            );
            let json = json_of(&answer);
            assert_eq!(
                json.as_object()
                    .expect("an object")
                    .keys()
                    .collect::<Vec<_>>(),
                ["refused"],
                "one field"
            );
            assert!(
                !answer.contains(&hub.key),
                "a refusal does not say what the key is: {answer}"
            );
        }
    }
    assert_eq!(tree(hub.home.path()), before, "the folders did not change");
}

#[test]
fn missing_key_refused() {
    let key = Key::generate().expect("a key");
    assert!(!key.check(None));
    let guard = Guard::new(key, false);
    assert_eq!(guard.admit(None), Err(Refusal::Key));

    let hub = start_hub(false);
    let before = tree(hub.home.path());
    for control in CONTROLS {
        let answer = hub.post(control, None);
        assert_eq!(status_of(&answer), "HTTP/1.1 403 Forbidden", "{control}");
        assert!(
            json_of(&answer)["refused"]
                .as_str()
                .is_some_and(|why| why.contains(KEY_HEADER)),
            "it names the header the key goes in: {answer}"
        );
    }
    // The key is not taken from the URL, where a log would keep it.
    let in_query = hub.post(&format!("/api/tick?key={}", hub.key), None);
    assert_eq!(status_of(&in_query), "HTTP/1.1 403 Forbidden", "{in_query}");
    // And a read needs none.
    assert_eq!(status_of(&hub.get("/api/state")), "HTTP/1.1 200 OK");
    assert_eq!(status_of(&hub.get("/")), "HTTP/1.1 200 OK");
    assert_eq!(tree(hub.home.path()), before, "the folders did not change");
}

#[test]
fn read_only_refuses_all() {
    let key = Key::generate().expect("a key");
    let right = key.reveal().to_owned();
    let guard = Guard::new(key, true);
    assert!(!guard.controls());
    for presented in [Some(right.as_str()), Some("wrong"), None] {
        assert_eq!(
            guard.admit(presented),
            Err(Refusal::ReadOnly),
            "{presented:?}"
        );
    }
    assert_eq!(guard.link("http://zaf/"), None, "nothing to open");

    // Through the server: the right key is refused too, and the state says there are no controls.
    let hub = start_hub(true);
    let before = tree(hub.home.path());
    for control in CONTROLS {
        for key in [Some(hub.key.as_str()), Some("wrong"), None] {
            let answer = hub.post(control, key);
            assert_eq!(
                status_of(&answer),
                "HTTP/1.1 403 Forbidden",
                "{control} {key:?}"
            );
            assert_eq!(
                json_of(&answer),
                serde_json::json!({ "refused": Refusal::ReadOnly.to_string() })
            );
        }
    }
    let state = json_of(&hub.get("/api/state"));
    assert_eq!(state["controls"], false, "{state}");
    assert_eq!(tree(hub.home.path()), before, "the folders did not change");
    // A hub that serves controls says so.
    assert_eq!(
        json_of(&start_hub(false).get("/api/state"))["controls"],
        true
    );
}

#[test]
fn key_never_written() {
    let hub = start_hub(false);
    // Everything the hub says, to a reader and to a request it refuses.
    let mut seen = vec![
        hub.get("/"),
        hub.get("/api/state"),
        hub.get("/missing"),
        hub.get("/api/missing"),
    ];
    for control in CONTROLS {
        seen.push(hub.post(control, None));
        seen.push(hub.post(control, Some("wrong")));
    }
    hub.page.publish("a frame after the requests\n");
    seen.push(hub.get("/"));
    for answer in &seen {
        assert!(
            !answer.contains(&hub.key),
            "an answer holds the key: {answer}"
        );
    }
    assert!(
        !format!("{:?}", hub.page).contains(&hub.key),
        "a debug print of the server holds the key"
    );
    assert_eq!(
        format!("{:?}", Key::generate().expect("a key")),
        "Key(..)",
        "a key's debug print hides it"
    );

    // And nothing it keeps: not the zaf home, not the project's .zaf/ with its logs and run
    // folders, whatever the hub or a request wrote there.
    let files = tree(hub.home.path());
    assert!(
        files.keys().any(|path| path.ends_with("runs/note")),
        "the folders were read"
    );
    for (path, bytes) in &files {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains(&hub.key), "{path} holds the key");
    }
    // The key comes only from the system's random source: two hubs do not share one.
    assert_ne!(hub.key, start_hub(false).key);
    assert_eq!(hub.key.len(), 64);
}

#[test]
fn the_link_carries_the_key_in_its_fragment() {
    let guard = Guard::start(false).expect("a key");
    let link = guard.link("http://192.168.7.20:7777/").expect("a link");
    assert_eq!(
        link,
        format!("http://192.168.7.20:7777/#key={}", guard.key.reveal())
    );
    assert_eq!(
        guard.link("http://zaf/#"),
        Some(format!("http://zaf/#key={}", guard.key.reveal())),
        "an empty fragment is replaced, not doubled"
    );
}
