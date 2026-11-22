//! The call budget of the chair's first read of a project (#502, Q4): at most 12 web calls, plus
//! 2 for each open epic and 5 for each open pull request, over a Forgejo board of 13 open epics.
//! The chair's gatherer reads a project with `gather_project`, which is run here as the gatherer
//! runs it, over the fake `curl`. A pull request costs the pull request, its files, its
//! statuses, its reviews and the line comments of its one review; each further review with line
//! comments adds 1 (ruling on #518). Q4's formula is raised by the ruling on #944: each open
//! pull request's task also costs the board 4 more reads, so a pull request is 9 here. Every web
//! call the Forgejo adapter makes counts but the
//! instance's page-size read, which the harness of `adapters/forgejo/tests.rs` leaves out too and
//! which is made once for the whole process: a test asserts that it is.

mod budget_scene;
mod common;

use std::sync::Mutex;
use std::time::SystemTime;

use budget_scene::{EPICS, TOKEN_VAR, VERSION, page_size_reads, scene, unanswered, web_calls};
use zaf::app::hub::state::gather_project;

/// The environment is the process's: one read at a time sets it.
static ONE_READ: Mutex<()> = Mutex::new(());

/// The most web calls the chair's first read may make with `epics` open epics and `pulls` open
/// pull requests, each with one review: Q4's 12 + 2 per epic + 5 per pull request, and 4 more
/// per pull request (ruling on #944: the task's comments, its blocks, `issues/N` and a second
/// blocks).
fn budget(epics: u64, pulls: u64) -> usize {
    usize::try_from(12 + 2 * epics + (5 + 4) * pulls).expect("a count")
}

fn first_read_calls(pulls: u64) -> Vec<String> {
    let _one = ONE_READ
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let (project, curl) = scene(pulls);
    let fixtures = common::fixtures();
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let search_path = std::env::join_paths(
        [fixtures.join("forgejo"), fixtures]
            .into_iter()
            .chain(std::env::split_paths(&inherited)),
    )
    .expect("search path");
    // SAFETY: the lock above lets one test at a time change the environment, and no thread of
    // this test reads it while it does.
    unsafe {
        std::env::set_var("PATH", search_path);
        std::env::set_var("FAKE_CURL_DIR", &curl.dir);
        std::env::set_var("ZAF_HOME", &project.home);
        std::env::set_var(TOKEN_VAR, "test-token");
        std::env::set_var("ZAF_VERSION", VERSION);
        for var in ["ZAF_INSTANCE", "ZAF_RUN", "ZAF_ROOT"] {
            std::env::remove_var(var);
        }
    }
    let state = gather_project(&project.root, SystemTime::now());
    assert_eq!(state.reason, None, "{:?}", state.problems);
    assert_eq!(state.epics.len(), usize::try_from(EPICS).expect("a count"));
    assert_eq!(unanswered(&curl.dir), Vec::<String>::new());
    assert_eq!(page_size_reads(&curl), 1, "the page size is read once");
    let calls = web_calls(&curl);
    let pull_reads = calls
        .iter()
        .filter(|asked| asked.contains("/pulls"))
        .count();
    println!(
        "the chair's first read with {pulls} open pull requests: {} web calls, {pull_reads} of \
         them pull request reads",
        calls.len()
    );
    // Each pull request is read: the pull request, its files, its statuses, its reviews and the
    // review's line comments.
    assert!(
        pull_reads >= usize::try_from(4 * pulls).expect("a count"),
        "the pull requests were read: {}",
        calls.join("\n")
    );
    calls
}

#[test]
fn chair_budget_no_pulls() {
    let calls = first_read_calls(0);
    assert!(
        calls.len() <= budget(EPICS, 0),
        "{} calls, at most {}:\n{}",
        calls.len(),
        budget(EPICS, 0),
        calls.join("\n")
    );
}

#[test]
fn chair_budget_five_pulls() {
    let calls = first_read_calls(5);
    assert!(
        calls.len() <= budget(EPICS, 5),
        "{} calls, at most {}:\n{}",
        calls.len(),
        budget(EPICS, 5),
        calls.join("\n")
    );
}
