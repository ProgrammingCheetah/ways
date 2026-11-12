//! The in-memory releases: tags set by the test, files copied from a folder it fills.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::ports::{PortError, Releases};

/// Releases a test arranges: the tags it lists, and a folder per tag holding that release's files.
#[derive(Debug, Default)]
pub struct FakeReleases {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    tags: Vec<String>,
    files: Option<PathBuf>,
    downloads: Vec<(String, Vec<String>)>,
}

impl FakeReleases {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Lists `tags` as the published releases.
    pub fn publish(&self, tags: &[&str]) {
        self.state().tags = tags.iter().map(|tag| (*tag).to_owned()).collect();
    }

    /// Serves each release's files from `<files>/<tag>/<asset>`.
    pub fn serve_from(&self, files: &Path) {
        self.state().files = Some(files.to_owned());
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
}
