use std::cell::Cell;

use super::*;

#[test]
fn the_exit_code_is_recorded() {
    let dir = tempfile::tempdir().expect("temp dir");

    record_exit(dir.path(), 3).expect("record");

    // The decimal code and a newline, and no staging file left behind.
    assert_eq!(
        std::fs::read_to_string(dir.path().join(EXIT_FILE)).expect("exit"),
        "3\n"
    );
    assert!(!dir.path().join("exit.new").exists());
    assert_eq!(exit_code(Some(3), None), 3);
    assert_eq!(exit_code(Some(0), None), 0);
}

#[test]
fn a_killed_phase_records_its_signal() {
    // SIGTERM is 15, so the shell's convention is 143; SIGKILL is 137.
    assert_eq!(exit_code(None, Some(15)), 143);
    assert_eq!(exit_code(None, Some(9)), 137);
    let dir = tempfile::tempdir().expect("temp dir");

    record_exit(dir.path(), exit_code(None, Some(15))).expect("record");

    assert_eq!(
        std::fs::read_to_string(dir.path().join(EXIT_FILE)).expect("exit"),
        "143\n"
    );
}

#[test]
fn the_exit_is_written_before_post_phase() {
    let dir = tempfile::tempdir().expect("temp dir");
    let seen = Cell::new(None);

    // What follows the exit sees the file already there, whole.
    end(dir.path(), 7, &HeadEnd::Unknown, &|| {
        seen.set(std::fs::read_to_string(dir.path().join(EXIT_FILE)).ok());
    })
    .expect("end");

    assert_eq!(seen.take().as_deref(), Some("7\n"));
}

#[test]
fn what_follows_still_runs_when_the_exit_cannot_be_written() {
    let dir = tempfile::tempdir().expect("temp dir");
    let ran = Cell::new(false);

    // A run folder that is gone cannot take the file; the error comes back, the rest still runs.
    let result = end(&dir.path().join("gone"), 1, &HeadEnd::Unknown, &|| {
        ran.set(true)
    });

    assert!(result.is_err());
    assert!(ran.get());
}

/// A project on disk whose task 7, of a signed and challenged epic, stands at `merge`, a `zaf`-kind
/// action: what two ticks that overlap both read.
struct Inline {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    state_dir: PathBuf,
    board: crate::fakes::FakeBoard,
    forge: crate::fakes::FakeForge,
    manifest: Manifest,
}

impl Inline {
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("project");
        let home = dir.path().join("home");
        let state_dir = root.join(".zaf");
        std::fs::create_dir_all(state_dir.join("actions")).expect("actions");
        std::fs::create_dir_all(&home).expect("home");
        let file = state_dir.join("actions/merge");
        std::fs::write(
            &file,
            "#!/bin/sh\n# zaf description: merge\n# zaf 1: failed\n\
             # zaf kind: zaf\n# zaf ends: merged\nexit 0\n",
        )
        .expect("action");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        let board = crate::fakes::FakeBoard::default();
        board.add_challenged_epic(TicketId::from(2), "Issue 2");
        board.add_ticket(crate::domain::Ticket {
            id: TicketId::from(7),
            epic: Some(TicketId::from(2)),
            title: "Issue 7".into(),
            body: "Goal: do it\nOwned paths: src/\nInterfaces: none\nDesign pages: none\n\
                   Acceptance:\n  - it works\nTests: it_works\nSecurity trigger: no\n\
                   Model: sonnet\nSize: S\nBlocked by: none\n"
                .into(),
            state: Some(crate::domain::TicketState::Claimed),
            url: "".into(),
            kind: None,
        });
        let manifest = "[project]\nname = \"bookshelf\"\n\n\
                        [board]\nkind = \"github-issues\"\nrepo = \"example/board\"\n\n\
                        [phases]\nTask = [\"merge\"]\n"
            .parse()
            .expect("manifest parses");
        Self {
            _dir: dir,
            root,
            home,
            state_dir,
            board,
            forge: crate::fakes::FakeForge::default(),
            manifest,
        }
    }

    /// Opens the run folder of `merge` on task 7 for the tick `this`, every process in `alive`
    /// running.
    fn open(&self, this: &Holder, alive: &dyn Fn(&Holder) -> bool) -> Result<PathBuf, PhaseError> {
        let ports = Ports {
            board: &self.board,
            forge: &self.forge,
            manifest: &self.manifest,
            root: &self.root,
            home: &self.home,
            state_dir: &self.state_dir,
            alive,
            version: None,
        };
        open_inline(&ports, TicketId::from(7), &ActionName::from("merge"), this)
    }
}

fn tick_process(pid: u32) -> Holder {
    Holder {
        pid,
        start: "100".to_owned(),
    }
}

#[test]
fn a_second_tick_finds_the_first_ticks_inline_run_and_runs_nothing() {
    let scene = Inline::new();
    let (first, second) = (tick_process(10), tick_process(20));
    let alive = |holder: &Holder| *holder == first || *holder == second;

    // Both ticks read the lane before either run began; the first opens its folder.
    let run = scene
        .open(&first, &alive)
        .expect("the first tick runs merge");
    assert_eq!(
        std::fs::read_to_string(run.join(record::INLINE_FILE)).expect("the inline mark"),
        first.to_string()
    );

    // The second asks the gate again under the lock, and finds the first run there, running.
    let refused = scene.open(&second, &alive);
    assert!(
        matches!(&refused, Err(PhaseError::Refused(reason)) if reason.contains("merge")),
        "{refused:?}"
    );
    let folders = std::fs::read_dir(scene.state_dir.join(PHASES_DIR))
        .expect("the run folders")
        .count();
    assert_eq!(folders, 1, "no second folder");
}

#[test]
fn an_inline_run_whose_tick_is_gone_no_longer_reads_running() {
    let scene = Inline::new();
    let first = tick_process(10);
    scene
        .open(&first, &|holder| *holder == first)
        .expect("opened");

    // The tick died mid-run: its mark names no live process, and the run reads as ended. The gate
    // then decides from the run as it stands, not from a run that holds the ticket for ever.
    let runs = record::read_runs(
        &scene.state_dir.join(PHASES_DIR),
        TicketId::from(7),
        &|_| false,
    )
    .expect("the runs read");
    assert_eq!(runs.len(), 1);
    assert!(!runs[0].alive, "{runs:?}");
    // While it lives, it reads running.
    let runs = record::read_runs(
        &scene.state_dir.join(PHASES_DIR),
        TicketId::from(7),
        &|holder| *holder == first,
    )
    .expect("the runs read");
    assert!(runs[0].alive, "{runs:?}");
}
