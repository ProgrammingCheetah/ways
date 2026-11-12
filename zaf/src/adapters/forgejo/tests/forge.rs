//! The Forgejo forge against the fake `curl`: a pull request and its head, checks from commit
//! statuses, reviews, the hand-off comment, opening and merging, and what `zaf doctor` asks.

use super::*;
use crate::adapters::notes::{HANDOFF_MARKER, marked};
use crate::domain::Location;
use crate::ports::{
    BranchName, CheckBucket, CheckLink, CheckLog, CheckName, CheckRun, CheckState, CommitId, Forge,
    MergeOutcome, MergeStrategy, NewPullRequest, Opened, PrBody, PrNumber, PrTitle,
};

/// The pull request's head commit.
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
/// The branch the pull request carries.
const HEAD: &str = "feature/v1-1-2";
/// The repository's id, which a pull request from a fork does not share.
const REPO_ID: u64 = 9;

fn the_forge(fake: &FakeCurl) -> ForgejoForge {
    ForgejoForge::from(fake.api())
}

fn pr(number: u64) -> PrNumber {
    PrNumber::from(number)
}

/// A pull request as Forgejo prints it, from `head` at [`SHA`] into `main`.
fn pull(number: u64, head: &str, state: &str, merged: bool) -> Value {
    pull_from(number, head, REPO_ID, state, merged)
}

fn pull_from(number: u64, head: &str, repo_id: u64, state: &str, merged: bool) -> Value {
    json!({
        "number": number,
        "state": state,
        "merged": merged,
        "created_at": "2026-11-12T10:00:00+01:00",
        "merged_at": if merged { json!("2026-11-12T11:00:00Z") } else { Value::Null },
        "head": { "ref": head, "sha": SHA, "repo_id": repo_id },
        "base": { "ref": "main", "sha": "fedcba", "repo_id": REPO_ID },
    })
}

/// A commit status as Forgejo prints it.
fn status(id: u64, context: &str, state: &str, link: &str) -> Value {
    json!({
        "id": id,
        "status": state,
        "context": context,
        "target_url": link,
        "created_at": format!("2026-11-12T10:{:02}:00Z", 10 + id),
    })
}

/// Pull request 7, open at [`SHA`].
fn open_seven(fake: &FakeCurl) {
    fake.on(
        &format!("GET {API}/pulls/7"),
        200,
        &pull(7, HEAD, "open", false),
    );
}

fn request() -> NewPullRequest {
    NewPullRequest {
        head: BranchName::from(HEAD),
        base: BranchName::from("feature/v1"),
        title: PrTitle::from("Export the list"),
        body: PrBody::from("Closes #2"),
    }
}

#[test]
fn checks_are_the_latest_status_of_each_context_at_the_head() {
    let fake = FakeCurl::new();
    open_seven(&fake);
    fake.on(
        &format!("GET {API}/commits/{SHA}/statuses*"),
        200,
        &json!([
            status(1, "ci / test (push)", "pending", ""),
            status(2, "ci / lint (push)", "failure", ""),
            status(
                3,
                "ci / test (push)",
                "success",
                "/example/tracker/actions/runs/4/jobs/0"
            ),
            status(4, "external", "warning", "https://ci.example.com/builds/5"),
        ]),
    );
    let forge = the_forge(&fake);

    let checks = forge.checks(pr(7)).expect("checks");
    let read: Vec<(&str, CheckState, bool)> = checks
        .iter()
        .map(|check| {
            (
                check.name.as_str(),
                check.state,
                check.finished_at.is_some(),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            ("ci / lint (push)", CheckState::Failed, true),
            ("ci / test (push)", CheckState::Passed, true),
            ("external", CheckState::Passed, true),
        ]
    );
    assert_eq!(checks[1].finished_at, Some(at(1_794_478_380)));

    let runs = forge.check_runs(pr(7)).expect("runs");
    assert_eq!(
        runs,
        [
            CheckRun {
                bucket: CheckBucket::Fail,
                name: CheckName::from("ci / lint (push)"),
                link: None,
            },
            CheckRun {
                bucket: CheckBucket::Pass,
                name: CheckName::from("ci / test (push)"),
                link: Some(CheckLink::from(
                    "http://forgejo.test/example/tracker/actions/runs/4/jobs/0"
                )),
            },
            CheckRun {
                bucket: CheckBucket::Pass,
                name: CheckName::from("external"),
                link: Some(CheckLink::from("https://ci.example.com/builds/5")),
            },
        ]
    );
    assert!(
        fake.asked()
            .iter()
            .any(|asked| asked.starts_with(&format!("GET {API}/commits/{SHA}/statuses?page=1"))),
        "{:?}",
        fake.asked()
    );
}

#[test]
fn a_pending_check_has_no_finish_and_an_unknown_status_is_a_backend_failure() {
    let fake = FakeCurl::new();
    open_seven(&fake);
    fake.once(
        &format!("GET {API}/commits/{SHA}/statuses*"),
        200,
        &json!([status(1, "ci", "pending", "")]),
    );
    fake.on(
        &format!("GET {API}/commits/{SHA}/statuses*"),
        200,
        &json!([status(1, "ci", "skipped", "")]),
    );
    let forge = the_forge(&fake);
    let checks = forge.checks(pr(7)).expect("checks");
    assert_eq!(checks[0].state, CheckState::Pending);
    assert_eq!(checks[0].finished_at, None);
    let error = forge.check_runs(pr(7)).expect_err("unknown status");
    assert!(
        matches!(&error, PortError::Backend(message) if message.contains("`skipped`")),
        "{error:?}"
    );
}

#[test]
fn no_statuses_is_no_checks() {
    let fake = FakeCurl::new();
    open_seven(&fake);
    fake.on(
        &format!("GET {API}/commits/{SHA}/statuses*"),
        200,
        &json!([]),
    );
    assert_eq!(the_forge(&fake).check_runs(pr(7)), Ok(Vec::new()));
}

#[test]
fn a_pull_request_reads_its_times_its_head_and_whether_it_merged() {
    let fake = FakeCurl::new();
    open_seven(&fake);
    fake.on(
        &format!("GET {API}/pulls/8"),
        200,
        &pull(8, "feature/v1-1-3", "closed", true),
    );
    fake.on(
        &format!("GET {API}/pulls/9"),
        404,
        &json!({ "message": "The target couldn't be found." }),
    );
    let forge = the_forge(&fake);

    let open = forge.pull_request(pr(7)).expect("open");
    // 10:00 at +01:00 is 09:00 UTC.
    assert_eq!(open.opened_at, at(1_794_474_000));
    assert_eq!(open.merged_at, None);
    let merged = forge.pull_request(pr(8)).expect("merged");
    assert_eq!(merged.merged_at, Some(at(1_794_481_200)));

    assert_eq!(forge.head(pr(7)), Ok(Some(SHA.to_owned())));
    assert_eq!(forge.merged(pr(7)), Ok(false));
    assert_eq!(forge.merged(pr(8)), Ok(true));
    assert!(matches!(forge.head(pr(9)), Err(PortError::NotFound(_))));
}

#[test]
fn review_comments_are_posted_reviews_and_their_line_comments_each_with_its_commit() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/pulls/7/reviews[?]*"),
        200,
        &json!([
            {
                "id": 1,
                "user": { "login": "reviewer" },
                "body": "Two things.",
                "state": "REQUEST_CHANGES",
                "commit_id": SHA,
                "submitted_at": "2026-11-12T10:10:00Z",
                "comments_count": 1,
            },
            {
                "id": 2,
                "user": { "login": "reviewer" },
                "body": "a draft",
                "state": "PENDING",
                "commit_id": SHA,
                "submitted_at": null,
                "comments_count": 3,
            },
            {
                "id": 3,
                "user": null,
                "body": "",
                "state": "APPROVED",
                "commit_id": SHA,
                "submitted_at": "2026-11-12T10:20:00Z",
                "comments_count": 0,
            },
        ]),
    );
    fake.on(
        &format!("GET {API}/pulls/7/reviews/1/comments"),
        200,
        &json!([{
            "user": { "login": "reviewer" },
            "body": "Off by one.",
            "path": "src/export.rs",
            "position": 12,
            "commit_id": SHA,
            "created_at": "2026-11-12T10:09:00Z",
        }]),
    );

    let comments = the_forge(&fake).review_comments(pr(7)).expect("comments");

    let read: Vec<(&str, &str, Option<Location>, Option<&str>)> = comments
        .iter()
        .map(|comment| {
            (
                comment.author.as_str(),
                comment.body.as_str(),
                comment.location.clone(),
                comment.head.as_ref().map(CommitId::as_str),
            )
        })
        .collect();
    assert_eq!(
        read,
        [
            (
                "reviewer",
                "Off by one.",
                Some(Location {
                    path: "src/export.rs".to_owned(),
                    line: Some(12),
                }),
                Some(SHA),
            ),
            ("reviewer", "Two things.", None, Some(SHA)),
        ]
    );
    // A draft's line comments are never read; a review with none asks for none.
    let asked = fake.asked();
    assert!(
        asked
            .iter()
            .all(|asked| !asked.contains("/reviews/2/") && !asked.contains("/reviews/3/")),
        "{asked:?}"
    );
}

#[test]
fn the_last_changed_file_is_the_last_listed_and_there_are_no_scanning_alerts() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/pulls/7/files*"),
        200,
        &json!([{ "filename": "src/a.rs" }, { "filename": "src/b.rs" }]),
    );
    fake.on(&format!("GET {API}/pulls/8/files*"), 200, &json!([]));
    let forge = the_forge(&fake);
    assert_eq!(
        forge.last_changed_file(pr(7)),
        Ok(Some("src/b.rs".to_owned()))
    );
    assert_eq!(forge.last_changed_file(pr(8)), Ok(None));

    let quiet = FakeCurl::new();
    assert_eq!(the_forge(&quiet).scanning_alerts(pr(7)), Ok(Vec::new()));
    assert_eq!(quiet.asked(), Vec::<String>::new());
}

#[test]
fn the_handoff_is_posted_once_then_edited_and_read_from_the_latest_marked_comment() {
    let fake = FakeCurl::new();
    fake.on(&format!("GET {API}/issues/7/comments"), 200, &json!([]));
    fake.on(
        &format!("POST {API}/issues/7/comments"),
        201,
        &json!({ "id": 100, "body": "" }),
    );
    let forge = the_forge(&fake);
    assert_eq!(forge.handoff(pr(7)), Ok(None));
    forge
        .set_handoff(pr(7), "Done: the writer.")
        .expect("posted");
    assert_eq!(
        fake.bodies(&format!("POST {API}/issues/7/comments")),
        [json!({ "body": marked(HANDOFF_MARKER, "Done: the writer.") })]
    );

    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/issues/7/comments"),
        200,
        &json!([
            { "id": 102, "body": "a person's comment" },
            { "id": 101, "body": marked(HANDOFF_MARKER, "old") },
            { "id": 100, "body": marked(HANDOFF_MARKER, "older") },
        ]),
    );
    fake.on(
        &format!("PATCH {API}/issues/comments/101"),
        200,
        &json!({ "id": 101, "body": "" }),
    );
    let forge = the_forge(&fake);
    assert_eq!(forge.handoff(pr(7)), Ok(Some("old".to_owned())));
    forge.set_handoff(pr(7), "old").expect("unchanged");
    assert!(fake.asked().iter().all(|asked| asked.starts_with("GET")));
    forge.set_handoff(pr(7), "new").expect("edited");
    assert_eq!(
        fake.bodies(&format!("PATCH {API}/issues/comments/101")),
        [json!({ "body": marked(HANDOFF_MARKER, "new") })]
    );
}

#[test]
fn open_hands_back_the_open_pull_request_of_the_branch_in_this_repository() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/pulls[?]state=open*"),
        200,
        &json!([
            pull(5, "feature/v1-1-9", "open", false),
            pull_from(6, HEAD, 77, "open", false),
            pull(7, HEAD, "open", false),
        ]),
    );
    assert_eq!(
        the_forge(&fake).open_pull_request(&request()),
        Ok(Opened::AlreadyOpen(pr(7)))
    );
    assert!(fake.asked().iter().all(|asked| asked.starts_with("GET")));
}

#[test]
fn open_creates_the_pull_request_when_none_is_open_for_the_branch() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/pulls[?]state=open*"),
        200,
        &json!([pull_from(6, HEAD, 77, "open", false)]),
    );
    fake.on(
        &format!("POST {API}/pulls"),
        201,
        &pull(12, HEAD, "open", false),
    );
    assert_eq!(
        the_forge(&fake).open_pull_request(&request()),
        Ok(Opened::New(pr(12)))
    );
    assert_eq!(
        fake.bodies(&format!("POST {API}/pulls")),
        [json!({
            "head": HEAD,
            "base": "feature/v1",
            "title": "Export the list",
            "body": "Closes #2",
        })]
    );
}

#[test]
fn an_open_forgejo_refuses_is_a_backend_failure() {
    let fake = FakeCurl::new();
    fake.on(&format!("GET {API}/pulls[?]state=open*"), 200, &json!([]));
    fake.on(
        &format!("POST {API}/pulls"),
        404,
        &json!({ "message": "branch does not exist" }),
    );
    let error = the_forge(&fake)
        .open_pull_request(&request())
        .expect_err("refused");
    assert!(
        matches!(&error, PortError::Backend(message) if message.contains("branch does not exist")),
        "{error:?}"
    );

    let down = FakeCurl::new();
    down.unreachable(&format!("GET {API}/pulls[?]state=open*"));
    assert!(matches!(
        the_forge(&down).open_pull_request(&request()),
        Err(PortError::Unavailable(_))
    ));
}

#[test]
fn a_merge_sends_the_strategy_and_the_pinned_head() {
    let fake = FakeCurl::new();
    open_seven(&fake);
    fake.on(&format!("POST {API}/pulls/7/merge"), 200, &Value::Null);
    let forge = the_forge(&fake);
    assert_eq!(
        forge.merge_pull_request(pr(7), MergeStrategy::Merge, None),
        Ok(MergeOutcome::Merged)
    );
    let pin = CommitId::from(SHA);
    assert_eq!(
        forge.merge_pull_request(pr(7), MergeStrategy::Squash, Some(&pin)),
        Ok(MergeOutcome::Merged)
    );
    assert_eq!(
        fake.bodies(&format!("POST {API}/pulls/7/merge")),
        [
            json!({ "Do": "merge" }),
            json!({ "Do": "squash", "head_commit_id": SHA }),
        ]
    );
}

#[test]
fn a_merged_pull_request_is_quiet_and_a_closed_or_moved_one_is_refused_before_any_merge() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/pulls/7"),
        200,
        &pull(7, HEAD, "closed", true),
    );
    fake.on(
        &format!("GET {API}/pulls/8"),
        200,
        &pull(8, HEAD, "closed", false),
    );
    fake.on(
        &format!("GET {API}/pulls/9"),
        200,
        &pull(9, HEAD, "open", false),
    );
    fake.on(
        &format!("GET {API}/pulls/10"),
        404,
        &json!({ "message": "not found" }),
    );
    let forge = the_forge(&fake);

    assert_eq!(
        forge.merge_pull_request(pr(7), MergeStrategy::Merge, None),
        Ok(MergeOutcome::AlreadyMerged)
    );
    assert_eq!(
        forge.merge_pull_request(pr(8), MergeStrategy::Merge, None),
        Err(PortError::Rejected(
            "pull request #8 is closed unmerged".to_owned()
        ))
    );
    let elsewhere = CommitId::from("fedcba9876543210");
    assert_eq!(
        forge.merge_pull_request(pr(9), MergeStrategy::Merge, Some(&elsewhere)),
        Err(PortError::Rejected(format!(
            "pull request #9's head is {SHA}, not fedcba9876543210"
        )))
    );
    // A pull request that does not read is a backend failure, not a refusal.
    assert!(matches!(
        forge.merge_pull_request(pr(10), MergeStrategy::Merge, None),
        Err(PortError::Backend(_))
    ));
    assert!(fake.asked().iter().all(|asked| asked.starts_with("GET")));
}

#[test]
fn a_merge_forgejo_cannot_make_yet_is_a_conflict_and_any_other_refusal_final() {
    for (status, message, not_yet) in [
        (405, "Please try again later", true),
        (409, "merge push out of date", true),
        (409, "head out of date", false),
        (405, "Not all required status checks successful", false),
        (405, "The PR is not mergeable: conflicts", false),
    ] {
        let fake = FakeCurl::new();
        open_seven(&fake);
        fake.on(
            &format!("POST {API}/pulls/7/merge"),
            status,
            &json!({ "message": message }),
        );
        let error = the_forge(&fake)
            .merge_pull_request(pr(7), MergeStrategy::Merge, None)
            .expect_err(message);
        let said = format!("Forgejo answered {status} {message}");
        if not_yet {
            assert_eq!(
                error,
                PortError::Conflict(format!("pull request #7 is not mergeable yet: {said}"))
            );
        } else {
            assert_eq!(
                error,
                PortError::Rejected(format!(
                    "pull request #7: Forgejo refused the merge: {said}"
                ))
            );
        }
    }
}

#[test]
fn a_refused_merge_another_merge_won_meanwhile_is_quiet() {
    let fake = FakeCurl::new();
    fake.once(
        &format!("GET {API}/pulls/7"),
        200,
        &pull(7, HEAD, "open", false),
    );
    fake.on(
        &format!("GET {API}/pulls/7"),
        200,
        &pull(7, HEAD, "closed", true),
    );
    fake.on(
        &format!("POST {API}/pulls/7/merge"),
        405,
        &json!({ "message": "The PR has already been merged" }),
    );
    assert_eq!(
        the_forge(&fake).merge_pull_request(pr(7), MergeStrategy::Merge, None),
        Ok(MergeOutcome::AlreadyMerged)
    );
}

#[test]
fn a_merge_the_instance_does_not_answer_is_unavailable() {
    let fake = FakeCurl::new();
    open_seven(&fake);
    fake.unreachable(&format!("POST {API}/pulls/7/merge"));
    assert!(matches!(
        the_forge(&fake).merge_pull_request(pr(7), MergeStrategy::Merge, None),
        Err(PortError::Unavailable(_))
    ));
}

#[test]
fn a_checks_log_is_its_link() {
    let fake = FakeCurl::new();
    let forge = the_forge(&fake);
    let link = CheckLink::from("http://forgejo.test/example/tracker/actions/runs/4/jobs/0");
    assert_eq!(forge.check_log(&link), Ok(CheckLog::AtLink));
    assert_eq!(forge.check_log(&CheckLink::from("")), Ok(CheckLog::NoJob));
    assert_eq!(fake.asked(), Vec::<String>::new());
}

/// The instance answering, the repository readable and writable, and both empty creates
/// refused as invalid.
fn answering(fake: &FakeCurl) {
    fake.on(
        "GET http://forgejo.test/api/v1/version",
        200,
        &json!({ "version": "15.0.11" }),
    );
    fake.on(
        &format!("GET {API}"),
        200,
        &json!({ "permissions": { "push": true, "pull": true } }),
    );
    for endpoint in ["pulls", "issues"] {
        fake.on(
            &format!("POST {API}/{endpoint}"),
            422,
            &json!({ "message": "[Title]: Required" }),
        );
    }
}

fn read(checks: Vec<crate::ports::AccessCheck>) -> Vec<(String, bool, String)> {
    checks
        .into_iter()
        .map(|check| (check.what, check.ok, check.detail))
        .collect()
}

#[test]
fn access_asks_the_instance_then_whether_the_token_writes_pull_requests_and_issues() {
    let fake = FakeCurl::new();
    answering(&fake);
    assert_eq!(
        read(the_forge(&fake).access()),
        [
            (
                "instance".to_owned(),
                true,
                "Forgejo 15.0.11 at http://forgejo.test".to_owned()
            ),
            (
                "token".to_owned(),
                true,
                "`FORGEJO_TOKEN` reads and writes example/tracker and its pull requests".to_owned()
            ),
        ]
    );
    assert_eq!(fake.bodies(&format!("POST {API}/pulls")), [json!({})]);
    assert_eq!(fake.bodies(&format!("POST {API}/issues")), [json!({})]);
}

#[test]
fn access_beside_the_board_asks_only_whether_the_token_writes_pull_requests() {
    let fake = FakeCurl::new();
    answering(&fake);
    let forge = ForgejoForge::beside_board(fake.api());
    assert_eq!(
        read(forge.access()),
        [(
            "token".to_owned(),
            true,
            "`FORGEJO_TOKEN` writes example/tracker's pull requests".to_owned()
        )]
    );
    assert_eq!(fake.asked(), [format!("POST {API}/pulls")]);

    let refusing = FakeCurl::new();
    refusing.on(
        &format!("POST {API}/pulls"),
        403,
        &json!({ "message": "token does not have at least one of required scope(s): [write:repository]" }),
    );
    let checks = ForgejoForge::beside_board(refusing.api()).access();
    assert!(!checks[0].ok);
    assert_eq!(
        checks[0].detail,
        "`FORGEJO_TOKEN` cannot write example/tracker's pull requests: Forgejo answered 403 \
         token does not have at least one of required scope(s): [write:repository]"
    );

    let tokenless = FakeCurl::new();
    let checks = ForgejoForge::beside_board(tokenless.api_with(None)).access();
    assert_eq!(
        read(checks),
        [(
            "token".to_owned(),
            false,
            "`FORGEJO_TOKEN` is not set: export a Forgejo access token in it, with read and write on the repository's issues and pull requests".to_owned()
        )]
    );
    assert_eq!(tokenless.asked(), Vec::<String>::new());
}
