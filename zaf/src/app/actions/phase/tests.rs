use std::time::{Duration, SystemTime};

use super::*;
use crate::ports::{CommitId, Vcs};

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

/// A workspace whose latest commit is `commit`; it is at the pull request's head when that is
/// `HEAD`.
fn workspace(commit: &str) -> Option<WorkspaceFacts> {
    Some(WorkspaceFacts {
        latest_commit: Some(commit.to_owned()),
        holds_head: commit == HEAD,
    })
}

/// A task with the `claimed` label and a claim comment, and nothing else.
fn claimed() -> PhaseFacts {
    PhaseFacts {
        claimed: true,
        claim_comment: true,
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
            "pr, failed check",
            PhaseFacts {
                task_check: task_check(Verdict::MustFix, PR_RECORDED + 1),
                ..open_pr()
            },
            Phase::Build,
        ),
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
        ("claimed, claim comment", claimed(), Phase::Claim),
        (
            "claimed, no claim comment",
            PhaseFacts {
                claim_comment: false,
                ..claimed()
            },
            Phase::Unclaimed,
        ),
        (
            "claim comment, no label",
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
fn failed_check_with_a_pr_record_is_build() {
    for verdict in [Verdict::MustFix, Verdict::StepFailed] {
        let newer = PhaseFacts {
            task_check: task_check(verdict, PR_RECORDED + 1),
            ..open_pr()
        };
        assert_eq!(derive_phase(&newer), Phase::Build, "{verdict}, newer");

        // The last task check failed, whenever it ran relative to the `.pr` record.
        let older = PhaseFacts {
            task_check: task_check(verdict, PR_RECORDED - 1),
            ..open_pr()
        };
        assert_eq!(derive_phase(&older), Phase::Build, "{verdict}, older");

        let mut no_time = task_check(verdict, PR_RECORDED + 1);
        if let Some(check) = &mut no_time {
            check.at = None;
        }
        let undated = PhaseFacts {
            task_check: no_time,
            ..open_pr()
        };
        assert_eq!(derive_phase(&undated), Phase::Build, "{verdict}, undated");

        // A record for an older head is no result.
        let older_head = PhaseFacts {
            pr_check: Some(record(Verdict::Clean, Some(OLD_HEAD), REVIEWED)),
            task_check: task_check(verdict, REVIEWED + 1),
            ..open_pr()
        };
        assert_eq!(
            derive_phase(&older_head),
            Phase::Build,
            "{verdict}, older head"
        );

        // A record with no head is no result.
        let no_head = PhaseFacts {
            pr_check: Some(record(Verdict::Clean, None, REVIEWED)),
            task_check: task_check(verdict, REVIEWED + 1),
            ..open_pr()
        };
        assert_eq!(derive_phase(&no_head), Phase::Build, "{verdict}, no head");
    }
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

/// A task whose workspace and pull request are read through the fake Vcs and the fake forge, as
/// the actions read them.
mod held_head {
    use std::fs;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::domain::{Ticket, TicketId, TicketState};
    use crate::fakes::{FakeBoard, FakeForge, FakeVcs, PrRequest};
    use crate::ports::{BaseName, PrNumber, SliceName};

    const TASK: u64 = 448;
    const SLICE: &str = "feature/0.5.3-420-448";
    const BASE: &str = "feature/0.5.3";

    pub(super) struct Scene {
        _dir: tempfile::TempDir,
        root: PathBuf,
        workspace: PathBuf,
        board: FakeBoard,
        forge: FakeForge,
        pub(super) vcs: FakeVcs,
        pr: PrNumber,
    }

    fn write(file: &Path, text: &str) {
        fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        fs::write(file, text).expect("write");
    }

    impl Scene {
        /// A claimed task with its slice open and one commit of work in the workspace; its
        /// pull request is open and reports no head yet.
        pub(super) fn new() -> (Self, CommitId) {
            let dir = tempfile::tempdir().expect("tempdir");
            let base = fs::canonicalize(dir.path()).expect("real path");
            let (root, workspace) = (base.join("project"), base.join("workspace"));
            fs::create_dir_all(&workspace).expect("mkdir");
            let task = TicketId::from(TASK);
            let board = FakeBoard::default();
            board.add_ticket(Ticket {
                id: task,
                epic: Some(TicketId::from(420)),
                title: "Holds".into(),
                body: "".into(),
                state: Some(TicketState::Claimed),
                url: "".into(),
                kind: None,
            });
            let (slice, base_name) = (SliceName::from(SLICE), BaseName::from(BASE));
            let vcs = FakeVcs::default();
            vcs.open(&slice, &base_name);
            vcs.place(&slice, &workspace).expect("place");
            vcs.edit(&slice, "a.rs", 1, 0, "+a").expect("edit");
            let latest = vcs.commit(&slice, "Work").expect("commit");
            let forge = FakeForge::default();
            let pr = forge
                .open_pr(&PrRequest {
                    slice,
                    base: base_name,
                    title: "Holds".to_owned(),
                    body: String::new(),
                })
                .expect("open pr");
            write(
                &root.join(".zaf/slices/448"),
                &workspace.display().to_string(),
            );
            write(&root.join(".zaf/slices/448.pr"), &pr.get().to_string());
            let scene = Self {
                _dir: dir,
                root,
                workspace,
                board,
                forge,
                vcs,
                pr,
            };
            (scene, latest)
        }

        /// The pull request is at `head`, and a clean `check pr` was made at `reviewed`.
        pub(super) fn at(&self, head: &CommitId, reviewed: &CommitId) {
            self.forge
                .set_head(self.pr, Some(head.as_str()))
                .expect("set head");
            let stored = CheckRecord {
                target: "#448".to_owned(),
                verdict: Verdict::Clean,
                lines: Vec::new(),
                head: Some(reviewed.to_string()),
                at: Some(humantime::format_rfc3339_seconds(time(REVIEWED)).to_string()),
            };
            let file = self
                .root
                .join(format!(".zaf/runs/check-pr-{}.json", self.pr.get()));
            write(&file, &serde_json::to_string(&stored).expect("json"));
        }

        pub(super) fn facts(&self) -> PhaseFacts {
            let records = crate::app::actions::SliceRecords::new(&self.root);
            let task = TicketId::from(TASK);
            gather_phase_facts(&self.board, &self.forge, &self.vcs, &records, task).expect("facts")
        }

        pub(super) fn phase(&self) -> Phase {
            derive_phase(&self.facts())
        }

        pub(super) fn workspace(&self) -> &Path {
            &self.workspace
        }
    }
}

#[test]
fn empty_commit_on_top_derives_land() {
    let (scene, latest) = held_head::Scene::new();
    // The head the pull request is at is an empty commit on the workspace's latest commit, and
    // the review is clean at that head.
    let head = scene.vcs.empty_commit(&latest).expect("empty commit");
    scene.at(&head, &head);
    assert_eq!(scene.phase(), Phase::Land);
    // The workspace's own latest commit as the head is held as before.
    scene.at(&latest, &latest);
    assert_eq!(scene.phase(), Phase::Land);
}

#[test]
fn other_work_at_head_does_not_derive_land() {
    let (scene, latest) = held_head::Scene::new();
    let head = scene.vcs.changed_commit(&latest).expect("changed commit");
    scene.at(&head, &head);
    assert_eq!(scene.phase(), Phase::Build);
    // A head that does not descend from the workspace's latest commit, with its tree.
    let beside = scene.vcs.sibling_commit(&latest).expect("sibling");
    scene.at(&beside, &beside);
    assert_eq!(scene.phase(), Phase::Build);
    // A head the repository does not hold.
    let unseen = CommitId::from("not-in-the-repository");
    scene.at(&unseen, &unseen);
    assert_eq!(scene.phase(), Phase::Build);
    assert_eq!(scene.vcs.holds(scene.workspace(), &unseen), Ok(false));
}

#[test]
fn review_at_an_earlier_head_does_not_count() {
    let (scene, latest) = held_head::Scene::new();
    let head = scene.vcs.empty_commit(&latest).expect("empty commit");
    // Clean at the workspace's commit, but the pull request has moved on to the empty commit:
    // the head holds the work, and the review is not for it.
    scene.at(&head, &latest);
    assert_eq!(scene.phase(), Phase::Publish);
    // The same review at the head it was made at counts.
    scene.at(&head, &head);
    assert_eq!(scene.phase(), Phase::Land);
}

#[test]
fn phase_refusal_names_head_not_held() {
    // A workspace ahead of the head of a clean review: the words name both commits.
    let held = ahead_of_clean_review().head_not_held();
    assert_eq!(
        held.as_deref(),
        Some(
            "the workspace does not hold the pull request's head head-new: \
             its latest commit is commit-local"
        )
    );
    // At the head, with no clean result or no workspace, nothing is said.
    let at_head = PhaseFacts {
        workspace: workspace(HEAD),
        ..ahead_of_clean_review()
    };
    assert_eq!(at_head.head_not_held(), None);
    assert_eq!(must_fix_review().head_not_held(), None);
    assert_eq!(open_pr().head_not_held(), None);
    let no_workspace = PhaseFacts {
        workspace: None,
        ..ahead_of_clean_review()
    };
    assert_eq!(no_workspace.head_not_held(), None);
}

#[test]
fn phase_refusal_head_missing_locally() {
    let (scene, latest) = held_head::Scene::new();
    let unseen = CommitId::from("not-in-the-repository");
    scene.at(&unseen, &unseen);
    let held = scene.facts().head_not_held().expect("the head is not held");
    assert!(
        held.starts_with(
            "the workspace does not hold the pull request's head not-in-the-repository"
        ),
        "{held}"
    );
    assert!(held.contains(latest.as_str()), "{held}");
}
