//! zaf's Vocabulary memory note (`zaf/memory/domain/vocabulary.md`) defines the terms of the plan
//! graph and the four words for how its nodes relate.

mod memory_vocabulary {
    use std::fs;
    use std::path::Path;

    const TERMS: [&str; 14] = [
        "Plan",
        "Node",
        "Behaviour",
        "Name",
        "Area",
        "Interface task",
        "Turn",
        "Kept wait",
        "Chain",
        "Width",
        "Critical path",
        "Plan record",
        "Filing mark",
        "Re-plan",
    ];

    const RELATION_WORDS: [&str; 4] = ["needs", "implements", "turn", "waits on"];

    #[test]
    fn vocabulary_names_every_plan_term() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("memory/domain/vocabulary.md");
        let note = fs::read_to_string(&path).expect("the Vocabulary note");

        for term in TERMS {
            let row = format!("| **{term}** | ");
            let line = note
                .lines()
                .find(|l| l.starts_with(&row))
                .unwrap_or_else(|| panic!("no table row defines the term `{term}`"));
            assert!(
                line.len() > row.len() + 20,
                "the term `{term}` has no real definition: {line}"
            );
        }
        for word in RELATION_WORDS {
            let row = format!("| **{word}** | ");
            assert!(
                note.lines().any(|l| l.starts_with(&row)),
                "no table row defines the relationship word `{word}`"
            );
        }
    }
}
