//! The layers keep their direction: the domain knows nothing outside itself, the ports know
//! only the domain and the manifest, and no adapter or fake reaches into the app or the CLI.
//! And one function decides who may start a phase: the tick and `zaf spawn phase` both pass
//! `may_start`, which asks the core's `schedule::place`, and nothing else derives where a ticket
//! sits.
//!
//! Read from the source on every run, so a new import that breaks a layer fails here rather
//! than in review. Test files (`tests.rs`, and the `support.rs` helpers only tests compile) and
//! comments are passed by: a test may wire any layer, and a doc link is not a dependency.

use std::path::{Path, PathBuf};

/// Each layer and the `crate::` modules it may name.
const LAYERS: &[(&str, &[&str])] = &[
    ("domain", &["domain", "newtype"]),
    ("ports", &["domain", "manifest", "newtype", "ports"]),
    ("manifest", &["domain", "manifest", "newtype"]),
    (
        "adapters",
        &["adapters", "domain", "manifest", "newtype", "ports"],
    ),
    (
        "fakes",
        &["domain", "fakes", "manifest", "newtype", "ports"],
    ),
];

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every non-test `.rs` file of a layer: its root file and everything under its folder.
fn layer_files(layer: &str) -> Vec<PathBuf> {
    let root = source_root();
    let mut files = Vec::new();
    let root_file = root.join(format!("{layer}.rs"));
    if root_file.is_file() {
        files.push(root_file);
    }
    let mut folders = vec![root.join(layer)];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && path.file_name().is_some_and(|name| name != "tests.rs")
            {
                files.push(path);
            }
        }
    }
    files
}

/// The `crate::<module>` names a line of code uses, comments dropped.
fn crate_modules(line: &str) -> Vec<String> {
    let code = line.split("//").next().unwrap_or_default();
    code.match_indices("crate::")
        .map(|(at, prefix)| {
            code[at + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect()
        })
        .collect()
}

#[test]
fn every_layer_names_only_the_layers_it_may() {
    let mut breaks = Vec::new();
    for (layer, allowed) in LAYERS {
        let files = layer_files(layer);
        assert!(!files.is_empty(), "the {layer} layer has no source files");
        for file in files {
            let text = std::fs::read_to_string(&file).expect("a source file reads");
            for (number, line) in text.lines().enumerate() {
                for module in crate_modules(line) {
                    if !allowed.contains(&module.as_str()) {
                        let shown = file.strip_prefix(source_root()).unwrap_or(&file);
                        breaks.push(format!(
                            "src/{}:{}: the {layer} layer names crate::{module}",
                            shown.display(),
                            number + 1
                        ));
                    }
                }
            }
        }
    }
    assert!(breaks.is_empty(), "layer breaks:\n{}", breaks.join("\n"));
}

#[test]
fn a_comment_is_not_an_import() {
    assert!(crate_modules("/// See [`X`](crate::adapters::X).").is_empty());
    assert_eq!(
        crate_modules("use crate::ports::Board; // crate::app"),
        ["ports"]
    );
}

/// Every non-test `.rs` file under `src`, as its path from `src` and its text.
fn source_files() -> Vec<(String, String)> {
    let root = source_root();
    let mut files = Vec::new();
    let mut folders = vec![root.clone()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder)
            .expect("a source folder reads")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && path
                    .file_name()
                    .is_some_and(|name| name != "tests.rs" && name != "support.rs")
            {
                let shown = path.strip_prefix(&root).expect("under src");
                let text = std::fs::read_to_string(&path).expect("a source file reads");
                files.push((shown.display().to_string(), text));
            }
        }
    }
    files.sort();
    files
}

/// The code of `text`, each line's comment dropped.
fn code_of(text: &str) -> String {
    text.lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `code` calls the function `name`: `name(` as a word of its own or at the end of a
/// path, never a method (`.name(`) and never its own definition (`fn name(`).
fn calls(code: &str, name: &str) -> bool {
    let call = format!("{name}(");
    code.match_indices(&call).any(|(at, _)| {
        let before = &code[..at];
        let word = before
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
        !word && !before.ends_with("fn ")
    })
}

/// The body of the function `name` in `code`: from `fn name(` to the first line that closes it.
fn body_of<'a>(code: &'a str, name: &str) -> &'a str {
    let start = code
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("no fn {name}"));
    let end = code[start..]
        .find("\n}")
        .map_or(code.len(), |end| start + end);
    &code[start..end]
}

#[test]
fn the_tick_and_spawn_phase_start_through_the_one_gate() {
    let files = source_files();
    let code = |path: &str| {
        let (_, text) = files
            .iter()
            .find(|(shown, _)| shown == path)
            .unwrap_or_else(|| panic!("no src/{path}"));
        code_of(text)
    };
    let tick = code("app/schedule/tick.rs");
    // The gate asks the core where the ticket sits.
    assert!(
        calls(body_of(&tick, "may_start"), "place"),
        "may_start asks place"
    );
    // The tick sorts its lane through the gate, and `zaf spawn phase` starts through it.
    assert!(calls(body_of(&tick, "sort_lane"), "may_start"));
    assert!(calls(body_of(&tick, "tick"), "sort_lane"));
    assert!(calls(&code("app/spawn/phase.rs"), "may_start"));
}

#[test]
fn nothing_but_the_core_and_the_gate_derives_where_a_ticket_sits() {
    // The core places; the gate (`tick.rs`) and the placement's JSON (`shadow.rs`) ask it.
    let askers = ["app/schedule/tick.rs", "app/schedule/shadow.rs"];
    let deriving = ["place", "Placement::of", "current_phase", "phase_status"];
    // The old path's names, gone with it.
    let gone = [
        "derive_standing",
        "gather_standing_facts",
        "check_current",
        "EpicGate",
        "TaskStanding",
        "TickTask",
        "PhaseTask",
    ];
    let mut breaks = Vec::new();
    for (path, text) in source_files() {
        if path.starts_with("domain/") || path == "domain.rs" {
            continue;
        }
        let code = code_of(&text);
        if !askers.contains(&path.as_str()) {
            for name in deriving {
                if calls(&code, name) {
                    breaks.push(format!("src/{path} calls {name}"));
                }
            }
            if path != "app/schedule/tick.rs" && calls(&code, "gets_nowhere") {
                breaks.push(format!("src/{path} calls gets_nowhere"));
            }
        }
        for name in gone {
            if code.contains(name) {
                breaks.push(format!("src/{path} names {name}"));
            }
        }
    }
    assert!(
        breaks.is_empty(),
        "standings derived beside the core:\n{}",
        breaks.join("\n")
    );
}

#[test]
fn a_call_is_told_from_a_method_and_a_definition() {
    assert!(calls("match place(facts, now) {", "place"));
    assert!(calls("schedule::place(facts, now)", "place"));
    assert!(!calls("self.vcs.place(slice, dir)", "place"));
    assert!(!calls("pub fn place(&self) {", "place"));
    assert!(!calls("let x = replace(a);", "place"));
}
