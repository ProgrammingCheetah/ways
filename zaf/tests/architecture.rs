//! The layers keep their direction: the domain knows nothing outside itself, the ports know
//! only the domain and the manifest, and no adapter or fake reaches into the app or the CLI.
//!
//! Read from the source on every run, so a new import that breaks a layer fails here rather
//! than in review. Test files (`tests.rs`) and comments are passed by: a test may wire any
//! layer, and a doc link is not a dependency.

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
