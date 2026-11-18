use std::cell::Cell;
use std::time::Duration;

use super::*;
use crate::domain::InstanceNumber;
use crate::fakes::{FakeForge, PrRequest};
use crate::ports::{BaseName, SliceName};

fn task() -> TicketId {
    TicketId::from(4)
}

fn instance(action: &str) -> Instance {
    Instance {
        role: action.into(),
        epic: TicketId::from(1),
        number: InstanceNumber::from(3),
    }
}

/// Opens a run folder of `action` for task 4 at `head` under `dir`.
fn opened(dir: &Path, action: &str, head: Option<&str>) -> PathBuf {
    let action = ActionName::from(action);
    let head = head.map(CommitId::from);
    open(
        dir,
        &Opening {
            task: task(),
            action: &action,
            head: head.as_ref(),
            size: TaskSize::S,
            instance: &instance(action.as_str()),
            inline: None,
        },
    )
    .expect("the folder opens")
}

fn never_alive(_: &Holder) -> bool {
    false
}

fn always_alive(_: &Holder) -> bool {
    true
}

fn read_one(dir: &Path, alive: &dyn Fn(&Holder) -> bool) -> RunRecord {
    let mut runs = read_runs(dir, task(), alive).expect("the runs read");
    assert_eq!(runs.len(), 1, "{runs:?}");
    runs.remove(0)
}

#[test]
fn an_opened_folder_holds_the_opening_files_named_by_instance_and_task() {
    let dir = tempfile::tempdir().expect("temp dir");

    let run = opened(dir.path(), "implement", Some("h1"));

    assert_eq!(
        run.file_name().and_then(|name| name.to_str()),
        Some("implement-instance-3-on-1-task-4")
    );
    let read = |name: &str| std::fs::read_to_string(run.join(name)).expect("a file");
    assert_eq!(read(ACTION_FILE), "implement");
    assert_eq!(read(HEAD_FILE), "h1");
    assert_eq!(read(SIZE_FILE), "S");
    assert_eq!(read(INSTANCE_FILE), "implement instance 3 on #1");
    assert_eq!(read(TASK_FILE), "4");
    // Opened and not yet started: no process, no end.
    let record = read_one(dir.path(), &always_alive);
    assert!(!record.alive);
    assert_eq!(record.exit, None);
    assert_eq!(record.head, Some(CommitId::from("h1")));
    assert_eq!(record.head_end, HeadEnd::Unknown);
}

#[test]
fn the_launcher_is_recorded_only_while_no_process_is() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = opened(dir.path(), "review", Some("h1"));
    let launcher = Holder::parse("999998 1").expect("a holder");
    let action = Holder::parse("999999 2").expect("a holder");
    let pid = || std::fs::read_to_string(run.join(PID_FILE)).expect("pid");
    let files = || std::fs::read_dir(&run).expect("run").count();
    let opening = files();

    // No record yet: the launcher is the run's process.
    record_launcher(&run, &launcher).expect("launcher");
    assert_eq!(pid(), launcher.to_string());

    // The launcher replaces it with the action's process, as it always did.
    record_pid(&run, &action).expect("pid");
    assert_eq!(pid(), action.to_string());

    // A verb that comes after the launcher's record leaves it, and leaves no staging file.
    record_launcher(&run, &launcher).expect("launcher");
    assert_eq!(pid(), action.to_string());
    assert_eq!(files(), opening + 1, "the pid file alone was added");
}

#[test]
fn a_phase_that_ended_with_no_scheduler_is_read_from_its_record() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = opened(dir.path(), "review", Some("h1"));
    record_pid(&run, &Holder::parse("999999 1").expect("a holder")).expect("pid");
    std::fs::write(run.join(OUTPUT_FILE), "checking\nfound two things\n\n").expect("output");
    assert!(record_question(&run, task(), TicketId::from(30)).expect("asked"));
    assert!(record_question(&run, task(), TicketId::from(31)).expect("asked"));

    end(&run, 0, &HeadEnd::Left(Some(CommitId::from("h2"))), &|| {}).expect("end");

    // Nothing runs any more: the folder alone says how the run ended.
    let record = read_one(dir.path(), &never_alive);
    assert_eq!(record.action, ActionName::from("review"));
    assert_eq!(record.head, Some(CommitId::from("h1")));
    assert_eq!(record.head_end, HeadEnd::Left(Some(CommitId::from("h2"))));
    assert!(!record.alive && !record.live());
    let exit = record.exit.expect("an exit");
    assert_eq!(exit.code, ExitCode::from(0));
    assert!(exit.at >= record.started_at);
    assert_eq!(
        record.questions,
        BTreeSet::from([TicketId::from(30), TicketId::from(31)])
    );
    assert_eq!(record.last_line, Some(OutputLine::from("found two things")));
    // Its report is the facts it left: the head it left, the questions, the exit.
    let report = record.report();
    assert_eq!(report.head, Some(CommitId::from("h2")));
    assert_eq!(report.questions.len(), 2);
    assert_eq!(report.exit, Some(exit));
}

#[test]
fn a_folder_written_before_this_record_still_reads() {
    let dir = tempfile::tempdir().expect("temp dir");
    // As the launcher of 0.3 wrote it: no `head-end`, no `questions`.
    let run = dir.path().join("implement-instance-1-on-1-task-4");
    std::fs::create_dir_all(&run).expect("folder");
    std::fs::write(run.join("action"), "implement").expect("action");
    std::fs::write(run.join("head"), "").expect("head");
    std::fs::write(run.join("task"), "4").expect("task");
    std::fs::write(run.join("pid"), "999999 1").expect("pid");
    std::fs::write(run.join("exit"), "3\n").expect("exit");

    let record = read_one(dir.path(), &never_alive);

    assert_eq!(record.head, None);
    assert_eq!(record.head_end, HeadEnd::Unknown);
    assert!(record.questions.is_empty());
    assert_eq!(record.exit.map(|exit| exit.code), Some(ExitCode::from(3)));
    assert_eq!(record.last_line, None);
    // An old folder's run is read at the head it started at.
    assert_eq!(record.left(), None);
}

#[test]
fn a_live_process_with_no_exit_runs_and_an_exit_ends_it_whatever_the_process_table_says() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = opened(dir.path(), "implement", None);
    record_pid(&run, &Holder::parse("999999 1").expect("a holder")).expect("pid");
    assert!(read_one(dir.path(), &always_alive).live());
    assert!(!read_one(dir.path(), &never_alive).live());

    // An exit that holds no number is no exit code, and the run has ended.
    std::fs::write(run.join(EXIT_FILE), "garbled").expect("exit");
    let record = read_one(dir.path(), &always_alive);
    assert!(!record.live());
    assert_eq!(record.exit, None);
}

#[test]
fn the_head_left_is_written_whole_and_none_reads_as_no_head_and_unknown_as_no_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = opened(dir.path(), "implement", None);

    record_head_end(&run, &HeadEnd::Unknown).expect("nothing to write");
    assert!(!run.join(HEAD_END_FILE).exists());
    assert_eq!(
        read_one(dir.path(), &never_alive).head_end,
        HeadEnd::Unknown
    );

    record_head_end(&run, &HeadEnd::Left(None)).expect("written");
    assert_eq!(
        std::fs::read_to_string(run.join(HEAD_END_FILE)).expect("head-end"),
        ""
    );
    assert_eq!(
        read_one(dir.path(), &never_alive).head_end,
        HeadEnd::Left(None)
    );

    record_head_end(&run, &HeadEnd::Left(Some(CommitId::from("abc")))).expect("written");
    assert_eq!(
        read_one(dir.path(), &never_alive).head_end,
        HeadEnd::Left(Some(CommitId::from("abc")))
    );
    let staged: Vec<_> = std::fs::read_dir(&run)
        .expect("the folder")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".new"))
        .collect();
    assert!(staged.is_empty(), "{staged:?}");
}

#[test]
fn the_head_is_recorded_before_the_exit_and_both_before_what_follows() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = opened(dir.path(), "implement", None);
    let seen = Cell::new(None);

    end(&run, 7, &HeadEnd::Left(Some(CommitId::from("h9"))), &|| {
        let head = std::fs::read_to_string(run.join(HEAD_END_FILE)).ok();
        let exit = std::fs::read_to_string(run.join(EXIT_FILE)).ok();
        seen.set(Some((head, exit)));
    })
    .expect("end");

    assert_eq!(
        seen.take(),
        Some((Some("h9".to_owned()), Some("7\n".to_owned())))
    );
}

#[test]
fn a_head_end_that_cannot_be_written_still_lets_the_exit_be_written() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = opened(dir.path(), "implement", None);
    // A directory where the file goes: its rename fails, the exit's does not.
    std::fs::create_dir_all(run.join(HEAD_END_FILE).join("blocker")).expect("a blocker");
    let ran = Cell::new(false);

    let result = end(&run, 1, &HeadEnd::Left(None), &|| ran.set(true));

    assert!(result.is_err());
    assert!(ran.get());
    assert_eq!(
        std::fs::read_to_string(run.join(EXIT_FILE)).expect("exit"),
        "1\n"
    );
}

#[test]
fn a_question_is_recorded_only_in_a_running_phase_of_the_task_it_is_raised_on() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = opened(dir.path(), "implement", None);
    let question = TicketId::from(30);

    // Raised on another issue (the epic, another task): not this run's.
    assert!(!record_question(&run, TicketId::from(1), question).expect("read"));
    assert!(!run.join(QUESTIONS_FILE).exists());
    // A folder with no task (a planning launch's) is no phase run.
    let launch = dir.path().join("planning-instance-1-on-1");
    std::fs::create_dir_all(&launch).expect("folder");
    assert!(!record_question(&launch, task(), question).expect("read"));
    assert!(!launch.join(QUESTIONS_FILE).exists());
    // The task's own question, during the run: recorded, again and again.
    assert!(record_question(&run, task(), question).expect("recorded"));
    assert!(record_question(&run, task(), TicketId::from(31)).expect("recorded"));
    assert_eq!(
        std::fs::read_to_string(run.join(QUESTIONS_FILE)).expect("questions"),
        "30\n31\n"
    );
    // Once the run has ended, a question is no longer raised during it.
    record_exit(&run, 0).expect("exit");
    assert!(!record_question(&run, task(), TicketId::from(32)).expect("read"));
    assert_eq!(
        read_one(dir.path(), &never_alive).questions,
        BTreeSet::from([TicketId::from(30), TicketId::from(31)])
    );
}

#[test]
fn a_questions_line_that_is_no_number_is_unreadable_never_skipped() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run = opened(dir.path(), "implement", None);
    std::fs::write(run.join(QUESTIONS_FILE), "30\nwhat\n").expect("questions");

    let read = read_runs(dir.path(), task(), &never_alive);

    let error = read.expect_err("a garbled record is no fact");
    assert!(error.contains("`what`"), "{error}");
}

#[test]
fn another_tasks_folder_is_not_this_ones_and_runs_come_oldest_first() {
    let dir = tempfile::tempdir().expect("temp dir");
    let first = opened(dir.path(), "implement", None);
    std::thread::sleep(Duration::from_millis(20));
    let second = opened(dir.path(), "review", Some("h1"));
    let other = dir.path().join("implement-instance-9-on-1-task-5");
    std::fs::create_dir_all(&other).expect("folder");
    std::fs::write(other.join(ACTION_FILE), "implement").expect("action");
    std::fs::write(other.join(TASK_FILE), "5").expect("task");

    let runs = read_runs(dir.path(), task(), &never_alive).expect("the runs");

    let actions: Vec<&str> = runs.iter().map(|run| run.action.as_str()).collect();
    assert_eq!(actions, ["implement", "review"]);
    assert!(first.exists() && second.exists());
    assert_eq!(
        task_of(&other).expect("reads"),
        Some(TicketId::from(5)),
        "the other folder names its own task"
    );
    assert!(
        read_runs(&dir.path().join("absent"), task(), &never_alive)
            .expect("an absent folder is no runs")
            .is_empty()
    );
}

#[test]
fn the_head_now_is_the_forges_word_on_the_recorded_pull_request() {
    let root = tempfile::tempdir().expect("temp dir");
    let forge = FakeForge::default();

    // No pull request recorded: the run left none, and the forge is not asked.
    assert_eq!(head_now(None, root.path(), task()), HeadEnd::Left(None));

    let slices = root
        .path()
        .join(crate::composition::STATE_DIR)
        .join("slices");
    std::fs::create_dir_all(&slices).expect("slices");
    std::fs::write(slices.join("4.pr"), "12").expect("the record");
    // A forge that cannot be had, or cannot say: unknown, never a guess.
    assert_eq!(head_now(None, root.path(), task()), HeadEnd::Unknown);
    assert_eq!(
        head_now(Some(&forge), root.path(), task()),
        HeadEnd::Unknown
    );

    let pr = forge
        .open_pr(&PrRequest {
            slice: SliceName::from("slice"),
            base: BaseName::from("base"),
            title: "Title".to_owned(),
            body: String::new(),
        })
        .expect("open pr");
    std::fs::write(slices.join("4.pr"), pr.get().to_string()).expect("the record");
    assert_eq!(
        head_now(Some(&forge), root.path(), task()),
        HeadEnd::Left(None),
        "a pull request with no head"
    );
    forge.set_head(pr, Some("abc")).expect("head");
    assert_eq!(
        head_now(Some(&forge), root.path(), task()),
        HeadEnd::Left(Some(CommitId::from("abc")))
    );
}
