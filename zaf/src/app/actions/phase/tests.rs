use std::time::{Duration, SystemTime};

use super::*;

const HEAD: &str = "head-new";
const OLD_HEAD: &str = "head-old";
const LOCAL: &str = "commit-local";

/// When the `.pr` record was written.
const PR_RECORDED: u64 = 1_000;
/// When the `check pr` record began, after the `.pr` record.
const REVIEWED: u64 = 2_000;

fn time(seconds: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
}

fn record(verdict: Verdict, head: Option<&str>, seconds: u64) -> CheckRecord {
    CheckRecord {
        target: "t".to_owned(),
        verdict,
        lines: Vec::new(),
        head: head.map(str::to_owned),
        at: Some(humantime::format_rfc3339_seconds(time(seconds)).to_string()),
    }
}

/// A `check pr` record on the current head.
fn review(verdict: Verdict) -> Option<CheckRecord> {
    Some(record(verdict, Some(HEAD), REVIEWED))
}

fn task_check(verdict: Verdict, seconds: u64) -> Option<CheckRecord> {
    Some(record(verdict, None, seconds))
}

fn workspace(commit: &str) -> Option<WorkspaceFacts> {
    Some(WorkspaceFacts {
        latest_commit: Some(commit.to_owned()),
    })
}

/// A claimed task under a live lease, and nothing else.
fn claimed() -> PhaseFacts {
    PhaseFacts {
        claimed: true,
        live_lease: true,
        ..PhaseFacts::default()
    }
}

/// A task whose pull request is open at `HEAD`, the workspace at that commit,
/// no check run yet.
fn open_pr() -> PhaseFacts {
    PhaseFacts {
        workspace: workspace(HEAD),
        pr: Some(PrFacts {
            recorded_at: Some(time(PR_RECORDED)),
            head: Some(HEAD.to_owned()),
        }),
        ..claimed()
    }
}

/// `open_pr` with a clean `check pr` on `HEAD` and the workspace ahead of it.
fn ahead_of_clean_review() -> PhaseFacts {
    PhaseFacts {
        workspace: workspace(LOCAL),
        pr_check: review(Verdict::Clean),
        ..open_pr()
    }
}

/// `open_pr` with a must-fix `check pr` on `HEAD`.
fn must_fix_review() -> PhaseFacts {
    PhaseFacts {
        pr_check: review(Verdict::MustFix),
        ..open_pr()
    }
}

#[test]
fn phase_table_covers_every_rule() {
    let land = PhaseFacts {
        pr_check: review(Verdict::Clean),
        ..open_pr()
    };
    let rows = [
        // The `done` label decides first, even over a `land` task.
        (
            "done",
            PhaseFacts {
                done: true,
                ..PhaseFacts::default()
            },
            Phase::Done,
        ),
        (
            "done over land",
            PhaseFacts {
                done: true,
                ..land.clone()
            },
            Phase::Done,
        ),
        ("land", land, Phase::Land),
        ("clean, ahead", ahead_of_clean_review(), Phase::Build),
        ("must-fix", must_fix_review(), Phase::Build),
        (
            "failed step",
            PhaseFacts {
                pr_check: review(Verdict::StepFailed),
                ..open_pr()
            },
            Phase::Review,
        ),
        // Unlike a must-fix, a failed step has no exception for a newer clean check.
        (
            "failed step, newer clean check",
            PhaseFacts {
                pr_check: review(Verdict::StepFailed),
                task_check: task_check(Verdict::Clean, REVIEWED + 1),
                ..open_pr()
            },
            Phase::Review,
        ),
        ("pr, no result", open_pr(), Phase::Publish),
        (
            "pr, older clean check",
            PhaseFacts {
                task_check: task_check(Verdict::Clean, PR_RECORDED - 1),
                ..open_pr()
            },
            Phase::Publish,
        ),
        (
            "pr, newer clean check",
            PhaseFacts {
                task_check: task_check(Verdict::Clean, PR_RECORDED + 1),
                ..open_pr()
            },
            Phase::Check,
        ),
        (
            "clean task check",
            PhaseFacts {
                workspace: workspace(LOCAL),
                task_check: task_check(Verdict::Clean, 1),
                ..claimed()
            },
            Phase::Check,
        ),
        (
            "workspace",
            PhaseFacts {
                workspace: workspace(LOCAL),
                ..claimed()
            },
            Phase::Build,
        ),
        ("claimed, live lease", claimed(), Phase::Claim),
        (
            "claimed, lease lapsed",
            PhaseFacts {
                live_lease: false,
                ..claimed()
            },
            Phase::Unclaimed,
        ),
        (
            "lease, no label",
            PhaseFacts {
                claimed: false,
                ..claimed()
            },
            Phase::Unclaimed,
        ),
        ("nothing", PhaseFacts::default(), Phase::Unclaimed),
    ];
    for (name, facts, phase) in rows {
        assert_eq!(derive_phase(&facts), phase, "{name}");
    }
}

#[test]
fn workspace_ahead_is_not_land() {
    assert_eq!(derive_phase(&ahead_of_clean_review()), Phase::Build);

    // A workspace whose commit could not be read is not at the head either.
    let unread = PhaseFacts {
        workspace: Some(WorkspaceFacts::default()),
        ..ahead_of_clean_review()
    };
    assert_eq!(derive_phase(&unread), Phase::Build);

    let no_workspace = PhaseFacts {
        workspace: None,
        ..ahead_of_clean_review()
    };
    assert_eq!(derive_phase(&no_workspace), Phase::Build);
}

#[test]
fn workspace_ahead_with_newer_clean_check_is_check() {
    let newer = PhaseFacts {
        task_check: task_check(Verdict::Clean, REVIEWED + 1),
        ..ahead_of_clean_review()
    };
    assert_eq!(derive_phase(&newer), Phase::Check);

    // The record is what it must be newer than, not the `.pr` record.
    let older = PhaseFacts {
        task_check: task_check(Verdict::Clean, REVIEWED - 1),
        ..ahead_of_clean_review()
    };
    assert_eq!(derive_phase(&older), Phase::Build);
}

#[test]
fn must_fix_returns_to_build() {
    assert_eq!(derive_phase(&must_fix_review()), Phase::Build);

    // Commits in the workspace do not answer a must-fix until a task check is clean.
    let ahead = PhaseFacts {
        workspace: workspace(LOCAL),
        ..must_fix_review()
    };
    assert_eq!(derive_phase(&ahead), Phase::Build);

    // A clean task check from before the review answers nothing it found.
    let older = PhaseFacts {
        task_check: task_check(Verdict::Clean, REVIEWED - 1),
        ..must_fix_review()
    };
    assert_eq!(derive_phase(&older), Phase::Build);
}

#[test]
fn fix_round_check_is_newer() {
    let newer = PhaseFacts {
        task_check: task_check(Verdict::Clean, REVIEWED + 1),
        ..must_fix_review()
    };
    assert_eq!(derive_phase(&newer), Phase::Check);

    // Newer is strict: the same second does not count.
    let same_second = PhaseFacts {
        task_check: task_check(Verdict::Clean, REVIEWED),
        ..must_fix_review()
    };
    assert_eq!(derive_phase(&same_second), Phase::Build);

    // A record without a readable time is never newer, on either side.
    let mut no_time = task_check(Verdict::Clean, REVIEWED + 1);
    if let Some(check) = &mut no_time {
        check.at = None;
    }
    let undated_check = PhaseFacts {
        task_check: no_time,
        ..must_fix_review()
    };
    assert_eq!(derive_phase(&undated_check), Phase::Build);

    let mut garbled = review(Verdict::MustFix);
    if let Some(result) = &mut garbled {
        result.at = Some("yesterday".to_owned());
    }
    let undated_result = PhaseFacts {
        pr_check: garbled,
        task_check: task_check(Verdict::Clean, REVIEWED + 1),
        ..open_pr()
    };
    assert_eq!(derive_phase(&undated_result), Phase::Build);
}

#[test]
fn failed_task_check_stays_in_build() {
    let in_workspace = PhaseFacts {
        workspace: workspace(LOCAL),
        task_check: task_check(Verdict::MustFix, 1),
        ..claimed()
    };
    assert_eq!(derive_phase(&in_workspace), Phase::Build);

    let step_failed = PhaseFacts {
        workspace: workspace(LOCAL),
        task_check: task_check(Verdict::StepFailed, 1),
        ..claimed()
    };
    assert_eq!(derive_phase(&step_failed), Phase::Build);

    let after_must_fix = PhaseFacts {
        task_check: task_check(Verdict::MustFix, REVIEWED + 1),
        ..must_fix_review()
    };
    assert_eq!(derive_phase(&after_must_fix), Phase::Build);

    let after_clean_review = PhaseFacts {
        task_check: task_check(Verdict::MustFix, REVIEWED + 1),
        ..ahead_of_clean_review()
    };
    assert_eq!(derive_phase(&after_clean_review), Phase::Build);
}

#[test]
fn new_push_returns_to_publish() {
    for verdict in [Verdict::Clean, Verdict::MustFix, Verdict::StepFailed] {
        let pushed = PhaseFacts {
            pr_check: Some(record(verdict, Some(OLD_HEAD), REVIEWED)),
            ..open_pr()
        };
        assert_eq!(derive_phase(&pushed), Phase::Publish, "{verdict}");
    }

    // After the push, the clean check that preceded it is older than the `.pr` record.
    let check_before_push = PhaseFacts {
        pr_check: Some(record(Verdict::MustFix, Some(OLD_HEAD), REVIEWED)),
        task_check: task_check(Verdict::Clean, PR_RECORDED - 1),
        ..open_pr()
    };
    assert_eq!(derive_phase(&check_before_push), Phase::Publish);

    // One newer than the `.pr` record is `check`, whatever the stale record says.
    let check_after_push = PhaseFacts {
        pr_check: Some(record(Verdict::MustFix, Some(OLD_HEAD), REVIEWED)),
        task_check: task_check(Verdict::Clean, PR_RECORDED + 1),
        ..open_pr()
    };
    assert_eq!(derive_phase(&check_after_push), Phase::Check);

    // Newer is strict here too, and a `.pr` record of unknown time is not older.
    let same_second = PhaseFacts {
        task_check: task_check(Verdict::Clean, PR_RECORDED),
        ..open_pr()
    };
    assert_eq!(derive_phase(&same_second), Phase::Publish);

    let undated = PhaseFacts {
        pr: Some(PrFacts {
            recorded_at: None,
            head: Some(HEAD.to_owned()),
        }),
        task_check: task_check(Verdict::Clean, PR_RECORDED + 1),
        ..open_pr()
    };
    assert_eq!(derive_phase(&undated), Phase::Publish);
}

#[test]
fn no_head_is_no_result() {
    // The record holds no head.
    let record_without = PhaseFacts {
        pr_check: Some(record(Verdict::Clean, None, REVIEWED)),
        ..open_pr()
    };
    assert_eq!(derive_phase(&record_without), Phase::Publish);

    // The forge reported no current head, so nothing matches it, not even a
    // record and a workspace that hold none themselves.
    let forge_without = PhaseFacts {
        pr: Some(PrFacts {
            recorded_at: Some(time(PR_RECORDED)),
            head: None,
        }),
        pr_check: Some(record(Verdict::Clean, None, REVIEWED)),
        workspace: Some(WorkspaceFacts::default()),
        ..claimed()
    };
    assert_eq!(derive_phase(&forge_without), Phase::Publish);

    // An empty head is no head: three empties do not agree their way to `land`.
    let empty = PhaseFacts {
        pr: Some(PrFacts {
            recorded_at: Some(time(PR_RECORDED)),
            head: Some(String::new()),
        }),
        pr_check: Some(record(Verdict::Clean, Some(""), REVIEWED)),
        workspace: workspace(""),
        ..claimed()
    };
    assert_eq!(derive_phase(&empty), Phase::Publish);

    // A clean record with no `.pr` record has no current head to be clean for.
    let no_pr = PhaseFacts {
        pr: None,
        pr_check: review(Verdict::Clean),
        workspace: workspace(HEAD),
        ..claimed()
    };
    assert_eq!(derive_phase(&no_pr), Phase::Build);
}

#[test]
fn clean_on_an_older_head_is_not_land() {
    // The workspace at the record's head is not enough: the PR has moved on.
    let moved_on = PhaseFacts {
        pr_check: Some(record(Verdict::Clean, Some(OLD_HEAD), REVIEWED)),
        workspace: workspace(OLD_HEAD),
        ..open_pr()
    };
    assert_eq!(derive_phase(&moved_on), Phase::Publish);

    let all_at_head = PhaseFacts {
        pr_check: Some(record(Verdict::Clean, Some(OLD_HEAD), REVIEWED)),
        ..open_pr()
    };
    assert_eq!(derive_phase(&all_at_head), Phase::Publish);
}

#[test]
fn forgotten_records_return_to_claim() {
    // A merged-or-dropped slice forgets the records; the board's facts remain.
    let forgotten = PhaseFacts {
        workspace: None,
        pr: None,
        task_check: None,
        pr_check: None,
        ..ahead_of_clean_review()
    };
    assert_eq!(derive_phase(&forgotten), Phase::Claim);
}

#[test]
fn unclaimed_is_never_declarable() {
    assert!(!Phase::DECLARABLE.contains(&Phase::Unclaimed));
    assert_eq!(Phase::declared("unclaimed"), None);
    assert_eq!(Phase::Unclaimed.to_string(), "unclaimed");
    assert_eq!(Phase::declared("land"), Some(Phase::Land));
}
