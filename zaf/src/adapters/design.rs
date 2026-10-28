//! The directory design source: Markdown pages beside an index file.
//!
//! Serves both the `repo-dir` kind (a folder in the code repository) and the
//! `git-sibling` kind (a folder in a repository cloned beside it): once the
//! folder is located, reading it is the same. It never shells out; keeping a
//! sibling clone current is the composition root's business.
//!
//! A page is a file named `<id>-<slug>.md`. The index is a Markdown file with
//! one `- <id> <status> — <title>` line per page; other lines are ignored.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::domain::{PageId, PageStatus};
use crate::ports::{Design, Page, PortError};

/// The index file's name when the manifest names none.
pub const DEFAULT_INDEX_FILE: &str = "index.md";

/// The page files' extension.
const PAGE_EXTENSION: &str = "md";

/// Where a page id ends and its slug begins in a file name.
const ID_SEPARATOR: char = '-';

/// What the index says about one page.
struct IndexEntry {
    id: String,
    status: String,
    title: String,
}

impl IndexEntry {
    /// Reads `- <id> <status> — <title>`; any other line is not an entry.
    fn parse(line: &str) -> Option<Self> {
        let entry = line.trim().strip_prefix("- ")?;
        let (id, rest) = entry.trim_start().split_once(char::is_whitespace)?;
        let (status, title) = rest.trim_start().split_once(char::is_whitespace)?;
        let title = title.trim_start().strip_prefix('—')?.trim();
        (!id.is_empty() && !status.is_empty()).then(|| Self {
            id: id.to_owned(),
            status: status.to_owned(),
            title: title.to_owned(),
        })
    }
}

/// A page that matched a search, with the lines that matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    /// The page.
    pub id: PageId,
    /// The matching lines, in page order, trimmed.
    pub lines: Vec<String>,
}

/// A design source that is a directory of pages and an index file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirDesign {
    dir: PathBuf,
    index: PathBuf,
}

impl DirDesign {
    /// A source over `dir`, with the index at [`DEFAULT_INDEX_FILE`] inside it.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            index: PathBuf::from(DEFAULT_INDEX_FILE),
        }
    }

    /// Names the index file, relative to the pages' directory.
    #[must_use]
    pub fn with_index(mut self, index: impl Into<PathBuf>) -> Self {
        self.index = index.into();
        self
    }

    /// Every page whose text holds the query, ignoring case, with its matching lines.
    ///
    /// [`Design::search`] is this without the lines.
    pub fn search_lines(&self, query: &str) -> Result<Vec<SearchHit>, PortError> {
        if query.trim().is_empty() {
            return Err(PortError::Rejected("an empty search query".to_owned()));
        }
        let needle = query.to_lowercase();
        let mut hits = Vec::new();
        for (id, path) in self.page_files()? {
            let text = self.read(&path)?;
            let lines: Vec<String> = text
                .lines()
                .filter(|line| line.to_lowercase().contains(&needle))
                .map(|line| line.trim().to_owned())
                .collect();
            if !lines.is_empty() {
                hits.push(SearchHit { id, lines });
            }
        }
        Ok(hits)
    }

    /// The page files by id, in id order; the index file is not a page.
    fn page_files(&self) -> Result<BTreeMap<PageId, PathBuf>, PortError> {
        let index_path = self.dir.join(&self.index);
        let entries = fs::read_dir(&self.dir).map_err(|error| self.io_error(&self.dir, &error))?;
        let mut files = BTreeMap::new();
        for entry in entries {
            let path = entry
                .map_err(|error| self.io_error(&self.dir, &error))?
                .path();
            let is_page = path.extension().is_some_and(|ext| ext == PAGE_EXTENSION)
                && path != index_path
                && path.is_file();
            let id = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.split_once(ID_SEPARATOR))
                .and_then(|(id, _slug)| PageId::from_str(id).ok());
            if let (true, Some(id)) = (is_page, id) {
                files.insert(id, path);
            }
        }
        Ok(files)
    }

    /// The index entry for a page, or `NotFound`.
    fn index_entry(&self, id: &PageId) -> Result<IndexEntry, PortError> {
        let index_path = self.dir.join(&self.index);
        let text = self.read(&index_path)?;
        text.lines()
            .filter_map(IndexEntry::parse)
            .find(|entry| entry.id == id.as_ref())
            .ok_or_else(|| PortError::NotFound(format!("page {id} in the index")))
    }

    fn read(&self, path: &Path) -> Result<String, PortError> {
        fs::read_to_string(path).map_err(|error| self.io_error(path, &error))
    }

    /// A missing file or folder is the source being absent; anything else is a failed backend.
    fn io_error(&self, path: &Path, error: &io::Error) -> PortError {
        let message = format!("design source {}: {error}", path.display());
        if error.kind() == io::ErrorKind::NotFound {
            PortError::Unavailable(message)
        } else {
            PortError::Backend(message)
        }
    }
}

fn parse_status(id: &PageId, status: &str) -> Result<PageStatus, PortError> {
    PageStatus::from_str(status)
        .map_err(|_| PortError::Backend(format!("page {id} has unknown status `{status}`")))
}

impl Design for DirDesign {
    fn page(&self, id: &PageId) -> Result<Page, PortError> {
        let files = self.page_files()?;
        let path = files
            .get(id)
            .ok_or_else(|| PortError::NotFound(format!("page {id}")))?;
        let entry = self.index_entry(id)?;
        Ok(Page {
            id: id.clone(),
            title: entry.title,
            status: parse_status(id, &entry.status)?,
            text: self.read(path)?,
        })
    }

    fn search(&self, query: &str) -> Result<Vec<PageId>, PortError> {
        let hits = self.search_lines(query)?;
        Ok(hits.into_iter().map(|hit| hit.id).collect())
    }

    fn status(&self, id: &PageId) -> Result<PageStatus, PortError> {
        let entry = self.index_entry(id)?;
        parse_status(id, &entry.status)
    }
}

#[cfg(test)]
mod tests;
