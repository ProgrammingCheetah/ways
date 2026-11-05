use std::path::{Path, PathBuf};

use super::*;

fn file() -> PathBuf {
    PathBuf::from(".zaf/actions/land")
}

fn parsed(text: &str) -> ParsedAction {
    parse(&file(), text).unwrap_or_else(|error| panic!("refused: {error}"))
}

/// The refusal for `text`: its line and reason.
fn refusal(text: &str) -> MetadataError {
    parse(&file(), text).expect_err("should be refused")
}

/// A valid block with `extra` lines after the two required ones.
fn block(extra: &str) -> String {
    format!("# zaf description: land it\n# zaf 1: failed\n{extra}echo body\n")
}

#[test]
fn metadata_parses_each_marker() {
    for marker in ["#", "//", "--", ";", "REM", "::", "///", ";;"] {
        let text = format!(
            "{marker} zaf description: do it\n{marker} zaf 1: failed\n{marker} zaf 2: broke\nbody\n"
        );
        let action = parsed(&text);
        assert_eq!(action.metadata.description, "do it", "marker {marker}");
        assert_eq!(action.metadata.codes.len(), 2, "marker {marker}");
        assert_eq!(action.body_line, 4, "marker {marker}");
    }
}

#[test]
fn metadata_ends_at_the_first_other_line() {
    // A shebang is skipped; the body starts at the first non-metadata line,
    // and a metadata-looking line after it is body.
    let text = "#!/bin/sh\n# zaf description: d\n# zaf 1: f\nset -e\n# zaf mode: 000\n";
    let action = parsed(text);
    assert_eq!(action.body_line, 4);
    assert_eq!(action.metadata.mode, ActionMode::DEFAULT);

    // A plain comment ends the block too.
    let action = parsed("# zaf description: d\n# zaf 1: f\n# just a comment\n# zaf 2: late\n");
    assert_eq!(action.body_line, 3);
    assert!(!action.metadata.codes.contains_key(&2));

    // A file that is all metadata has its body one past the last line.
    assert_eq!(parsed("# zaf description: d\n# zaf 1: f\n").body_line, 3);

    // Lookalikes are body: no blank after the marker, no colon, REM run on.
    for lookalike in [
        "#zaf mode: 000",
        "# zaf mode 000",
        "REMzaf mode: 000",
        "echo # zaf mode: 000",
    ] {
        let text = format!("# zaf description: d\n# zaf 1: f\n{lookalike}\n");
        assert_eq!(parsed(&text).body_line, 3, "{lookalike}");
    }

    // The shebang is only skipped on the first line.
    assert!(parse(&file(), "\n#!/bin/sh\n# zaf description: d\n# zaf 1: f\n").is_err());
}

#[test]
fn metadata_refusals_name_the_line() {
    // No metadata at all: the first line.
    let error = refusal("echo hi\n");
    assert_eq!((error.line, error.file.as_path()), (1, file().as_path()));
    assert!(
        error.to_string().starts_with(".zaf/actions/land:1: "),
        "{error}"
    );
    assert!(error.reason.contains("no metadata"), "{error}");

    // After a shebang, the line after it.
    assert_eq!(refusal("#!/bin/sh\necho hi\n").line, 2);
    assert_eq!(refusal("").line, 1);

    // No description, or no code 1: the line the block opens on.
    let error = refusal("# zaf 1: f\n");
    assert_eq!(error.line, 1);
    assert!(error.reason.contains("description"), "{error}");
    let error = refusal("#!/bin/sh\n# zaf description: d\n# zaf 2: f\n");
    assert_eq!(error.line, 2);
    assert!(error.reason.contains("code 1"), "{error}");

    // The rest name the offending line itself.
    for (extra, reason) in [
        ("# zaf 0: success\n", "code 0"),
        ("# zaf one: x\n", "unknown key"),
        ("# zaf 1.5: x\n", "not an integer"),
        ("# zaf 256: x\n", "1 to 255"),
        ("# zaf -3: x\n", "1 to 255"),
        ("# zaf 3:\n", "no meaning"),
        ("# zaf 1: again\n", "twice"),
        ("# zaf timeout: soon\n", "timeout"),
        ("# zaf timeout: 0\n", "timeout"),
        ("# zaf timeout: 1.5\n", "timeout"),
        ("# zaf timeout:\n", "timeout"),
        ("# zaf colour: red\n", "unknown key"),
        ("# zaf mode: 755\n# zaf mode: 750\n", "twice"),
        ("# zaf description: again\n", "twice"),
    ] {
        let error = refusal(&block(extra));
        let first_bad = extra.lines().last().expect("a line");
        let expected_line = 3 + extra.lines().count() - 1;
        assert_eq!(error.line, expected_line, "{first_bad}: {error}");
        assert!(error.reason.contains(reason), "{first_bad}: {error}");
    }

    // An empty description is not a description.
    assert_eq!(refusal("# zaf description:\n# zaf 1: f\n").line, 1);
}

#[test]
fn declared_125_is_refused() {
    let error = refusal(&block("# zaf 125: nested\n"));
    assert_eq!(error.line, 3);
    assert!(error.reason.contains("125"), "{error}");
    assert!(error.reason.contains("reserved"), "{error}");
    // 124 and 126 are ordinary codes.
    let action = parsed(&block("# zaf 124: a\n# zaf 126: b\n"));
    assert_eq!(
        action.metadata.codes.keys().copied().collect::<Vec<_>>(),
        [1, 124, 126]
    );
}

#[test]
fn mode_and_group_parse() {
    let plain = parsed(&block(""));
    assert_eq!(plain.metadata.mode, ActionMode::DEFAULT);
    assert_eq!(plain.metadata.mode.to_string(), "755");
    assert!(plain.metadata.group.is_empty());
    assert_eq!(plain.metadata.target, None);
    assert!(plain.metadata.phases.is_empty());
    assert_eq!(plain.metadata.timeout, None);

    let action = parsed(&block(
        "# zaf mode: 750\n# zaf group: team, release\n# zaf timeout: 10\n",
    ));
    let mode = action.metadata.mode;
    assert_eq!(mode.to_string(), "750");
    assert!(mode.owner().read() && mode.owner().write() && mode.owner().run());
    assert!(mode.group().read() && !mode.group().write() && mode.group().run());
    assert!(!mode.others().read() && !mode.others().write() && !mode.others().run());
    assert_eq!(action.metadata.group, ["team", "release"]);
    assert_eq!(action.metadata.timeout, Some(10));

    // One role, and leading zeros in a mode.
    assert_eq!(
        parsed(&block("# zaf group: team\n")).metadata.group,
        ["team"]
    );
    assert_eq!(
        parsed(&block("# zaf mode: 005\n"))
            .metadata
            .mode
            .to_string(),
        "005"
    );
    assert_eq!(
        parsed("# zaf description: d\n# zaf 1: f\n# zaf 2: g\n")
            .metadata
            .codes[&2],
        "g"
    );
}

#[test]
fn bad_mode_is_refused() {
    for mode in ["", "75", "7555", "758", "abc", "0o75", "-75", "7 5", "٧٥٥"] {
        let error = refusal(&block(&format!("# zaf mode: {mode}\n")));
        assert_eq!(error.line, 3, "mode {mode:?}");
        assert!(
            error.reason.contains("three octal digits"),
            "mode {mode:?}: {error}"
        );
    }
    // An empty group or an empty entry.
    for group in ["", "team,", ",team", "team,,release", " , "] {
        let error = refusal(&block(&format!("# zaf group: {group}\n")));
        assert_eq!(error.line, 3, "group {group:?}");
        assert!(error.reason.contains("group"), "group {group:?}: {error}");
    }
}

#[test]
fn target_parses() {
    let action = parsed(&block("# zaf target: task\n"));
    assert_eq!(action.metadata.target, Some(Target::Task));
    assert_eq!(Target::Task.to_string(), "task");
    for target in ["", "release", "Task", "tasks", "task, epic"] {
        let error = refusal(&block(&format!("# zaf target: {target}\n")));
        assert_eq!(error.line, 3, "target {target:?}");
        assert!(
            error.reason.contains("target"),
            "target {target:?}: {error}"
        );
    }
}

#[test]
fn phases_need_a_task_target() {
    // The phases line is the one named, whichever order the lines come in.
    let error = refusal(&block("# zaf phases: land\n"));
    assert_eq!(error.line, 3);
    assert!(error.reason.contains("target: task"), "{error}");
    let error = refusal(&block("# zaf phases: land\n# zaf mode: 750\n"));
    assert_eq!(error.line, 3);

    let phases = |extra: &str| parsed(&block(extra)).metadata.phases;
    assert_eq!(
        phases("# zaf target: task\n# zaf phases: land\n"),
        [Phase::Land]
    );
    assert_eq!(
        phases("# zaf phases: check, land\n# zaf target: task\n"),
        [Phase::Check, Phase::Land]
    );
    assert!(phases("# zaf target: task\n").is_empty());
}

#[test]
fn unknown_phase_is_refused() {
    let all = "claim, build, check, publish, review, land, done";
    let phases = parsed(&block(&format!(
        "# zaf target: task\n# zaf phases: {all}\n"
    )))
    .metadata
    .phases;
    assert_eq!(phases.len(), 7);
    assert_eq!(
        phases
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
        all
    );

    for phase in ["merge", "Land", "unclaimed", "land,", "", "land build"] {
        let error = refusal(&block(&format!(
            "# zaf target: task\n# zaf phases: {phase}\n"
        )));
        assert_eq!(error.line, 4, "phase {phase:?}");
    }
    let error = refusal(&block("# zaf target: task\n# zaf phases: land, merge\n"));
    assert!(error.reason.contains("merge"), "{error}");
}

/// A project root and a machine home in one temp folder.
struct Tiers {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Tiers {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("project");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&root).expect("root");
        std::fs::create_dir_all(&home).expect("home");
        Self {
            _dir: dir,
            root,
            home,
        }
    }

    fn project(&self, name: &str) -> PathBuf {
        let path = self.root.join(".zaf").join("actions").join(name);
        write(&path);
        path
    }

    fn machine(&self, name: &str) -> PathBuf {
        let path = self.home.join("actions").join(name);
        write(&path);
        path
    }

    fn resolve(&self, name: &str) -> Option<Resolved> {
        resolve(&self.root, &self.home, name).expect("resolve")
    }

    fn list(&self) -> Vec<Resolved> {
        list(&self.root, &self.home).expect("list")
    }
}

fn write(path: &Path) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, "# zaf description: d\n# zaf 1: f\n").expect("write");
}

#[test]
fn project_shadows_machine() {
    let tiers = Tiers::new();
    let project = tiers.project("land");
    let machine = tiers.machine("land");
    tiers.machine("only-machine");
    tiers.project("only-project");

    let land = tiers.resolve("land").expect("land");
    assert_eq!(
        (land.tier, land.path.as_path()),
        (Tier::Project, project.as_path())
    );
    assert_eq!(land.shadows, Some(Tier::Machine));
    assert_eq!(
        tiers.resolve("only-machine").expect("machine").tier,
        Tier::Machine
    );
    assert_eq!(
        tiers.resolve("only-project").expect("project").tier,
        Tier::Project
    );

    // The other tier is still reachable by asking for it.
    let hidden = resolve_in(Tier::Machine, &tiers.root, &tiers.home, "land")
        .expect("resolve_in")
        .expect("found");
    assert_eq!(
        (hidden.tier, hidden.path.as_path()),
        (Tier::Machine, machine.as_path())
    );
    let own = resolve_in(Tier::Project, &tiers.root, &tiers.home, "land").expect("resolve_in");
    assert_eq!(own.expect("found").path, project);
    assert!(
        resolve_in(Tier::Project, &tiers.root, &tiers.home, "only-machine")
            .expect("ok")
            .is_none()
    );
    assert!(
        resolve_in(Tier::Machine, &tiers.root, &tiers.home, "only-project")
            .expect("ok")
            .is_none()
    );
}

#[test]
fn list_names_the_shadowed_tier() {
    let tiers = Tiers::new();
    assert!(tiers.list().is_empty(), "no folders, no actions");
    tiers.project("land");
    tiers.machine("land");
    tiers.machine("beta");
    tiers.project("alpha");
    std::fs::create_dir_all(tiers.root.join(".zaf/actions/a-directory")).expect("dir");

    let listed: Vec<_> = tiers
        .list()
        .into_iter()
        .map(|found| (found.name, found.tier, found.shadows))
        .collect();
    assert_eq!(
        listed,
        [
            ("alpha".to_owned(), Tier::Project, None),
            ("beta".to_owned(), Tier::Machine, None),
            ("land".to_owned(), Tier::Project, Some(Tier::Machine)),
        ]
    );
    assert_eq!(Tier::Project.to_string(), "project");
    assert_eq!(Tier::Machine.to_string(), "machine");
}

#[test]
fn unknown_name_resolves_to_nothing() {
    let tiers = Tiers::new();
    // No built-in table: not even the names the design uses are special.
    for name in [
        "land",
        "slice-open",
        "slice-merge",
        "gate",
        "pr-merge",
        "release",
        "x",
    ] {
        assert_eq!(tiers.resolve(name), None, "{name}");
        for tier in [Tier::Project, Tier::Machine] {
            let found = resolve_in(tier, &tiers.root, &tiers.home, name).expect("ok");
            assert_eq!(found, None, "{name}");
        }
    }
    tiers.project("land");
    assert_eq!(tiers.resolve("slice-open"), None);
    assert_eq!(tiers.list().len(), 1);
}

#[test]
fn action_name_rules() {
    let tiers = Tiers::new();
    for name in [
        "land",
        "slice-open",
        "pr_merge",
        "build.sh",
        "a.b-c_d",
        "v1.2",
        "UPPER",
        ".hidden",
        "..x",
    ] {
        let path = tiers.project(name);
        let found = tiers
            .resolve(name)
            .unwrap_or_else(|| panic!("{name} not found"));
        assert_eq!(
            (found.name.as_str(), found.path.as_path()),
            (name, path.as_path())
        );
    }
    // No extension is dropped: `build.sh` is `build.sh`, never `build`.
    assert_eq!(tiers.resolve("build"), None);

    for name in [
        "",
        " ",
        "two words",
        "tab\there",
        "new\nline",
        "trailing ",
        "a/b",
        "/abs",
        "../land",
        "..",
        ".",
        "a\\b",
        "x/../y",
        "nul\0",
    ] {
        for result in [
            resolve(&tiers.root, &tiers.home, name).map(|_| ()),
            resolve_in(Tier::Machine, &tiers.root, &tiers.home, name).map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(ResolveError::BadName { .. })),
                "{name:?}: {result:?}"
            );
        }
        assert!(check_name(name).is_err(), "{name:?}");
    }
}

#[test]
fn workspace_actions_are_not_resolved() {
    let tiers = Tiers::new();
    // A slice workspace lives under the root's .zaf/workspaces and carries its
    // own copy of the repository, actions folder included.
    let workspace = tiers.root.join(".zaf/workspaces/feature/0.2.0-103-107");
    let edited = workspace.join(".zaf/actions/land");
    write(&edited);
    write(&workspace.join(".zaf/actions/brand-new"));
    assert!(edited.is_file());

    assert_eq!(tiers.resolve("land"), None);
    assert_eq!(tiers.resolve("brand-new"), None);
    assert!(tiers.list().is_empty());

    // The root's file wins over the workspace edit, and the machine's is
    // still seen when the root has none.
    let committed = tiers.project("land");
    assert_eq!(tiers.resolve("land").expect("land").path, committed);
    let machine = tiers.machine("brand-new");
    assert_eq!(tiers.resolve("brand-new").expect("machine").path, machine);

    // A workspace passed as the root is the caller's mistake to make, not a
    // fallback: resolution never looks above or below the root it is given.
    assert_eq!(
        resolve(&tiers.root.join(".zaf"), &tiers.home, "land").expect("ok"),
        None
    );
}

fn role(name: &str) -> Caller {
    Caller::Role(name.into())
}

/// Whether `caller` may run under `digits` with `group`.
fn permits(caller: &Caller, digits: &str, group: &[&str]) -> bool {
    let mode = ActionMode::parse(digits).expect("a mode");
    let group: Vec<String> = group.iter().map(|name| (*name).to_owned()).collect();
    caller.permits(mode, &group)
}

#[test]
fn permits_reads_only_the_callers_digit() {
    for digit in 0..8_u8 {
        let open = digit & 1 == 1;
        let group = ["team"];
        let cases = [
            (Caller::Engineer, format!("{digit}00")),
            (role("team"), format!("0{digit}0")),
            (role("programmer"), format!("00{digit}")),
            (role("auditor"), format!("00{digit}")),
        ];
        for (caller, mode) in cases {
            assert_eq!(permits(&caller, &mode, &group), open, "{caller:?} {mode}");
        }
        // A role named in the group reads the group digit, whatever its name.
        let auditor = ["auditor"];
        assert_eq!(
            permits(&role("auditor"), &format!("0{digit}0"), &auditor),
            open
        );
        assert!(!permits(&role("auditor"), &format!("70{digit}"), &auditor));
    }
}

#[test]
fn permits_read_takes_the_read_bit_of_the_callers_digit() {
    let reads = |caller: &Caller, digits: &str, group: &[String]| {
        caller.permits_read(ActionMode::parse(digits).expect("a mode"), group)
    };
    let team = vec!["team".to_owned()];
    for digit in 0..8_u8 {
        let open = digit & 4 == 4;
        let cases = [
            (Caller::Engineer, format!("{digit}00")),
            (role("team"), format!("0{digit}0")),
            (role("programmer"), format!("00{digit}")),
        ];
        for (caller, mode) in cases {
            assert_eq!(reads(&caller, &mode, &team), open, "{caller:?} {mode}");
        }
    }
    // Only the caller's own digit counts: the group is not read in by the others digit.
    assert!(!reads(&role("team"), "707", &team));
    // Neither bit stands in for the other.
    assert!(!reads(&role("programmer"), "711", &team));
    assert!(permits(&role("programmer"), "711", &["team"]));
    assert!(reads(&role("programmer"), "744", &team));
    assert!(!permits(&role("programmer"), "744", &["team"]));
}

#[test]
fn can_shadow_reads_only_the_owner_w() {
    for owner in 0..8_u8 {
        for rest in ["00", "22", "77"] {
            let mode = ActionMode::parse(&format!("{owner}{rest}")).expect("a mode");
            assert_eq!(can_shadow(mode), owner & 2 == 2, "{mode}");
        }
    }
    assert!(can_shadow(ActionMode::DEFAULT));
}

#[test]
fn only_an_absent_instance_is_the_engineer() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    assert_eq!(Caller::from_value(None), Ok(Caller::Engineer));
    assert_eq!(
        Caller::from_value(Some(OsStr::new("team instance 2 on #7"))),
        Ok(role("team"))
    );
    assert_eq!(
        Caller::from_value(Some(OsStr::new("engineer instance 2 on #7"))),
        Ok(role("engineer"))
    );
    for value in [
        OsStr::new(""),
        OsStr::new("local"),
        OsStr::from_bytes(b"team\xff instance 2 on #7"),
    ] {
        let refusal = Caller::from_value(Some(value)).expect_err("should be refused");
        assert!(refusal.starts_with("ZAF_INSTANCE is "), "{refusal}");
    }
}

#[test]
fn a_caller_prints_on_one_line() {
    assert_eq!(Caller::Engineer.to_string(), "the Engineer");
    assert_eq!(role("programmer").to_string(), "programmer");
    assert_eq!(role("a\nNOT_RUN b").to_string(), "a\\nNOT_RUN b");
}

/// Runs the project action `name` as the Engineer from the project root.
fn run_project_action(place: &Tiers, name: &str) -> RunOutcome {
    run_action(&RunRequest {
        name,
        args: &[],
        cwd: &place.root,
        project_root: Some(&place.root),
        home: &place.home,
        timeout_minutes: 1,
        minute: MINUTE,
        nesting: Nesting::default(),
        caller: Caller::Engineer,
        only: None,
        slice: None,
        env: std::collections::BTreeMap::new(),
    })
}

/// An executable project action with valid metadata that leaves `ran` in the root when it runs.
fn executable_action(place: &Tiers, name: &str, shebang: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = place.root.join(".zaf").join("actions").join(name);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    let text = format!("{shebang}# zaf description: d\n# zaf 1: failed\n: > \"$ZAF_ROOT/ran\"\n");
    std::fs::write(&path, text).expect("write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[test]
fn no_shebang_is_refused_before_spawning() {
    let place = Tiers::new();
    executable_action(&place, "bare", "");

    let outcome = run_project_action(&place, "bare");
    assert_eq!(outcome.result, RunResult::NotRun);
    assert_eq!(outcome.exit, NOT_RUN_EXIT);
    assert_eq!(outcome.log, None, "no log is left");
    let headline = outcome.headline().expect("a refusal prints");
    assert!(headline.starts_with("NOT_RUN bare: "), "{headline}");
    assert!(headline.contains("no shebang"), "{headline}");
    assert!(!place.root.join("ran").exists(), "the body never ran");
    let logs = place.root.join(".zaf").join("logs");
    assert!(!logs.exists(), "no log folder was made");

    // The same file with a shebang runs as before.
    executable_action(&place, "bare", "#!/bin/sh\n");
    let outcome = run_project_action(&place, "bare");
    assert_eq!(outcome.result, RunResult::Ok, "{outcome:?}");
    assert!(place.root.join("ran").exists(), "the body ran");
}
