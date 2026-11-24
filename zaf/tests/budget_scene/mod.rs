//! The scene of the call-budget tests (#502): a Forgejo board of 13 open epics, each with two
//! open tasks, and a number of open pull requests, each with one review that has one line
//! comment, behind the fake `curl` of `tests/fixtures/forgejo/`. The tests count the web calls a
//! read makes, so every endpoint the reads ask is answered the way Forgejo does, and a call no
//! rule answers is a failure of the scene, not a call that cost nothing.

#![allow(dead_code, reason = "each test crate uses its own part of this module")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::{Value, json};

use crate::common::{Curl, OWN_LOGIN, Project, fixtures};

pub const URL: &str = "http://forgejo.test";
pub const REPO: &str = "example/tracker";
pub const API: &str = "http://forgejo.test/api/v1/repos/example/tracker";
pub const TOKEN_VAR: &str = "ZAF_BUDGET_TOKEN";
pub const VERSION: &str = "0.5.3";

/// The open epics the board holds.
pub const EPICS: u64 = 13;

const EPIC_BODY: &str = "Goal: ship the shelf";

/// The head every pull request is at.
const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";

pub const MANIFEST: &str = r#"[project]
name = "example"

[board]
kind = "forgejo-issues"
url = "http://forgejo.test"
repo = "example/tracker"
token_env = "ZAF_BUDGET_TOKEN"

[forge]
kind = "forgejo"
url = "http://forgejo.test"
repo = "example/tracker"
token_env = "ZAF_BUDGET_TOKEN"

[branches]
integration = "feature/{version}"
task = "feature/{version}-{epic}-{task}"

[check.pr]
steps = ["ci"]

[phases]
Task = ["implement", "ci", "review", "merge"]
"#;

/// The four phase actions of the ways repository's own phase list, as far as the reads look at
/// them: each declares its kind and the fact that ends it.
const ACTIONS: &[(&str, &str)] = &[
    ("implement", "model\n# zaf ends: pr-open"),
    ("ci", "wait\n# zaf ends: ci-done\n# zaf on-fail: fix"),
    ("review", "wait\n# zaf ends: exit\n# zaf on-fail: fix"),
    ("merge", "zaf\n# zaf ends: merged\n# zaf on-fail: fix"),
    ("fix", "model\n# zaf ends: pr-open"),
];

const LABEL_IDS: &[(&str, u64)] = &[
    ("task", 1),
    ("epic", 2),
    ("question", 3),
    ("door", 4),
    ("state/ready", 11),
    ("state/claimed", 12),
    ("state/blocked", 13),
    ("state/question", 14),
    ("state/done", 15),
    ("type/Task", 21),
];

fn label(name: &str) -> Value {
    let id = LABEL_IDS
        .iter()
        .find(|(known, _)| *known == name)
        .map(|(_, id)| *id)
        .unwrap_or_else(|| panic!("no test id for label `{name}`"));
    json!({ "id": id, "name": name, "exclusive": name.contains('/') })
}

fn labels(names: &[&str]) -> Value {
    Value::Array(names.iter().map(|name| label(name)).collect())
}

/// The epic's number: epics are #100 to #112.
pub fn epic(index: u64) -> u64 {
    100 + index
}

/// A task of epic `index`: `0` and `1` are done, `2` is the claimed one a pull request is open
/// for (#1000 and up).
pub fn task(index: u64, which: u64) -> u64 {
    1000 + index * 3 + which
}

/// The pull request of epic `index`'s first task.
pub fn pull_number(index: u64) -> u64 {
    2000 + index
}

fn body(blocked_by: Option<u64>) -> String {
    let blocked = blocked_by.map_or_else(|| "none".to_owned(), |task| format!("#{task}"));
    format!(
        "Goal: the export lists the shelf\n\
         Scope: one function in the writer\n\
         Out of scope: the reader\n\
         Interfaces: consumes `Shelf::books`\n\
         Design pages: zaf\n\
         Architecture: app\n\
         Acceptance:\n  - the export lists every book\n\
         Tests: `export_lists_every_book`\n\
         Security trigger: no\n\
         Model: sonnet\n\
         Size: S\n\
         Blocked by: {blocked}\n"
    )
}

fn epic_issue(index: u64) -> Value {
    let number = epic(index);
    json!({
        "id": 5000 + number,
        "number": number,
        "title": format!("epic {number}"),
        "body": EPIC_BODY,
        "labels": labels(&["epic"]),
        "state": "open",
        "html_url": format!("{URL}/{REPO}/issues/{number}"),
        "pull_request": null,
        "created_at": "2026-11-10T08:00:00Z",
    })
}

/// A task of epic `index`: done (closed) for `0` and `1`, claimed and open for `2`.
fn task_issue(index: u64, which: u64) -> Value {
    let number = task(index, which);
    let done = which < 2;
    let state = if done { "state/done" } else { "state/claimed" };
    json!({
        "id": 5000 + number,
        "number": number,
        "title": format!("task {number}"),
        "body": body(None),
        "labels": labels(&["task", state, "type/Task"]),
        "state": if done { "closed" } else { "open" },
        "html_url": format!("{URL}/{REPO}/issues/{number}"),
        "pull_request": null,
        "created_at": "2026-11-11T08:00:00Z",
        "closed_at": if done { json!("2026-11-18T08:00:00Z") } else { Value::Null },
    })
}

/// The tasks epic `index` depends on: its two done ones and, when it has a pull request, the
/// claimed one.
fn tasks_of(index: u64, pulled: bool) -> Vec<Value> {
    let count = if pulled { 3 } else { 2 };
    (0..count).map(|which| task_issue(index, which)).collect()
}

fn pull(index: u64) -> Value {
    let number = pull_number(index);
    json!({
        "number": number,
        "state": "open",
        "merged": false,
        "mergeable": true,
        "html_url": format!("{URL}/{REPO}/pulls/{number}"),
        "created_at": "2026-11-20T08:00:00Z",
        "head": {
            "ref": format!("feature/{VERSION}-{}-{}", epic(index), task(index, 2)),
            "sha": HEAD,
            "repo_id": 9,
        },
        "base": { "ref": format!("feature/{VERSION}"), "sha": "fedcba", "repo_id": 9 },
    })
}

/// What an open epic carries once it is built on: the Engineer's signature of its body, the
/// planning instance's exit and the challenger's report. A signed epic needs no read of its own
/// issue for the link its signer would open.
fn epic_comments(index: u64) -> Value {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(EPIC_BODY.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let number = epic(index);
    json!([
        {
            "id": 100,
            "body": format!(
                "<!-- zaf:signature sha256={hash} at=2026-11-10T09:00:00Z -->\nSigned."
            ),
            "created_at": "2026-11-10T09:00:00Z",
            "user": { "login": OWN_LOGIN },
        },
        {
            "id": 101,
            "body": format!("<!-- zaf:exit planning instance 1 on #{number} done -->\nPlanned."),
            "created_at": "2026-11-10T10:00:00Z",
        },
        {
            "id": 102,
            "body": format!("<!-- zaf:report challenger instance 1 on #{number} -->\nChallenged."),
            "created_at": "2026-11-10T11:00:00Z",
            "user": { "login": OWN_LOGIN },
        },
    ])
}

/// The claim a task's instance left, as the board writes it.
fn claim_comment(index: u64) -> Value {
    json!([{
        "id": 7000 + index,
        "body": format!(
            "claimed by programmer instance 1 on #{} at 2026-11-20T08:00:00Z",
            epic(index)
        ),
        "created_at": "2026-11-20T08:00:00Z",
    }])
}

/// A scene with `pulls` open pull requests: a project whose manifest names the board and the
/// forge, its four phase actions, and the fake `curl` answering every read. The first `pulls`
/// epics each have a claimed task with a pull request; the rest have a ready one.
pub fn scene(pulls: u64) -> (Project, Curl) {
    let mut project = Project::new(MANIFEST);
    let actions = project.root.join(".zaf/actions");
    fs::create_dir_all(&actions).expect("actions folder");
    for (name, kind) in ACTIONS {
        let file = actions.join(name);
        fs::write(
            &file,
            format!(
                "#!/bin/sh\n# zaf description: phase {name}\n# zaf 1: refused\n# zaf target: task\n\
                 # zaf kind: {kind}\nexit 0\n"
            ),
        )
        .expect("action");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).expect("mode");
    }
    // A task's pull request is the one its slice record names: the reads find it there.
    let slices = project.root.join(".zaf/slices");
    fs::create_dir_all(&slices).expect("slices folder");
    for index in 0..pulls {
        fs::write(
            slices.join(format!("{}.pr", task(index, 2))),
            pull_number(index).to_string(),
        )
        .expect("pull request record");
    }
    let mut curl = Curl::new(project.temp.path());
    answer_board(&mut curl, pulls);
    answer_forge(&mut curl, pulls);
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        [fixtures().join("forgejo"), fixtures()]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    project
        .env("PATH", &search_path.to_string_lossy())
        .env("FAKE_CURL_DIR", &curl.dir.to_string_lossy())
        .env(TOKEN_VAR, "test-token")
        .env("ZAF_VERSION", VERSION);
    (project, curl)
}

fn answer_board(curl: &mut Curl, pulls: u64) {
    let json = |value: Value| value.to_string();
    curl.on(&format!("GET {API}/labels?*"), 200, &json(json!([])));
    let epics: Vec<Value> = (0..EPICS).map(epic_issue).collect();
    curl.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=epic*"),
        200,
        &json(Value::Array(epics)),
    );
    // Only the tasks with a pull request are open, and they are claimed: the claims list them by
    // their state label alone, the ready list (with `task` too) is empty, and the open tickets
    // are listed by `task` alone.
    let claimed: Vec<Value> = (0..pulls).map(|index| task_issue(index, 2)).collect();
    curl.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=state%2Fclaimed&*"),
        200,
        &json(Value::Array(claimed.clone())),
    );
    curl.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=task&*"),
        200,
        &json(Value::Array(claimed)),
    );
    // Nothing is ready, blocked or asking, and no question or door is open.
    curl.on(
        &format!("GET {API}/issues?state=open&type=issues&labels=*"),
        200,
        &json(json!([])),
    );
    for index in 0..EPICS {
        curl.on(
            &format!("GET {API}/issues/{}/dependencies?*", epic(index)),
            200,
            &json(Value::Array(tasks_of(index, index < pulls))),
        );
        curl.on(
            &format!("GET {API}/issues/{}/comments*", epic(index)),
            200,
            &json(epic_comments(index)),
        );
        // An epic blocks nothing.
        curl.on(
            &format!("GET {API}/issues/{}/blocks?*", epic(index)),
            200,
            &json(json!([])),
        );
        curl.on(
            &format!("GET {API}/issues/{}", epic(index)),
            200,
            &json(epic_issue(index)),
        );
        for which in 0..3 {
            curl.on(
                &format!("GET {API}/issues/{}", task(index, which)),
                200,
                &json(task_issue(index, which)),
            );
            curl.on(
                &format!("GET {API}/issues/{}/blocks?*", task(index, which)),
                200,
                &json(json!([epic_issue(index)])),
            );
            let comments = if which == 2 {
                claim_comment(index)
            } else {
                json!([])
            };
            curl.on(
                &format!("GET {API}/issues/{}/comments*", task(index, which)),
                200,
                &json(comments),
            );
        }
    }
}

fn answer_forge(curl: &mut Curl, pulls: u64) {
    let json = |value: Value| value.to_string();
    let open: Vec<Value> = (0..pulls).map(pull).collect();
    curl.on(
        &format!("GET {API}/pulls?state=open*"),
        200,
        &json(Value::Array(open)),
    );
    for index in 0..pulls {
        let number = pull_number(index);
        curl.on(
            &format!("GET {API}/pulls/{number}"),
            200,
            &json(pull(index)),
        );
        curl.on(
            &format!("GET {API}/pulls/{number}/files?*"),
            200,
            &json(json!([{ "filename": "src/writer.rs" }])),
        );
        curl.on(
            &format!("GET {API}/commits/{HEAD}/statuses?*"),
            200,
            &json(json!([{
                "id": 1,
                "status": "success",
                "context": "ci",
                "target_url": "/example/tracker/actions/runs/4/jobs/0",
                "created_at": "2026-11-20T09:00:00Z",
            }])),
        );
        curl.on(
            &format!("GET {API}/pulls/{number}/reviews?*"),
            200,
            &json(json!([{
                "id": 30 + index,
                "user": { "login": "reviewer" },
                "body": "Looks right.",
                "state": "COMMENT",
                "commit_id": HEAD,
                "submitted_at": "2026-11-20T09:30:00Z",
                "comments_count": 1,
            }])),
        );
        curl.on(
            &format!("GET {API}/pulls/{number}/reviews/{}/comments*", 30 + index),
            200,
            &json(json!([{
                "user": { "login": "reviewer" },
                "body": "A nit.",
                "path": "src/writer.rs",
                "position": 3,
                "commit_id": HEAD,
                "created_at": "2026-11-20T09:31:00Z",
            }])),
        );
        curl.on(
            &format!("GET {API}/issues/{number}/comments*"),
            200,
            &json(json!([])),
        );
    }
    curl.on(
        &format!("GET {API}/branches/feature*"),
        200,
        &json(json!({ "commit": { "id": "fedcba", "timestamp": "2026-11-20T07:00:00Z" } })),
    );
    curl.on(
        &format!("GET {API}/commits/fedcba/statuses?*"),
        200,
        &json(json!([])),
    );
}

/// Every web call the reads made but the two that ask the instance, not the project, and are
/// made once for the process: the instance's page-size read ([`page_size_reads`]), as the
/// fake-curl harness of `adapters/forgejo/tests.rs` leaves it out, and the read of the login the
/// token posts as ([`own_login_reads`]), whose signatures and reports the board trusts.
pub fn web_calls(curl: &Curl) -> Vec<String> {
    all_calls(curl)
        .into_iter()
        .filter(|asked| !asked.ends_with(PAGE_SIZE) && !asked.ends_with(OWN_LOGIN_READ))
        .collect()
}

const PAGE_SIZE: &str = "/settings/api";
const OWN_LOGIN_READ: &str = "/api/v1/user";

/// The reads of the instance's page size: one for the whole process, board and forge alike.
pub fn page_size_reads(curl: &Curl) -> usize {
    let calls = all_calls(curl);
    calls
        .iter()
        .filter(|asked| asked.ends_with(PAGE_SIZE))
        .count()
}

/// The reads of the login the token posts as: one for the whole process.
pub fn own_login_reads(curl: &Curl) -> usize {
    let calls = all_calls(curl);
    calls
        .iter()
        .filter(|asked| asked.ends_with(OWN_LOGIN_READ))
        .count()
}

fn all_calls(curl: &Curl) -> Vec<String> {
    let Ok(entries) = fs::read_dir(curl.dir.join("calls")) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .map(|entry| entry.expect("call").path())
        .filter(|path| path.extension().is_none())
        .collect();
    files.sort();
    files
        .iter()
        .map(|file| {
            let argv: Vec<String> = fs::read_to_string(file)
                .expect("call")
                .split_terminator('\0')
                .map(str::to_owned)
                .collect();
            let method = argv
                .iter()
                .position(|arg| arg == "--request")
                .map_or("GET", |at| argv[at + 1].as_str());
            format!("{method} {}", argv.last().expect("a URL"))
        })
        .collect()
}

/// The calls no rule of the scene answered: the fake `curl` notes each in `unmatched`.
pub fn unanswered(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("unmatched"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}
