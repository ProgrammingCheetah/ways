use std::time::{Duration, SystemTime};

use super::*;
use crate::app::actions::ActionName;
use crate::app::guard::Level;
use crate::app::spawn::outcome::{Launched, Ran, Started};
use crate::app::spawn::{EpicSkip, Plan, SlotCount, TickOutcome};
use crate::app::status::Launch;

fn id(number: u64) -> TicketId {
    TicketId::from(number)
}

/// 2026-11-12 14:32:00 UTC.
fn at() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_794_493_920)
}

fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * 60)
}

fn launch(instance: &str, task: Option<u64>, started: Option<SystemTime>) -> Launch {
    Launch {
        task,
        instance: instance.to_owned(),
        pid: Some(1),
        started_at: started.map(|time| humantime::format_rfc3339_seconds(time).to_string()),
    }
}

fn state_of(launches: Vec<Launch>) -> SpawnState {
    SpawnState {
        launches,
        ..SpawnState::default()
    }
}

#[test]
fn the_clock_is_local_time_of_day() {
    assert_eq!(clock(at(), 0), "14:32");
    assert_eq!(clock(at(), 2 * 3600), "16:32");
    assert_eq!(
        clock(at(), -(15 * 3600) - 30 * 60),
        "23:02",
        "wraps the day"
    );
    assert_eq!(clock(SystemTime::UNIX_EPOCH, 0), "00:00");
}

#[test]
fn an_age_is_in_minutes_then_hours() {
    assert_eq!(age(Duration::from_secs(59)), "under 1 min");
    assert_eq!(age(minutes(6)), "6 min");
    assert_eq!(age(minutes(59)), "59 min");
    assert_eq!(age(minutes(65)), "1 h 5 min");
}

#[test]
fn a_held_lock_is_one_line_with_the_time() {
    assert_eq!(
        skipped_line(at(), 0),
        "tick 14:32 skipped: another tick holds spawn.lock"
    );
}

#[test]
fn the_next_tick_is_one_interval_on() {
    assert_eq!(next_tick_line(at(), minutes(5), 0), "next tick 14:37");
    assert_eq!(next_tick_line(at(), minutes(30), 3600), "next tick 16:02");
}

#[test]
fn a_running_instance_shows_how_long_it_has_run() {
    let started = at() - minutes(6);
    let state = state_of(vec![launch(
        "challenger instance 2 on #4",
        None,
        Some(started),
    )]);

    assert_eq!(
        running_lines(&state, at()),
        ["running  challenger instance 2 on #4, 6 min"]
    );
}

#[test]
fn a_running_instance_names_its_tasks_and_its_oldest_start() {
    let state = state_of(vec![
        launch("team instance 3 on #1", Some(5), Some(at() - minutes(12))),
        launch("team instance 3 on #1", Some(6), Some(at() - minutes(2))),
    ]);

    assert_eq!(
        running_lines(&state, at()),
        ["running  team instance 3 on #1 (#5 #6), 12 min"]
    );
}

#[test]
fn a_running_instance_with_no_start_shows_no_duration() {
    let state = state_of(vec![launch("planning instance 1 on #1", None, None)]);

    assert_eq!(
        running_lines(&state, at()),
        ["running  planning instance 1 on #1"]
    );
}

#[test]
fn waiting_names_the_questions_and_the_unsigned_epics() {
    assert_eq!(
        waiting_lines(&[id(29), id(30), id(31)], &[id(1)]),
        [
            "waiting on you: 3 questions (#29 #30 #31)",
            "waiting on you: epic #1 is not signed"
        ]
    );
    assert_eq!(
        waiting_lines(&[id(29)], &[]),
        ["waiting on you: 1 question (#29)"]
    );
    assert!(waiting_lines(&[], &[]).is_empty());
}

#[test]
fn an_idle_tick_with_nothing_running_or_waiting_has_nothing_to_do() {
    let mut block = Block::new(at(), 0);
    block.read_running(Ok(&SpawnState::default()), at());
    block.read_waiting(Ok(&[]), Ok(&[]));

    let text = block.render_text();

    assert_eq!(text, "tick 14:32\nnothing to do\n");
    assert!(!text.contains("idle"));
}

#[test]
fn skips_alone_do_not_hide_nothing_to_do() {
    let mut block = Block::new(at(), 0);
    block.skipped = vec!["#4 skipped: every task is finished".to_owned()];
    block.read_running(Ok(&SpawnState::default()), at());
    block.read_waiting(Ok(&[]), Ok(&[]));

    assert_eq!(
        block.render_text(),
        "tick 14:32\n#4 skipped: every task is finished\nnothing to do\n"
    );
}

#[test]
fn the_outcome_lines_are_what_the_tick_did() {
    let outcome = TickOutcome {
        ran: vec![Ran {
            task: id(7),
            action: ActionName::from("merge"),
            exit: 0,
        }],
        started: Some(Started {
            task: id(8),
            action: ActionName::from("implement"),
            units: 2,
        }),
        waits: vec![Launched {
            task: id(9),
            action: ActionName::from("review"),
        }],
        questions: vec![id(10)],
        asked: Vec::new(),
    };
    let mut block = Block::new(at(), 0);
    block
        .did
        .extend(outcome.render_text().lines().map(str::to_owned));
    block.read_running(Ok(&SpawnState::default()), at());
    block.read_waiting(Ok(&[]), Ok(&[]));

    assert_eq!(
        block.render_text(),
        "tick 14:32\nran merge #7: exit 0\nstarted implement #8: 2 units\n\
         wait review #9\nquestion on #10\n"
    );
}

#[test]
fn an_idle_outcome_prints_no_text_and_no_bare_idle() {
    assert_eq!(TickOutcome::default().render_text(), "");
}

#[test]
fn a_launched_planning_instance_is_never_idle() {
    // The phase outcome of a tick that only launched planning is empty, and so idle in `--json`;
    // the block still says what was launched.
    let outcome = TickOutcome::default();
    assert!(outcome.is_idle());
    let mut block = Block::new(at(), 0);
    block
        .did
        .push("launching planning instance 1 on #1".to_owned());
    block
        .launched
        .push("launched planning instance 1 on #1: exit 0 (.zaf/runs/x/report.md)".to_owned());
    block.read_running(Ok(&SpawnState::default()), at());
    block.read_waiting(Ok(&[]), Ok(&[]));

    let text = block.render_text();

    assert!(
        text.contains("launching planning instance 1 on #1\n"),
        "{text}"
    );
    assert!(
        text.contains("launched planning instance 1 on #1: exit 0"),
        "{text}"
    );
    assert!(!text.contains("nothing to do"), "{text}");
    assert!(!text.contains("idle"), "{text}");
}

#[test]
fn a_launched_challenger_is_never_idle() {
    let mut block = Block::new(at(), 0);
    block
        .did
        .push("launching challenger instance 1 on #4".to_owned());
    block.read_running(Ok(&SpawnState::default()), at());
    block.read_waiting(Ok(&[]), Ok(&[]));

    let text = block.render_text();

    assert!(
        text.contains("launching challenger instance 1 on #4"),
        "{text}"
    );
    assert!(!text.contains("nothing to do"), "{text}");
}

#[test]
fn a_tick_with_an_instance_running_and_questions_open_says_so() {
    let state = state_of(vec![launch(
        "challenger instance 2 on #4",
        None,
        Some(at() - minutes(6)),
    )]);
    let mut block = Block::new(at(), 0);
    block.skipped = vec!["#4 skipped: the challenger runs on it".to_owned()];
    block.read_running(Ok(&state), at());
    block.read_waiting(Ok(&[id(29), id(30), id(31)]), Ok(&[id(1)]));

    let text = block.render_text();

    assert_eq!(
        text,
        "tick 14:32\n\
         running  challenger instance 2 on #4, 6 min\n\
         #4 skipped: the challenger runs on it\n\
         waiting on you: 3 questions (#29 #30 #31)\n\
         waiting on you: epic #1 is not signed\n"
    );
}

#[test]
fn the_head_holds_what_was_done_and_the_tail_the_rest() {
    let mut block = Block::new(at(), 0);
    block
        .did
        .push("launching planning instance 1 on #1".to_owned());
    block
        .launched
        .push("launched planning instance 1 on #1: exit 0".to_owned());
    block.read_running(Ok(&SpawnState::default()), at());
    block.read_waiting(Ok(&[id(3)]), Ok(&[]));

    assert_eq!(
        block.head(),
        "tick 14:32\nlaunching planning instance 1 on #1\n"
    );
    assert_eq!(
        block.tail(),
        "launched planning instance 1 on #1: exit 0\nwaiting on you: 1 question (#3)\n"
    );
}

#[test]
fn a_read_that_fails_is_a_line_and_not_a_nothing_to_do() {
    let mut block = Block::new(at(), 0);
    block.read_running(Err("the run folder is unreadable".to_owned()), at());
    block.read_waiting(
        Err("the board did not answer".to_owned()),
        Err("the board did not answer".to_owned()),
    );

    let text = block.render_text();

    assert_eq!(
        text,
        "tick 14:32\n\
         could not read running instances: the run folder is unreadable\n\
         could not read questions: the board did not answer\n\
         could not read epics: the board did not answer\n"
    );
}

#[test]
fn the_plan_gives_the_skips_as_the_dry_run_words_them() {
    let plan = Plan {
        slots: SlotCount {
            total: 1,
            level: Level::Green,
            allowed: 1,
            running: 1,
            free: 0,
        },
        launches: Vec::new(),
        epics: vec![
            (id(1), EpicSkip::Unsigned),
            (id(4), EpicSkip::ChallengerRuns),
            (id(5), EpicSkip::Questioned),
        ],
        cycles: Vec::new(),
        dead: Vec::new(),
        phases_running: 0,
    };

    assert_eq!(
        plan.skipped_lines(),
        [
            "#4 skipped: the challenger runs on it",
            "#5 skipped: a question is open on it"
        ],
        "an unsigned epic is the waiting line's"
    );
}

#[test]
fn the_slots_line_follows_the_header_and_does_not_count_as_work() {
    let mut block = Block::new(SystemTime::UNIX_EPOCH, 0);
    block.slots = Some("slots 4 of 4 (guard green), 1 running, 3 free".to_owned());
    assert_eq!(
        block.render_text(),
        "tick 00:00\nslots 4 of 4 (guard green), 1 running, 3 free\nnothing to do\n"
    );
}

#[test]
fn a_running_phase_is_a_running_line_with_its_epic_and_age() {
    let phases = vec![LivePhase {
        task: id(3),
        action: ActionName::from("implement"),
        instance: Some("implement instance 1 on #1".to_owned()),
        started_at: at() - minutes(4),
    }];
    let mut block = Block::new(at(), 0);
    block.read_running(Ok(&SpawnState::default()), at());
    block.read_phases(Ok(&phases), at());
    block.read_waiting(Ok(&[]), Ok(&[]));

    assert_eq!(
        block.render_text(),
        "tick 14:32\nrunning  implement #3 (epic #1), 4 min\n"
    );
}

#[test]
fn a_phase_with_no_instance_names_no_epic_and_phases_read_failing_is_a_line() {
    let phases = vec![LivePhase {
        task: id(3),
        action: ActionName::from("implement"),
        instance: None,
        started_at: at(),
    }];
    assert_eq!(
        phase_lines(&phases, at()),
        ["running  implement #3, under 1 min"]
    );
    let mut block = Block::new(at(), 0);
    block.read_phases(Err("denied".to_owned()), at());
    assert_eq!(
        block.render_text(),
        "tick 14:32\ncould not read running phases: denied\n"
    );
}

#[test]
fn the_places_follow_the_slots_come_before_what_the_tick_did_and_are_no_act() {
    let mut block = Block::new(SystemTime::UNIX_EPOCH, 0);
    block.slots = Some("slots 4 of 4 (guard green), 1 running, 3 free".to_owned());
    block.places = vec![
        "#7 lane(implement) -> running(implement)".to_owned(),
        "2 tickets: 1 running, 0 in the lane, 1 waiting, 0 done".to_owned(),
    ];
    assert_eq!(
        block.render_text(),
        "tick 00:00\nslots 4 of 4 (guard green), 1 running, 3 free\n\
         #7 lane(implement) -> running(implement)\n\
         2 tickets: 1 running, 0 in the lane, 1 waiting, 0 done\nnothing to do\n",
        "where the tickets sit is no act: nothing to do still says so"
    );
    block.did = vec!["ran merge #3: exit 0".to_owned()];
    assert!(
        block
            .head()
            .ends_with("1 waiting, 0 done\nran merge #3: exit 0\n"),
        "{}",
        block.head()
    );
}
