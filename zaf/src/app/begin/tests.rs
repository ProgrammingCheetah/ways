use std::path::Path;
use std::time::UNIX_EPOCH;

use super::*;
use crate::app::actions::{Caller, Nesting, RunRequest, run_action};
use crate::app::spawn::runs;
use crate::app::spawn::tick::PlannedLaunch;
use crate::domain::{
    DocumentSignature, Instance, InstanceNumber, RoleName, Signature, TicketId, with_release,
};
use crate::fakes::FakeBoard;
use crate::manifest::Manifest;
use crate::ports::Holder;

const EPIC: u64 = 10;
const VERSION: &str = "0.6.0";
const MANIFEST: &str =
    "[project]\nname = \"bookshelf\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/b\"\n";

fn holder(pid: u32) -> Holder {
    Holder {
        pid,
        start: "1000".to_owned(),
    }
}

fn all_alive(_: &Holder) -> bool {
    true
}

fn folder_in(root: &Path) -> ReleaseFolder {
    let manifest: Manifest = MANIFEST.parse().expect("the manifest loads");
    ReleaseFolder::of(root, &manifest, VERSION)
}

fn write_signed(folder: &ReleaseFolder, name: &str, body: &str) {
    folder.ensure().expect("ensure");
    std::fs::write(
        folder.path().join(name),
        DocumentSignature::sign(body, UNIX_EPOCH),
    )
    .expect("write");
}

/// A project whose objectives and design are signed.
fn signed_folder(root: &Path) -> ReleaseFolder {
    let folder = folder_in(root);
    write_signed(&folder, "objectives.md", "Objectives.\n");
    write_signed(&folder, "design.md", "Design.\n");
    folder
}

/// A board with one open epic of the version, signed or not.
fn board_with_epic(signed: bool) -> FakeBoard {
    let board = FakeBoard::default();
    let id = TicketId::from(EPIC);
    board.add_epic(id, "tickets");
    let body = with_release(VERSION, "Goal: one ticket shape.");
    board.set_body(id, &body);
    if signed {
        board
            .sign(id, &Signature::of(&body.as_str().into(), UNIX_EPOCH))
            .expect("signs");
    }
    board
}

fn begin_as(
    board: &FakeBoard,
    folder: &ReleaseFolder,
    state_dir: &Path,
    ancestors: &[Holder],
    terminal: Terminal,
) -> BeginOutcome {
    begin_phase(board, folder, state_dir, ancestors, terminal, None)
}

fn begin_phase(
    board: &FakeBoard,
    folder: &ReleaseFolder,
    state_dir: &Path,
    ancestors: &[Holder],
    terminal: Terminal,
    phase: Option<Phase>,
) -> BeginOutcome {
    let guard = SignGuard {
        state_dir,
        ancestors,
        alive: &all_alive,
    };
    let request = BeginRequest {
        version: VERSION,
        folder,
        terminal,
        guard: &guard,
        phase,
    };
    begin(board, &request).expect("begins")
}

fn launch() -> PlannedLaunch {
    PlannedLaunch {
        instance: Instance {
            role: RoleName::from("team"),
            epic: TicketId::from(EPIC),
            number: InstanceNumber::from(3),
        },
    }
}

#[test]
fn begin_refuses_inside_an_instance() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = signed_folder(dir.path());
    let state = dir.path().join("state");
    runs::reserve(&state, &launch(), &holder(4242), "2026-11-09T08:00:00Z").expect("reserves");
    let outcome = begin_as(
        &board_with_epic(false),
        &folder,
        &state,
        &[holder(5000), holder(4242), holder(1)],
        Terminal::Present,
    );
    assert_eq!(
        outcome,
        BeginOutcome::Refused(BeginRefusal::InsideInstance {
            instance: "team instance 3 on #10".to_owned()
        })
    );
    let BeginOutcome::Refused(refusal) = outcome else {
        unreachable!()
    };
    assert!(!refusal.to_string().contains('\n'), "one sentence");
}

#[test]
fn begin_refuses_without_a_terminal() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = signed_folder(dir.path());
    let outcome = begin_as(
        &board_with_epic(false),
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Absent,
    );
    assert_eq!(outcome, BeginOutcome::Refused(BeginRefusal::NoTerminal));
}

#[test]
fn begin_with_nothing_due_lists_the_epics_to_sign() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = signed_folder(dir.path());
    let outcome = begin_as(
        &board_with_epic(false),
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Present,
    );
    let BeginOutcome::NothingDue(epics) = outcome else {
        panic!("expected nothing due, got {outcome:?}");
    };
    assert_eq!(epics.len(), 1);
    assert_eq!(
        epics[0].render(),
        "#10 unsigned 0 of 0  tickets\n  zaf epic sign #10"
    );
}

#[test]
fn begin_omits_the_sign_command_for_a_signed_epic() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = signed_folder(dir.path());
    let outcome = begin_as(
        &board_with_epic(true),
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Present,
    );
    let BeginOutcome::NothingDue(epics) = outcome else {
        panic!("expected nothing due, got {outcome:?}");
    };
    assert_eq!(epics.len(), 1);
    assert!(!epics[0].unsigned);
    assert!(epics[0].render().starts_with("#10 signed "));
    assert!(!epics[0].render().contains("zaf epic sign"));
}

#[test]
fn begin_with_a_phase_due_exits_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    let outcome = begin_as(
        &board_with_epic(false),
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Present,
    );
    assert_eq!(
        outcome,
        BeginOutcome::Due {
            phase: Phase::Objectives,
            malformed: Vec::new()
        }
    );
}

/// Writes the project action `name` under `root`, its body `body`, with `3` declared as a code.
fn project_action(root: &Path, name: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(".zaf/actions").join(name);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let text =
        format!("#!/bin/sh\n# zaf description: d\n# zaf 1: failed\n# zaf 3: three\n{body}\n");
    std::fs::write(&path, text).expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// An action that records the four phase variables, one per line, `unset` for one it lacks, and
/// each run on a line of `runs`.
const RECORD: &str = "echo run >> \"$ZAF_ROOT/runs\"\nprintf '%s\\n' \"${ZAF_VERSION-unset}\" \"${ZAF_PHASE-unset}\" \"${ZAF_RELEASE_DIR-unset}\" \"${ZAF_PHASE_INPUT-unset}\" > \"$ZAF_ROOT/seen\"";

/// [`run_phase`] with the real action runner, the project at `root` and no home.
fn run_due(root: &Path, folder: &ReleaseFolder, phase: Phase) -> PhaseRun {
    let home = root.join("home");
    let run = |name: &str, env: PhaseEnv| {
        run_action(
            &RunRequest {
                name,
                args: &[],
                cwd: root,
                project_root: Some(root),
                home: &home,
                timeout_minutes: 1,
                minute: crate::app::actions::MINUTE,
                nesting: Nesting::default(),
                caller: Caller::Engineer,
                only: None,
                slice: None,
                env: std::collections::BTreeMap::new(),
                terminal: Terminal::Absent,
                run: None,
                phase: None,
            }
            .with_phase(env),
        )
    };
    run_phase(folder, VERSION, phase, &run).expect("runs")
}

fn seen(root: &Path) -> Vec<String> {
    std::fs::read_to_string(root.join("seen"))
        .expect("seen")
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn begin_runs_the_objectives_action_with_its_phase_env() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    project_action(dir.path(), "objectives", RECORD);

    assert_eq!(
        run_due(dir.path(), &folder, Phase::Objectives),
        PhaseRun::Done
    );

    let runs = std::fs::read_to_string(dir.path().join("runs")).expect("runs");
    assert_eq!(runs.lines().count(), 1, "it ran once");
    let seen = seen(dir.path());
    assert_eq!(seen[0], VERSION);
    assert_eq!(seen[1], "objectives");
    assert!(Path::new(&seen[2]).is_absolute(), "{seen:?}");
    assert!(seen[2].ends_with(".teams/releases/0.6.0"), "{seen:?}");
    assert_eq!(seen[3], "unset", "objectives has no input");
}

#[test]
fn begin_gives_design_the_objectives_as_input() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_signed(&folder, "objectives.md", "Objectives.\n");
    project_action(dir.path(), "design", RECORD);

    assert_eq!(run_due(dir.path(), &folder, Phase::Design), PhaseRun::Done);

    let seen = seen(dir.path());
    assert_eq!(seen[1], "design");
    assert_eq!(
        Path::new(&seen[3]),
        folder.path().join("objectives.md"),
        "{seen:?}"
    );
}

#[test]
fn begin_gives_design_the_scope_document_when_there_is_no_objectives_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_signed(&folder, "scope.md", "Scope.\n");
    assert_eq!(
        phase_env(&folder, VERSION, Phase::Design).input,
        Some(folder.path().join("scope.md"))
    );
}

#[test]
fn begin_makes_a_missing_release_folder() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    project_action(dir.path(), "objectives", RECORD);
    assert!(!folder.path().exists());

    run_due(dir.path(), &folder, Phase::Objectives);

    assert!(folder.path().is_dir());
}

#[test]
fn begin_reports_a_failed_phase_action() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    project_action(dir.path(), "objectives", "exit 3");

    let PhaseRun::Undone { outcome } = run_due(dir.path(), &folder, Phase::Objectives) else {
        panic!("expected the phase undone");
    };
    assert_eq!(outcome.exit, 3);
}

#[test]
fn begin_reports_a_phase_action_that_is_not_found() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());

    let PhaseRun::NotRun { outcome } = run_due(dir.path(), &folder, Phase::Design) else {
        panic!("expected the action not run");
    };
    let headline = outcome.headline().expect("headline");
    assert!(headline.starts_with("NOT_RUN design: "), "{headline}");
}

/// The answer the Engineer types, as the injected reader hands it back.
fn answering(answer: &'static str) -> impl Fn(&str) -> std::io::Result<String> {
    move |_prompt| Ok(answer.to_owned())
}

/// [`complete_phase`] for `phase` with the Engineer answering `answer`.
fn complete(
    board: &FakeBoard,
    folder: &ReleaseFolder,
    phase: Phase,
    answer: &'static str,
) -> Completion {
    complete_phase(
        board,
        folder,
        VERSION,
        phase,
        UNIX_EPOCH,
        &answering(answer),
    )
    .expect("completes")
}

fn write_plain(folder: &ReleaseFolder, name: &str, text: &str) {
    folder.ensure().expect("ensure");
    std::fs::write(folder.path().join(name), text).expect("write");
}

fn is_signed(folder: &ReleaseFolder, name: &str) -> bool {
    let text = std::fs::read_to_string(folder.path().join(name)).expect("read");
    matches!(
        DocumentSignature::read(&text),
        crate::domain::SignatureState::Signed { .. }
    )
}

/// A board with one open epic of the version whose whole body is `body`.
fn board_with_body(body: &str) -> FakeBoard {
    let board = FakeBoard::default();
    let id = TicketId::from(EPIC);
    board.add_epic(id, "tickets");
    board.set_body(id, body);
    board
}

#[test]
fn begin_says_the_objectives_were_not_written() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    folder.ensure().expect("ensure");
    let board = FakeBoard::default();
    assert_eq!(
        complete(&board, &folder, Phase::Objectives, "y\n"),
        Completion::Missing(vec!["the objectives were not written".to_owned()])
    );
    write_plain(&folder, "objectives.md", "  \n\n");
    assert_eq!(
        complete(&board, &folder, Phase::Objectives, "y\n"),
        Completion::Missing(vec!["the objectives were not written".to_owned()]),
        "an empty body is not written"
    );
    assert!(!is_signed(&folder, "objectives.md"));
}

#[test]
fn begin_signs_the_objectives_on_yes_and_opens_design() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "objectives.md", "The objectives.\n");
    let board = board_with_epic(false);

    let done = complete(&board, &folder, Phase::Objectives, "y\n");

    assert_eq!(
        done,
        Completion::Signed(folder.path().join("objectives.md"))
    );
    let text = std::fs::read_to_string(folder.path().join("objectives.md")).expect("read");
    assert!(text.starts_with("---\nsigned: "), "{text}");
    assert_eq!(DocumentSignature::body(&text), "The objectives.\n");
    assert!(is_signed(&folder, "objectives.md"));
    let next = begin_as(
        &board,
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Present,
    );
    assert_eq!(
        next,
        BeginOutcome::Due {
            phase: Phase::Design,
            malformed: Vec::new()
        }
    );
}

#[test]
fn complete_phase_signs_an_unchanged_document() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "objectives.md", "The objectives.\n");

    let done = complete(&board_with_epic(false), &folder, Phase::Objectives, "y\n");

    assert_eq!(
        done,
        Completion::Signed(folder.path().join("objectives.md"))
    );
    assert!(is_signed(&folder, "objectives.md"));
}

#[test]
fn complete_phase_leaves_a_document_edited_during_the_prompt_unsigned() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "objectives.md", "The objectives.\n");
    let path = folder.path().join("objectives.md");
    let rewrite = |_prompt: &str| {
        std::fs::write(&path, "The objectives, rewritten.\n")?;
        Ok("y\n".to_owned())
    };

    let done = complete_phase(
        &board_with_epic(false),
        &folder,
        VERSION,
        Phase::Objectives,
        UNIX_EPOCH,
        &rewrite,
    )
    .expect("completes");

    assert_eq!(done, Completion::Changed(path.clone()));
    assert_eq!(
        std::fs::read_to_string(&path).expect("read"),
        "The objectives, rewritten.\n",
        "byte for byte, no block"
    );
}

#[test]
fn begin_leaves_the_objectives_unsigned_on_no() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "objectives.md", "The objectives.\n");
    for answer in ["n\n", "\n", "maybe\n", ""] {
        assert_eq!(
            complete(&board_with_epic(false), &folder, Phase::Objectives, answer),
            Completion::NotSigned(folder.path().join("objectives.md")),
            "{answer:?}"
        );
        let text = std::fs::read_to_string(folder.path().join("objectives.md")).expect("read");
        assert_eq!(text, "The objectives.\n", "no block");
    }
}

#[test]
fn begin_names_a_missing_epic_after_design() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "design.md", "The design.\n");

    let done = complete(&FakeBoard::default(), &folder, Phase::Design, "y\n");

    let Completion::Missing(missing) = done else {
        panic!("expected missing, got {done:?}");
    };
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(missing[0].contains("no open epic of 0.6.0"), "{missing:?}");
    assert!(!is_signed(&folder, "design.md"));
}

#[test]
fn begin_names_a_missing_design_document() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    folder.ensure().expect("ensure");

    let done = complete(&board_with_epic(false), &folder, Phase::Design, "y\n");

    let Completion::Missing(missing) = done else {
        panic!("expected missing, got {done:?}");
    };
    assert_eq!(missing.len(), 1, "{missing:?}");
    assert!(
        missing[0].contains("design.md was not written"),
        "{missing:?}"
    );
}

#[test]
fn begin_ignores_an_epic_with_only_its_release_line() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "design.md", "The design.\n");
    for body in [
        "Release: 0.6.0",
        "Release: 0.6.0\n",
        "Release: 0.6.0\n\n  \n",
    ] {
        let done = complete(&board_with_body(body), &folder, Phase::Design, "y\n");
        assert!(matches!(done, Completion::Missing(_)), "{body:?}: {done:?}");
    }
    assert!(!is_signed(&folder, "design.md"));
}

#[test]
fn begin_signs_the_design_and_lists_the_epics() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_signed(&folder, "objectives.md", "Objectives.\n");
    write_plain(&folder, "design.md", "The design.\n");
    let board = board_with_epic(false);

    let done = complete(&board, &folder, Phase::Design, "y\n");

    assert_eq!(done, Completion::Signed(folder.path().join("design.md")));
    assert!(is_signed(&folder, "design.md"));
    let outcome = begin_as(
        &board,
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Present,
    );
    let BeginOutcome::NothingDue(epics) = outcome else {
        panic!("expected nothing due, got {outcome:?}");
    };
    assert_eq!(epics.len(), 1);
}

#[test]
fn begin_asks_nothing_of_a_document_already_signed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_signed(&folder, "design.md", "The design.\n");
    let ask = |_: &str| -> std::io::Result<String> { panic!("no prompt for a signed document") };
    let done = complete_phase(
        &board_with_epic(false),
        &folder,
        VERSION,
        Phase::Design,
        UNIX_EPOCH,
        &ask,
    )
    .expect("completes");
    assert_eq!(done, Completion::Signed(folder.path().join("design.md")));
}

#[test]
fn begin_signing_again_replaces_the_block() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "objectives.md", "The objectives.\n");
    complete(&board_with_epic(false), &folder, Phase::Objectives, "y\n");
    let path = folder.path().join("objectives.md");
    let edited = std::fs::read_to_string(&path)
        .expect("read")
        .replace("The objectives.", "Edited objectives.");
    std::fs::write(&path, edited).expect("write");

    complete(&board_with_epic(false), &folder, Phase::Objectives, "y\n");

    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(text.matches("signed: ").count(), 1, "{text}");
    assert_eq!(DocumentSignature::body(&text), "Edited objectives.\n");
    assert!(is_signed(&folder, "objectives.md"));
}

#[test]
fn begin_writes_no_signoff() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "objectives.md", "The objectives.\n");
    write_plain(&folder, "design.md", "The design.\n");
    let board = board_with_epic(false);
    complete(&board, &folder, Phase::Objectives, "y\n");
    complete(&board, &folder, Phase::Design, "n\n");
    complete(&board, &folder, Phase::Design, "y\n");
    complete(&FakeBoard::default(), &folder, Phase::Design, "y\n");
    assert!(!folder.has_signoff());
    assert!(!folder.path().join("signoff.md").exists());
}

#[test]
fn begin_phase_objectives_reopens_a_signed_document() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = signed_folder(dir.path());
    let board = board_with_epic(true);
    let outcome = begin_phase(
        &board,
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Present,
        Some(Phase::Objectives),
    );
    assert_eq!(
        outcome,
        BeginOutcome::Due {
            phase: Phase::Objectives,
            malformed: Vec::new()
        }
    );
    // The action edits the signed document; signing again leaves one fresh block.
    let path = folder.path().join("objectives.md");
    let edited = std::fs::read_to_string(&path)
        .expect("read")
        .replace("Objectives.", "Changed objectives.");
    std::fs::write(&path, edited).expect("write");
    let done = complete(&board, &folder, Phase::Objectives, "y\n");
    assert_eq!(done, Completion::Signed(path.clone()));
    let text = std::fs::read_to_string(&path).expect("read");
    assert_eq!(text.matches("signed: ").count(), 1, "{text}");
    assert_eq!(DocumentSignature::body(&text), "Changed objectives.\n");
    assert!(is_signed(&folder, "objectives.md"));
}

#[test]
fn begin_phase_design_is_refused_without_signed_objectives() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = folder_in(dir.path());
    write_plain(&folder, "objectives.md", "The objectives.\n");
    let outcome = begin_phase(
        &board_with_epic(false),
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Present,
        Some(Phase::Design),
    );
    assert_eq!(
        outcome,
        BeginOutcome::Refused(BeginRefusal::ObjectivesNotSigned)
    );
    let BeginOutcome::Refused(refusal) = outcome else {
        unreachable!()
    };
    assert!(!refusal.to_string().contains('\n'), "one sentence");
}

#[test]
fn begin_phase_design_runs_with_signed_objectives() {
    let dir = tempfile::tempdir().expect("temp dir");
    let folder = signed_folder(dir.path());
    let outcome = begin_phase(
        &board_with_epic(true),
        &folder,
        &dir.path().join("state"),
        &[holder(1)],
        Terminal::Present,
        Some(Phase::Design),
    );
    assert_eq!(
        outcome,
        BeginOutcome::Due {
            phase: Phase::Design,
            malformed: Vec::new()
        }
    );
    project_action(dir.path(), "design", RECORD);
    assert_eq!(run_due(dir.path(), &folder, Phase::Design), PhaseRun::Done);
    let seen = seen(dir.path());
    assert_eq!(seen[1], "design");
    assert_eq!(
        Path::new(&seen[3]),
        folder.path().join("objectives.md"),
        "{seen:?}"
    );
}
