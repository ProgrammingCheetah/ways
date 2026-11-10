use super::*;

#[test]
fn a_line_runs_as_the_slot_then_sh_c_with_the_variable_exported_inside() {
    let slot = Slot::new("with-slot", Path::new("/state"), "the Engineer");
    assert_eq!(
        slot.command("echo 'hi'"),
        "with-slot sh -c 'export ZAF_SLOT_HELD=1\necho '\\''hi'\\'''"
    );
}

#[test]
fn a_record_stays_while_the_guard_lives_and_goes_with_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let slot = Slot::new("x", dir.path(), "team instance 1 on #7");
    let path = dir
        .path()
        .join(SLOTS_DIR)
        .join(std::process::id().to_string());
    {
        let _held = slot.hold("light").expect("holds");
        let record: SlotRecord =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("record")).expect("json");
        assert_eq!(record.holder, "team instance 1 on #7");
        assert_eq!(record.level, "light");
        assert_eq!(
            record.start,
            process::this().expect("this process").start,
            "the record carries this process's start, so a reused pid reads as dead"
        );
    }
    assert!(!path.exists(), "the record is removed at the end");
}
