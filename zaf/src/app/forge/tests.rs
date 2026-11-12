use super::*;
use crate::fakes::{FakeForge, PrRequest};
use crate::ports::{BaseName, CheckBucket, CheckState, JobId, PrBody, PrTitle, SliceName};

fn request(head: &str, base: &str, title: &str) -> NewPullRequest {
    NewPullRequest {
        head: BranchName::from(head),
        base: BranchName::from(base),
        title: PrTitle::from(title),
        body: PrBody::from("Refs #2."),
    }
}

fn opened(forge: &FakeForge, slice: &str) -> PrNumber {
    forge
        .open_pr(&PrRequest {
            slice: SliceName::from(slice),
            base: BaseName::from("feature/v1"),
            title: "Export the list".to_owned(),
            body: String::new(),
        })
        .expect("open")
}

#[test]
fn open_opens_once_per_head_and_hands_back_the_open_one() {
    let forge = FakeForge::default();
    let first = open(&forge, &request("feature/v1-1-2", "feature/v1", "Export")).expect("open");
    let again = open(&forge, &request("feature/v1-1-2", "feature/v1", "Export")).expect("again");
    assert!(matches!(first, Opened::New(_)), "{first:?}");
    assert_eq!(again, Opened::AlreadyOpen(first.number()));
    let request = forge.request(first.number()).expect("recorded");
    assert_eq!(request.slice, SliceName::from("feature/v1-1-2"));
    assert_eq!(request.base, BaseName::from("feature/v1"));
    assert_eq!(request.body, "Refs #2.");
}

#[test]
fn open_refuses_a_branch_it_cannot_use_and_an_empty_title_before_the_forge() {
    let forge = FakeForge::default();
    for (head, base, title, said) in [
        ("", "feature/v1", "T", "head `` is not a usable branch name"),
        (
            "-x",
            "feature/v1",
            "T",
            "head `-x` is not a usable branch name",
        ),
        ("s", "", "T", "base `` is not a usable branch name"),
        (
            "s",
            "--draft",
            "T",
            "base `--draft` is not a usable branch name",
        ),
        ("s", "feature/v1", "", "a pull request needs a title"),
    ] {
        let error = open(&forge, &request(head, base, title)).expect_err(said);
        assert_eq!(error.to_string(), said);
    }
    assert_eq!(forge.request(PrNumber::from(1)), None, "nothing opened");
}

#[test]
fn merge_merges_with_the_strategy_and_is_quiet_once_merged() {
    let forge = FakeForge::default();
    let pr = opened(&forge, "feature/v1-1-2");
    assert_eq!(
        merge(&forge, pr, MergeStrategy::Squash, None),
        Ok(MergeOutcome::Merged)
    );
    assert_eq!(forge.merged_with(pr), Some(MergeStrategy::Squash));
    assert_eq!(
        merge(&forge, pr, MergeStrategy::Merge, None),
        Ok(MergeOutcome::AlreadyMerged)
    );
}

#[test]
fn merge_at_a_head_merges_only_that_head() {
    let forge = FakeForge::default();
    let pr = opened(&forge, "feature/v1-1-2");
    forge.set_head(pr, Some("abc123")).expect("head");
    let moved = merge(&forge, pr, MergeStrategy::Merge, Some("def456")).expect_err("moved");
    assert!(
        matches!(&moved, ForgeError::Port(PortError::Rejected(message)) if message.contains("abc123")),
        "{moved:?}"
    );
    assert_eq!(
        merge(&forge, pr, MergeStrategy::Merge, Some("abc123")),
        Ok(MergeOutcome::Merged)
    );
}

#[test]
fn merge_refuses_a_head_that_is_no_commit_id_before_the_forge() {
    let forge = FakeForge::default();
    let pr = opened(&forge, "feature/v1-1-2");
    for head in ["", "main", "ABC123", "abc;1"] {
        assert_eq!(
            merge(&forge, pr, MergeStrategy::Merge, Some(head)),
            Err(ForgeError::NotACommit(head.to_owned()))
        );
    }
    assert_eq!(forge.merged_with(pr), None);
}

#[test]
fn merge_passes_the_forges_refusals_on() {
    let forge = FakeForge::default();
    let closed = opened(&forge, "feature/v1-1-2");
    forge.close(closed).expect("close");
    assert!(matches!(
        merge(&forge, closed, MergeStrategy::Merge, None),
        Err(ForgeError::Port(PortError::Rejected(_)))
    ));
    let pr = opened(&forge, "feature/v1-1-3");
    forge.refuse_merges(PortError::Conflict(
        "pull request #2 is not mergeable yet".to_owned(),
    ));
    assert!(matches!(
        merge(&forge, pr, MergeStrategy::Merge, None),
        Err(ForgeError::Port(PortError::Conflict(_)))
    ));
}

#[test]
fn checks_print_one_tab_separated_line_each() {
    let forge = FakeForge::default();
    let pr = opened(&forge, "feature/v1-1-2");
    forge
        .set_check_runs(
            pr,
            vec![
                CheckRun {
                    bucket: CheckBucket::Pass,
                    name: CheckName::from("ci / test"),
                    link: Some(CheckLink::from("http://forgejo.test/runs/1/jobs/0")),
                },
                CheckRun {
                    bucket: CheckBucket::Pending,
                    name: CheckName::from("external"),
                    link: None,
                },
            ],
        )
        .expect("runs");
    let lines: Vec<String> = checks(&forge, pr)
        .expect("checks")
        .iter()
        .map(check_line)
        .collect();
    assert_eq!(
        lines,
        [
            "pass\tci / test\thttp://forgejo.test/runs/1/jobs/0",
            "pending\texternal\t"
        ]
    );
}

#[test]
fn checks_with_none_arranged_are_the_checks_by_their_state() {
    let forge = FakeForge::default();
    let pr = opened(&forge, "feature/v1-1-2");
    forge
        .set_checks(
            pr,
            vec![crate::ports::Check {
                name: CheckName::from("ci"),
                state: CheckState::Failed,
                finished_at: None,
            }],
        )
        .expect("checks");
    let runs = checks(&forge, pr).expect("checks");
    assert_eq!(runs[0].bucket, CheckBucket::Fail);
    assert_eq!(runs[0].bucket.state(), CheckState::Failed);
}

#[test]
fn head_is_the_forges_or_none() {
    let forge = FakeForge::default();
    let pr = opened(&forge, "feature/v1-1-2");
    assert_eq!(head(&forge, pr), Ok(None));
    forge.set_head(pr, Some("abc123")).expect("head");
    assert_eq!(head(&forge, pr), Ok(Some("abc123".to_owned())));
}

#[test]
fn a_jobs_failed_log_is_its_last_lines_under_a_header() {
    let forge = FakeForge::default();
    let link = CheckLink::from("https://github.com/o/r/actions/runs/31/job/102");
    let lines: String = (1..=100).map(|n| format!("line {n}\n")).collect();
    forge.set_check_log(
        link.clone(),
        CheckLog::Failed {
            job: JobId::from("102"),
            lines,
        },
    );
    let text = log(&forge, &CheckName::from("zaf / test"), &link)
        .expect("log")
        .expect("some text");
    let printed: Vec<&str> = text.lines().collect();
    assert_eq!(printed[0], "--- failed log lines of zaf / test (job 102)");
    assert_eq!(printed.len(), 1 + LOG_TAIL);
    assert_eq!(printed[1], "line 21");
    assert_eq!(printed[LOG_TAIL], "line 100");
}

#[test]
fn a_log_the_forge_cannot_give_is_its_link_and_no_job_is_nothing() {
    let forge = FakeForge::default();
    let link = CheckLink::from("http://forgejo.test/o/r/actions/runs/4/jobs/0");
    forge.set_check_log(link.clone(), CheckLog::AtLink);
    assert_eq!(
        log(&forge, &CheckName::from("ci / test"), &link),
        Ok(Some(
            "--- ci / test: the forge has no API for a job's log; read it at \
             http://forgejo.test/o/r/actions/runs/4/jobs/0\n"
                .to_owned()
        ))
    );
    let elsewhere = CheckLink::from("https://ci.example.com/builds/5");
    assert_eq!(
        log(&forge, &CheckName::from("external"), &elsewhere),
        Ok(None)
    );
}
