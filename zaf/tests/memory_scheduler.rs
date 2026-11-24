//! zaf's scheduler memory note (`zaf/memory/domain/scheduler.md`) takes a planned epic's waits from
//! the graph and keeps `Blocked by` lines for an epic with no plan.

mod memory_scheduler {
    use std::fs;
    use std::path::Path;

    #[test]
    fn scheduler_note_takes_waits_from_the_graph() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("memory/domain/scheduler.md");
        let note = fs::read_to_string(&path).expect("the scheduler note");

        let line = note
            .lines()
            .find(|l| l.starts_with("4. **Waiting on a ticket:**"))
            .expect("the note's Waiting on a ticket line");

        assert!(
            line.contains("plan record") && line.contains("graph"),
            "the note does not name the graph as the wait source of a planned epic: {line}"
        );
        assert!(
            line.contains("kept waits"),
            "the note does not say the graph's kept waits are the waits: {line}"
        );
        assert!(
            line.contains("no plan record keeps its `Blocked by` lines"),
            "the note does not keep `Blocked by` lines for an epic with no plan: {line}"
        );
    }
}
