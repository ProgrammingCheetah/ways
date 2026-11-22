//! The CI filter on GitHub: the workflows of `.github/workflows`, read through the contents API
//! at the base branch's ref (`gh api repos/{repo}/contents/{path} -f ref={base}`), never at a
//! pull request's head.

use serde::Deserialize;

use super::GhCommand;
use crate::adapters::ci_filter::{is_plain_name, is_workflow_name, read_workflow, unreadable};
use crate::domain::WorkflowFilter;
use crate::ports::{BranchName, CiFilter, PortError};

/// The folder GitHub Actions reads workflows from.
const WORKFLOW_DIR: &str = ".github/workflows";

/// The header that makes the contents API answer a file's own text.
const RAW: &str = "Accept: application/vnd.github.raw+json";

/// The workflows of one GitHub repository.
#[derive(Debug, Clone)]
pub struct GithubCiFilter {
    gh: GhCommand,
}

impl From<GhCommand> for GithubCiFilter {
    fn from(gh: GhCommand) -> Self {
        Self { gh }
    }
}

/// One entry of a folder as `GET contents/{dir}` lists it.
#[derive(Debug, Deserialize)]
struct EntryRow {
    #[serde(default)]
    name: String,
    #[serde(default, rename = "type")]
    kind: String,
}

/// Whether a failed call is GitHub's 404.
fn is_missing(error: &PortError) -> bool {
    matches!(error, PortError::NotFound(_))
        || matches!(error, PortError::Backend(message) if message.contains("HTTP 404"))
}

impl GithubCiFilter {
    /// The text of the file at `path` on `base`; none when GitHub refuses it.
    fn text(&self, path: &str, base: &str) -> Option<String> {
        self.gh
            .api_text(
                &format!("contents/{path}"),
                &["--method", "GET", "-H", RAW, "-f", &format!("ref={base}")],
            )
            .ok()
    }
}

impl CiFilter for GithubCiFilter {
    /// The folder is listed at the base's ref and each `.yml` and `.yaml` file read at the same
    /// ref. A file that cannot be fetched is unreadable. A folder GitHub answers 404 for is no
    /// workflows when the base exists, and an error when it does not: a missing base must not
    /// read as a repository with no CI.
    fn workflows(&self, base: &BranchName) -> Result<Vec<WorkflowFilter>, PortError> {
        if !base.is_usable() {
            return Err(PortError::Rejected(format!(
                "`{base}` is not a branch to read workflows at"
            )));
        }
        let listed = self.gh.api_text(
            &format!("contents/{WORKFLOW_DIR}"),
            &["--method", "GET", "-f", &format!("ref={base}")],
        );
        let text = match listed {
            Ok(text) => text,
            Err(error) if is_missing(&error) => {
                return match self.gh.api_text(&format!("branches/{base}"), &[]) {
                    Ok(_) => Ok(Vec::new()),
                    Err(_) => Err(error.into_backend()),
                };
            }
            Err(error) => return Err(error),
        };
        let entries: Vec<EntryRow> = serde_json::from_str(&text).map_err(|error| {
            PortError::Backend(format!(
                "gh api contents/{WORKFLOW_DIR}: not a list: {error}"
            ))
        })?;
        let mut names: Vec<(String, String)> = entries
            .into_iter()
            .filter(|entry| entry.kind != "dir" && is_workflow_name(&entry.name))
            .map(|entry| (entry.name, entry.kind))
            .collect();
        names.sort();
        Ok(names
            .into_iter()
            .map(|(name, kind)| {
                let path = format!("{WORKFLOW_DIR}/{name}");
                if kind != "file" || !is_plain_name(&name) {
                    return unreadable(&path);
                }
                match self.text(&path, base.as_str()) {
                    Some(text) => read_workflow(&path, &text),
                    None => unreadable(&path),
                }
            })
            .collect())
    }
}
