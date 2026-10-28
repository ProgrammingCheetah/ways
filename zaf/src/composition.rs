//! The composition root: the one place that reads the manifest and will pick the adapters.
//!
//! Nothing else knows which backend is live. Adapters land in [`crate::adapters`]
//! one by one; until then the root only loads and validates the manifest.

use std::path::Path;

use crate::manifest::{Manifest, ManifestError};

/// The manifest's file name at the project root.
pub const MANIFEST_FILE: &str = "ZAF.toml";

/// A project as zaf sees it: its manifest, and later the adapters it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composition {
    manifest: Manifest,
}

impl Composition {
    /// Loads `ZAF.toml` from the project root.
    pub fn load(project_root: &Path) -> Result<Self, ManifestError> {
        let manifest = Manifest::load(&project_root.join(MANIFEST_FILE))?;
        Ok(Self { manifest })
    }

    /// The project's manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
}

#[cfg(test)]
mod tests;
