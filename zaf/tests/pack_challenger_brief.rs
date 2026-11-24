//! The challenger's standing brief has it read the epic's graph with `plan show` before and after
//! optimising, and raise each defect as a question with a proposed answer, never a filing (design
//! D21, D22, D28).

use zaf::app::spawn::CHALLENGER_BRIEF;

#[test]
fn challenger_brief_reads_the_graph_before_and_after() {
    assert!(
        CHALLENGER_BRIEF.contains("`plan` tool"),
        "{CHALLENGER_BRIEF}"
    );
    assert!(
        CHALLENGER_BRIEF.contains(r#"["show", "<epic>", "--before"]"#),
        "{CHALLENGER_BRIEF}"
    );
    assert!(
        CHALLENGER_BRIEF.contains(r#"`["show", "<epic>"]`"#),
        "{CHALLENGER_BRIEF}"
    );
}

#[test]
fn challenger_brief_raises_findings_with_a_proposed_answer() {
    assert!(
        CHALLENGER_BRIEF.contains("`board` tool"),
        "{CHALLENGER_BRIEF}"
    );
    assert!(
        CHALLENGER_BRIEF.contains(r#"["question", "<task>", "<question>"]"#),
        "{CHALLENGER_BRIEF}"
    );
    assert!(
        CHALLENGER_BRIEF.contains("Proposed answer:"),
        "{CHALLENGER_BRIEF}"
    );
}

#[test]
fn challenger_brief_names_no_filing_tool() {
    assert!(
        !CHALLENGER_BRIEF.contains("board file"),
        "{CHALLENGER_BRIEF}"
    );
    assert!(
        !CHALLENGER_BRIEF.contains("plan file"),
        "{CHALLENGER_BRIEF}"
    );
    assert!(
        !CHALLENGER_BRIEF.contains(r#"["file""#),
        "{CHALLENGER_BRIEF}"
    );
}
