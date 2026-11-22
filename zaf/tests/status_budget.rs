//! The call budget of `zaf status` (#502, Q4): at most 6 web calls, plus 2 for each open epic and
//! 5 for each open pull request, over a Forgejo board of 13 open epics. A pull request costs
//! the pull request, its files, its statuses, its reviews and the line comments of its one
//! review; each further review with line comments adds 1 (ruling on #518). Q4's formula is
//! raised by the ruling on #944: each open pull request's task also costs the board 2 more
//! reads (its comments for the claim, its blocks for its epic), so a pull request is 7 here
//! (the board is self-hosted, so the extra reads cost only time). Every web call the
//! Forgejo adapter makes counts but the instance's page-size read, which the harness of
//! `adapters/forgejo/tests.rs` leaves out too and which is made once for the whole process: a
//! test asserts that it is.

mod budget_scene;
mod common;

use budget_scene::{EPICS, page_size_reads, scene, unanswered, web_calls};
use common::outcome;

/// The most web calls `zaf status` may make with `epics` open epics and `pulls` open pull
/// requests, each with one review: Q4's 6 + 2 per epic + 5 per pull request, and 2 more per
/// pull request (ruling on #944).
fn budget(epics: u64, pulls: u64) -> usize {
    usize::try_from(6 + 2 * epics + (5 + 2) * pulls).expect("a count")
}

fn status_calls(pulls: u64) -> Vec<String> {
    let (project, curl) = scene(pulls);
    // The first verb makes the zaf homes and says so; it reads no board.
    let (code, stdout, stderr) = outcome(&project.zaf(&["do"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let (code, stdout, stderr) = outcome(&project.zaf(&["status"]));
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("100"),
        "the screen shows the epics: {stdout}{stderr}"
    );
    assert_eq!(unanswered(&curl.dir), Vec::<String>::new(), "{stderr}");
    assert_eq!(page_size_reads(&curl), 1, "the page size is read once");
    let calls = web_calls(&curl);
    let pull_reads = calls
        .iter()
        .filter(|asked| asked.contains("/pulls"))
        .count();
    println!(
        "status with {pulls} open pull requests: {} web calls, {pull_reads} of them pull request reads",
        calls.len()
    );
    // Each pull request is read: the pull request, its files, its statuses, its reviews and the
    // review's line comments, and the list of the open ones.
    assert!(
        pull_reads >= usize::try_from(4 * pulls).expect("a count"),
        "the pull requests were read: {}",
        calls.join("\n")
    );
    calls
}

#[test]
fn status_budget_no_pulls() {
    let calls = status_calls(0);
    assert!(
        calls.len() <= budget(EPICS, 0),
        "{} calls, at most {}:\n{}",
        calls.len(),
        budget(EPICS, 0),
        calls.join("\n")
    );
}

#[test]
fn status_budget_five_pulls() {
    let calls = status_calls(5);
    assert!(
        calls.len() <= budget(EPICS, 5),
        "{} calls, at most {}:\n{}",
        calls.len(),
        budget(EPICS, 5),
        calls.join("\n")
    );
}
