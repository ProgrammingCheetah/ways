//! The plan as a planner writes it (I1): every fixture's plan, written out, parses back into the
//! fixture's own nodes; a plan is refused with every fault, each at its section and line; and a
//! plan of random bytes is refused and never panics.

use zaf::domain::plan::text::{FaultKind, MAX_PLAN_BYTES, parse_plan, parse_plan_bytes};
use zaf::domain::plan::{Node, NodeKind, Size};

use super::{Outcome, fixtures};

/// Whether a name is one a planner may write: no `#`, `sig:` or `turn:`, no capital.
fn writable(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"./-_".contains(&b))
}

/// The nodes of a fixture's plan that the plan format can say: one behaviour of weight 1 or 2,
/// no filed ticket, no `Blocked by`, written in names a planner may use.
fn sayable(node: &Node) -> bool {
    let [behaviour] = node.behaviours.as_slice() else {
        return false;
    };
    node.kind == NodeKind::Body
        && node.filed.is_none()
        && node.blocked_by.is_empty()
        && node.turns.is_empty()
        && !node.id.as_str().is_empty()
        && node
            .id
            .as_str()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && matches!(&behaviour.size, Size::Weight(weight) if (1..=2).contains(&weight.0))
        && !behaviour.text.is_empty()
        && !behaviour.text.contains('\n')
        && behaviour.text == behaviour.text.trim()
        && behaviour.writes.iter().all(|a| writable(a.as_str()))
        && behaviour.provides.iter().all(|n| writable(n.as_str()))
        && behaviour.needs.iter().all(|n| writable(n.as_str()))
}

/// One section of a plan, as a planner writes it.
fn section(node: &Node) -> String {
    let behaviour = &node.behaviours[0];
    let size = match &behaviour.size {
        Size::Weight(weight) if weight.0 == 1 => "S",
        _ => "M",
    };
    let list = |names: Vec<&str>| {
        if names.is_empty() {
            "none".to_owned()
        } else {
            names.join(", ")
        }
    };
    format!(
        "Task: Build {id}\nNode: {id}\nGoal: {goal}\nScope: the part\nOut of scope: the rest\n\
         Interfaces: none\nDesign pages: D2\nAcceptance:\n- it works\nTests: it_works\n\
         Security trigger: no\nModel: sonnet\nSize: {size}\nWrites: {writes}\n\
         Provides: {provides}\nNeeds: {needs}\n\n",
        id = node.id,
        goal = behaviour.text,
        writes = list(behaviour.writes.iter().map(|a| a.as_str()).collect()),
        provides = list(behaviour.provides.iter().map(|n| n.as_str()).collect()),
        needs = list(behaviour.needs.iter().map(|n| n.as_str()).collect()),
    )
}

#[test]
fn plan_text_parses_every_fixture_plan() {
    let mut covered = 0;
    let mut nodes_covered = 0;
    for (file, fixture) in fixtures() {
        if matches!(fixture.expect.outcome(), Outcome::Refused(_)) {
            continue;
        }
        let nodes: Vec<&Node> = fixture.plan.nodes.iter().filter(|n| sayable(n)).collect();
        if nodes.is_empty() {
            // Fixture 15 plans nodes that are already merged (`A+B`): zaf's ids, never a plan's.
            assert_eq!(file, "15-split-then-merged-back.json");
            continue;
        }
        let text: String = nodes.iter().map(|node| section(node)).collect();
        let parsed = parse_plan(&text).unwrap_or_else(|faults| panic!("{file}:\n{faults}\n{text}"));
        // The epic is the verb's argument, not a line of the plan.
        let expected: Vec<Node> = nodes
            .iter()
            .map(|node| Node {
                epic: None,
                ..(*node).clone()
            })
            .collect();
        assert_eq!(
            parsed.input.nodes, expected,
            "{file}: the same nodes, behaviours and lists"
        );
        let expected: Vec<&Node> = nodes;
        assert_eq!(parsed.sections.len(), expected.len(), "{file}");
        for (section, node) in parsed.sections.iter().zip(&expected) {
            assert_eq!(section.node, node.id, "{file}");
            assert_eq!(section.title, format!("Build {}", node.id), "{file}");
            assert_eq!(section.acceptance, ["it works"], "{file}");
        }
        covered += 1;
        nodes_covered += expected.len();
    }
    assert_eq!(covered, 20, "every other non-refused fixture is read");
    assert_eq!(nodes_covered, 54, "the nodes a plan can say");
}

/// A plan of two sections, the second taking its extra lines.
fn plan_with(second: &str) -> String {
    format!(
        "Task: First\nNode: A\nGoal: g\nScope: s\nOut of scope: o\nInterfaces: none\n\
         Design pages: D2\nAcceptance:\n- a\nTests: t\nSecurity trigger: no\nModel: sonnet\n\
         Size: S\nWrites: app/x\n\nTask: Second\nNode: B\nGoal: g\nScope: s\nOut of scope: o\n\
         Interfaces: none\nDesign pages: D2\nAcceptance:\n- a\nTests: t\n\
         Security trigger: no\nModel: sonnet\nSize: S\n{second}"
    )
}

#[test]
fn plan_text_reads_the_lists_the_sections_and_the_headers() {
    let text = format!(
        "New names:\n- area app/x\n- signature cat.api: fn catalog()\n- no signature prompt.fix\n\
         Drop: #41, #42\n\n{}",
        plan_with("Group: update\nType: Bug\nProvides: cat.api\nNeeds:\n- prompt.fix\n- other\n")
    );
    let parsed = parse_plan(&text).unwrap_or_else(|faults| panic!("{faults}"));
    let names = parsed.input.new_names.as_ref().expect("new names");
    assert_eq!(names.areas.len(), 1);
    assert!(names.signature.contains(&"cat.api".into()));
    assert!(names.no_signature.contains(&"prompt.fix".into()));
    assert_eq!(parsed.signatures[&"cat.api".into()], "fn catalog()");
    assert_eq!(
        parsed.drop.iter().map(|t| t.get()).collect::<Vec<_>>(),
        [41, 42]
    );
    let second = &parsed.sections[1];
    assert_eq!(second.group.as_deref(), Some("update"));
    assert_eq!(second.kind.as_deref(), Some("Bug"));
    assert_eq!(parsed.input.nodes[1].behaviours[0].needs.len(), 2);
}

/// D4, D13: `Needs` may name a filed task by its number; nothing else may hold a `#`.
#[test]
fn plan_text_reads_a_need_on_a_filed_task_by_number() {
    let parsed = parse_plan(&plan_with("Needs: #1012, other, #0012\n"))
        .unwrap_or_else(|faults| panic!("{faults}"));
    let needs: Vec<&str> = parsed.input.nodes[1].behaviours[0]
        .needs
        .iter()
        .map(|name| name.as_str())
        .collect();
    assert_eq!(needs, ["#1012", "#12", "other"]);
    let task = parsed.input.nodes[1].behaviours[0]
        .needs
        .iter()
        .find_map(|name| name.task());
    assert_eq!(task.map(|task| task.get()), Some(1012));

    for bad in [
        "Needs: #abc\n",
        "Needs: #\n",
        "Needs: a#1\n",
        "Provides: #12\n",
    ] {
        let faults = parse_plan(&plan_with(bad)).expect_err(bad);
        assert!(
            faults
                .0
                .iter()
                .any(|fault| matches!(fault.kind, FaultKind::BadName { .. })),
            "{bad}: {faults}"
        );
    }
}

#[test]
fn plan_text_refuses_blocked_by() {
    let text = plan_with("Blocked by: #12\n");
    let faults = parse_plan(&text).expect_err("a Blocked by line is refused");
    let blocked: Vec<_> = faults
        .0
        .iter()
        .filter(|f| f.kind == FaultKind::BlockedBy)
        .collect();
    let line = text
        .lines()
        .position(|l| l.starts_with("Blocked by"))
        .unwrap()
        + 1;
    assert_eq!(blocked.len(), 1, "{faults}");
    assert_eq!(blocked[0].line, line);
    let section = blocked[0].section.as_ref().expect("in a section");
    assert_eq!((section.number, section.title.as_str()), (2, "Second"));
    assert_eq!(faults.0.len(), 1, "nothing else is wrong:\n{faults}");
    let said = faults.to_string();
    assert!(
        said.contains("\"Second\"") && said.contains(&format!("line {line}")),
        "{said}"
    );
}

#[test]
fn plan_text_names_every_fault() {
    // A bad node id, a bad name, a bad area, a missing field, a duplicate node and a Blocked by.
    let text = "Task: One\nNode: a b\nGoal: g\nScope: s\nOut of scope: o\nInterfaces: none\n\
                Design pages: D2\nAcceptance:\n- a\nTests: t\nSecurity trigger: no\n\
                Model: sonnet\nSize: S\nNeeds: Bad_Name\n\n\
                Task: Two\nNode: B\nGoal: g\nScope: s\nOut of scope: o\nInterfaces: none\n\
                Acceptance:\n- a\nTests: t\nSecurity trigger: no\nModel: sonnet\nSize: S\n\
                Writes: not a file.rs?\n\n\
                Task: Three\nNode: B\nGoal: g\nScope: s\nOut of scope: o\nInterfaces: none\n\
                Design pages: D2\nAcceptance:\n- a\nTests: t\nSecurity trigger: no\n\
                Model: sonnet\nSize: S\nBlocked by: #3\n";
    let faults = parse_plan(text).expect_err("a plan with faults is refused");
    let line_of = |needle: &str| text.lines().position(|l| l.starts_with(needle)).unwrap() + 1;
    let has = |number: usize, line: usize, matches: &dyn Fn(&FaultKind) -> bool| {
        faults.0.iter().any(|f| {
            f.line == line
                && f.section.as_ref().is_some_and(|s| s.number == number)
                && matches(&f.kind)
        })
    };
    assert!(
        has(1, line_of("Node: a b"), &|k| matches!(
            k,
            FaultKind::BadNode { .. }
        )),
        "{faults}"
    );
    assert!(
        has(1, line_of("Needs: Bad_Name"), &|k| matches!(
            k,
            FaultKind::BadName { .. }
        )),
        "{faults}"
    );
    assert!(
        has(
            2,
            line_of("Task: Two"),
            &|k| matches!(k, FaultKind::Missing { field } if field == "Design pages")
        ),
        "{faults}"
    );
    assert!(
        has(2, line_of("Writes:"), &|k| matches!(
            k,
            FaultKind::BadArea { .. }
        )),
        "{faults}"
    );
    let duplicate = text
        .lines()
        .enumerate()
        .filter(|(_, l)| *l == "Node: B")
        .nth(1)
        .unwrap()
        .0
        + 1;
    assert!(
        has(3, duplicate, &|k| matches!(
            k,
            FaultKind::DuplicateNode { .. }
        )),
        "{faults}"
    );
    assert!(
        has(3, line_of("Blocked by"), &|k| *k == FaultKind::BlockedBy),
        "{faults}"
    );
    // The refusal says it all at once, one line each.
    assert!(faults.to_string().lines().count() >= 6, "{faults}");
}

#[test]
fn plan_text_refuses_every_malformed_header_and_field() {
    for (text, what) in [
        ("", "an empty plan"),
        ("Goal: x\n", "a field before any task"),
        ("New names:\n- field zone\n", "a bad new name"),
        ("Drop: 12\n", "a drop without #"),
        (
            "Task: T\nNode: A\nSize: XL\n",
            "a size a planner does not file",
        ),
    ] {
        assert!(parse_plan(text).is_err(), "{what}");
    }
}

/// A small generator that needs no crate: xorshift.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn plan_text_refuses_random_bytes() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for round in 0..3_000_u64 {
        let len = (rng.next() % 3_000) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
        assert!(
            parse_plan_bytes(&bytes).is_err(),
            "round {round}: random bytes are no plan"
        );
        // Lossy text keeps the shape of the noise but is valid UTF-8: it must also not panic.
        let _ = parse_plan(&String::from_utf8_lossy(&bytes));
    }
    // Noise made of the plan's own words: any outcome but a panic.
    let words = [
        "Task:",
        "Node:",
        "Goal:",
        "Writes:",
        "Needs:",
        "Blocked by:",
        "New names:",
        "Drop:",
        "- ",
        "area ",
        "signature ",
        "no signature ",
        "#",
        ",",
        ":",
        "\n",
        "\n  ",
        " ",
        "é",
        "A",
        "x.y",
        "Acceptance:",
        "Size:",
        "XS",
        "none",
        "—",
    ];
    for _ in 0..3_000 {
        let count = (rng.next() % 200) as usize;
        let text: String = (0..count)
            .map(|_| words[(rng.next() % words.len() as u64) as usize])
            .collect();
        let _ = parse_plan(&text);
    }
    // The input is bounded.
    let big = vec![b'a'; MAX_PLAN_BYTES + 1];
    let faults = parse_plan_bytes(&big).expect_err("too large");
    assert!(matches!(faults.0[0].kind, FaultKind::TooLarge { .. }));
    let long_line = format!("Task: {}\n", "x".repeat(5_000));
    assert!(parse_plan(&long_line).is_err());
    let many: String = "Task: t\n".repeat(1_000);
    let faults = parse_plan(&many).expect_err("too many sections");
    assert!(faults.0.len() <= 51, "the faults listed are bounded");
}
