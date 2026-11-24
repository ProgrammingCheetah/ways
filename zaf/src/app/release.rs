//! The release folder of a version: where its objectives and design documents live.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use semver::Version;

use super::epic::EpicLine;
use crate::domain::plan::vocabulary::{self, VocabularyError};
use crate::domain::plan::{Name, Vocabulary};
use crate::domain::screen::{Line, Meaning, Text};
use crate::domain::{DocumentSignature, Epic, EpicStage, SignatureState, of_version};
use crate::manifest::Manifest;
use crate::ports::{Board, PortError, Vcs};

/// The placeholder `[release] folder` holds for the version.
const VERSION_PLACEHOLDER: &str = "{version}";

/// The document that states a release's objectives.
const OBJECTIVES_FILE: &str = "objectives.md";

/// What an older release folder holds in place of the objectives document.
const SCOPE_FILE: &str = "scope.md";

/// The release's design document.
const DESIGN_FILE: &str = "design.md";

/// The file the old way of signing kept in a release folder.
const SIGNOFF_FILE: &str = "signoff.md";

/// The folder holding one version's release documents, resolved from `[release] folder`.
///
/// Read by [`phase_due`], and through it by `zaf begin` and `zaf status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseFolder {
    path: PathBuf,
}

impl ReleaseFolder {
    /// `ReleaseFolder::of(root, manifest, version)`: resolves the folder of `version` under the
    /// project `root`, from the manifest's `[release] folder` with `{version}` replaced. Nothing
    /// is read or made.
    pub fn of(root: &Path, manifest: &Manifest, version: &str) -> Self {
        let pattern = manifest.release.folder.to_string_lossy();
        let relative = pattern.replace(VERSION_PLACEHOLDER, version);
        Self {
            path: root.join(relative),
        }
    }

    /// The folder's absolute path (under `root`), whether or not it exists yet.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Makes the folder, with its parents, when it is missing; an existing one is left as it is.
    pub fn ensure(&self) -> io::Result<()> {
        std::fs::create_dir_all(&self.path)
    }

    /// `objectives_document() -> Option<PathBuf>`: the objectives document the folder holds,
    /// `objectives.md` when present, else `scope.md`, else none.
    pub fn objectives_document(&self) -> Option<PathBuf> {
        [OBJECTIVES_FILE, SCOPE_FILE]
            .into_iter()
            .map(|name| self.path.join(name))
            .find(|path| path.is_file())
    }

    /// Whether the folder holds `signoff.md`: it was made the old way, and zaf leaves it alone.
    pub fn has_signoff(&self) -> bool {
        self.path.join(SIGNOFF_FILE).is_file()
    }

    /// `design_document() -> PathBuf`: where the release's `design.md` is, whether or not it
    /// exists.
    pub fn design_document(&self) -> PathBuf {
        self.path.join(DESIGN_FILE)
    }
}

/// The phase a version is due for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The objectives document is missing, unsigned, or edited since it was signed.
    Objectives,
    /// The objectives are signed; the design is unsigned.
    Design,
    /// Both documents are signed.
    Nothing,
}

impl Phase {
    /// The phase's name, as `zaf status` prints it and its JSON carries it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Objectives => "objectives",
            Self::Design => "design",
            Self::Nothing => "nothing",
        }
    }
}

/// A signature block that does not read, for the caller to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalformedBlock {
    /// The document it is in.
    pub path: PathBuf,
    /// The 1-based line at fault.
    pub line: usize,
    /// What is wrong there.
    pub reason: String,
}

/// The phase due for a version, with the malformed block that held its document back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseDue {
    /// The phase due.
    pub phase: Phase,
    /// The malformed block of the document the phase is due for; empty when it is not malformed.
    pub malformed: Vec<MalformedBlock>,
}

/// How a document stands: signed, not signed (missing, or edited since), or malformed.
enum Standing {
    Signed,
    Unsigned,
    Malformed(MalformedBlock),
}

fn standing(path: &Path) -> Standing {
    // A document that cannot be read counts as not signed.
    let Ok(text) = std::fs::read_to_string(path) else {
        return Standing::Unsigned;
    };
    match DocumentSignature::read(&text) {
        SignatureState::Signed { .. } => Standing::Signed,
        SignatureState::Unsigned => Standing::Unsigned,
        SignatureState::Malformed { line, reason } => Standing::Malformed(MalformedBlock {
            path: path.to_path_buf(),
            line,
            reason,
        }),
    }
}

/// `git hash-object` of `bytes`: the SHA-1 of `blob <len>\0` then the bytes, in lowercase hex.
///
/// Computed with the `sha1` crate (the RustCrypto one, beside `sha2`), so no `git` process runs.
fn git_blob_hash(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(format!("blob {}\0", bytes.len()));
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The text of a table cell inside one pair of backticks; none for any other cell.
fn backticked(cell: &str) -> Option<&str> {
    cell.trim().strip_prefix('`')?.strip_suffix('`')
}

/// Whether a `signoff.md` holds a row signing `name` with `hash`: a Markdown table row whose
/// first cell is the file name and whose second is its 40-hex hash, each in backticks. Later
/// cells and header rows are ignored.
fn signoff_row_signs(signoff: &str, name: &str, hash: &str) -> bool {
    signoff.lines().any(|line| {
        let mut cells = line.trim().strip_prefix('|').unwrap_or("").split('|');
        let (Some(file), Some(value)) = (cells.next(), cells.next()) else {
            return false;
        };
        backticked(file) == Some(name)
            && backticked(value).is_some_and(|value| value.eq_ignore_ascii_case(hash))
    })
}

/// `document_signed(folder, document)`: whether the release document `document` (a file of
/// `folder`, such as its design) reads as signed.
///
/// It is signed when zaf's signature block holds ([`DocumentSignature`]), or when a row of the
/// folder's `signoff.md` names the file and holds its `git hash-object` value as it reads now. A
/// row for another file, or a stale hash, does not sign; neither does a missing or unreadable
/// document. Read by the vocabulary load, which refuses a plan on an unsigned design.
pub fn document_signed(folder: &ReleaseFolder, document: &Path) -> bool {
    if matches!(standing(document), Standing::Signed) {
        return true;
    }
    let (Some(name), Ok(bytes), Ok(signoff)) = (
        document.file_name().and_then(|name| name.to_str()),
        std::fs::read(document),
        std::fs::read_to_string(folder.path().join(SIGNOFF_FILE)),
    ) else {
        return false;
    };
    signoff_row_signs(&signoff, name, &git_blob_hash(&bytes))
}

impl PhaseDue {
    fn of(phase: Phase, standing: Standing) -> Self {
        let malformed = match standing {
            Standing::Malformed(block) => vec![block],
            Standing::Signed | Standing::Unsigned => Vec::new(),
        };
        Self { phase, malformed }
    }
}

/// `phase_due(folder, epics, version)`: the phase due for `version`, worked out from the folder
/// and the board's open `epics` each time and stored nowhere.
///
/// Objectives are due while the objectives document is missing or not signed (an edit below its
/// block unsigns it). Design is due once they are signed, while `design.md` is not signed.
/// Nothing is due after that, whether or not an epic of `version` is open: signing the design
/// takes an open epic of the version, so a signed design with none means every epic of it is
/// closed and the version is built ([`stage`]). A malformed block counts as not signed and comes
/// back with its path and line.
pub fn phase_due(folder: &ReleaseFolder, _epics: &[Epic], _version: &str) -> PhaseDue {
    let objectives = folder
        .objectives_document()
        .map_or(Standing::Unsigned, |path| standing(&path));
    if !matches!(objectives, Standing::Signed) {
        return PhaseDue::of(Phase::Objectives, objectives);
    }
    let design = standing(&folder.design_document());
    if !matches!(design, Standing::Signed) {
        return PhaseDue::of(Phase::Design, design);
    }
    PhaseDue::of(Phase::Nothing, Standing::Signed)
}

/// An open epic of the version, as `zaf begin` prints it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct EpicToSign {
    /// The epic's line: its id, stage and tally.
    #[serde(flatten)]
    pub epic: EpicLine,
    /// Whether it still wants the Engineer's signature (not signed, or edited since).
    pub unsigned: bool,
}

impl EpicToSign {
    /// The lines printed: the epic's line, then for an epic not signed `  zaf epic sign <id>`.
    pub fn render(&self) -> String {
        if self.unsigned {
            format!(
                "{}\n  zaf epic sign {}",
                self.epic.render(),
                self.epic.number
            )
        } else {
            self.epic.render()
        }
    }
}

/// Where a version stands, worked out on each read and stored nowhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// The objectives are missing, unsigned, or edited since they were signed.
    Objectives,
    /// The objectives are signed; the design is not.
    Design,
    /// Both documents are signed and an open epic of the version is unsigned: every open epic of
    /// the version, each marked as signed or not.
    Epics(Vec<EpicToSign>),
    /// Every open epic of the version is signed and one is not done.
    Build,
    /// Every open epic of the version is done, or none is open.
    Built,
}

/// `stage(root, manifest, board, version)`: the stage `version` stands at, read from its release
/// folder and the board's open epics.
///
/// Objectives and design are as [`phase_due`] says. Past them it is [`Stage::Epics`] while an
/// open epic of the version is unsigned, [`Stage::Build`] once every one is signed and one is not
/// done, and [`Stage::Built`] once every one is done or none is open.
pub fn stage(
    root: &Path,
    manifest: &Manifest,
    board: &dyn Board,
    version: &str,
) -> Result<Stage, PortError> {
    let folder = ReleaseFolder::of(root, manifest, version);
    let epics = board.epics()?;
    Ok(match phase_due(&folder, &epics, version).phase {
        Phase::Objectives => Stage::Objectives,
        Phase::Design => Stage::Design,
        Phase::Nothing => stage_of_epics(&epics, version),
    })
}

fn stage_of_epics(epics: &[Epic], version: &str) -> Stage {
    let open: Vec<&Epic> = of_version(epics, version).collect();
    if open.iter().any(|epic| !epic.signed()) {
        let lines = open
            .into_iter()
            .map(|epic| EpicToSign {
                epic: EpicLine::from(epic),
                unsigned: !epic.signed(),
            })
            .collect();
        Stage::Epics(lines)
    } else if open.iter().all(|epic| epic.stage() == EpicStage::Done) {
        Stage::Built
    } else {
        Stage::Build
    }
}

/// `release_versions(root, manifest)`: the versions of the release folders under `root`, the
/// `{version}` segment of `[release] folder` read as the version, oldest first. A name that does
/// not start with a digit is no version, and a folder that does not hold the pattern's rest is
/// no release folder.
pub fn release_versions(root: &Path, manifest: &Manifest) -> Vec<String> {
    let pattern = &manifest.release.folder;
    let components: Vec<_> = pattern.components().collect();
    let Some(at) = components.iter().position(|part| {
        part.as_os_str()
            .to_string_lossy()
            .contains(VERSION_PLACEHOLDER)
    }) else {
        return Vec::new();
    };
    let parent: PathBuf = components[..at].iter().collect();
    let segment = components[at].as_os_str().to_string_lossy();
    let (prefix, suffix) = segment
        .split_once(VERSION_PLACEHOLDER)
        .unwrap_or((&segment, ""));
    let Ok(entries) = std::fs::read_dir(root.join(parent)) else {
        return Vec::new();
    };
    let mut versions: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter_map(|name| {
            let version = name.strip_prefix(prefix)?.strip_suffix(suffix)?;
            version
                .starts_with(|c: char| c.is_ascii_digit())
                .then(|| version.to_owned())
        })
        .filter(|version| ReleaseFolder::of(root, manifest, version).path().is_dir())
        .collect();
    versions.sort_by_cached_key(|version| version_key(version));
    versions
}

/// A version's numeric parts, so `0.9.0` sorts before `0.10.0`; the text breaks a tie.
fn version_key(version: &str) -> (Vec<u64>, String) {
    let numbers = version
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .collect();
    (numbers, version.to_owned())
}

/// `is_released(root, manifest, vcs, version)`: whether the version is released, which is when
/// the version control holds the tag `[release] tag` names for it. With no such key, or a
/// version control that cannot say, no version counts as released.
pub fn is_released(root: &Path, manifest: &Manifest, vcs: Option<&dyn Vcs>, version: &str) -> bool {
    let (Some(pattern), Some(vcs)) = (&manifest.release.tag, vcs) else {
        return false;
    };
    let tag = pattern.replace(VERSION_PLACEHOLDER, version);
    vcs.has_tag(root, &tag).unwrap_or(false)
}

/// `next_version(root, manifest, vcs)`: the minor after the highest version among the release
/// tags and the release folders, with patch 0 (0.5.2 gives 0.6.0), or 0.1.0 with neither.
///
/// The tags are those of `vcs` that match `[release] tag`, the `{version}` segment read as the
/// version; with no such key, or no `vcs`, or one that cannot list its tags, only the folders
/// count. A name that does not parse as a version is skipped. The one place setup, begin and the
/// build view ask for the next version.
pub fn next_version(root: &Path, manifest: &Manifest, vcs: Option<&dyn Vcs>) -> Version {
    let from_folders = release_versions(root, manifest);
    let from_tags = tag_versions(root, manifest, vcs);
    from_folders
        .iter()
        .chain(&from_tags)
        .filter_map(|text| Version::parse(text).ok())
        .max()
        .map_or(Version::new(0, 1, 0), |highest| {
            Version::new(highest.major, highest.minor + 1, 0)
        })
}

/// The version text of each tag of `vcs` that the `[release] tag` pattern matches.
fn tag_versions(root: &Path, manifest: &Manifest, vcs: Option<&dyn Vcs>) -> Vec<String> {
    let (Some(pattern), Some(vcs)) = (&manifest.release.tag, vcs) else {
        return Vec::new();
    };
    let (prefix, suffix) = pattern
        .split_once(VERSION_PLACEHOLDER)
        .unwrap_or((pattern, ""));
    let tags = vcs.tags(root).unwrap_or_default();
    tags.iter()
        .filter_map(|tag| tag.strip_prefix(prefix)?.strip_suffix(suffix))
        .map(str::to_owned)
        .collect()
}

/// A release whose objectives or design are due.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueRelease {
    /// The version.
    pub version: String,
    /// The phase due: objectives or design.
    pub phase: Phase,
}

/// The way on from a due objectives or design phase (D9): those phases are the Engineer's
/// interviews, run in zaf's chair, so a terminal opens it with `zaf`. One text, read by `zaf
/// status`, `zaf epic list` and the MCP's refusal of `begin`.
pub const WAY_ON: &str =
    "Objectives and design are the Engineer's interviews: open zaf's chair in a terminal (`zaf`).";

impl DueRelease {
    /// The command that runs the phase.
    pub fn command(&self) -> String {
        format!("zaf begin --version {}", self.version)
    }

    /// The way on to the phase: [`WAY_ON`].
    pub fn way_on(&self) -> &'static str {
        WAY_ON
    }
}

/// `due_lines(version, phase)`: a due release as `zaf status` and `zaf epic list` print it, its
/// `release <v>: <phase> due` line and then the way-on line.
pub fn due_lines(version: &str, phase: &str) -> Vec<Line> {
    let piece = |meaning, text: &str| -> (Meaning, Text) { (meaning, Text::from(text)) };
    vec![
        vec![
            piece(Meaning::Waiting, "release "),
            piece(Meaning::Noun, version),
            piece(Meaning::Waiting, ": "),
            piece(Meaning::Waiting, phase),
            piece(Meaning::Waiting, " due"),
        ],
        vec![piece(Meaning::Verb, WAY_ON)],
    ]
}

/// `due_releases(root, manifest, epics, released)`: each release folder whose phase due is
/// objectives or design, oldest first.
///
/// A folder is left out when `released` says its version is already released, or when it holds a
/// `signoff.md`: those were made the old way, and zaf leaves them alone.
pub fn due_releases(
    root: &Path,
    manifest: &Manifest,
    epics: &[Epic],
    released: &dyn Fn(&str) -> bool,
) -> Vec<DueRelease> {
    release_versions(root, manifest)
        .into_iter()
        .filter(|version| !released(version))
        .filter_map(|version| {
            let folder = ReleaseFolder::of(root, manifest, &version);
            if folder.has_signoff() {
                return None;
            }
            let phase = phase_due(&folder, epics, &version).phase;
            (phase != Phase::Nothing).then_some(DueRelease { version, phase })
        })
        .collect()
}

/// The line that opens the vocabulary block in a design (D5, I3).
const VOCABULARY_FENCE: &str = "```toml vocabulary";

/// The line that closes a fenced block.
const CLOSING_FENCE: &str = "```";

/// Why an epic's release vocabulary is not loaded. Each message is one line and ends with the way
/// on.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VocabularyLoadError {
    /// The epic's body has no `Release: <version>` first line.
    #[error("epic #{epic} has no Release line: put `Release: <version>` first in its body")]
    NoRelease {
        /// The epic.
        epic: String,
    },
    /// The release's design is missing, not signed, or edited since it was signed.
    #[error(
        "the design of release {version} ({path}) is not signed: sign it, or sign it again after an edit, in zaf's chair (`zaf`)"
    )]
    Unsigned {
        /// The release.
        version: String,
        /// The design document.
        path: String,
    },
    /// The design holds more than one block; D5 allows the one.
    #[error(
        "the design of release {version} holds {count} vocabulary blocks: keep the one `toml vocabulary` block and sign the design again"
    )]
    ManyBlocks {
        /// The release.
        version: String,
        /// How many blocks.
        count: usize,
    },
    /// The block does not read; the parser's refusal is passed on.
    #[error(
        "the vocabulary block of release {version} does not read: {error}; fix the block and sign the design again"
    )]
    Block {
        /// The release.
        version: String,
        /// The parser's refusal.
        error: VocabularyError,
    },
    /// A name that is `existing` points at a path the integration head does not hold.
    #[error(
        "name \"{name}\" is existing at `{path}`, which is not in the tree at {revision}: fix the path in the design and sign it again, or drop `existing` from the name"
    )]
    MissingPath {
        /// The name.
        name: String,
        /// Its `existing` path.
        path: String,
        /// The integration head it was looked for at.
        revision: String,
    },
    /// The integration branch pattern names a ticket, so no one branch is the integration head.
    #[error("[branches] integration {0} names a ticket: it must name a release alone")]
    IntegrationPattern(String),
    /// The version control could not answer.
    #[error("the version control could not say what the integration head holds: {0}")]
    Vcs(String),
}

/// `release_vocabulary(root, manifest, vcs, epic)`: the vocabulary of the signed design of the
/// release `epic`'s `Release:` line names (D5 to D7).
///
/// The folder is `[release] folder` for that version and the block is the one fenced
/// ```` ```toml vocabulary ```` in its `design.md`, read by [`vocabulary::parse`]. The design must be
/// signed ([`document_signed`]: zaf's block, or a matching `signoff.md` row). A design with no
/// block gives an empty vocabulary, so the plan declares its own names (D6). Each name that has
/// `existing` must have that path in the tree at the head of the integration branch
/// (`Vcs::path_at`), or the load is refused naming the name and the path (D7). Nothing is written.
///
/// Read by `plan check` and the outside-names check of the epic.
pub fn release_vocabulary(
    root: &Path,
    manifest: &Manifest,
    vcs: &dyn Vcs,
    epic: &Epic,
) -> Result<Vocabulary, VocabularyLoadError> {
    let version = epic
        .release()
        .ok_or_else(|| VocabularyLoadError::NoRelease {
            epic: epic.id.to_string(),
        })?;
    let folder = ReleaseFolder::of(root, manifest, version);
    let design = folder.design_document();
    let unsigned = || VocabularyLoadError::Unsigned {
        version: version.to_owned(),
        path: design.display().to_string(),
    };
    if !document_signed(&folder, &design) {
        return Err(unsigned());
    }
    let text = std::fs::read_to_string(&design).map_err(|_| unsigned())?;
    let blocks = vocabulary_blocks(&text);
    let block = match blocks.as_slice() {
        [] => return Ok(Vocabulary::default()),
        [block] => block,
        many => {
            return Err(VocabularyLoadError::ManyBlocks {
                version: version.to_owned(),
                count: many.len(),
            });
        }
    };
    let parsed = vocabulary::parse(block).map_err(|error| VocabularyLoadError::Block {
        version: version.to_owned(),
        error,
    })?;
    let existing = existing_paths(block, &parsed);
    if !existing.is_empty() {
        let revision = integration_head(manifest, version)?;
        for (name, path) in existing {
            match vcs.path_at(root, &revision, &path) {
                Ok(true) => {}
                Ok(false) => {
                    return Err(VocabularyLoadError::MissingPath {
                        name,
                        path,
                        revision,
                    });
                }
                Err(error) => return Err(VocabularyLoadError::Vcs(error.to_string())),
            }
        }
    }
    Ok(parsed)
}

/// The design document of `version` as it reads now, whole; `None` when it cannot be read.
///
/// `plan file` hashes it into the plan record (I8, D17).
pub fn design_text(root: &Path, manifest: &Manifest, version: &str) -> Option<String> {
    let folder = ReleaseFolder::of(root, manifest, version);
    std::fs::read_to_string(folder.design_document()).ok()
}

/// The shape of each name of the design's vocabulary block that has a signature, by name: what an
/// interface ticket writes for it (D11). A design with no block, or a block that does not read,
/// gives none; [`release_vocabulary`] has refused those before a plan is filed.
pub fn vocabulary_shapes(design: &str) -> BTreeMap<Name, String> {
    let blocks = vocabulary_blocks(design);
    let Some(table) = blocks
        .first()
        .and_then(|block| block.parse::<toml::Table>().ok())
    else {
        return BTreeMap::new();
    };
    let Some(names) = table.get("names").and_then(toml::Value::as_table) else {
        return BTreeMap::new();
    };
    names
        .iter()
        .filter_map(|(name, entry)| {
            let shape = entry.get("signature")?.as_str()?;
            Some((Name::from(name.as_str()), shape.to_owned()))
        })
        .collect()
}

/// The text inside each ```` ```toml vocabulary ```` fence of `text`, in order. A fence never
/// closed runs to the end.
fn vocabulary_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut open: Option<Vec<&str>> = None;
    for line in text.lines() {
        match open.as_mut() {
            Some(lines) if line.trim_end() == CLOSING_FENCE => {
                blocks.push(lines.join("\n"));
                open = None;
            }
            Some(lines) => lines.push(line),
            None if line.trim_end() == VOCABULARY_FENCE => open = Some(Vec::new()),
            None => {}
        }
    }
    if let Some(lines) = open {
        blocks.push(lines.join("\n"));
    }
    blocks
}

/// Each name of `parsed` that has an `existing` path in `block`, with that path, by name.
///
/// The parsed vocabulary keeps the names alone, so the paths are read from the block again; the
/// block already parsed, so every entry is a table of known keys.
fn existing_paths(block: &str, parsed: &Vocabulary) -> Vec<(String, String)> {
    let Ok(table) = block.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(names) = table.get("names").and_then(toml::Value::as_table) else {
        return Vec::new();
    };
    names
        .iter()
        .filter(|(name, _)| parsed.existing.contains(&Name::from(name.as_str())))
        .filter_map(|(name, entry)| {
            let path = entry.get("existing")?.as_str()?;
            Some((name.clone(), path.to_owned()))
        })
        .collect()
}

/// The integration branch of `version`, which names the head the paths are looked for at.
fn integration_head(manifest: &Manifest, version: &str) -> Result<String, VocabularyLoadError> {
    let pattern: &str = manifest.branches.integration.as_ref();
    if pattern.contains("{task}") || pattern.contains("{epic}") {
        return Err(VocabularyLoadError::IntegrationPattern(pattern.to_owned()));
    }
    Ok(pattern.replace(VERSION_PLACEHOLDER, version))
}

#[cfg(test)]
mod tests;
