//! The docs pages say what zaf does.

/// The actions page (docs/zaf-actions.md) explains how an epic is planned as a graph.
mod docs_plan {
    use std::fs;
    use std::path::Path;

    #[test]
    fn actions_doc_explains_the_plan() {
        let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/zaf-actions.md");
        let text = fs::read_to_string(&page).expect("the actions page is readable");
        for word in [
            "plan check",
            "plan file",
            "plan show",
            "plan verify",
            "Provides",
            "Needs",
            "Drop:",
        ] {
            assert!(
                text.contains(word),
                "docs/zaf-actions.md does not name `{word}`"
            );
        }
    }
}
