//! `zaf hub --port <n>` through the built binary: a port that is taken is refused by name.

mod common;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::{Project, Running, outcome};

const MANIFEST: &str = "[project]\nname = \"demo\"\n[board]\nkind = \"github-issues\"\n\
    repo = \"example/tracker\"\n[forge]\nkind = \"github\"\nrepo = \"example/tracker\"\n[vcs]\nkind = \"jj\"\n";

#[test]
fn a_used_port_exits_1_naming_the_port() {
    let held = TcpListener::bind("0.0.0.0:0").expect("binds");
    let port = held.local_addr().expect("address").port().to_string();
    let project = project(MANIFEST);
    let (code, stdout, stderr) = outcome(&project.zaf(&["hub", "--port", &port]));
    assert_eq!(code, 1, "{stderr}");
    assert!(stdout.is_empty(), "nothing is served: {stdout}");
    assert!(
        stderr.contains(&format!("port {port} is already in use")),
        "{stderr}"
    );
}

/// A project whose hub reads no transcript of the machine it is tested on: the default Claude
/// config dir is an empty folder of the test's own, so the ledger's fill finds nothing and takes
/// no time, whatever the machine holds.
fn project(manifest: &str) -> Project {
    let mut project = Project::new(manifest);
    let config = project.temp.path().join("claude-config");
    std::fs::create_dir_all(&config).expect("config dir");
    project.env("CLAUDE_CONFIG_DIR", config.to_str().expect("utf-8"));
    project
}

/// Waits until the hub has stopped writing under the project and its zaf home: the ledger is
/// there (the hub makes it once its URL is out) and nothing has changed for a moment.
fn settle(project: &Project) {
    let read = || (files_under(&project.root), files_under(&project.home));
    let mut before = read();
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(200));
        let now = read();
        if now == before && project.home.join("ledger.db").exists() {
            return;
        }
        before = now;
    }
    panic!("the hub never settled");
}

/// One request to the hub at `port` on the loopback: the whole answer.
fn http(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connects");
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .expect("timeout");
    stream.write_all(request.as_bytes()).expect("writes");
    let mut answer = String::new();
    stream.read_to_string(&mut answer).expect("reads");
    answer
}

#[test]
fn api_state_holds_every_project() {
    let mut first = project(MANIFEST);
    // The board of the project the hub runs in is empty.
    first.on("issue list *", "[]");
    let mut second = project(&MANIFEST.replace("demo", "other"));
    // Both share the first's zaf home, and so its list of projects.
    second.env("ZAF_HOME", &first.home.to_string_lossy());
    outcome(&first.zaf(&["status"]));
    outcome(&second.zaf(&["status"]));

    let hub = Running::start(first.command_in(&first.root, &["hub", "--port", "0"]));
    hub.wait_for("printed its URL", |out, _| out.contains("http://"));
    let url = hub.stdout();
    let port: u16 = url
        .trim()
        .rsplit_once(':')
        .and_then(|(_, port)| port.trim_end_matches('/').parse().ok())
        .unwrap_or_else(|| panic!("a URL with a port: {url}"));

    let answer = http(
        port,
        "GET /api/state HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n",
    );
    assert!(answer.starts_with("HTTP/1.1 200 OK"), "{answer}");
    assert!(
        answer.contains("Content-Type: application/json"),
        "{answer}"
    );
    let (_, body) = answer.split_once("\r\n\r\n").expect("a body");
    let state: serde_json::Value = serde_json::from_str(body).expect("JSON");
    assert_eq!(state["machine"]["projects"], 2, "{state}");
    assert!(state["next"].is_array(), "{state}");
    let names: Vec<_> = state["projects"]
        .as_array()
        .expect("projects")
        .iter()
        .map(|project| project["project"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, ["demo", "other"], "every project, as they joined");

    let refused = http(
        port,
        "POST /api/state HTTP/1.1\r\nHost: zaf\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    );
    assert!(refused.starts_with("HTTP/1.1 405 "), "{refused}");
    assert!(refused.ends_with("{\"refused\":\"GET only\"}"), "{refused}");
}

/// Starts `zaf hub --port 0 <extra>` in `project`, which has an empty board, and returns it with
/// the port it serves on. Its output is a pipe, so it is not a terminal.
fn serve(project: &mut Project, extra: &[&str]) -> (Running, u16) {
    project.on("issue list *", "[]");
    let mut args = vec!["hub", "--port", "0"];
    args.extend_from_slice(extra);
    let hub = Running::start(project.command_in(&project.root, &args));
    hub.wait_for("printed its URL", |out, _| out.contains("http://"));
    let url = hub.stdout();
    let port = url
        .lines()
        .next()
        .and_then(|line| line.trim().rsplit_once(':'))
        .and_then(|(_, port)| port.trim_end_matches('/').parse().ok())
        .unwrap_or_else(|| panic!("a URL with a port: {url}"));
    (hub, port)
}

/// A `POST` to `path` with a small body, with `key` in the key header when there is one.
fn post(port: u16, path: &str, key: Option<&str>) -> String {
    let header = key.map_or_else(String::new, |key| format!("X-Zaf-Key: {key}\r\n"));
    http(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: zaf\r\n{header}Content-Length: 2\r\n\
             Connection: close\r\n\r\n{{}}"
        ),
    )
}

/// Every file under `dir` with what it holds.
fn files_under(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut found = BTreeMap::new();
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).expect("reads").flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let bytes = std::fs::read(&path).unwrap_or_default();
                found.insert(path, bytes);
            }
        }
    }
    found
}

/// Whether `text` holds a run of 64 hex digits, which is what a hub's key looks like.
fn holds_a_key(text: &str) -> bool {
    text.as_bytes()
        .split(|byte| !byte.is_ascii_hexdigit())
        .any(|run| run.len() >= 64)
}

#[test]
fn a_control_with_no_key_or_a_wrong_one_is_refused_and_changes_nothing() {
    let mut project = project(MANIFEST);
    let (hub, port) = serve(&mut project, &[]);
    // Reading is open, and the hub has settled whatever it writes when it reads.
    let state = http(
        port,
        "GET /api/state HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n",
    );
    assert!(state.contains("\"controls\":true"), "{state}");
    settle(&project);
    let before = (files_under(&project.root), files_under(&project.home));
    for control in ["/api/tick", "/api/answer", "/api/watch", "/api/nothing"] {
        for key in [None, Some("wrong"), Some("")] {
            let answer = post(port, control, key);
            assert!(answer.starts_with("HTTP/1.1 403 Forbidden"), "{answer}");
            assert!(
                answer.ends_with(
                    "{\"refused\":\"a control needs the key this hub made when it started, \
                     in the X-Zaf-Key header\"}"
                ),
                "{answer}"
            );
        }
    }
    assert_eq!(
        (files_under(&project.root), files_under(&project.home)),
        before,
        "the board's folders and the runs are as they were"
    );
    // The hub started with no terminal: neither stream holds a key.
    assert!(!holds_a_key(&hub.stdout()), "{}", hub.stdout());
    assert!(!holds_a_key(&hub.stderr()), "{}", hub.stderr());
}

#[test]
fn read_only_refuses_every_control_and_says_so() {
    let mut project = project(MANIFEST);
    let (_hub, port) = serve(&mut project, &["--read-only"]);
    let state = http(
        port,
        "GET /api/state HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n",
    );
    assert!(state.starts_with("HTTP/1.1 200 OK"), "{state}");
    let (_, body) = state.split_once("\r\n\r\n").expect("a body");
    let state: serde_json::Value = serde_json::from_str(body).expect("JSON");
    assert_eq!(state["controls"], false, "{state}");
    // The page is still served, since reading stays open.
    let page = http(
        port,
        "GET / HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n",
    );
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    for key in [None, Some("wrong")] {
        let answer = post(port, "/api/tick", key);
        assert!(answer.starts_with("HTTP/1.1 403 Forbidden"), "{answer}");
        assert!(answer.contains("read-only"), "{answer}");
    }
}

#[test]
fn read_only_is_for_a_served_page() {
    let project = project(MANIFEST);
    let (code, _, stderr) = outcome(&project.zaf(&["hub", "--read-only"]));
    assert_eq!(
        code, 2,
        "a terminal frame has no controls to refuse: {stderr}"
    );
}

#[test]
fn a_hub_run_leaves_no_key_in_a_file() {
    let mut project = project(MANIFEST);
    let (hub, port) = serve(&mut project, &[]);
    for request in [
        "GET / HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n",
        "GET /api/state HTTP/1.1\r\nHost: zaf\r\nConnection: close\r\n\r\n",
    ] {
        let answer = http(port, request);
        assert!(!holds_a_key(&answer), "{answer}");
    }
    assert!(post(port, "/api/tick", Some("wrong")).contains("403"));
    drop(hub);
    // Nothing the run left in the zaf home, the project's .zaf/ or the logs looks like a key.
    for (path, bytes) in files_under(&project.root)
        .into_iter()
        .chain(files_under(&project.home))
    {
        assert!(
            !holds_a_key(&String::from_utf8_lossy(&bytes)),
            "{} holds a key",
            path.display()
        );
    }
}

#[test]
fn a_port_is_not_taken_with_the_hook() {
    let project = project(MANIFEST);
    let (code, _, stderr) = outcome(&project.zaf(&["hub", "report", "--port", "0"]));
    assert_eq!(code, 2, "the report verb has no port: {stderr}");
}
