//! The in-memory releases: tags set by the test, files copied from a folder it fills.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use semver::Version;

use crate::ports::{PortError, Releases};

/// Releases a test arranges: the tags it lists, and a folder per tag holding that release's files.
#[derive(Debug, Default)]
pub struct FakeReleases {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    tags: Vec<String>,
    notes: Vec<(String, String)>,
    files: Option<PathBuf>,
    downloads: Vec<(String, Vec<String>)>,
    plugin: Option<Result<Version, String>>,
}

impl FakeReleases {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lists `tags` as the published releases.
    pub fn publish(&self, tags: &[&str]) {
        self.state().tags = tags.iter().map(|tag| (*tag).to_owned()).collect();
    }

    /// Gives the release at `tag` the notes `body`.
    pub fn write_notes(&self, tag: &str, body: &str) {
        let mut state = self.state();
        state.notes.retain(|(known, _)| known != tag);
        state.notes.push((tag.to_owned(), body.to_owned()));
    }

    /// Serves each release's files from `<files>/<tag>/<asset>`.
    pub fn serve_from(&self, files: &Path) {
        self.state().files = Some(files.to_owned());
    }

    /// Makes `version` the teams plugin's version on `main`; text that is not semver makes
    /// `plugin_version` fail as a `plugin.json` without one does.
    pub fn set_plugin_version(&self, version: &str) {
        self.state().plugin = Some(Version::parse(version).map_err(|error| error.to_string()));
    }

    /// Every download asked for: the tag and the assets, in order.
    pub fn downloads(&self) -> Vec<(String, Vec<String>)> {
        self.state().downloads.clone()
    }
}

impl Releases for FakeReleases {
    fn tags(&self) -> Result<Vec<String>, PortError> {
        Ok(self.state().tags.clone())
    }

    fn notes(&self, tag: &str) -> Result<Option<String>, PortError> {
        let state = self.state();
        if !state.tags.iter().any(|known| known == tag) {
            return Err(PortError::NotFound(format!("release {tag}")));
        }
        Ok(state
            .notes
            .iter()
            .find(|(known, _)| known == tag)
            .map(|(_, body)| body.clone())
            .filter(|body| !body.trim().is_empty()))
    }

    fn download(&self, tag: &str, assets: &[String], dir: &Path) -> Result<(), PortError> {
        let mut state = self.state();
        state.downloads.push((tag.to_owned(), assets.to_vec()));
        let Some(files) = state.files.clone() else {
            return Err(PortError::NotFound(format!("release {tag}")));
        };
        for asset in assets {
            let from = files.join(tag).join(asset);
            std::fs::copy(&from, dir.join(asset))
                .map_err(|_| PortError::NotFound(format!("{asset} in release {tag}")))?;
        }
        Ok(())
    }

    fn plugin_version(&self) -> Result<Version, PortError> {
        match self.state().plugin.clone() {
            Some(Ok(version)) => Ok(version),
            Some(Err(error)) => Err(PortError::Backend(format!(
                "plugin.json holds no semver version: {error}"
            ))),
            None => Err(PortError::NotFound("plugin.json on main".to_owned())),
        }
    }
}
