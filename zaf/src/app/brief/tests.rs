use std::cell::RefCell;

use super::*;
use crate::domain::{PageStatus, TaskLabel};
use crate::fakes::{FakeBoard, FakeDesign};

const BODY: &str = "\
Goal: Members export their reading list as CSV.
Owned paths: src/export/**, src/cli/export.rs (new)
Interfaces: none
Design pages: 1001 (decided), §2 of 1002, 1001 again
Architecture: app over ports
Acceptance:
  - an empty list exports the header row only
Tests: beside the use case
Security trigger: no
Model: sonnet
Size: S
Blocked by: —
";

fn task() -> IssueNumber {
    IssueNumber::from(12)
}

fn board_with(body: &str) -> FakeBoard {
    let board = FakeBoard::default();
    board.add_ticket(Ticket {
        number: task(),
        epic: Some(IssueNumber::from(10)),
        title: "Export a reading list".to_owned(),
        body: body.to_owned(),
        label: Some(TaskLabel::Claimed),
    });
    board
}

fn design() -> FakeDesign {
    let design = FakeDesign::default();
    for (id, title, text) in [
        (
            "1001",
            "Export",
            "An empty list exports the header row only.",
        ),
        (
            "1002",
            "Lists",
            "A reading list belongs to exactly one account.",
        ),
        ("1003", "Unnamed", "A page the ticket does not name."),
    ] {
        design.add_page(Page {
            id: id.parse().expect("page id"),
            title: title.to_owned(),
            status: PageStatus::Decided,
            text: text.to_owned(),
        });
    }
    design
}

fn query<'a>(state_dir: &'a Path, style: Option<&'a Path>) -> BriefQuery<'a> {
    BriefQuery {
        task: task(),
        instance: "programmer-3",
        state_dir,
        style,
    }
}

fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let start = text
        .find(&format!("## {heading}\n"))
        .unwrap_or_else(|| panic!("no {heading} section in:\n{text}"));
    let rest = &text[start..];
    let end = rest[3..]
        .find("\n## ")
        .map_or(rest.len(), |offset| offset + 3);
    &rest[..end]
}

#[test]
fn the_pack_holds_every_section_in_order() {
    let state = tempfile::tempdir().expect("temp dir");
    let style = state.path().join("style.md");
    fs::write(&style, "Name things by what they mean.\n").expect("style");
    let board = board_with(BODY);
    let design = design();
    let chained = RefCell::new(Vec::new());
    let chain = |path: &str| {
        chained.borrow_mut().push(path.to_owned());
        Ok(format!("root\n  {path}"))
    };
    let ports = BriefPorts {
        board: &board,
        design: Some(&design),
        nodes: Some(&chain),
    };

    let brief = brief(&query(state.path(), Some(&style)), &ports).expect("brief");

    let expected_path = state.path().join("runs/programmer-3/brief-12.md");
    assert_eq!(brief.path, expected_path);
    let text = fs::read_to_string(&brief.path).expect("pack");
    assert_eq!(brief.chars, text.chars().count());
    assert_eq!(brief.tokens, text.chars().count().div_ceil(4));
    assert!(text.starts_with("# Brief: #12 Export a reading list\n"));

    let headings: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("## "))
        .collect();
    assert_eq!(
        headings,
        [
            "## Ticket",
            "## Design pages",
            "## Owned paths",
            "## Style",
            "## Interface"
        ]
    );
    assert!(section(&text, "Ticket").contains("Goal: Members export their reading list as CSV."));

    let pages = section(&text, "Design pages");
    assert!(
        pages.contains("### 1001 Export (decided)\n\nAn empty list exports the header row only.")
    );
    assert!(
        pages
            .contains("### 1002 Lists (decided)\n\nA reading list belongs to exactly one account.")
    );
    assert!(!pages.contains("1003"), "an unnamed page stays out");
    assert_eq!(
        pages.matches("### 1001").count(),
        1,
        "a page named twice appears once"
    );

    assert_eq!(*chained.borrow(), ["src/export", "src/cli/export.rs"]);
    let owned = section(&text, "Owned paths");
    assert!(owned.contains("### src/export\n\n```\nroot\n  src/export\n```"));
    assert!(!owned.contains(NODES_ABSENT_NOTE));

    assert!(section(&text, "Style").contains("Name things by what they mean."));
    assert!(section(&text, "Interface").contains("zaf ask -t <task>"));
    assert_eq!(
        fs::read_to_string(state.path().join("runs/.gitignore")).expect("ignore"),
        "*\n"
    );
}

#[test]
fn without_nodes_the_owned_paths_carry_the_skip_note() {
    let state = tempfile::tempdir().expect("temp dir");
    let board = board_with(BODY);
    let design = design();
    let ports = BriefPorts {
        board: &board,
        design: Some(&design),
        nodes: None,
    };

    let brief = brief(&query(state.path(), None), &ports).expect("brief");

    let text = fs::read_to_string(&brief.path).expect("pack");
    let owned = section(&text, "Owned paths");
    assert!(owned.contains(NODES_ABSENT_NOTE));
    assert!(!owned.contains("```"));
    assert!(section(&text, "Style").contains("declares no style file"));
}

#[test]
fn a_failing_nodes_run_is_noted_in_place() {
    let state = tempfile::tempdir().expect("temp dir");
    let board = board_with(BODY);
    let chain = |_: &str| Err("exit 3".to_owned());
    let ports = BriefPorts {
        board: &board,
        design: None,
        nodes: Some(&chain),
    };

    let brief = brief(&query(state.path(), None), &ports).expect("brief");

    let text = fs::read_to_string(&brief.path).expect("pack");
    assert!(section(&text, "Owned paths").contains("nodes failed: exit 3"));
    assert!(section(&text, "Design pages").contains("no design source"));
}

#[test]
fn a_body_off_the_template_is_still_packed_with_a_note() {
    let state = tempfile::tempdir().expect("temp dir");
    let board = board_with("Just prose naming 1001.");
    let design = design();
    let ports = BriefPorts {
        board: &board,
        design: Some(&design),
        nodes: None,
    };

    let brief = brief(&query(state.path(), None), &ports).expect("brief");

    let text = fs::read_to_string(&brief.path).expect("pack");
    assert!(section(&text, "Ticket").contains("does not follow the task template"));
    assert!(!section(&text, "Design pages").contains("### 1001"));
}

#[test]
fn an_unknown_task_is_a_board_error() {
    let state = tempfile::tempdir().expect("temp dir");
    let board = FakeBoard::default();
    let ports = BriefPorts {
        board: &board,
        design: None,
        nodes: None,
    };
    let error = brief(&query(state.path(), None), &ports).expect_err("no ticket");
    assert!(matches!(error, BriefError::Board(PortError::NotFound(_))));
}

#[test]
fn the_run_folder_is_the_instance_slug() {
    let state = tempfile::tempdir().expect("temp dir");
    let board = board_with(BODY);
    let ports = BriefPorts {
        board: &board,
        design: None,
        nodes: None,
    };
    let mut named = query(state.path(), None);
    named.instance = "Team instance 12 on #16";

    let brief = brief(&named, &ports).expect("brief");

    let expected = state.path().join("runs/team-instance-12-on-16/brief-12.md");
    assert_eq!(brief.path, expected);
    assert!(expected.is_file());
}

#[test]
fn the_instance_must_be_one_path_segment() {
    let state = tempfile::tempdir().expect("temp dir");
    let board = board_with(BODY);
    let ports = BriefPorts {
        board: &board,
        design: None,
        nodes: None,
    };
    for instance in ["", ".", "..", "a/b"] {
        let bad = BriefQuery {
            instance,
            ..query(state.path(), None)
        };
        let error = brief(&bad, &ports).expect_err("bad instance");
        assert!(matches!(error, BriefError::Instance(_)), "{instance:?}");
    }
}

#[test]
fn owned_paths_drop_globs_prose_and_repeats() {
    let field = "zaf/src/app/brief/**, `zaf/src/cli/brief.rs` (new) plus lines in zaf/src/app.rs; docs/*, zaf/src/app.rs";
    assert_eq!(
        owned_paths(field),
        [
            "zaf/src/app/brief",
            "zaf/src/cli/brief.rs",
            "zaf/src/app.rs",
            "docs"
        ]
    );
}
