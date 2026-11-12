//! Releases: where zaf's own published builds are read from, for `zaf update`.

use std::path::Path;

use super::PortError;

/// A place zaf's releases are published: their tags, and each release's files.
pub trait Releases {
    /// The tag of every published release, drafts and pre-releases left out, in no order.
    fn tags(&self) -> Result<Vec<String>, PortError>;

    /// Downloads the named files of the release at `tag` into `dir`, replacing any there.
    /// A release or a file that is not there is [`PortError::NotFound`].
    fn download(&self, tag: &str, assets: &[String], dir: &Path) -> Result<(), PortError>;
}
