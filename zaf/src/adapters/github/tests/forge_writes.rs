//! The GitHub forge's writes and the reads the actions wait on, against the fake `gh`: each is
//! the gh call the standard actions made, argument for argument.

use super::*;
use crate::ports::{
    BranchName, CheckBucket, CheckLink, CheckLog, CheckRun, JobId, MergeOutcome, MergeStrategy,
    NewPullRequest, Opened, PrBody, PrTitle,
};

const SLICE: &str = "feature/v1-1-2";

fn request() -> NewPullRequest {
    NewPullRequest {
        head: BranchName::from(SLICE),
        base: BranchName::from("feature/v1"),
        title: PrTitle::from("Add the CSV writer"),
        body: PrBody::from("Closes #2"),
    }
}

fn pr(number: u64) -> PrNumber {
    PrNumber::from(number)
}

fn list_call() -> Vec<String> {
    argv(&[
        "pr",
        "list",
        "--head",
        SLICE,
        "--state",
        "open",
        "--json",
        "number",
        "--jq",
        ".[].number",
        "--repo",
        REPO,
    ])
}

fn state_call(number: &str) -> Vec<String> {
    argv(&[
        "pr", "view", number, "--json", "state", "--jq", ".state", "--repo", REPO,
    ])
}

#[test]
fn open_hands_back_the_open_pull_request_of_the_slice() {
    let fake = FakeGh::new();
    fake.on("pr list *", "5\n");
    assert_eq!(
        fake.forge().open_pull_request(&request()),
        Ok(Opened::AlreadyOpen(pr(5)))
    );
    assert_eq!(fake.calls(), [list_call()]);
}

#[test]
fn open_creates_one_and_reads_its_number_from_the_url_gh_prints_last() {
    let fake = FakeGh::new();
    fake.on("pr list *", "").on(
        "pr create *",
        &format!("Creating pull request\nhttps://github.com/{REPO}/pull/7\n"),
    );
    assert_eq!(
        fake.forge().open_pull_request(&request()),
        Ok(Opened::New(pr(7)))
    );
    assert_eq!(
        fake.calls(),
        [
            list_call(),
            argv(&[
                "pr",
                "create",
                "--head",
                SLICE,
                "--base",
                "feature/v1",
                "--title",
                "Add the CSV writer",
                "--body",
                "Closes #2",
                "--repo",
                REPO,
            ]),
        ]
    );
}

#[test]
fn open_fails_as_a_backend_on_any_gh_failure_or_an_answer_with_no_number() {
    for (scene, said) in [
        (vec![("pr list *", 1, "HTTP 502")], "HTTP 502"),
        (vec![("pr list *", 0, "seven\n")], "gh listed seven"),
        (
            vec![
                ("pr list *", 0, ""),
                ("pr create *", 1, "a pull request exists"),
            ],
            "a pull request exists",
        ),
        (
            vec![("pr list *", 0, ""), ("pr create *", 0, "done\n")],
            "gh printed no pull request number",
        ),
    ] {
        let fake = FakeGh::new();
        for (pattern, status, text) in scene {
            fake.rule(pattern, status, text);
        }
        let error = fake.forge().open_pull_request(&request()).expect_err(said);
        assert!(
            matches!(&error, PortError::Backend(message) if message.contains(said)),
            "{said}: {error:?}"
        );
    }
}

#[test]
fn merge_reads_the_state_then_merges_with_the_strategy_and_the_pin() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let fake = FakeGh::new();
    fake.on("pr view 7 *", "OPEN\n").on("pr merge *", "");
    let forge = fake.forge();
    assert_eq!(
        forge.merge_pull_request(pr(7), MergeStrategy::Merge, None),
        Ok(MergeOutcome::Merged)
    );
    assert_eq!(
        forge.merge_pull_request(pr(7), MergeStrategy::Squash, Some(&CommitId::from(sha))),
        Ok(MergeOutcome::Merged)
    );
    assert_eq!(
        fake.calls(),
        [
            state_call("7"),
            argv(&["pr", "merge", "7", "--merge", "--repo", REPO]),
            state_call("7"),
            argv(&[
                "pr",
                "merge",
                "7",
                "--squash",
                "--match-head-commit",
                sha,
                "--repo",
                REPO,
            ]),
        ]
    );
}

#[test]
fn merge_is_quiet_once_merged_and_refuses_a_closed_pull_request_before_merging() {
    let fake = FakeGh::new();
    fake.on("pr view 7 *", "MERGED\n")
        .on("pr view 8 *", "CLOSED\n")
        .on("pr view 9 *", "\n")
        .fail_on("pr view 10 *", "HTTP 502");
    let forge = fake.forge();
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
    assert_eq!(
        forge.merge_pull_request(pr(9), MergeStrategy::Merge, None),
        Err(PortError::Backend(
            "gh printed no state for pull request #9".to_owned()
        ))
    );
    assert!(matches!(
        forge.merge_pull_request(pr(10), MergeStrategy::Merge, None),
        Err(PortError::Backend(message)) if message.contains("HTTP 502")
    ));
    assert!(
        fake.calls().iter().all(|call| call[1] == "view"),
        "no merge was tried"
    );
}

#[test]
fn a_merge_refused_for_a_moved_base_is_one_for_now_and_any_other_refusal_final() {
    for (said, not_yet) in [
        (
            "X Pull request #7 Base branch was modified. Review and try the merge again.",
            true,
        ),
        (
            "X Pull request #7 is not mergeable: the merge commit cannot be cleanly created",
            true,
        ),
        ("X Pull request #7 has failing required checks", false),
    ] {
        let fake = FakeGh::new();
        fake.on("pr view 7 *", "OPEN\n").fail_on("pr merge *", said);
        let error = fake
            .forge()
            .merge_pull_request(pr(7), MergeStrategy::Merge, None)
            .expect_err(said);
        let expected = if not_yet {
            PortError::Conflict(format!(
                "pull request #7 is not mergeable yet: gh pr merge: {said}"
            ))
        } else {
            PortError::Rejected(format!("pull request #7: gh pr merge refused: {said}"))
        };
        assert_eq!(error, expected);
    }
}

#[test]
fn check_runs_are_gh_pr_checks_lines_and_none_reported_is_none() {
    let fake = FakeGh::new();
    fake.on(
        "pr checks 7 *",
        &format!(
            "pass\tzaf / test\thttps://github.com/{REPO}/actions/runs/31/job/101\n\
             skipping\tzaf / release\t\n\
             pending\texternal\thttps://ci.example.com/builds/5\n"
        ),
    )
    .fail_on(
        "pr checks 8 *",
        "no checks reported on the 'feature/v1-1-2' branch",
    )
    .fail_on("pr checks 9 *", "HTTP 502")
    .on("pr checks 10 *", "done\tci\t\n");
    let forge = fake.forge();
    assert_eq!(
        forge.check_runs(pr(7)),
        Ok(vec![
            CheckRun {
                bucket: CheckBucket::Pass,
                name: CheckName::from("zaf / test"),
                link: Some(CheckLink::from(format!(
                    "https://github.com/{REPO}/actions/runs/31/job/101"
                ))),
            },
            CheckRun {
                bucket: CheckBucket::Skipping,
                name: CheckName::from("zaf / release"),
                link: None,
            },
            CheckRun {
                bucket: CheckBucket::Pending,
                name: CheckName::from("external"),
                link: Some(CheckLink::from("https://ci.example.com/builds/5")),
            },
        ])
    );
    assert_eq!(
        fake.calls()[0],
        argv(&[
            "pr",
            "checks",
            "7",
            "--json",
            "bucket,name,link",
            "--jq",
            ".[] | [.bucket, .name, .link] | @tsv",
            "--repo",
            REPO,
        ])
    );
    assert_eq!(forge.check_runs(pr(8)), Ok(Vec::new()));
    assert_eq!(
        forge.check_runs(pr(9)),
        Err(PortError::Backend("gh pr checks: HTTP 502".to_owned()))
    );
    assert!(matches!(
        forge.check_runs(pr(10)),
        Err(PortError::Backend(message)) if message.contains("`done`")
    ));
}

#[test]
fn a_failed_jobs_log_is_read_by_its_job_and_a_link_with_none_reads_nothing() {
    let fake = FakeGh::new();
    fake.on("run view --job 102 *", "zaf / test\tRun tests\tpanicked\n")
        .fail_on("run view --job 103 *", "HTTP 404: the log has expired");
    let forge = fake.forge();
    let link = |job: u32| {
        CheckLink::from(format!(
            "https://github.com/{REPO}/actions/runs/31/job/{job}"
        ))
    };
    assert_eq!(
        forge.check_log(&link(102)),
        Ok(CheckLog::Failed {
            job: JobId::from("102"),
            lines: "zaf / test\tRun tests\tpanicked\n".to_owned(),
        })
    );
    assert_eq!(
        fake.calls()[0],
        argv(&[
            "run",
            "view",
            "--job",
            "102",
            "--log-failed",
            "--repo",
            REPO
        ])
    );
    assert_eq!(
        forge.check_log(&link(103)),
        Err(PortError::Backend(
            "gh run view --job 103: HTTP 404: the log has expired".to_owned()
        ))
    );
    assert_eq!(
        forge.check_log(&CheckLink::from("https://ci.example.com/builds/5")),
        Ok(CheckLog::NoJob)
    );
    assert_eq!(fake.calls().len(), 2, "a link with no job asks gh nothing");
}

#[test]
fn a_github_forge_asks_doctor_nothing() {
    let fake = FakeGh::new();
    assert!(fake.forge().access().is_empty());
    assert!(fake.calls().is_empty());
}
