use super::*;
use crate::fakes::{FakeBoard, FakeForge, FakeGate, FakeVcs};
use crate::ports::GateRun;

const SLICE: &str = "feature/v1-1-2";
const BASE: &str = "feature/v1";

fn ticket(label: TaskLabel) -> Ticket {
    Ticket {
        number: IssueNumber::from(2),
        epic: Some(IssueNumber::from(1)),
        title: "Export the list".to_owned(),
        body: String::new(),
        label: Some(label),
    }
}

fn query() -> SliceQuery {
    SliceQuery {
        task: IssueNumber::from(2),
        version: Some("v1".to_owned()),
    }
}

/// A board holding task 2 of epic 1 and the other fakes, empty.
#[derive(Default)]
struct Scene {
    board: FakeBoard,
    vcs: FakeVcs,
    forge: FakeForge,
    gate: FakeGate,
}

impl Scene {
    fn new(label: TaskLabel) -> Self {
        let scene = Self::default();
        scene.board.add_ticket(ticket(label));
        let passed = GateRun {
            verdict: GateVerdict::Passed,
            output: "ok".to_owned(),
        };
        scene.gate.declare(GateLevel::Light, passed);
        scene
    }

    fn target(&self) -> SliceTarget {
        resolve(&self.board, &BranchesSection::default(), &query()).expect("resolve")
    }

    /// Opens the slice, edits a file and opens its pull request.
    fn reviewed(&self) -> (SliceTarget, PrNumber) {
        let target = self.target();
        open(&self.vcs, &target).expect("open");
        self.vcs
            .edit(&target.slice, "src/export.rs", 12, 3, "@@ -1,3 +1,12 @@")
            .expect("edit");
        let report = pr(&self.vcs, &self.forge, &target).expect("pr");
        (target, report.pr)
    }

    fn ports(&self) -> MergePorts<'_> {
        MergePorts {
            board: &self.board,
            vcs: &self.vcs,
            forge: &self.forge,
            gate: Some(&self.gate),
        }
    }

    fn label(&self) -> Option<TaskLabel> {
        self.board.task(IssueNumber::from(2)).expect("task").label
    }
}

#[test]
fn the_slice_name_and_base_come_from_the_manifest_patterns() {
    let scene = Scene::new(TaskLabel::Claimed);
    let target = scene.target();
    assert_eq!(target.slice, SliceName::from(SLICE));
    assert_eq!(target.base, BaseName::from(BASE));
    let branches = BranchesSection {
        integration: BranchPattern::from("main"),
        task: BranchPattern::from("task-{task}"),
    };
    let unversioned = SliceQuery {
        version: None,
        ..query()
    };
    let target = resolve(&scene.board, &branches, &unversioned).expect("resolve");
    assert_eq!(target.slice, SliceName::from("task-2"));
    assert_eq!(target.base, BaseName::from("main"));
}

#[test]
fn a_pattern_without_its_values_is_refused() {
    let scene = Scene::new(TaskLabel::Claimed);
    let unversioned = SliceQuery {
        version: None,
        ..query()
    };
    assert_eq!(
        resolve(&scene.board, &BranchesSection::default(), &unversioned),
        Err(SliceError::NoVersion)
    );
    let orphan = Ticket {
        epic: None,
        ..ticket(TaskLabel::Claimed)
    };
    scene.board.add_ticket(orphan);
    assert_eq!(
        resolve(&scene.board, &BranchesSection::default(), &query()),
        Err(SliceError::NoEpic {
            task: IssueNumber::from(2)
        })
    );
    let missing = SliceQuery {
        task: IssueNumber::from(9),
        ..query()
    };
    let error = resolve(&scene.board, &BranchesSection::default(), &missing).unwrap_err();
    assert_eq!(error.step(), Some(Step::Ticket));
}

#[test]
fn open_is_idempotent_and_refuses_another_base() {
    let scene = Scene::new(TaskLabel::Claimed);
    let target = scene.target();
    assert_eq!(open(&scene.vcs, &target), Ok(SliceOpening::Opened));
    assert_eq!(open(&scene.vcs, &target), Ok(SliceOpening::AlreadyOpen));
    let moved = SliceTarget {
        base: BaseName::from("feature/v2"),
        ..target
    };
    let error = open(&scene.vcs, &moved).unwrap_err();
    assert!(matches!(
        error,
        SliceError::Port {
            step: Step::Open,
            error: PortError::Conflict(_)
        }
    ));
}

#[test]
fn pr_commits_publishes_and_titles_the_pull_request_with_the_task() {
    let scene = Scene::new(TaskLabel::Claimed);
    let target = scene.target();
    open(&scene.vcs, &target).expect("open");
    scene
        .vcs
        .edit(&target.slice, "src/export.rs", 12, 3, "@@")
        .expect("edit");
    let first = pr(&scene.vcs, &scene.forge, &target).expect("pr");
    assert!(first.commit.is_some());
    assert_eq!(first.publication, Publication::Published);
    let request = scene.forge.request(first.pr).expect("request");
    assert_eq!(request.title, "Export the list (#2)");
    assert_eq!(request.slice, SliceName::from(SLICE));
    assert_eq!(request.base, BaseName::from(BASE));
    let again = pr(&scene.vcs, &scene.forge, &target).expect("a repeat");
    let quiet = PrReport {
        pr: first.pr,
        commit: None,
        publication: Publication::AlreadyPublished,
    };
    assert_eq!(again, quiet);
}

#[test]
fn pr_on_a_slice_never_opened_names_its_step() {
    let scene = Scene::new(TaskLabel::Claimed);
    let error = pr(&scene.vcs, &scene.forge, &scene.target()).unwrap_err();
    assert_eq!(error.step(), Some(Step::Commit));
}

#[test]
fn merge_runs_every_step_then_a_repeat_is_quiet() {
    let scene = Scene::new(TaskLabel::Claimed);
    let (target, number) = scene.reviewed();
    scene.vcs.advance_base(&target.base);
    let command = MergeCommand {
        target: target.clone(),
        pr: Some(number),
    };
    let outcome = merge(scene.ports(), &command).expect("merge");
    let report = MergeReport {
        base_merge: BaseMerge::Merged,
        gate: Some(GateVerdict::Passed),
        publication: Publication::AlreadyPublished,
        pr: number,
        merge: PrMerge::Merged,
    };
    assert_eq!(outcome, MergeOutcome::Merged(report));
    assert_eq!(scene.gate.runs(), [GateLevel::Light]);
    assert_eq!(scene.label(), Some(TaskLabel::Done));
    assert!(!scene.vcs.has_slice(&target.slice));
    let done = MergeCommand {
        target: scene.target(),
        pr: Some(number),
    };
    assert_eq!(merge(scene.ports(), &done), Ok(MergeOutcome::AlreadyDone));
}

#[test]
fn merge_finds_the_open_pull_request_when_none_is_recorded() {
    let scene = Scene::new(TaskLabel::Claimed);
    let (target, number) = scene.reviewed();
    let command = MergeCommand { target, pr: None };
    let MergeOutcome::Merged(report) = merge(scene.ports(), &command).expect("merge") else {
        panic!("expected a merge");
    };
    assert_eq!(report.pr, number);
    assert_eq!(report.merge, PrMerge::Merged);
}

#[test]
fn merge_after_a_merged_pull_request_finishes_the_remaining_steps() {
    let scene = Scene::new(TaskLabel::Claimed);
    let (target, number) = scene.reviewed();
    scene.forge.merge(number).expect("merged by hand");
    let command = MergeCommand {
        target,
        pr: Some(number),
    };
    let MergeOutcome::Merged(report) = merge(scene.ports(), &command).expect("merge") else {
        panic!("expected a merge");
    };
    assert_eq!(report.merge, PrMerge::AlreadyMerged);
    assert_eq!(scene.label(), Some(TaskLabel::Done));
}

#[test]
fn merge_skips_the_gate_when_the_project_has_none() {
    let scene = Scene::new(TaskLabel::Claimed);
    let (target, number) = scene.reviewed();
    let ports = MergePorts {
        gate: None,
        ..scene.ports()
    };
    let command = MergeCommand {
        target,
        pr: Some(number),
    };
    let MergeOutcome::Merged(report) = merge(ports, &command).expect("merge") else {
        panic!("expected a merge");
    };
    assert_eq!(report.gate, None);
    assert!(scene.gate.runs().is_empty());
}

#[test]
fn merge_stops_on_a_base_conflict() {
    let scene = Scene::new(TaskLabel::Claimed);
    let (target, number) = scene.reviewed();
    scene
        .vcs
        .conflict_on_merge(&target.slice, &["src/export.rs"])
        .expect("conflict");
    let command = MergeCommand {
        target: target.clone(),
        pr: Some(number),
    };
    let error = merge(scene.ports(), &command).unwrap_err();
    assert_eq!(
        error,
        SliceError::Conflicted {
            paths: vec!["src/export.rs".to_owned()]
        }
    );
    assert_eq!(error.step(), Some(Step::MergeBase));
    assert!(scene.gate.runs().is_empty());
    assert_eq!(scene.label(), Some(TaskLabel::Claimed));
    assert!(scene.vcs.has_slice(&target.slice));
}

#[test]
fn merge_stops_on_a_failed_gate() {
    let scene = Scene::new(TaskLabel::Claimed);
    let (target, number) = scene.reviewed();
    let failed = GateRun {
        verdict: GateVerdict::Failed,
        output: "1 test failed".to_owned(),
    };
    scene.gate.declare(GateLevel::Light, failed);
    let command = MergeCommand {
        target: target.clone(),
        pr: Some(number),
    };
    let error = merge(scene.ports(), &command).unwrap_err();
    assert_eq!(
        error,
        SliceError::GateFailed {
            output: "1 test failed".to_owned()
        }
    );
    assert_eq!(scene.label(), Some(TaskLabel::Claimed));
    assert!(scene.vcs.has_slice(&target.slice));
    assert_eq!(
        scene.forge.merge(number),
        Ok(PrMerge::Merged),
        "the pull request is still open"
    );
}

#[test]
fn merge_names_the_step_a_port_failed_in() {
    let scene = Scene::new(TaskLabel::Claimed);
    let (target, _number) = scene.reviewed();
    let undeclared = FakeGate::default();
    let ports = MergePorts {
        gate: Some(&undeclared),
        ..scene.ports()
    };
    let command = MergeCommand {
        target: target.clone(),
        pr: Some(PrNumber::from(1)),
    };
    let error = merge(ports, &command).unwrap_err();
    assert!(matches!(
        error,
        SliceError::Port {
            step: Step::Gate,
            error: PortError::Unavailable(_)
        }
    ));
    let unknown = MergeCommand {
        target,
        pr: Some(PrNumber::from(99)),
    };
    let error = merge(scene.ports(), &unknown).unwrap_err();
    assert!(matches!(
        error,
        SliceError::Port {
            step: Step::Merge,
            error: PortError::NotFound(_)
        }
    ));
    assert_eq!(scene.label(), Some(TaskLabel::Claimed));
}

#[test]
fn drop_removes_the_slice_and_a_repeat_is_quiet() {
    let scene = Scene::new(TaskLabel::Claimed);
    let target = scene.target();
    open(&scene.vcs, &target).expect("open");
    drop(&scene.vcs, &target).expect("drop");
    assert!(!scene.vcs.has_slice(&target.slice));
    drop(&scene.vcs, &target).expect("a second drop is quiet");
}
