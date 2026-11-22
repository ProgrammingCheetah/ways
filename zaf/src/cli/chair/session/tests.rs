//! The phase pane end to end: `o` runs a fake objectives action on a real pseudo-terminal through
//! begin's app path and the runner's `OnPty` plan; the action writes its document, says so and runs
//! `zaf begin ready`; the chair sees the ready file, types `/exit`, the action ends, the pane closes
//! and the sign question comes; Enter signs.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{BoardOf, PhasePorts, Phases, RunOf};
use crate::app::actions::{Caller, MINUTE, Nesting, PhaseEnv, RunRequest, Terminal, run_action};
use crate::app::chair::gatherer::GatherEvent;
use crate::cli::chair::keys::Key;
use crate::cli::chair::layout::PaneId;
use crate::cli::chair::model::{Chair, Command, Status, Tab};
use crate::cli::chair::panes::Tone;
use crate::cli::chair::panes::phase::{Ask, PhaseCommand, QUIET_FOR};
use crate::cli::chair::support::ways;
use crate::cli::chair::theme::Theme;
use crate::domain::{DocumentSignature, SignatureState};
use crate::fakes::FakeBoard;
use crate::manifest::ProjectName;
use crate::ports::Board;

/// Set on the child this test binary runs as the fake action's `zaf`.
const CHILD_VAR: &str = "ZAF_CHAIR_TEST_BEGIN_READY";

const MANIFEST: &str =
    "[project]\nname = \"ways\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/ways\"\n";

/// The version the support state's phase item is due for.
const VERSION: &str = "0.5.3";

/// The fake objectives session: writes the document, says it is ready, runs `zaf begin ready`, and
/// ends with 0 once `/exit` is typed, as Claude does; any other line ends it with 3. What it was
/// typed is left in `typed`.
const ACTION: &str = "#!/bin/sh
# zaf description: a fake objectives session
# zaf 1: failed
# zaf terminal: yes
mkdir -p \"$ZAF_RELEASE_DIR\"
printf 'Objectives of the fake release.\\n' > \"$ZAF_RELEASE_DIR/objectives.md\"
echo 'The objectives are written.'
echo 'Ready for Exit.'
zaf begin ready || exit 4
IFS= read -r line
printf '%s' \"$line\" > \"$ZAF_ROOT/typed\"
[ \"$line\" = /exit ] && exit 0
exit 3
";

/// Run by the fake action as its `zaf`: `zaf begin ready` through the CLI, in this test binary,
/// with the `ZAF_READY` the action was given. Does nothing in a plain test run.
#[test]
fn zaf_begin_ready_as_a_child() {
    if std::env::var_os(CHILD_VAR).is_none() {
        return;
    }
    let code = crate::cli::run(["zaf", "begin", "ready"]);
    std::process::exit(i32::from(code));
}

/// A `zaf` on a folder of its own that runs [`zaf_begin_ready_as_a_child`]; the folder.
fn shim(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let (_crate_name, module) = module_path!().split_once("::").expect("a crate's module");
    let test = format!("{module}::zaf_begin_ready_as_a_child");
    let exe = std::env::current_exe().expect("this test binary");
    let zaf = bin.join("zaf");
    std::fs::write(
        &zaf,
        format!(
            "#!/bin/sh\nexec env {CHILD_VAR}=1 '{}' '{test}' --exact --quiet >/dev/null\n",
            exe.display()
        ),
    )
    .expect("the shim");
    std::fs::set_permissions(&zaf, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    bin
}

/// The project: its manifest and the fake objectives action.
fn project(dir: &Path) -> PathBuf {
    let root = dir.join("ways");
    let action = root.join(".zaf/actions/objectives");
    std::fs::create_dir_all(action.parent().expect("parent")).expect("actions");
    std::fs::write(root.join(".zaf.toml"), MANIFEST).expect("manifest");
    std::fs::write(&action, ACTION).expect("action");
    std::fs::set_permissions(&action, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    root
}

/// The ports of the test: a board with no epics, and the runner run from the project with the
/// test's own home and the shim first on the action's PATH.
fn ports(home: PathBuf, bin: &Path) -> PhasePorts {
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let board: Arc<BoardOf> = Arc::new(|_: &crate::composition::Composition| {
        Ok(Box::new(FakeBoard::default()) as Box<dyn Board>)
    });
    let run: Arc<RunOf> = Arc::new(
        move |root: &Path, name: &str, terminal: Terminal, env: PhaseEnv| {
            let request = RunRequest {
                name,
                args: &[],
                cwd: root,
                project_root: Some(root),
                home: &home,
                timeout_minutes: 1,
                minute: MINUTE,
                nesting: Nesting::default(),
                caller: Caller::Engineer,
                only: None,
                slice: None,
                env: BTreeMap::from([("PATH".to_owned(), path.clone())]),
                terminal,
                run: None,
                phase: None,
                vcs: None,
            };
            run_action(&request.with_phase(env))
        },
    );
    PhasePorts { board, run }
}

/// The chair on the project, read, its inbox holding the phase due of 0.5.3.
fn chair_on(root: &Path) -> Chair {
    let mut state = ways();
    state.root = root.to_owned();
    let tabs = vec![Tab::new(ProjectName::from("ways"), root.to_owned())];
    let mut chair = Chair::new(tabs, 0, Theme::new(true), (100, 30));
    chair.gathered(0, GatherEvent::Gathered(Box::new(state)));
    chair
}

/// Runs the loop as the runtime does until `done` holds, or `limit` passes; whether it held. Every
/// screen and note seen on the way is kept.
fn drive(
    chair: &mut Chair,
    phases: &mut Phases,
    seen: &mut Vec<String>,
    done: impl Fn(&Chair) -> bool,
    limit: Duration,
) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        let now = Instant::now();
        let mut commands = phases.poll(chair, now);
        commands.extend(chair.tick(now));
        for command in commands {
            if let Command::Phase(command) = command {
                phases.carry(command);
            }
        }
        if let Some(screen) = chair
            .phase
            .session
            .as_ref()
            .and_then(|session| session.screen.as_ref())
        {
            seen.push(screen.contents());
        }
        if let Some(note) = &chair.note {
            seen.push(note.words.clone());
        }
        if done(chair) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn a_ready_session_closes_its_pane_and_the_sign_question_comes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let root = project(dir.path());
    let bin = shim(dir.path());
    let mut chair = chair_on(&root);
    let mut phases = Phases::new(Some(home.clone()), ports(home, &bin));
    let mut seen = Vec::new();

    let commands = chair.key(Key::Char('o'), Instant::now());
    assert!(
        chair.layout.phase_open(),
        "the pane is over Detail and Live"
    );
    assert_eq!(chair.focus, PaneId::Phase);
    for command in commands {
        let Command::Phase(command) = command else {
            panic!("only the phase is started: {command:?}");
        };
        assert!(matches!(command, PhaseCommand::Start { .. }), "{command:?}");
        phases.carry(command);
    }

    let asked = drive(
        &mut chair,
        &mut phases,
        &mut seen,
        |chair| {
            chair
                .phase
                .session
                .as_ref()
                .is_some_and(|session| matches!(session.ask, Some(Ask::Sign(_))))
        },
        Duration::from_secs(30),
    );

    assert!(asked, "the sign question came: {seen:#?}");
    assert!(
        seen.iter()
            .any(|seen| seen.contains("The objectives are written.")),
        "the session's output was drawn: {seen:#?}"
    );
    assert!(
        seen.iter().any(|seen| seen == "Ready for Exit."),
        "the status line said it: {seen:#?}"
    );
    assert!(!chair.layout.phase_open(), "the pane closed");
    assert_eq!(chair.focus, PaneId::Inbox);
    assert_eq!(
        std::fs::read_to_string(root.join("typed")).expect("the session read a line"),
        "/exit",
        "the chair typed /exit and Enter into the session"
    );
    // The pane closed on its own: keys count again a second later (R1).
    let later = Instant::now() + QUIET_FOR;
    let Status::Phase(question) = chair.status(later) else {
        panic!("the status line asks");
    };
    assert_eq!(
        question.to_string(),
        format!("Sign it? [Y/n]  .teams/releases/{VERSION}/objectives.md")
    );

    let commands = chair.key(Key::Enter, later);
    assert_eq!(
        commands,
        [Command::Phase(PhaseCommand::Answer("y\n".to_owned()))],
        "Enter signs"
    );
    for command in commands {
        if let Command::Phase(command) = command {
            phases.carry(command);
        }
    }
    let settled = drive(
        &mut chair,
        &mut phases,
        &mut seen,
        |chair| chair.phase.session.is_none(),
        Duration::from_secs(10),
    );

    assert!(settled, "{seen:#?}");
    let note = chair.note.as_ref().expect("a note");
    assert_eq!(note.tone, Tone::Done);
    assert_eq!(
        note.words,
        format!(
            "signed .teams/releases/{VERSION}/objectives.md · the design is due next: o runs it"
        )
    );
    assert!(chair.phase.follow, "the next read selects the design item");
    let document = root.join(format!(".teams/releases/{VERSION}/objectives.md"));
    let text = std::fs::read_to_string(&document).expect("the objectives");
    assert!(
        matches!(
            DocumentSignature::read(&text),
            SignatureState::Signed { .. }
        ),
        "{text}"
    );
    assert!(
        !root
            .join(format!(".zaf/begin/{VERSION}/objectives.ready"))
            .exists(),
        "the ready file is gone once the signature is settled"
    );
}

/// An objectives session that ignores the hang-up: only a kill ends it. It leaves its pid.
const DEAF: &str = "#!/bin/sh
# zaf description: a session that ignores SIGHUP
# zaf 1: failed
# zaf terminal: yes
trap '' HUP
echo $$ > \"$ZAF_ROOT/pid\"
echo 'listening'
exec sleep 30
";

#[test]
fn closing_hangs_up_a_session_then_kills_it_and_waits_for_its_thread() {
    let dir = tempfile::tempdir().expect("tempdir");
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).expect("home");
    let root = project(dir.path());
    std::fs::write(root.join(".zaf/actions/objectives"), DEAF).expect("the deaf action");
    let bin = shim(dir.path());
    let mut chair = chair_on(&root);
    let mut phases = Phases::new(Some(home.clone()), ports(home, &bin));
    let mut seen = Vec::new();
    for command in chair.key(Key::Char('o'), Instant::now()) {
        if let Command::Phase(command) = command {
            phases.carry(command);
        }
    }
    let pid_file = root.join("pid");
    let listening = drive(
        &mut chair,
        &mut phases,
        &mut seen,
        |chair| {
            chair
                .phase
                .session
                .as_ref()
                .and_then(|session| session.screen.as_ref())
                .is_some_and(|screen| screen.contents().contains("listening"))
        },
        Duration::from_secs(30),
    );
    assert!(listening, "{seen:#?}");
    let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
        .expect("its pid")
        .trim()
        .parse()
        .expect("a pid");

    let started = Instant::now();
    phases.end();

    assert!(
        started.elapsed() < super::HANG_UP_WAIT + super::THREAD_WAIT + Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    // SAFETY: kill with signal 0 only asks whether the process is there.
    let there = unsafe { libc::kill(pid, 0) } == 0;
    assert!(
        !there,
        "the session that ignored the hang-up was killed and reaped"
    );
}
