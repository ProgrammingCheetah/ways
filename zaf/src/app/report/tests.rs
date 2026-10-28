use super::*;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app/report/fixtures")
}

fn query(matching: &str) -> Query {
    Query {
        roots: vec![fixtures().join("projects")],
        hours: 100_000.0,
        matching: matching.to_owned(),
        top: 25,
        prices: PriceTable::default(),
        now: SystemTime::now(),
    }
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(fixtures().join(name)).expect("golden file")
}

/// The golden files are `tools/burn.py --root fixtures/projects --hours 100000`, run once.
#[test]
fn the_text_is_what_burn_prints_for_the_fixtures() {
    assert_eq!(
        report(&query("")).render_text(),
        golden("burn.expected.txt")
    );
}

#[test]
fn match_keeps_the_runs_burn_keeps() {
    assert_eq!(
        report(&query("Plan")).render_text(),
        golden("burn.match-plan.expected.txt")
    );
    assert_eq!(
        report(&query("/subagents/")).runs,
        1,
        "a path match works too"
    );
}

#[test]
fn the_counts_are_pinned() {
    let report = report(&query(""));
    assert_eq!((report.runs, report.turns), (2, 7));
    let sonnet = &report.models[0];
    assert_eq!(sonnet.model, "claude-sonnet-5-5");
    assert_eq!(
        (
            sonnet.turns,
            sonnet.input,
            sonnet.cache_write,
            sonnet.cache_read,
            sonnet.output
        ),
        (4, 22, 4_510_000, 12_700_000, 520_000)
    );
    let opus = &report.models[1];
    assert_eq!(opus.model, "claude-opus-5-5");
    assert_eq!(
        (
            opus.turns,
            opus.input,
            opus.cache_write,
            opus.cache_read,
            opus.output
        ),
        (3, 18, 2_150_000, 4_100_000, 200_000)
    );
}

#[test]
fn a_repeated_message_id_counts_once() {
    let mut q = query("Plan");
    q.top = 1;
    let report = report(&q);
    assert_eq!(report.turns, 3, "four assistant lines, three ids");
    assert_eq!(report.table[0].context_last, 2_150_003);
}

#[test]
fn the_growth_profile_covers_subagents_of_three_turns_or_more() {
    let growth = report(&query("")).growth.expect("one subagent");
    assert_eq!(growth.runs, 1);
    assert_eq!(growth.median_turns, 4.0);
    assert!((growth.fixed_percent - 93.0).abs() < 0.5, "{growth:?}");
    assert!((growth.fixed_percent + growth.growth_percent - 100.0).abs() < 1e-9);
    assert!(report(&query("Plan")).growth.is_none());
}

#[test]
fn a_prompt_is_the_first_real_one_and_collapsed() {
    let rows = report(&query("Plan")).table;
    assert_eq!(rows[0].first_prompt, "Plan the export module for the demo");
    assert_eq!(rows[0].kind, "main");
}

#[test]
fn the_window_cuts_by_modification_time() {
    let mut q = query("");
    q.now = SystemTime::now() + Duration::from_secs(2 * 3600);
    q.hours = 1.0;
    let report = report(&q);
    assert_eq!(report.render_text(), "no runs\n");
    assert!(report.table.is_empty());
}

#[test]
fn json_carries_the_whole_report() {
    let value: serde_json::Value =
        serde_json::from_str(&report(&query("")).to_json()).expect("json");
    assert_eq!(value["runs"], 2);
    assert_eq!(value["turns"], 7);
    assert_eq!(value["models"][0]["model"], "claude-sonnet-5-5");
    assert_eq!(value["growth"]["runs"], 1);
    assert_eq!(value["table"][0]["kind"], "sub");
}

#[test]
fn a_missing_root_reads_nothing() {
    let mut q = query("");
    q.roots = vec![fixtures().join("nowhere")];
    assert_eq!(report(&q).runs, 0);
}

#[test]
fn roots_are_every_profile_and_the_default_config_on_request() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path();
    std::fs::create_dir_all(state.join("profiles/build")).expect("build");
    std::fs::create_dir_all(state.join("profiles/lean")).expect("lean");
    std::fs::write(state.join("profiles/stray.txt"), "").expect("file");
    let own = vec![
        state.join("profiles/build/projects"),
        state.join("profiles/lean/projects"),
    ];
    assert_eq!(transcript_roots(state, None), own);
    let all = transcript_roots(state, Some(Path::new("/home/me/.claude")));
    assert_eq!(all[2], Path::new("/home/me/.claude/projects"));
    assert_eq!(all.len(), 3);
    assert!(transcript_roots(&state.join("absent"), None).is_empty());
}
