//! What can be wrong with a manifest.

use std::path::{Path, PathBuf};

/// A manifest that could not be read, parsed or validated.
///
/// Every message names the file it came from, or the files that were combined, so no
/// text says `ZAF.toml` unless that file was read.
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    /// The file could not be read.
    #[error("cannot read {path}: {source}")]
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// Not valid TOML for the schema: a syntax error, a missing required
    /// section or key, a wrong type, or an unknown key, which the message names.
    #[error("{0}")]
    Syntax(#[from] toml::de::Error),
    /// A key the declared kind needs, left out.
    #[error("[{section}] needs `{key}` when kind is `{kind}`")]
    MissingKey {
        /// The section.
        section: &'static str,
        /// The key.
        key: &'static str,
        /// The declared kind.
        kind: String,
    },
    /// A machine path where the manifest allows only paths relative to the project.
    #[error("[{section}] `{key}` must be relative to the project, got {path}")]
    AbsolutePath {
        /// The section.
        section: &'static str,
        /// The key.
        key: &'static str,
        /// The path given.
        path: PathBuf,
    },
    /// A value outside what its key allows.
    #[error("[{section}] `{key}` {reason}")]
    Invalid {
        /// The section.
        section: &'static str,
        /// The key.
        key: &'static str,
        /// What the key allows.
        reason: String,
    },
    /// An `[env]` value uses `{version}` and no version was given to resolve it with.
    #[error("[env] `{name}` uses `{{version}}`, and no version was given")]
    EnvNeedsVersion {
        /// The variable.
        name: String,
    },
    /// A root `ZAF.toml` beside `.zaf.toml` or `.zaf/config.toml`: the retired name and the
    /// new ones never mix.
    #[error(
        "{} sits beside {}: ZAF.toml is retired and the new file is the one to keep, \
         so move what ZAF.toml adds into it, then delete {}",
        .legacy.display(),
        paths(.beside),
        .legacy.display()
    )]
    LegacyBesideNew {
        /// The retired `ZAF.toml`.
        legacy: PathBuf,
        /// The new manifest files found beside it.
        beside: Vec<PathBuf>,
    },
    /// A manifest setting in `.zaf/local.toml` other than the machine facts it may hold.
    ///
    /// The file is git-ignored, so it may never loosen what the project commits.
    ///
    /// The message reads: `.zaf/local.toml may set only [spawn] slots, [gate] slot and its own tables; it sets <key>`
    #[error(
        ".zaf/local.toml may set only [spawn] slots, [gate] slot and its own tables; it sets {key}"
    )]
    LocalSetting {
        /// The setting, as `[table]` or `[table] key`.
        key: String,
    },
    /// Another error, with the file or files it was found in.
    #[error("{file}: {source}")]
    In {
        /// The file, or the files combined, joined with ` + `.
        file: String,
        /// What is wrong.
        #[source]
        source: Box<ManifestError>,
    },
}

impl ManifestError {
    /// This error placed in `files`, the manifest files it came from.
    pub(super) fn in_files(self, files: &[&Path]) -> Self {
        let file = files
            .iter()
            .map(|file| file.display().to_string())
            .collect::<Vec<_>>()
            .join(" + ");
        Self::In {
            file,
            source: Box::new(self),
        }
    }
}

fn paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" and ")
}
