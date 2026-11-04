//! The files a manifest is read from and how they combine.
//!
//! A project's manifest is `.zaf.toml` at its root, `.zaf/config.toml`, or both; the
//! `config.toml` of the zaf home sits below them. Each file is read as a TOML table and the
//! tables merge before the typed parse: table by table, property by property, the higher
//! layer winning a conflicting property and an array replaced whole, never merged. A root
//! `ZAF.toml` is the retired name: it loads when it stands alone and is an error beside
//! either new file.

use std::path::{Path, PathBuf};

use toml::{Table, Value};

use super::{Manifest, ManifestError};

/// The manifest at the project root; it wins over [`FOLDER_FILE`].
pub const ROOT_FILE: &str = ".zaf.toml";

/// The manifest inside the project's `.zaf/` folder.
pub const FOLDER_FILE: &str = ".zaf/config.toml";

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
        Err(ManifestError::LegacyBesideNew {
            legacy: legacy.clone(),
            beside,
        })
    }

    /// The files to read, lowest layer first: the legacy file or the folder file, then
    /// the root file.
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
    let machine = home
        .map(|home| home.join(HOME_FILE))
        .filter(|file| file.is_file());
    let files: Vec<&Path> = machine
        .iter()
        .chain(found.layers())
        .map(PathBuf::as_path)
        .collect();
    let mut merged = Table::new();
    for file in &files {
        merge(&mut merged, read_table(file)?);
    }
    // Named highest layer first, the order a reader checks them in.
    let named: Vec<&Path> = files.iter().rev().copied().collect();
    Manifest::from_table(merged).map_err(|error| error.in_files(&named))
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
