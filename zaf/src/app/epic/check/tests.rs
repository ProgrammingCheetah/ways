use std::sync::Mutex;

use super::*;
use crate::ports::{GateRun, GateVerdict, PortError};

/// A shell whose test lines fail when they contain a scripted name, keeping every line it ran.
#[derive(Default)]
struct ScriptedShell {
    failing: Vec<&'static str>,
    ran: Mutex<Vec<String>>,
}

impl Shell for ScriptedShell {
    fn run(&self, line: &CommandLine) -> Result<GateRun, PortError> {
        let line = line.to_string();
        self.ran.lock().expect("lock").push(line.clone());
        let failed = self.failing.iter().any(|name| line.contains(name));
        Ok(GateRun {
            verdict: if failed {
                GateVerdict::Failed
            } else {
                GateVerdict::Passed
            },
            output: if failed {
                "test result: FAILED\n".to_owned()
            } else {
                "test result: ok\n".to_owned()
            },
        })
    }
}

fn template() -> CommandLine {
    CommandLine::from("cargo test {name}".to_owned())
}

#[test]
fn done_when_passing_test_gives_no_failure() {
    let shell = ScriptedShell::default();
    let body = "Release: 0.6.0\n\nDone when: the list exports, `export_lists_rows` passes.\n";
    let failures = done_when_failures(body, &template(), &shell).expect("a Done when line");
    assert_eq!(failures, vec![]);
    assert_eq!(
        *shell.ran.lock().expect("lock"),
        vec!["cargo test export_lists_rows".to_owned()]
    );
}

#[test]
fn done_when_failing_test_names_line_and_test() {
    let shell = ScriptedShell {
        failing: vec!["export_fails"],
        ..Default::default()
    };
    let body = "- Done when: `export_ok` and `export_fails` pass.\n";
    let failures = done_when_failures(body, &template(), &shell).expect("a Done when line");
    assert_eq!(
        failures,
        vec![DoneWhenFailure::TestFailed {
            line: "- Done when: `export_ok` and `export_fails` pass.".to_owned(),
            test: "export_fails".to_owned(),
            reason: "test result: FAILED".to_owned(),
        }]
    );
}

#[test]
fn done_when_line_without_test_is_not_checkable() {
    let shell = ScriptedShell::default();
    let body = "  done when: it feels fast.\n";
    let failures = done_when_failures(body, &template(), &shell).expect("a Done when line");
    assert_eq!(
        failures,
        vec![DoneWhenFailure::NotCheckable {
            line: "done when: it feels fast.".to_owned(),
        }]
    );
    assert!(failures[0].to_string().contains("not checkable"));
    assert!(failures[0].to_string().contains("it feels fast."));
    assert!(shell.ran.lock().expect("lock").is_empty());
}

#[test]
fn epic_without_done_when_is_refused() {
    let shell = ScriptedShell::default();
    let body = "Release: 0.6.0\n\nThe epic says what it wants but never when it is done.\n";
    assert_eq!(
        done_when_failures(body, &template(), &shell),
        Err(NoDoneWhen)
    );
    assert!(shell.ran.lock().expect("lock").is_empty());
}

#[test]
fn a_test_name_is_one_shell_word_never_a_shell_string() {
    let shell = ScriptedShell::default();
    let body = "Done when: `a; rm -rf x` passes.\n";
    done_when_failures(body, &template(), &shell).expect("a Done when line");
    assert_eq!(
        *shell.ran.lock().expect("lock"),
        vec!["cargo test 'a; rm -rf x'".to_owned()]
    );
}

// The behaviours check (D25, check one).

mod behaviours {
    use std::collections::BTreeSet;

    use crate::app::epic::check::{
        BehaviourCheck, BehaviourFailure, BehaviourId, behaviour_failures,
    };
    use crate::domain::plan::body::{PlannedTicket, Section};
    use crate::domain::plan::record::PlanRecord;
    use crate::domain::plan::{NodeId, NodeKey, NodeKind};
    use crate::domain::{
        BodyHash, Epic, EpicRecord, EpicTask, SecurityTrigger, Standing, TaskModel, TaskSize,
        TaskTemplate, TicketBody, TicketId,
    };

    pub(super) fn plan_of(sections: &[(&str, &str)]) -> String {
        sections
            .iter()
            .map(|(node, goal)| {
                format!(
                    "Task: {goal}\nNode: {node}\nGoal: {goal}\nScope: the {node} part\n\
                     Out of scope: nothing else\nInterfaces: none\nDesign pages: D1\n\
                     Acceptance:\n- `{node}` works\nTests: {node}_works\n\
                     Security trigger: no\nModel: sonnet\nSize: S\nWrites: area-{}\n",
                    node.to_lowercase()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(super) fn record(k: u64, dropped: &[u64], sections: &[(&str, &str)]) -> EpicRecord {
        EpicRecord {
            record: PlanRecord {
                k,
                design: BodyHash::of(&TicketBody::from("a design")),
                dropped: dropped.iter().map(|n| TicketId::from(*n)).collect(),
                plan: plan_of(sections),
            },
            posted_at: None,
            mark: None,
        }
    }

    fn section(key: Option<&str>, goal: &str) -> Section {
        Section {
            key: key.map(NodeKey::from),
            template: TaskTemplate {
                goal: goal.to_owned(),
                owned_paths: "a path".to_owned(),
                scoped: false,
                out_of_scope: None,
                interfaces: "none".to_owned(),
                design_pages: "D1".to_owned(),
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
        }
    }

    fn body(node: &str, kind: NodeKind, sections: Vec<Section>) -> String {
        PlannedTicket {
            node: NodeId::from(node),
            kind,
            signatures: Vec::new(),
            turns: Vec::new(),
            waits: Vec::new(),
            sections,
        }
        .to_string()
    }

    fn ticket(n: u64, standing: Standing, body: &str) -> EpicTask {
        EpicTask {
            number: TicketId::from(n),
            title: format!("task {n}").into(),
            body: body.into(),
            url: "".into(),
            state: None,
            standing,
            group: None,
            kind: None,
            closed_at: None,
        }
    }

    pub(super) fn one(n: u64, standing: Standing, node: &str, goal: &str) -> EpicTask {
        let text = body(node, NodeKind::Body, vec![section(None, goal)]);
        ticket(n, standing, &text)
    }

    fn epic() -> Epic {
        Epic {
            id: TicketId::from(1),
            title: "an epic".into(),
            body: "Release: 0.6.0".into(),
            tasks: Vec::new(),
            signature: None,
            planned_at: None,
            challengers: Vec::new(),
            records: Vec::new(),
        }
    }

    fn id(key: &str, goal: &str) -> BehaviourId {
        BehaviourId {
            key: key.to_owned(),
            goal: goal.to_owned(),
        }
    }

    #[test]
    fn epic_check_passes_a_fully_built_epic() {
        let records = [record(1, &[], &[("A", "adds a"), ("B", "adds b")])];
        let tickets = [
            one(2, Standing::Done, "A", "adds a"),
            one(3, Standing::Done, "B", "adds b"),
        ];
        assert_eq!(
            behaviour_failures(&epic(), &records, &tickets),
            BehaviourCheck::Failures(Vec::new())
        );
    }

    #[test]
    fn epic_check_fails_naming_an_unbuilt_behaviour() {
        let records = [record(1, &[], &[("A", "adds a"), ("B", "adds b")])];
        let tickets = [
            one(2, Standing::Done, "A", "adds a"),
            one(3, Standing::Open, "B", "adds b"),
        ];
        let check = behaviour_failures(&epic(), &records, &tickets);
        assert_eq!(
            check,
            BehaviourCheck::Failures(vec![BehaviourFailure::NotBuilt(id("B", "adds b"))])
        );
        assert_eq!(check.to_string(), "planned, not built: B: adds b");
    }

    #[test]
    fn epic_check_names_a_built_behaviour_no_plan_asked_for() {
        let records = [record(1, &[], &[("A", "adds a")])];
        let tickets = [
            one(2, Standing::Done, "A", "adds a"),
            one(3, Standing::Done, "Z", "adds z"),
        ];
        assert_eq!(
            behaviour_failures(&epic(), &records, &tickets),
            BehaviourCheck::Failures(vec![BehaviourFailure::NotPlanned(id("Z", "adds z"))])
        );
    }

    #[test]
    fn epic_check_asks_nothing_of_a_dropped_ticket() {
        let records = [
            record(1, &[], &[("A", "adds a"), ("B", "adds b")]),
            record(2, &[3], &[("C", "adds c")]),
        ];
        let tickets = [
            one(2, Standing::Done, "A", "adds a"),
            one(3, Standing::ClosedNotPlanned, "B", "adds b"),
            one(4, Standing::Done, "C", "adds c"),
        ];
        assert_eq!(
            behaviour_failures(&epic(), &records, &tickets),
            BehaviourCheck::Failures(Vec::new())
        );
    }

    #[test]
    fn epic_check_counts_each_behaviour_of_a_merged_ticket() {
        let records = [record(1, &[], &[("A", "adds a"), ("B", "adds b")])];
        let text = body(
            "A+B",
            NodeKind::Body,
            vec![section(Some("A"), "adds a"), section(Some("B"), "adds b")],
        );
        let merged = ticket(2, Standing::Done, &text);
        assert_eq!(
            behaviour_failures(&epic(), &records, &[merged]),
            BehaviourCheck::Failures(Vec::new())
        );
    }

    #[test]
    fn epic_check_says_not_planned_as_a_graph() {
        let tickets = [one(2, Standing::Done, "A", "adds a")];
        let check = behaviour_failures(&epic(), &[], &tickets);
        assert_eq!(check, BehaviourCheck::NotPlannedAsGraph);
        assert_eq!(check.to_string(), "not planned as a graph");
    }

    #[test]
    fn epic_check_does_not_count_an_interface_ticket() {
        let records = [record(1, &[], &[("A", "adds a")])];
        let text = body(
            "I@1.1",
            NodeKind::Interface,
            vec![section(None, "the stand-ins")],
        );
        let tickets = [
            one(2, Standing::Done, "A", "adds a"),
            ticket(3, Standing::Done, &text),
        ];
        assert_eq!(
            behaviour_failures(&epic(), &records, &tickets),
            BehaviourCheck::Failures(Vec::new())
        );
    }
}

// The verb's whole read (I9): `check` on a fake board.

mod verb {
    use super::behaviours::{one, record};
    use super::{ScriptedShell, template};
    use crate::app::epic::check::{EpicCheckError, check};
    use crate::domain::{Signature, Standing, Ticket, TicketId};
    use crate::fakes::{FakeBoard, OWN_LOGIN};
    use crate::ports::Board;
    use std::time::SystemTime;

    const EPIC: u64 = 1;
    const GOOD: &str =
        "Release: 0.6.0\n\nDone when: the list exports, `export_lists_rows` passes.\n";

    /// A signed epic with `body`, its plan records posted and its tickets filed (a ticket closed
    /// as completed is built).
    fn board_with(body: &str, planned: &[(&str, &str)], built: &[(&str, &str)]) -> FakeBoard {
        let board = FakeBoard::default();
        let epic = TicketId::from(EPIC);
        board.add_epic(epic, "an epic");
        board.set_body(epic, body);
        board
            .sign(epic, &Signature::of(&body.into(), SystemTime::UNIX_EPOCH))
            .expect("signs");
        if !planned.is_empty() {
            let posted = record(1, &[], planned).record.to_string();
            board.comment_as(epic, OWN_LOGIN, &posted);
        }
        for (n, (node, goal)) in (2u64..).zip(built) {
            let task = one(n, Standing::Done, node, goal);
            board.add_ticket(Ticket {
                id: task.number,
                epic: Some(epic),
                title: task.title,
                body: task.body,
                state: None,
                url: "".into(),
                kind: None,
            });
            board.close(task.number);
        }
        board
    }

    fn run(board: &FakeBoard, shell: &ScriptedShell) -> Result<Vec<String>, EpicCheckError> {
        check(board, TicketId::from(EPIC), Some(&template()), shell)
    }

    #[test]
    fn epic_check_verb_prints_nothing_on_a_good_epic() {
        let board = board_with(GOOD, &[("A", "adds a")], &[("A", "adds a")]);
        let lines = run(&board, &ScriptedShell::default()).expect("a verdict");
        assert_eq!(lines, Vec::<String>::new());
    }

    #[test]
    fn epic_check_verb_prints_each_failure_and_exits_1() {
        let body = format!("{GOOD}Done when: it also works, with no test named.\n");
        let board = board_with(
            &body,
            &[("A", "adds a"), ("B", "adds b")],
            &[("A", "adds a")],
        );
        let lines = run(&board, &ScriptedShell::default()).expect("a verdict");
        assert_eq!(
            lines,
            [
                "planned, not built: B: adds b",
                "not checkable, it names no test: Done when: it also works, with no test named.",
            ]
        );
    }

    #[test]
    fn epic_check_verb_says_not_planned_as_a_graph() {
        let board = board_with(GOOD, &[], &[]);
        let lines = run(&board, &ScriptedShell::default()).expect("a verdict");
        assert_eq!(lines, ["not planned as a graph"]);
    }

    #[test]
    fn epic_check_verb_refuses_an_epic_without_done_when() {
        let board = board_with("Release: 0.6.0\n\nGoal: g.\n", &[("A", "adds a")], &[]);
        let error = run(&board, &ScriptedShell::default()).expect_err("refused");
        assert!(
            matches!(error, EpicCheckError::NoDoneWhen { .. }),
            "{error}"
        );
        assert!(
            error.to_string().contains("no `Done when:` line"),
            "{error}"
        );
    }

    #[test]
    fn epic_check_verb_refuses_an_epic_not_signed() {
        let board = board_with(GOOD, &[], &[]);
        board.set_body(TicketId::from(EPIC), &format!("{GOOD}An edit.\n"));
        let error = run(&board, &ScriptedShell::default()).expect_err("refused");
        assert!(matches!(error, EpicCheckError::NotSigned(_)), "{error}");
    }
}
