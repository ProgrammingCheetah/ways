//! The Forgejo forge against the fake `curl`: a pull request and its head, checks from commit
//! statuses, reviews, the hand-off comment, opening and merging, and what `zaf doctor` asks.

use super::*;
use crate::adapters::forgejo::forge::MAX_FILE_PAGES;
use crate::adapters::notes::{HANDOFF_MARKER, marked};
use crate::domain::{ChangedFiles, Location, PullRequestFilter, RepoPath, WorkflowFilter};
use crate::ports::{
    BranchName, CheckBucket, CheckLink, CheckLog, CheckName, CheckRun, CheckState, CiFilter,
    CommitId, Forge, MergeOutcome, MergeStrategy, NewPullRequest, Opened, PrBody, PrNumber, PrRef,
    PrTitle,
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

/// The page Forgejo shows pull request `number` on.
fn page(number: u64) -> String {
    format!("http://forgejo.test/example/tracker/pulls/{number}")
}

/// A pull request as the port hands it back.
fn handed(number: u64) -> PrRef {
    PrRef::new(pr(number), page(number))
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
        "html_url": page(number),
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
fn forgejo_pull_reads_mergeable() {
    let fake = FakeCurl::new();
    let with = |number: u64, mergeable: Option<Value>| {
        let mut row = pull(number, "feature/v1-1-3", "open", false);
        if let Some(value) = mergeable {
            row["mergeable"] = value;
        }
        fake.on(&format!("GET {API}/pulls/{number}"), 200, &row);
    };
    with(1, Some(json!(true)));
    with(2, Some(json!(false)));
    with(3, Some(Value::Null));
    with(4, None);
    let forge = the_forge(&fake);

    assert_eq!(
        forge.pull_request(pr(1)).expect("true").mergeable,
        Some(true)
    );
    assert_eq!(
        forge.pull_request(pr(2)).expect("false").mergeable,
        Some(false)
    );
    assert_eq!(forge.pull_request(pr(3)).expect("null").mergeable, None);
    assert_eq!(forge.pull_request(pr(4)).expect("absent").mergeable, None);
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
fn changed_files_are_listed_in_order_and_there_are_no_scanning_alerts() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/pulls/7/files*"),
        200,
        &json!([{ "filename": "src/a.rs" }, { "filename": "src/b.rs" }]),
    );
    fake.on(&format!("GET {API}/pulls/8/files*"), 200, &json!([]));
    let forge = the_forge(&fake);
    assert_eq!(
        forge.changed_files(pr(7)),
        Ok(ChangedFiles::of(&["src/a.rs", "src/b.rs"]))
    );
    assert_eq!(forge.changed_files(pr(8)), Ok(ChangedFiles::default()));

    let quiet = FakeCurl::new();
    assert_eq!(the_forge(&quiet).scanning_alerts(pr(7)), Ok(Vec::new()));
    assert_eq!(quiet.asked(), Vec::<String>::new());
}

/// A scene whose instance pages `size` items at a time.
fn paging(size: u32) -> FakeCurl {
    let fake = FakeCurl::new();
    fs::write(fake.scene.path().join("rules"), "").expect("rules reset");
    fake.on(
        "GET */api/v1/settings/api",
        200,
        &json!({ "max_response_items": size }),
    );
    fake
}

#[test]
fn changed_files_lists_a_rename_under_both_names() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/pulls/7/files*"),
        200,
        &json!([
            { "filename": "README.md", "status": "changed" },
            { "filename": "docs/x.rs", "status": "renamed", "previous_filename": "zaf/x.rs" },
        ]),
    );
    let listed = the_forge(&fake)
        .changed_files(pr(7))
        .expect("the files read");
    assert_eq!(
        listed,
        ChangedFiles::of(&["README.md", "zaf/x.rs", "docs/x.rs"])
    );
    assert_eq!(listed.last().map(RepoPath::as_str), Some("docs/x.rs"));
}

#[test]
fn changed_files_reads_past_one_page() {
    let fake = paging(2);
    fake.on(
        &format!("GET {API}/pulls/7/files?page=1&limit=2"),
        200,
        &json!([{ "filename": "a.rs" }, { "filename": "b.rs" }]),
    );
    fake.on(
        &format!("GET {API}/pulls/7/files?page=2&limit=2"),
        200,
        &json!([{ "filename": "c.rs" }, { "filename": "d.rs" }]),
    );
    fake.on(
        &format!("GET {API}/pulls/7/files?page=3&limit=2"),
        200,
        &json!([{ "filename": "e.rs", "previous_filename": "old.rs" }]),
    );
    let listed = the_forge(&fake)
        .changed_files(pr(7))
        .expect("the files read");
    assert_eq!(
        listed,
        ChangedFiles::of(&["a.rs", "b.rs", "c.rs", "d.rs", "old.rs", "e.rs"])
    );
    assert_eq!(fake.asked().len(), 3, "three pages, then stop");
}

#[test]
fn changed_files_cut_short_when_capped() {
    // Every page is full, so the list never ends: the pages read stop at the cap.
    let fake = paging(2);
    fake.on(
        &format!("GET {API}/pulls/7/files*"),
        200,
        &json!([{ "filename": "a.rs" }, { "filename": "b.rs" }]),
    );
    let listed = the_forge(&fake)
        .changed_files(pr(7))
        .expect("the files read");
    assert!(listed.cut_short, "{listed:?}");
    assert_eq!(listed.paths.len(), 2 * MAX_FILE_PAGES);
    assert_eq!(fake.asked().len(), MAX_FILE_PAGES);
}

#[test]
fn changed_files_cut_short_when_a_later_page_fails_and_fail_when_the_first_does() {
    let fake = paging(1);
    fake.on(
        &format!("GET {API}/pulls/7/files?page=1&limit=1"),
        200,
        &json!([{ "filename": "a.rs" }]),
    );
    fake.on(
        &format!("GET {API}/pulls/7/files?page=2&limit=1"),
        500,
        &json!({ "message": "boom" }),
    );
    let listed = the_forge(&fake)
        .changed_files(pr(7))
        .expect("the files read");
    assert_eq!(listed.paths, ChangedFiles::of(&["a.rs"]).paths);
    assert!(listed.cut_short);

    let broken = FakeCurl::new();
    broken.on(&format!("GET {API}/pulls/7/files*"), 500, &json!({}));
    assert!(matches!(
        the_forge(&broken).changed_files(pr(7)),
        Err(PortError::Backend(_))
    ));
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
        Ok(Opened::AlreadyOpen(handed(7)))
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
        Ok(Opened::New(handed(12)))
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
fn a_pull_request_forgejo_gives_no_link_for_is_a_backend_failure() {
    let fake = FakeCurl::new();
    fake.on(&format!("GET {API}/pulls[?]state=open*"), 200, &json!([]));
    fake.on(
        &format!("POST {API}/pulls"),
        201,
        &json!({ "number": 12, "state": "open" }),
    );
    let error = the_forge(&fake)
        .open_pull_request(&request())
        .expect_err("no link");
    assert!(
        matches!(&error, PortError::Backend(message) if message.contains("Forgejo gave no link")),
        "{error:?}"
    );
}

#[test]
fn the_default_branch_is_the_repositorys_own() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}"),
        200,
        &json!({ "default_branch": "trunk", "permissions": { "pull": true, "push": true } }),
    );
    assert_eq!(
        the_forge(&fake).default_branch(),
        Ok(BranchName::from("trunk"))
    );
    assert_eq!(fake.asked(), [format!("GET {API}")]);
}

#[test]
fn a_repository_with_no_default_branch_or_a_forge_that_is_down_is_a_failure() {
    let fake = FakeCurl::new();
    fake.on(&format!("GET {API}"), 200, &json!({ "default_branch": "" }));
    assert!(matches!(
        the_forge(&fake).default_branch(),
        Err(PortError::Backend(message)) if message.contains("no default branch")
    ));

    let down = FakeCurl::new();
    down.unreachable(&format!("GET {API}"));
    assert!(matches!(
        the_forge(&down).default_branch(),
        Err(PortError::Unavailable(_))
    ));
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

/// The branch `feature/v1` as Forgejo prints it, at [`SHA`].
fn integration_branch(fake: &FakeCurl) {
    fake.on(
        &format!("GET {API}/branches/feature/v1"),
        200,
        &json!({
            "name": "feature/v1",
            "commit": { "id": SHA, "timestamp": "2027-01-15T08:00:00+02:00" },
        }),
    );
}

#[test]
fn a_branchs_checks_are_the_latest_status_of_each_context_at_the_commit_it_is_at() {
    let fake = FakeCurl::new();
    integration_branch(&fake);
    fake.on(
        &format!("GET {API}/commits/{SHA}/statuses*"),
        200,
        &json!([
            status(1, "ci / test (push)", "pending", ""),
            status(2, "ci / lint (push)", "failure", ""),
            status(3, "ci / test (push)", "success", ""),
            status(4, "ci / build (push)", "pending", ""),
        ]),
    );
    let read = the_forge(&fake)
        .branch_checks(&BranchName::from("feature/v1"))
        .expect("the branch reads");
    assert_eq!(read.head, Some(CommitId::from(SHA)));
    assert_eq!(
        read.reached_at,
        humantime::parse_rfc3339("2027-01-15T06:00:00Z").ok(),
        "the commit's time, read with its offset"
    );
    let checks: Vec<(&str, CheckState, bool)> = read
        .checks
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
        checks,
        [
            ("ci / build (push)", CheckState::Pending, false),
            ("ci / lint (push)", CheckState::Failed, true),
            ("ci / test (push)", CheckState::Passed, true),
        ]
    );
    assert_eq!(read.checks[2].finished_at, Some(at(1_794_478_380)));
    assert!(
        fake.asked()
            .iter()
            .any(|asked| asked.starts_with(&format!("GET {API}/commits/{SHA}/statuses?page=1"))),
        "{:?}",
        fake.asked()
    );
}

#[test]
fn a_branch_with_no_status_has_a_head_and_no_check() {
    let fake = FakeCurl::new();
    integration_branch(&fake);
    fake.on(
        &format!("GET {API}/commits/{SHA}/statuses*"),
        200,
        &json!([]),
    );
    let read = the_forge(&fake)
        .branch_checks(&BranchName::from("feature/v1"))
        .expect("the branch reads");
    assert_eq!(read.head, Some(CommitId::from(SHA)));
    assert_eq!(read.checks, Vec::new());
}

#[test]
fn a_branch_forgejo_does_not_have_is_no_head_and_its_statuses_are_not_asked() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/branches/feature/v2"),
        404,
        &json!({ "message": "The target couldn't be found." }),
    );
    let read = the_forge(&fake)
        .branch_checks(&BranchName::from("feature/v2"))
        .expect("no branch is no error");
    assert_eq!(read.head, None);
    assert_eq!(read.checks, Vec::new());
    assert_eq!(fake.asked().len(), 1, "{:?}", fake.asked());
}

#[test]
fn a_branch_that_cannot_be_read_or_carries_an_unknown_status_is_a_backend_failure() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/branches/feature/v3"),
        500,
        &json!({ "message": "boom" }),
    );
    let error = the_forge(&fake)
        .branch_checks(&BranchName::from("feature/v3"))
        .expect_err("a failed read");
    assert!(matches!(error, PortError::Backend(_)), "{error:?}");

    integration_branch(&fake);
    fake.on(
        &format!("GET {API}/commits/{SHA}/statuses*"),
        200,
        &json!([status(1, "ci", "skipped", "")]),
    );
    let error = the_forge(&fake)
        .branch_checks(&BranchName::from("feature/v1"))
        .expect_err("unknown status");
    assert!(
        matches!(&error, PortError::Backend(message)
            if message.contains("branch feature/v1") && message.contains("`skipped`")),
        "{error:?}"
    );
}

/// Pull request 7 with a file, a posted review and two checks, as a gather reads them.
fn arrange_gather(fake: &FakeCurl) {
    open_seven(fake);
    fake.on(
        &format!("GET {API}/pulls/7/files*"),
        200,
        &json!([{ "filename": "src/export.rs" }]),
    );
    fake.on(
        &format!("GET {API}/pulls/7/reviews[?]*"),
        200,
        &json!([{
            "id": 1,
            "user": { "login": "reviewer" },
            "body": "Fine.",
            "state": "APPROVED",
            "commit_id": SHA,
            "submitted_at": "2026-11-12T10:10:00Z",
            "comments_count": 0,
        }]),
    );
    fake.on(
        &format!("GET {API}/commits/{SHA}/statuses*"),
        200,
        &json!([
            status(1, "ci / test (push)", "success", ""),
            status(2, "ci / lint (push)", "pending", ""),
        ]),
    );
}

/// How many times the fake was asked for the pull request itself, `pulls/7`, and not for what
/// hangs under it.
fn pull_reads(fake: &FakeCurl) -> usize {
    let own = format!("GET {API}/pulls/7");
    fake.asked().iter().filter(|asked| **asked == own).count()
}

#[test]
fn pull_facts_fetch_pull_once() {
    let fake = FakeCurl::new();
    arrange_gather(&fake);
    let forge = the_forge(&fake);

    crate::app::schedule::gather::read_pull(&forge, pr(7)).expect("the facts");

    assert_eq!(pull_reads(&fake), 1, "{:?}", fake.asked());
}

#[test]
fn pull_facts_unchanged() {
    let fake = FakeCurl::new();
    arrange_gather(&fake);
    let forge = the_forge(&fake);

    let facts = crate::app::schedule::gather::read_pull(&forge, pr(7)).expect("the facts");

    assert_eq!(facts.opened_at, at(1_794_474_000));
    assert_eq!(facts.merged_at, None);
    assert_eq!(facts.head, Some(CommitId::from(SHA)));
    assert_eq!(facts.mergeable, None);
    assert_eq!(facts.changed_files.paths, [RepoPath::from("src/export.rs")]);
    let names: Vec<&str> = facts
        .checks
        .iter()
        .map(|check| check.name.as_str())
        .collect();
    assert_eq!(names, ["ci / lint (push)", "ci / test (push)"]);
    assert_eq!(facts.reviews.len(), 1);
    assert_eq!(facts.reviews[0].head, Some(CommitId::from(SHA)));

    // A merged pull request reads its merge time from the same one read.
    fake.on(
        &format!("GET {API}/pulls/8"),
        200,
        &pull(8, "feature/v1-1-3", "closed", true),
    );
    fake.on(&format!("GET {API}/pulls/8/files*"), 200, &json!([]));
    fake.on(&format!("GET {API}/pulls/8/reviews[?]*"), 200, &json!([]));
    let merged = crate::app::schedule::gather::read_pull(&forge, pr(8)).expect("merged facts");
    assert_eq!(merged.merged_at, Some(at(1_794_481_200)));
}

#[test]
fn pull_facts_not_kept_between_reads() {
    let fake = FakeCurl::new();
    arrange_gather(&fake);
    let forge = the_forge(&fake);

    crate::app::schedule::gather::read_pull(&forge, pr(7)).expect("the first read");
    crate::app::schedule::gather::read_pull(&forge, pr(7)).expect("the second read");

    assert_eq!(pull_reads(&fake), 2, "{:?}", fake.asked());
}

/// The contents API's reply for a file whose text is `text`.
fn file_reply(text: &str) -> Value {
    json!({ "type": "file", "encoding": "base64", "content": encode_base64(text.as_bytes()) })
}

/// Standard base64, as the contents API prints a file's bytes, in lines of 60 characters.
fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in bytes.chunks(3) {
        let word = chunk.iter().enumerate().fold(0u32, |word, (at, byte)| {
            word | u32::from(*byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                text.push(char::from(
                    ALPHABET[((word >> (18 - 6 * at)) & 0x3f) as usize],
                ));
            } else {
                text.push('=');
            }
        }
    }
    text.as_bytes()
        .chunks(60)
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

fn entry(name: &str, kind: &str) -> Value {
    json!({ "name": name, "path": format!(".forgejo/workflows/{name}"), "type": kind })
}

fn workflow_at(path: &str, filter: PullRequestFilter) -> WorkflowFilter {
    WorkflowFilter {
        path: RepoPath::from(path),
        filter,
    }
}

const WORKFLOWS: &str = ".forgejo/workflows";

#[test]
fn ci_filter_reads_yml_and_yaml_files_at_the_base_and_nothing_else() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}?ref=feature%2Fv1"),
        200,
        &json!([
            entry("b.yaml", "file"),
            entry("a.yml", "file"),
            entry("notes.md", "file"),
            entry("nested.yml", "dir"),
        ]),
    );
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}/a.yml?ref=feature%2Fv1"),
        200,
        &file_reply("on: pull_request\njobs: {}\n"),
    );
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}/b.yaml?ref=feature%2Fv1"),
        200,
        &file_reply("on: push\njobs: {}\n"),
    );
    let found = ForgejoCiFilter::from(fake.api())
        .workflows(&BranchName::from("feature/v1"))
        .expect("the workflows read");
    assert_eq!(
        found,
        [
            workflow_at(".forgejo/workflows/a.yml", PullRequestFilter::AnyChange),
            workflow_at(".forgejo/workflows/b.yaml", PullRequestFilter::AnyChange),
        ]
    );
    // Every read names the base's ref; no call reaches for a pull request or its head.
    for asked in fake.asked() {
        assert!(asked.ends_with("?ref=feature%2Fv1"), "{asked}");
    }
}

#[test]
fn ci_filter_marks_a_refused_file_unreadable() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}?ref=main"),
        200,
        &json!([
            entry("a.yml", "file"),
            entry("b.yml", "file"),
            entry("c.yml", "symlink"),
            entry("odd name.yml", "file"),
        ]),
    );
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}/a.yml?ref=main"),
        403,
        &json!({ "message": "refused" }),
    );
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}/b.yml?ref=main"),
        200,
        &file_reply("on: pull_request\n"),
    );
    let found = ForgejoCiFilter::from(fake.api())
        .workflows(&BranchName::from("main"))
        .expect("the list reads");
    assert_eq!(
        found,
        [
            workflow_at(".forgejo/workflows/a.yml", PullRequestFilter::Unsure),
            workflow_at(".forgejo/workflows/b.yml", PullRequestFilter::AnyChange),
            workflow_at(".forgejo/workflows/c.yml", PullRequestFilter::Unsure),
            workflow_at(".forgejo/workflows/odd name.yml", PullRequestFilter::Unsure),
        ]
    );
}

#[test]
fn ci_filter_marks_a_file_that_is_not_base64_text_unreadable() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}?ref=main"),
        200,
        &json!([entry("a.yml", "file"), entry("b.yml", "file")]),
    );
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}/a.yml?ref=main"),
        200,
        &json!({ "type": "file", "encoding": "base64", "content": "!!!!" }),
    );
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}/b.yml?ref=main"),
        200,
        &json!({ "type": "file", "encoding": "none", "content": "" }),
    );
    let found = ForgejoCiFilter::from(fake.api())
        .workflows(&BranchName::from("main"))
        .expect("the list reads");
    assert_eq!(found.len(), 2);
    assert!(
        found
            .iter()
            .all(|workflow| workflow.filter == PullRequestFilter::Unsure),
        "{found:?}"
    );
}

#[test]
fn ci_filter_has_no_workflows_in_a_base_with_no_folder_but_fails_on_a_missing_base() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}?ref=main"),
        404,
        &json!({ "message": "not found" }),
    );
    fake.on(
        &format!("GET {API}/branches/main"),
        200,
        &json!({ "commit": { "id": SHA } }),
    );
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}?ref=gone"),
        404,
        &json!({ "message": "not found" }),
    );
    fake.on(
        &format!("GET {API}/branches/gone"),
        404,
        &json!({ "message": "not found" }),
    );
    let filter = ForgejoCiFilter::from(fake.api());
    assert_eq!(filter.workflows(&BranchName::from("main")), Ok(Vec::new()));
    assert!(matches!(
        filter.workflows(&BranchName::from("gone")),
        Err(PortError::Backend(_))
    ));
    assert!(matches!(
        filter.workflows(&BranchName::from("-main")),
        Err(PortError::Rejected(_))
    ));
}

#[test]
fn ci_filter_fails_when_the_list_cannot_be_read() {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}?ref=main"),
        500,
        &json!({ "message": "boom" }),
    );
    let error = ForgejoCiFilter::from(fake.api())
        .workflows(&BranchName::from("main"))
        .expect_err("a failed list");
    assert!(matches!(error, PortError::Backend(_)), "{error:?}");
}

/// The filter the Forgejo adapter reads from the one workflow `text` at the base of a fake
/// Forgejo.
fn forgejo_workflows(text: &str) -> Vec<WorkflowFilter> {
    let fake = FakeCurl::new();
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}?ref=main"),
        200,
        &json!([entry("a.yml", "file")]),
    );
    fake.on(
        &format!("GET {API}/contents/{WORKFLOWS}/a.yml?ref=main"),
        200,
        &file_reply(text),
    );
    ForgejoCiFilter::from(fake.api())
        .workflows(&BranchName::from("main"))
        .expect("the workflows read")
}

/// Whether a check is expected on a pull request from `branch` that changes a doc.
fn forgejo_ci_expected(text: &str, branch: &str) -> bool {
    crate::domain::checks_expected(
        &forgejo_workflows(text),
        &[RepoPath::from("docs/a.md")],
        false,
        Some(branch),
    )
}

#[test]
fn ci_filter_push_no_branches_expected_forgejo() {
    assert!(forgejo_ci_expected("on: push\n", "feature/x"));
    assert!(forgejo_ci_expected("on:\n  push:\n", "feature/x"));
}

#[test]
fn ci_filter_push_matching_branch_expected_forgejo() {
    let text = "on:\n  push:\n    branches: ['feature/**']\n";
    assert!(forgejo_ci_expected(text, "feature/x"));
}

#[test]
fn ci_filter_push_other_branch_not_expected_forgejo() {
    let text = "on:\n  push:\n    branches: [main]\n";
    assert!(!forgejo_ci_expected(text, "feature/x"));
}

#[test]
fn ci_filter_pull_request_target_expected_forgejo() {
    assert!(forgejo_ci_expected(
        "on: pull_request_target\n",
        "feature/x"
    ));
}

#[test]
fn ci_filter_unreadable_unsure_forgejo() {
    assert!(forgejo_ci_expected(
        "on:\n  push:\n    branches-ignore: [wip]\n",
        "feature/x"
    ));
    assert_eq!(
        forgejo_workflows("on: [unclosed")[0].filter,
        PullRequestFilter::Unsure
    );
}

#[test]
fn base64_reads_the_standard_alphabet_with_line_breaks() {
    use crate::adapters::forgejo::ci_filter::decode_base64;

    assert_eq!(decode_base64(""), Some(Vec::new()));
    assert_eq!(decode_base64("Zm9v\nYmFy"), Some(b"foobar".to_vec()));
    assert_eq!(decode_base64("Zm8="), Some(b"fo".to_vec()));
    assert_eq!(decode_base64("Zm8=Zg"), None);
    assert_eq!(decode_base64("Z$8="), None);
}
