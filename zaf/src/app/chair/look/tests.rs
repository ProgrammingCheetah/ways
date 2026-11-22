use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use super::{
    MOST_REPORTS, document, log_names, pick_log, read_ticket, release_read, reports_of, tail,
};
use crate::app::hub::reports_file;
use crate::domain::{DocumentSignature, TicketId};
use crate::manifest::Manifest;
use crate::ports::Holder;

const MANIFEST: &str =
    "[project]\nname = \"ways\"\n\n[board]\nkind = \"github-issues\"\nrepo = \"example/ways\"\n";

fn manifest() -> Manifest {
    MANIFEST.parse().expect("the manifest loads")
}

fn alive(_holder: &Holder) -> bool {
    true
}

fn dead(_holder: &Holder) -> bool {
    false
}

#[test]
fn a_release_document_is_shown_without_its_signature_block() {
    let signed = DocumentSignature::sign("\n# Objectives\n\nShip the chair.\n\n", UNIX_EPOCH);
    assert!(signed.starts_with("---\nsigned:"), "the block is there");

    let shown = document("objectives.md", &signed);

    assert_eq!(shown.name, "objectives.md");
    assert_eq!(shown.text, "# Objectives\n\nShip the chair.");
}

#[test]
fn a_document_with_no_block_is_shown_whole_and_printable() {
    let shown = document("design.md", "# Design\n\nA bell\u{7} rings.\n");

    assert_eq!(shown.text, "# Design\n\nA bell rings.");
}

#[test]
fn a_release_read_takes_scope_when_there_are_no_objectives() {
    let root = tempfile::tempdir().expect("temp dir");
    let folder = root.path().join(".teams/releases/0.5.3");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("scope.md"), "# Scope\n").expect("scope");
    std::fs::write(
        folder.join("design.md"),
        DocumentSignature::sign("# Design\n", UNIX_EPOCH),
    )
    .expect("design");

    let read = release_read(root.path(), &manifest(), "0.5.3");

    assert_eq!(read.folder, ".teams/releases/0.5.3");
    let objectives = read.objectives.expect("scope stands in");
    assert_eq!(
        (objectives.name.as_str(), objectives.text.as_str()),
        ("scope.md", "# Scope")
    );
    assert_eq!(
        read.design.map(|design| design.text),
        Some("# Design".to_owned())
    );
}

#[test]
fn a_release_folder_with_nothing_reads_as_nothing() {
    let root = tempfile::tempdir().expect("temp dir");

    let read = release_read(root.path(), &manifest(), "0.6.0");

    assert_eq!((read.objectives, read.design), (None, None));
}

#[test]
fn reports_keep_the_tickets_own_the_newest_last() {
    let text = "100 243 team started\n\
                110 241 team started\n\
                120 243 programmer wrote the tests\n\
                not a report\n\
                130 - hub restarted\n\
                140 243 programmer pushed\n";

    let reports = reports_of(text, TicketId::from(243), MOST_REPORTS);

    let whats: Vec<&str> = reports.iter().map(|report| report.what.as_str()).collect();
    assert_eq!(whats, ["started", "wrote the tests", "pushed"]);
    assert_eq!(reports[2].at, UNIX_EPOCH + Duration::from_secs(140));
    assert_eq!(reports[2].worker, "programmer");
    assert_eq!(reports_of(text, TicketId::from(243), 1).len(), 1);
}

#[test]
fn log_names_follow_the_runners_stamp_the_starts_second_first() {
    let started = UNIX_EPOCH + Duration::from_secs(1_763_500_000);

    let names = log_names("implement", started);

    assert_eq!(names[0], "20251118T210640Z_implement.log");
    assert_eq!(names[1], "20251118T210640Z_implement.2.log");
    assert!(names.contains(&"20251118T210641Z_implement.log".to_owned()));
    assert!(names.contains(&"20251118T210639Z_implement.log".to_owned()));
}

#[test]
fn of_two_logs_started_together_the_one_naming_the_ticket_wins() {
    let found = vec![
        (PathBuf::from("a.log"), "task #2430 built\n".to_owned()),
        (PathBuf::from("b.log"), "task #243 built\n".to_owned()),
    ];
    assert_eq!(
        pick_log(found.clone(), TicketId::from(243)),
        Some(PathBuf::from("b.log"))
    );
    assert_eq!(
        pick_log(found, TicketId::from(7)),
        Some(PathBuf::from("a.log")),
        "none names it: the closest"
    );
    assert_eq!(pick_log(Vec::new(), TicketId::from(7)), None);
}

#[test]
fn a_tail_keeps_the_last_lines_with_words() {
    let lines = tail("one\n\ntwo\n  \nthree\u{1b}[0m\nfour\n", 3);

    assert_eq!(lines, ["two", "three[0m", "four"]);
}

/// A run folder of `ticket` for `action` under `state`, and the second its run started.
fn run(state: &Path, ticket: u64, action: &str) -> (PathBuf, std::time::SystemTime) {
    let folder = state.join(format!("phases/ways-task-{ticket}"));
    std::fs::create_dir_all(&folder).expect("run folder");
    std::fs::write(folder.join("task"), format!("{ticket}\n")).expect("task");
    std::fs::write(folder.join("action"), format!("{action}\n")).expect("action");
    std::fs::write(folder.join("pid"), "4242 99\n").expect("pid");
    let started = std::fs::metadata(folder.join("task"))
        .and_then(|meta| meta.modified())
        .expect("mtime");
    (folder, started)
}

#[test]
fn a_ticket_read_finds_its_reports_and_its_runs_log() {
    let state = tempfile::tempdir().expect("temp dir");
    let (_, started) = run(state.path(), 243, "implement");
    let logs = state.path().join("logs");
    std::fs::create_dir_all(&logs).expect("logs");
    let name = log_names("implement", started).remove(0);
    std::fs::write(logs.join(&name), "reading #243\nwrote src/chair.rs\n").expect("log");
    let reports = reports_file(state.path());
    std::fs::create_dir_all(reports.parent().expect("folder")).expect("hub");
    std::fs::write(&reports, "100 243 team started\n").expect("reports");

    let read = read_ticket(state.path(), TicketId::from(243), &dead);

    assert_eq!(read.reports.len(), 1);
    let run = read.run.expect("its run");
    assert_eq!(run.action, "implement");
    assert!(!run.live, "its pid is not alive");
    assert_eq!(run.log, Some(logs.join(name)));
    assert_eq!(run.lines, ["reading #243", "wrote src/chair.rs"]);
}

#[test]
fn a_live_run_with_no_log_yet_is_working() {
    let state = tempfile::tempdir().expect("temp dir");
    run(state.path(), 243, "implement");

    let read = read_ticket(state.path(), TicketId::from(243), &alive);

    let run = read.run.expect("its run");
    assert!(run.working());
    assert!(read.reports.is_empty());
    assert_eq!(
        read_ticket(state.path(), TicketId::from(7), &alive).run,
        None
    );
}

#[test]
fn the_log_the_output_names_is_the_one_read() {
    let state = tempfile::tempdir().expect("temp dir");
    let (folder, _) = run(state.path(), 243, "review");
    let logs = state.path().join("logs");
    std::fs::create_dir_all(&logs).expect("logs");
    let log = logs.join("20200101T000000Z_review.log");
    std::fs::write(&log, "approved\n").expect("log");
    std::fs::write(
        folder.join("output"),
        format!("review ended, log {}.\n", log.display()),
    )
    .expect("output");
    std::fs::write(folder.join("exit"), "0\n").expect("exit");

    let run = read_ticket(state.path(), TicketId::from(243), &alive)
        .run
        .expect("its run");

    assert_eq!((run.exit, run.live), (Some(0), false));
    assert_eq!(run.log, Some(log));
    assert_eq!(run.lines, ["approved"]);
}

#[test]
fn a_log_outside_the_logs_folder_or_not_a_file_is_never_read() {
    let state = tempfile::tempdir().expect("temp dir");
    let (folder, started) = run(state.path(), 243, "review");
    let logs = state.path().join("logs");
    std::fs::create_dir_all(&logs).expect("logs");
    let secret = state.path().join("secret");
    std::fs::write(&secret, "the token\n").expect("secret");
    // The output names a path that climbs out of the logs folder.
    std::fs::write(
        folder.join("output"),
        format!(
            "review ended, log {}/../secret_review.log.\n",
            logs.display()
        ),
    )
    .expect("output");
    std::fs::write(state.path().join("secret_review.log"), "the token\n").expect("outside");
    // The log the runner would have named is a link out of the folder.
    let name = log_names("review", started).remove(0);
    std::os::unix::fs::symlink(&secret, logs.join(&name)).expect("a link");

    let read = read_ticket(state.path(), TicketId::from(243), &dead)
        .run
        .expect("its run");
    assert!(read.lines.is_empty(), "{:?}", read.lines);

    // A FIFO where the log should be is skipped at once, never waited on.
    std::fs::remove_file(logs.join(&name)).expect("unlinked");
    std::fs::remove_file(folder.join("output")).expect("no output");
    let fifo = std::ffi::CString::new(logs.join(&name).to_string_lossy().as_bytes()).expect("path");
    // SAFETY: mkfifo takes a NUL-ended path that lives for the call and a mode.
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
    let read = read_ticket(state.path(), TicketId::from(243), &dead)
        .run
        .expect("its run");
    assert!(read.lines.is_empty(), "{:?}", read.lines);
}

#[test]
fn only_a_logs_last_64_kib_are_read_for_its_tail() {
    let state = tempfile::tempdir().expect("temp dir");
    let (_, started) = run(state.path(), 243, "implement");
    let logs = state.path().join("logs");
    std::fs::create_dir_all(&logs).expect("logs");
    let mut text = String::from("started #243\n");
    for line in 0..20_000 {
        text.push_str(&format!("line {line} of a long run\n"));
    }
    let name = log_names("implement", started).remove(0);
    std::fs::write(logs.join(&name), &text).expect("log");

    let read = read_ticket(state.path(), TicketId::from(243), &dead)
        .run
        .expect("its run");

    assert_eq!(read.lines.len(), super::MOST_LOG_LINES);
    assert_eq!(
        read.lines.last().map(String::as_str),
        Some("line 19999 of a long run")
    );
    assert!(
        read.lines.iter().all(|line| line.starts_with("line ")),
        "whole lines only"
    );
}
