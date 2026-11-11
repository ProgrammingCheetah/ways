//! The files a manifest is read from and how they combine.
//!
//! A project's manifest is `.zaf.toml` at its root, `.zaf/config.toml`, or both; the
//! `config.toml` of the zaf home sits below them. Each file is read as a TOML table and the
//! tables merge before the typed parse: table by table, property by property, the higher
//! layer winning a conflicting property and an array replaced whole, never merged. A root
//! `ZAF.toml` is the retired name: it loads when it stands alone and is an error beside
//! either new file.
//!
//! Above them all sits `.zaf/local.toml`, git-ignored and the machine's: of the manifest it
//! may set only `[spawn] slots` and `[gate] slot`, and any other manifest setting in it
//! fails the load. Its own tables (`[budget]`, `[report.prices]`, `[auth]`) are read by their
//! own readers and never reach the manifest.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::{Manifest, ManifestError, WorkspacesSection};

/// The manifest at the project root; it wins over [`FOLDER_FILE`].
pub const ROOT_FILE: &str = ".zaf.toml";

/// The manifest inside the project's `.zaf/` folder.
pub const FOLDER_FILE: &str = ".zaf/config.toml";

/// The machine's own file inside the project's `.zaf/` folder: the manifest's top layer,
/// for machine facts only.
pub const LOCAL_FILE: &str = ".zaf/local.toml";

/// The retired manifest name, still read when it stands alone.
pub const LEGACY_FILE: &str = "ZAF.toml";

/// The machine-wide defaults inside the zaf home.
pub const HOME_FILE: &str = "config.toml";

/// The manifest files one directory holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectFiles {
    /// `<dir>/.zaf.toml`.
    pub root: Option<PathBuf>,
    /// `<dir>/.zaf/config.toml`, unless `<dir>/.zaf` is the zaf home: that file is the
    /// machine's, never a project's.
    pub folder: Option<PathBuf>,
    /// `<dir>/ZAF.toml`.
    pub legacy: Option<PathBuf>,
    /// `<dir>/.zaf/local.toml`, left out like `folder` when `<dir>/.zaf` is the zaf home. It
    /// never makes a directory a project, so [`is_empty`](Self::is_empty) ignores it.
    pub local: Option<PathBuf>,
}

impl ProjectFiles {
    /// The manifest files in `dir`, leaving out the zaf home's own `config.toml`.
    pub fn find(dir: &Path, home: Option<&Path>) -> Self {
        let file = |name: &str| Some(dir.join(name)).filter(|path| path.is_file());
        let folder_is_home = home.is_some_and(|home| same_dir(&dir.join(".zaf"), home));
        Self {
            root: file(ROOT_FILE),
            folder: file(FOLDER_FILE).filter(|_| !folder_is_home),
            legacy: file(LEGACY_FILE),
            local: file(LOCAL_FILE).filter(|_| !folder_is_home),
        }
    }

    /// Whether `dir` holds no manifest file, so it is not a project.
    pub fn is_empty(&self) -> bool {
        self.root.is_none() && self.folder.is_none() && self.legacy.is_none()
    }

    /// Refuses a retired `ZAF.toml` beside a new manifest file.
    pub fn refuse_legacy_beside_new(&self) -> Result<(), ManifestError> {
        let Some(legacy) = &self.legacy else {
            return Ok(());
        };
        let beside: Vec<PathBuf> = self.root.iter().chain(&self.folder).cloned().collect();
        if beside.is_empty() {
            return Ok(());
        }
        // Absolute, so the message names the files whatever the directory the verb ran in.
        let absolute = |path: &PathBuf| std::path::absolute(path).unwrap_or_else(|_| path.clone());
        Err(ManifestError::LegacyBesideNew {
            legacy: absolute(legacy),
            beside: beside.iter().map(absolute).collect(),
        })
    }

    /// The project's own manifest files, lowest layer first: the legacy file or the folder
    /// file, then the root file. `.zaf/local.toml` is not among them: it is filtered before
    /// it is laid on top.
    fn layers(&self) -> impl Iterator<Item = &PathBuf> {
        self.legacy.iter().chain(&self.folder).chain(&self.root)
    }
}

/// Reads and combines the manifest of the project at `root`, with the zaf home's
/// `config.toml` below it when `home` is given and holds one.
pub(super) fn load(root: &Path, home: Option<&Path>) -> Result<Manifest, ManifestError> {
    let found = ProjectFiles::find(root, home);
    found.refuse_legacy_beside_new()?;
    if found.is_empty() {
        return Err(ManifestError::Read {
            path: root.to_owned(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no {ROOT_FILE}, {FOLDER_FILE} or {LEGACY_FILE} here"),
            ),
        });
    }
    let files = files_read_in(&found, home);
    let mut merged = Table::new();
    for file in &files {
        let table = read_table(file)?;
        if found.local.as_ref() == Some(file) {
            merge(&mut merged, local_layer(table)?);
        } else {
            if found.layers().any(|project| project == file) {
                refuse_machine_budget(&table, file)?;
            }
            merge(&mut merged, table);
        }
    }
    // Named highest layer first, the order a reader checks them in.
    let named: Vec<&Path> = files.iter().rev().map(PathBuf::as_path).collect();
    Manifest::from_table(merged).map_err(|error| error.in_files(&named))
}

/// The manifest files of the project at `root` that a load reads, highest layer first: what
/// `zaf doctor` names.
pub fn files_read(root: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut files = files_read_in(&ProjectFiles::find(root, home), home);
    files.reverse();
    files
}

/// The files a load reads, lowest layer first: the zaf home's `config.toml`, the legacy or
/// folder file, the root file, then `.zaf/local.toml`.
fn files_read_in(found: &ProjectFiles, home: Option<&Path>) -> Vec<PathBuf> {
    home.map(|home| home.join(HOME_FILE))
        .filter(|file| file.is_file())
        .into_iter()
        .chain(found.layers().cloned())
        .chain(found.local.clone())
        .collect()
}

/// What `.zaf/local.toml` lays over the manifest: `[spawn] slots` and `[gate] slot`. Its own
/// tables, `[budget]`, `[report.prices]` and `[auth]`, are for their own readers and are
/// dropped here; anything else fails the load, so a git-ignored file never loosens what the
/// project commits (`[forbidden]`, `[requires]`, the guard thresholds, the machine budget).
fn local_layer(table: Table) -> Result<Table, ManifestError> {
    let mut layer = Table::new();
    for (name, value) in table {
        let (allowed, own): (&[&str], &[&str]) = match name.as_str() {
            "spawn" => (&["slots"], &[]),
            "gate" => (&["slot"], &[]),
            "report" => (&[], &["prices"]),
            "budget" | "auth" => (&[], &[]),
            _ => return Err(refused(&name, &value)),
        };
        let Value::Table(settings) = value else {
            return Err(refused(&name, &value));
        };
        // `[budget]` and `[auth]` are own tables whole; the others name their keys.
        let own_whole = matches!(name.as_str(), "budget" | "auth");
        let mut kept = Table::new();
        for (key, setting) in settings {
            if allowed.contains(&key.as_str()) {
                kept.insert(key, setting);
            } else if !own_whole && !own.contains(&key.as_str()) {
                return Err(ManifestError::LocalSetting {
                    key: format!("[{name}] {key}"),
                });
            }
        }
        if !kept.is_empty() {
            layer.insert(name, Value::Table(kept));
        }
    }
    Ok(layer)
}

/// Refuses `[spawn] budget` and `[spawn] reserve` in a project file: both are the machine's,
/// read only from the zaf home's `config.toml`, and the error names the file that set it.
fn refuse_machine_budget(table: &Table, file: &Path) -> Result<(), ManifestError> {
    let spawn = table.get("spawn").and_then(Value::as_table);
    for key in ["budget", "reserve"] {
        if spawn.is_some_and(|spawn| spawn.contains_key(key)) {
            let error = ManifestError::Invalid {
                section: "spawn",
                key,
                reason: format!("is the machine's: set it only in the zaf home's {HOME_FILE}"),
            };
            return Err(error.in_files(&[file]));
        }
    }
    Ok(())
}

/// A top-level setting of `.zaf/local.toml` that it may not hold: a table is named as
/// `[table]`, anything else by its key.
fn refused(name: &str, value: &Value) -> ManifestError {
    let key = if value.is_table() {
        format!("[{name}]")
    } else {
        name.to_owned()
    };
    ManifestError::LocalSetting { key }
}

/// The `[workspaces] path` of the project at `root`, read without loading the manifest: the
/// files that parse are merged, and a path that is missing or not a string is the default.
/// Discovery uses it to place a root whose manifest does not load.
pub fn workspaces_path_lenient(root: &Path, home: Option<&Path>) -> PathBuf {
    let found = ProjectFiles::find(root, home);
    // The files of the project and the zaf home only: `.zaf/local.toml` may not move the
    // workspaces path.
    let mut files = files_read_in(&found, home);
    files.retain(|file| found.local.as_ref() != Some(file));
    let mut merged = Table::new();
    for file in &files {
        if let Ok(table) = read_table(file) {
            merge(&mut merged, table);
        }
    }
    merged
        .get("workspaces")
        .and_then(Value::as_table)
        .and_then(|workspaces| workspaces.get("path"))
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| WorkspacesSection::default().path)
}

/// Reads one file as a TOML table; an error names the file.
pub(super) fn read_table(file: &Path) -> Result<Table, ManifestError> {
    let text = std::fs::read_to_string(file).map_err(|source| ManifestError::Read {
        path: file.to_owned(),
        source,
    })?;
    text.parse::<Table>()
        .map_err(|error| ManifestError::from(error).in_files(&[file]))
}

/// Lays `above` over `below`: tables combine property by property, anything else in
/// `above` replaces what `below` had, an array included.
pub(super) fn merge(below: &mut Table, above: Table) {
    for (key, value) in above {
        match (below.get_mut(&key), value) {
            (Some(Value::Table(lower)), Value::Table(upper)) => merge(lower, upper),
            (_, value) => {
                below.insert(key, value);
            }
        }
    }
}

/// Whether two paths are the same directory, by their real paths when both exist.
fn same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests;
