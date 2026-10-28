//! What can be wrong with a manifest.

use std::path::PathBuf;

/// A manifest that could not be read, parsed or validated.
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
    #[error("ZAF.toml: {0}")]
    Syntax(#[from] toml::de::Error),
    /// A key the declared kind needs, left out.
    #[error("ZAF.toml: [{section}] needs `{key}` when kind is `{kind}`")]
    MissingKey {
        /// The section.
        section: &'static str,
        /// The key.
        key: &'static str,
        /// The declared kind.
        kind: String,
    },
    /// A machine path where the manifest allows only paths relative to the project.
    #[error("ZAF.toml: [{section}] `{key}` must be relative to the project, got {path}")]
    AbsolutePath {
        /// The section.
        section: &'static str,
        /// The key.
        key: &'static str,
        /// The path given.
        path: PathBuf,
    },
    /// A value outside what its key allows.
    #[error("ZAF.toml: [{section}] `{key}` {reason}")]
    Invalid {
        /// The section.
        section: &'static str,
        /// The key.
        key: &'static str,
        /// What the key allows.
        reason: String,
    },
}
