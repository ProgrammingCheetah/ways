//! The decisions note `memory/decisions/scheduler-rulings.md`, read as a file: what a ruling says
//! must stay what the design says (0.6.0's S11).

mod memory_rulings {
    use std::fs;
    use std::path::PathBuf;

    /// Every line of the note that is about ruling `id`: its table row and its observation.
    fn rows(id: &str) -> Vec<String> {
        let file =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("memory/decisions/scheduler-rulings.md");
        let text =
            fs::read_to_string(&file).unwrap_or_else(|e| panic!("read {}: {e}", file.display()));
        let row = format!("| {id} |");
        let tag = format!("#{id}");
        text.lines()
            .filter(|l| {
                l.starts_with(&row) || (l.starts_with("- [") && l.trim_end().ends_with(&tag))
            })
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn r31_names_the_skeleton_as_a_providing_node() {
        let rows = rows("R31");
        assert!(rows.len() >= 2, "R31's table row and observation: {rows:?}");
        for row in &rows {
            assert!(row.contains("`Provides`"), "R31 names `Provides`: {row}");
            assert!(
                !row.contains("Blocked by"),
                "R31 holds no `Blocked by`: {row}"
            );
        }
    }
}
