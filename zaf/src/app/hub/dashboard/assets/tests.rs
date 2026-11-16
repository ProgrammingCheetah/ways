use std::path::Path;

use super::*;

/// Every file under `dir`, as its path below `root` with a leading slash.
fn files_below(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("reads the build") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            files_below(root, &path, found);
        } else {
            let below = path.strip_prefix(root).expect("below the build");
            found.push(format!("/{}", below.to_string_lossy().replace('\\', "/")));
        }
    }
}

#[test]
fn every_built_file_is_served() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("web/dist");
    let mut built = Vec::new();
    files_below(&dist, &dist, &mut built);
    assert!(!built.is_empty(), "web/dist holds the build");
    for path in built {
        let asset = find(&path).unwrap_or_else(|| {
            panic!("{path} is built but not embedded: add it to FILES in assets.rs")
        });
        let on_disk = std::fs::read(dist.join(path.trim_start_matches('/'))).expect("reads");
        assert_eq!(asset.body, on_disk, "{path} is the build's file");
    }
}

#[test]
fn an_unknown_path_is_not_a_file() {
    assert!(find("/assets/other.js").is_none());
    assert!(find("/../Cargo.toml").is_none());
}
