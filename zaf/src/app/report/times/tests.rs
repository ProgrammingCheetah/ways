use super::*;

fn at(minute: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000 + minute * 60)
}

fn closed(opened: u64, closed: u64) -> Question {
    Question {
        opened: at(opened),
        closed: Some(at(closed)),
    }
}

fn epic(questions: Vec<Question>) -> Minutes {
    EpicTimes {
        start: at(0),
        end: at(120),
        questions,
    }
    .minutes()
}

#[test]
fn overlapping_questions_count_once() {
    let got = epic(vec![closed(10, 40), closed(10, 40)]);
    assert_eq!((got.questions, got.work), (30.0, 90.0));
    let got = epic(vec![closed(10, 40), closed(20, 50)]);
    assert_eq!((got.questions, got.work), (40.0, 80.0));
}

#[test]
fn separate_questions_add_up() {
    let got = epic(vec![closed(10, 20), closed(50, 80)]);
    assert_eq!((got.questions, got.work), (40.0, 80.0));
}

#[test]
fn question_time_is_clipped_to_the_window() {
    let got = epic(vec![closed(0, 0), closed(200, 300)]);
    assert_eq!((got.questions, got.work), (0.0, 120.0));
    let got = EpicTimes {
        start: at(10),
        end: at(110),
        questions: vec![closed(0, 30), closed(100, 150)],
    }
    .minutes();
    assert_eq!((got.questions, got.work), (30.0, 70.0));
}

#[test]
fn an_open_question_counts_until_the_end() {
    let got = epic(vec![Question {
        opened: at(90),
        closed: None,
    }]);
    assert_eq!((got.questions, got.work), (30.0, 90.0));
}

mod gathering {
    use std::fs::File;
    use std::path::Path;

    use super::*;
    use crate::domain::{EpicTask, Ticket, TicketId, TicketState};
    use crate::fakes::FakeBoard;
    use crate::ports::PortError;

    fn id(number: u64) -> TicketId {
        TicketId::from(number)
    }

    fn task(number: u64, closed_at: Option<u64>) -> EpicTask {
        EpicTask {
            number: id(number),
            title: format!("Task {number}").into(),
            body: "".into(),
            url: "".into(),
            state: None,
            standing: match closed_at {
                Some(_) => Standing::Done,
                None => Standing::Open,
            },
            group: None,
            kind: None,
            closed_at: closed_at.map(at),
        }
    }

    fn epic_of(tasks: Vec<EpicTask>) -> Epic {
        Epic {
            id: id(1),
            title: "Epic".into(),
            body: "".into(),
            tasks,
            signature: None,
            planned_at: None,
            records: Vec::new(),
            challengers: Vec::new(),
        }
    }

    /// A phase run folder of `task` that started at `minute`.
    pub(super) fn run_folder(dir: &Path, name: &str, task: u64, minute: u64) {
        let folder = dir.join(name);
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(folder.join("action"), "implement").expect("action");
        let task_file = folder.join("task");
        std::fs::write(&task_file, task.to_string()).expect("task");
        File::options()
            .write(true)
            .open(&task_file)
            .and_then(|file| file.set_modified(at(minute)))
            .expect("time");
    }

    fn question_on(board: &FakeBoard, number: u64, on: u64, opened: Option<u64>) {
        board.add_question(Ticket {
            id: id(number),
            epic: None,
            title: format!("question {number}").into(),
            body: format!("Raised on {}.", id(on)).as_str().into(),
            state: Some(TicketState::Question),
            url: "".into(),
            kind: None,
        });
        if let Some(minute) = opened {
            board.set_opened_at(id(number), at(minute));
        }
    }

    fn times(got: Result<Timed, PortError>) -> EpicTimes {
        match got.expect("gathers") {
            Timed::EpicTimes(times) => times,
            Timed::LeftOut(reason) => panic!("left out: {reason}"),
        }
    }

    #[test]
    fn epic_start_is_the_first_phase_run() {
        let dir = tempfile::tempdir().expect("dir");
        run_folder(dir.path(), "late", 3, 30);
        run_folder(dir.path(), "early", 2, 10);
        run_folder(dir.path(), "other", 9, 1);
        let epic = epic_of(vec![task(2, Some(50)), task(3, Some(60))]);

        let got = times(epic_times(&FakeBoard::default(), dir.path(), &epic));

        assert_eq!(got.start, at(10));
    }

    #[test]
    fn epic_end_is_the_last_task_closed() {
        let dir = tempfile::tempdir().expect("dir");
        run_folder(dir.path(), "run", 2, 10);
        let epic = epic_of(vec![
            task(2, Some(50)),
            task(3, Some(70)),
            task(4, Some(60)),
        ]);

        let got = times(epic_times(&FakeBoard::default(), dir.path(), &epic));

        assert_eq!(got.end, at(70));
    }

    #[test]
    fn questions_on_epic_and_tasks_are_intervals() {
        let dir = tempfile::tempdir().expect("dir");
        run_folder(dir.path(), "run", 2, 10);
        let board = FakeBoard::default();
        question_on(&board, 20, 1, Some(15));
        question_on(&board, 21, 2, Some(20));
        question_on(&board, 22, 9, Some(25));
        board.close_at(id(21), at(35));
        let epic = epic_of(vec![task(2, Some(50))]);

        let got = times(epic_times(&board, dir.path(), &epic));

        assert_eq!(
            got.questions,
            vec![
                Question {
                    opened: at(15),
                    closed: None,
                },
                closed(20, 35),
            ]
        );
    }

    #[test]
    fn epic_without_phase_record_is_left_out() {
        let dir = tempfile::tempdir().expect("dir");
        run_folder(dir.path(), "other", 9, 10);
        let epic = epic_of(vec![task(2, Some(50))]);

        let got = epic_times(&FakeBoard::default(), dir.path(), &epic);

        assert_eq!(got, Ok(Timed::LeftOut("no phase record".to_owned())));
    }

    #[test]
    fn epic_with_an_open_task_is_left_out() {
        let dir = tempfile::tempdir().expect("dir");
        run_folder(dir.path(), "run", 2, 10);
        let epic = epic_of(vec![task(2, Some(50)), task(3, None)]);

        let got = epic_times(&FakeBoard::default(), dir.path(), &epic);

        assert_eq!(got, Ok(Timed::LeftOut("#3 is still open".to_owned())));
    }

    #[test]
    fn epic_with_a_question_of_no_opened_time_is_left_out() {
        let dir = tempfile::tempdir().expect("dir");
        run_folder(dir.path(), "run", 2, 10);
        let board = FakeBoard::default();
        question_on(&board, 20, 2, None);
        let epic = epic_of(vec![task(2, Some(50))]);

        let got = epic_times(&board, dir.path(), &epic).expect("gathers");

        assert!(matches!(got, Timed::LeftOut(reason) if reason.contains("no opened time")));
    }
}

mod planned {
    use super::*;
    use std::collections::BTreeSet;

    use crate::domain::plan::body::{Section, Signature};
    use crate::domain::plan::record::PlanRecord;
    use crate::domain::plan::{Name, NodeId, NodeKey, NodeKind, PlannedTicket};
    use crate::domain::{
        BodyHash, EpicTask, SecurityTrigger, TaskBody, TaskModel, TaskSize, TaskTemplate,
        TicketBody, TicketId,
    };

    fn id(number: u64) -> TicketId {
        TicketId::from(number)
    }

    /// A plan of `count` sections, valid to parse.
    pub(super) fn plan(count: usize) -> String {
        let sections: Vec<String> = (0..count)
            .map(|n| {
                [
                    format!("Task: Task {n}"),
                    format!("Node: N{n}"),
                    format!("Goal: goal {n}"),
                    "Scope: scope".to_owned(),
                    "Out of scope: none".to_owned(),
                    "Interfaces: none".to_owned(),
                    "Design pages: D1".to_owned(),
                    "Acceptance:".to_owned(),
                    "- it works".to_owned(),
                    format!("Tests: t{n}"),
                    "Security trigger: no".to_owned(),
                    "Model: sonnet".to_owned(),
                    "Size: S".to_owned(),
                ]
                .join("\n")
            })
            .collect();
        sections.join("\n\n")
    }

    pub(super) fn record(k: u64, tasks: usize, dropped: &[u64]) -> EpicRecord {
        EpicRecord {
            record: PlanRecord {
                k,
                design: BodyHash::of(&TicketBody::from("a design")),
                dropped: dropped.iter().map(|n| id(*n)).collect(),
                plan: plan(tasks),
            },
            posted_at: None,
            mark: None,
        }
    }

    fn task(number: u64, standing: Standing) -> EpicTask {
        EpicTask {
            number: id(number),
            title: format!("Task {number}").into(),
            body: "".into(),
            url: "".into(),
            state: None,
            standing,
            group: None,
            kind: None,
            closed_at: None,
        }
    }

    fn epic_of(tasks: Vec<EpicTask>) -> Epic {
        Epic {
            id: id(1),
            title: "Epic".into(),
            body: "".into(),
            tasks,
            signature: None,
            planned_at: None,
            records: Vec::new(),
            challengers: Vec::new(),
        }
    }

    #[test]
    fn planned_count_sums_records_less_dropped() {
        let records = [record(1, 4, &[]), record(2, 2, &[7])];
        assert_eq!(planned_count(&epic_of(Vec::new()), &records), 5);
    }

    #[test]
    fn a_ticket_named_dropped_twice_comes_off_once() {
        let records = [record(1, 4, &[7]), record(2, 2, &[7])];
        assert_eq!(planned_count(&epic_of(Vec::new()), &records), 5);
    }

    /// A ticket filed from a plan: `sections` behaviours under `node`, or an interface ticket.
    fn filed(number: u64, node: &str, kind: NodeKind, sections: usize) -> EpicTask {
        let section = |n: usize| Section {
            key: (sections > 1).then(|| NodeKey::from(format!("K{n}").as_str())),
            template: TaskTemplate {
                goal: "a goal".to_owned(),
                owned_paths: "a path".to_owned(),
                scoped: false,
                out_of_scope: None,
                interfaces: "none".to_owned(),
                design_pages: "none".to_owned(),
                architecture: String::new(),
                acceptance: vec!["a line".to_owned()],
                tests: "a test".to_owned(),
                security_trigger: SecurityTrigger::No,
                model: TaskModel::Sonnet,
                size: TaskSize::S,
                blocked_by: Vec::new(),
                style: None,
            },
            writes: BTreeSet::new(),
            provides: BTreeSet::new(),
            implements: BTreeSet::new(),
            needs: BTreeSet::new(),
        };
        let ticket = PlannedTicket {
            node: NodeId::from(node),
            kind,
            signatures: match kind {
                NodeKind::Interface => vec![Signature {
                    name: Name::from("s"),
                    shape: "fn s()".to_owned(),
                }],
                NodeKind::Body => Vec::new(),
            },
            turns: Vec::new(),
            waits: Vec::new(),
            sections: (0..sections).map(section).collect(),
        };
        let body = ticket.to_string();
        assert_eq!(
            TaskBody::parse(&body),
            Ok(TaskBody::Planned(ticket)),
            "the body reads back"
        );
        EpicTask {
            body: body.as_str().into(),
            ..task(number, Standing::ClosedNotPlanned)
        }
    }

    #[test]
    fn a_dropped_merged_ticket_takes_off_each_task_it_held() {
        // Four tasks written, two of them filed as one ticket `N0+N1`, which record 2 drops and
        // replaces with two tasks: 4 - 2 + 2.
        let epic = epic_of(vec![filed(7, "N0+N1", NodeKind::Body, 2)]);
        let records = [record(1, 4, &[]), record(2, 2, &[7])];
        assert_eq!(planned_count(&epic, &records), 4);
    }

    #[test]
    fn a_dropped_interface_ticket_takes_off_no_task() {
        // The interface ticket is zaf's, no `Task:` section of any plan: dropping it leaves the
        // four tasks written.
        let epic = epic_of(vec![filed(9, "I@1.1", NodeKind::Interface, 1)]);
        let records = [record(1, 4, &[]), record(2, 0, &[9])];
        assert_eq!(planned_count(&epic, &records), 4);
    }

    #[test]
    fn a_dropped_ticket_of_one_task_takes_off_one() {
        let epic = epic_of(vec![filed(7, "N0", NodeKind::Body, 1)]);
        let records = [record(1, 4, &[]), record(2, 2, &[7])];
        assert_eq!(planned_count(&epic, &records), 5);
    }

    #[test]
    fn planned_count_of_an_older_epic_counts_tasks_that_count() {
        let epic = epic_of(vec![
            task(2, Standing::Open),
            task(3, Standing::Done),
            task(4, Standing::ClosedNotPlanned),
        ]);
        assert_eq!(planned_count(&epic, &[]), 2);
    }
}

mod view {
    use std::path::Path;

    use super::gathering::run_folder;
    use super::planned::record;
    use super::*;
    use crate::app::report::epics::Verdict;
    use crate::domain::{Ticket, TicketId, with_release};
    use crate::fakes::FakeBoard;
    use crate::ports::Board;

    fn id(number: u64) -> TicketId {
        TicketId::from(number)
    }

    /// Epic `number` of `release` with one done task per number in `tasks`, the first phase run
    /// at minute 0 and the last task closed at `closed`; `planned` gives it a plan record of that
    /// many tasks; `run` says whether the epic has a phase run folder.
    #[allow(
        clippy::too_many_arguments,
        reason = "one fixture line per epic, each fact a column"
    )]
    fn epic(
        board: &FakeBoard,
        dir: &Path,
        number: u64,
        release: Option<&str>,
        tasks: &[u64],
        closed: u64,
        planned: Option<usize>,
        run: bool,
    ) {
        board.add_epic(id(number), "Epic");
        if let Some(release) = release {
            board.set_body(id(number), &with_release(release, "Goal."));
        }
        for task in tasks {
            board.add_ticket(Ticket {
                id: id(*task),
                epic: Some(id(number)),
                title: format!("Task {task}").into(),
                body: "".into(),
                state: None,
                url: "".into(),
                kind: None,
            });
            board.close_at(id(*task), at(closed));
        }
        if run {
            run_folder(dir, &format!("run-{number}"), tasks[0], 0);
        }
        if let Some(count) = planned {
            let posted = record(1, count, &[]).record;
            board
                .comment(id(number), &posted.to_string())
                .expect("posts");
        }
    }

    /// Two 0.5.x epics of 60 and 20 minutes per planned task, two graph epics of 30 and 20.
    fn both_rows(board: &FakeBoard, dir: &Path) {
        epic(board, dir, 10, Some("0.5.2"), &[101], 60, None, true);
        epic(board, dir, 11, Some("0.5.3"), &[111, 112], 40, None, true);
        epic(board, dir, 20, Some("0.6.0"), &[201], 60, Some(2), true);
        epic(board, dir, 21, Some("0.6.0"), &[211], 20, Some(1), true);
    }

    #[test]
    fn report_times_prints_both_rows() {
        let dir = tempfile::tempdir().expect("dir");
        let board = FakeBoard::default();
        both_rows(&board, dir.path());

        let view = times_view(&board, dir.path()).expect("reads");

        assert_eq!(view.baseline.count, 2);
        assert_eq!(view.baseline.average, Some(40.0));
        assert_eq!(view.baseline.median, Some(40.0));
        assert_eq!(view.graph.count, 2);
        assert_eq!(view.graph.average, Some(25.0));
        assert_eq!(view.graph.median, Some(25.0));
        assert_eq!(view.lower_average, Some(Verdict::Lower));
        assert_eq!(view.lower_median, Some(Verdict::Lower));
        assert!(view.left_out.is_empty());
        let text = view.render_text();
        assert!(text.contains("0.5.x        2     40.0     40.0"), "{text}");
        assert!(text.contains("graph        2     25.0     25.0"), "{text}");
        assert!(
            text.contains("graph against 0.5.x: average lower, median lower"),
            "{text}"
        );
        let json: serde_json::Value = serde_json::from_str(&view.to_json()).expect("json");
        assert_eq!(json["baseline"]["average"], 40.0);
        assert_eq!(json["graph"]["median"], 25.0);
        assert_eq!(json["lower_average"], "lower");
    }

    #[test]
    fn a_graph_row_that_is_not_lower_says_so() {
        let dir = tempfile::tempdir().expect("dir");
        let board = FakeBoard::default();
        epic(
            &board,
            dir.path(),
            10,
            Some("0.5.2"),
            &[101],
            10,
            None,
            true,
        );
        epic(
            &board,
            dir.path(),
            20,
            Some("0.6.0"),
            &[201],
            60,
            Some(1),
            true,
        );

        let view = times_view(&board, dir.path()).expect("reads");

        assert_eq!(view.lower_average, Some(Verdict::NotLower));
        assert_eq!(view.lower_median, Some(Verdict::NotLower));
    }

    #[test]
    fn report_times_names_left_out_epics_with_why() {
        let dir = tempfile::tempdir().expect("dir");
        let board = FakeBoard::default();
        both_rows(&board, dir.path());
        epic(
            &board,
            dir.path(),
            30,
            Some("0.6.0"),
            &[301],
            50,
            None,
            true,
        );
        epic(
            &board,
            dir.path(),
            31,
            Some("0.5.4"),
            &[311],
            50,
            None,
            false,
        );
        epic(&board, dir.path(), 32, None, &[321], 50, None, true);
        epic(
            &board,
            dir.path(),
            33,
            Some("0.5.5"),
            &[331],
            50,
            None,
            true,
        );
        board.add_ticket(Ticket {
            id: id(332),
            epic: Some(id(33)),
            title: "Open".into(),
            body: "".into(),
            state: None,
            url: "".into(),
            kind: None,
        });

        let view = times_view(&board, dir.path()).expect("reads");

        let left: Vec<(u64, &str)> = view
            .left_out
            .iter()
            .map(|out| (out.epic, out.reason.as_str()))
            .collect();
        assert_eq!(
            left,
            [
                (30, "0.6.0's own, built the old way"),
                (31, "no phase record"),
                (32, "no release"),
                (33, "#332 is still open"),
            ]
        );
        assert_eq!(view.baseline.count, 2);
        assert_eq!(view.graph.count, 2);
        let text = view.render_text();
        assert!(
            text.contains("#30 0.6.0's own, built the old way"),
            "{text}"
        );
        assert!(text.contains("#31 no phase record"), "{text}");
    }

    #[test]
    fn report_times_empty_row_has_no_verdict() {
        let dir = tempfile::tempdir().expect("dir");
        let board = FakeBoard::default();
        epic(
            &board,
            dir.path(),
            10,
            Some("0.5.2"),
            &[101],
            60,
            None,
            true,
        );

        let view = times_view(&board, dir.path()).expect("reads");

        assert_eq!(view.graph.count, 0);
        assert_eq!((view.graph.average, view.graph.median), (None, None));
        assert_eq!((view.lower_average, view.lower_median), (None, None));
        let text = view.render_text();
        assert!(text.contains("graph        0        -        -"), "{text}");
        assert!(text.contains("no verdict"), "{text}");
        assert!(!text.contains("lower"), "{text}");
        let json: serde_json::Value = serde_json::from_str(&view.to_json()).expect("json");
        assert!(json["lower_average"].is_null());
        assert!(json["graph"]["average"].is_null());
    }

    #[test]
    fn no_epic_at_all_is_two_empty_rows() {
        let dir = tempfile::tempdir().expect("dir");
        let view = times_view(&FakeBoard::default(), dir.path()).expect("reads");
        assert_eq!((view.baseline.count, view.graph.count), (0, 0));
        assert_eq!(view.lower_average, None);
    }
}
