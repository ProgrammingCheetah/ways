//! The CI filter on Forgejo: the workflows of `.forgejo/workflows`, read through the contents API
//! at the base branch's ref (`GET contents/{path}?ref={base}`), never at a pull request's head.

use serde::Deserialize;

use super::api::{ForgejoApi, query_value};
use super::rows::BranchHeadRow;
use crate::adapters::ci_filter::{is_plain_name, is_workflow_name, read_workflow, unreadable};
use crate::domain::WorkflowFilter;
use crate::ports::{BranchName, CiFilter, PortError};

/// The folder Forgejo Actions reads workflows from.
const WORKFLOW_DIR: &str = ".forgejo/workflows";

/// The workflows of one Forgejo repository.
#[derive(Debug, Clone)]
pub struct ForgejoCiFilter {
    api: ForgejoApi,
}

impl From<ForgejoApi> for ForgejoCiFilter {
    fn from(api: ForgejoApi) -> Self {
        Self { api }
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

/// One file as `GET contents/{path}` prints it: its bytes in base64.
#[derive(Debug, Deserialize)]
struct FileRow {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    encoding: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

impl ForgejoCiFilter {
    /// The text of the file at `path` on `base`; none when Forgejo refuses it, it is not a plain
    /// file, or its text is not readable.
    fn text(&self, path: &str, at: &str) -> Option<String> {
        let row: FileRow = self.api.get(&format!("contents/{path}?ref={at}")).ok()?;
        if row.kind != "file" || row.encoding.as_deref() != Some("base64") {
            return None;
        }
        String::from_utf8(decode_base64(&row.content?)?).ok()
    }

    /// Whether Forgejo has the branch `base`.
    fn has_branch(&self, base: &BranchName) -> Result<bool, PortError> {
        match self
            .api
            .get::<BranchHeadRow>(&format!("branches/{}", base.as_str()))
        {
            Ok(_) => Ok(true),
            Err(PortError::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }
}

impl CiFilter for ForgejoCiFilter {
    /// The folder is listed at the base's ref and each `.yml` and `.yaml` file read at the same
    /// ref. A file that cannot be fetched is unreadable. A folder Forgejo answers 404 for is no
    /// workflows when the base exists, and an error when it does not: a missing base must not
    /// read as a repository with no CI.
    fn workflows(&self, base: &BranchName) -> Result<Vec<WorkflowFilter>, PortError> {
        if !base.is_usable() {
            return Err(PortError::Rejected(format!(
                "`{base}` is not a branch to read workflows at"
            )));
        }
        let at = query_value(base.as_str());
        let entries: Vec<EntryRow> =
            match self.api.get(&format!("contents/{WORKFLOW_DIR}?ref={at}")) {
                Ok(entries) => entries,
                Err(PortError::NotFound(message)) => {
                    return if self.has_branch(base)? {
                        Ok(Vec::new())
                    } else {
                        Err(PortError::Backend(message))
                    };
                }
                Err(error) => return Err(error),
            };
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
                match self.text(&path, &at) {
                    Some(text) => read_workflow(&path, &text),
                    None => unreadable(&path),
                }
            })
            .collect())
    }
}

/// The bytes of standard base64 text, line breaks allowed; none when it is not valid.
pub(super) fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(text.len() / 4 * 3);
    let mut word = 0u32;
    let mut bits = 0u32;
    let mut padding = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            b'\n' | b'\r' => continue,
            _ => return None,
        };
        if padding > 0 {
            return None;
        }
        word = word << 6 | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            bytes.push((word >> bits) as u8);
            word &= (1 << bits) - 1;
        }
    }
    (padding <= 2).then_some(bytes)
}
