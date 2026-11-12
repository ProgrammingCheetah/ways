//! zaf's releases on GitHub, read and fetched through `gh release`.

use std::path::Path;

use serde::Deserialize;

use super::GhCommand;
use crate::ports::{PortError, Releases};

/// The most releases one listing reads; zaf publishes far fewer.
const LIST_LIMIT: &str = "200";

/// A release as `gh release list --json` gives it.
#[derive(Debug, Deserialize)]
struct ReleaseRow {
    #[serde(rename = "tagName")]
    tag: String,
    #[serde(rename = "isDraft", default)]
    draft: bool,
    #[serde(rename = "isPrerelease", default)]
    prerelease: bool,
}

/// The releases of the repository its [`GhCommand`] addresses.
#[derive(Debug, Clone)]
pub struct GithubReleases {
    gh: GhCommand,
}

impl From<GhCommand> for GithubReleases {
    fn from(gh: GhCommand) -> Self {
        Self { gh }
    }
}

impl Releases for GithubReleases {
    fn tags(&self) -> Result<Vec<String>, PortError> {
        let rows: Vec<ReleaseRow> = self.gh.json(&[
            "release",
            "list",
            "--json",
            "tagName,isDraft,isPrerelease",
            "--limit",
            LIST_LIMIT,
        ])?;
        Ok(rows
            .into_iter()
            .filter(|row| !row.draft && !row.prerelease)
            .map(|row| row.tag)
            .collect())
    }

    fn download(&self, tag: &str, assets: &[String], dir: &Path) -> Result<(), PortError> {
        let dir = dir.to_string_lossy();
        let mut args = vec!["release", "download", tag, "--dir", &dir, "--clobber"];
        for asset in assets {
            args.extend(["--pattern", asset.as_str()]);
        }
        self.gh.text(&args).map(|_| ())
    }
}
