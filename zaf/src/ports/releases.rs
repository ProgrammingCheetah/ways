//! Releases: where zaf's own published builds are read from, for `zaf update`.

use std::path::Path;

use semver::Version;

use super::PortError;

/// A place zaf's releases are published: their tags, and each release's files.
pub trait Releases {
    /// The tag of every published release, drafts and pre-releases left out, in no order.
    fn tags(&self) -> Result<Vec<String>, PortError>;

    /// The notes of the release at `tag`: its body, `None` when it has none (or only blank
    /// text). A release that is not there is [`PortError::NotFound`].
    fn notes(&self, tag: &str) -> Result<Option<String>, PortError>;

    /// Downloads the named files of the release at `tag` into `dir`, replacing any there.
    /// A release or a file that is not there is [`PortError::NotFound`].
    fn download(&self, tag: &str, assets: &[String], dir: &Path) -> Result<(), PortError>;

    /// The version the teams plugin declares on `main`, read from its `plugin.json` there; the
    /// marketplace is not touched. A file that is not JSON or holds no semver `version` is
    /// [`PortError::Backend`] naming `plugin.json`.
    fn plugin_version(&self) -> Result<Version, PortError>;
}
