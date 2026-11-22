//! The files a pull request changes, as the forge lists them.

use super::RepoPath;

/// Every file a pull request changes, in the order the forge lists them. A renamed file is
/// listed under both names, the old one first, so the last path is always a name the head
/// holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedFiles {
    /// The paths, a rename under its old and its new name.
    pub paths: Vec<RepoPath>,
    /// Whether the list may be missing files: the forge capped it, or a page failed to read
    /// fully.
    pub cut_short: bool,
}

impl ChangedFiles {
    /// A complete list of `paths`.
    #[must_use]
    pub fn complete(paths: Vec<RepoPath>) -> Self {
        Self {
            paths,
            cut_short: false,
        }
    }

    /// Lists a file as the forge names it, and `previous` first when the file is a renamed one,
    /// so the rename stands under both names.
    pub fn push(&mut self, name: String, previous: Option<String>) {
        if let Some(old) = previous.filter(|old| !old.is_empty() && *old != name) {
            self.paths.push(RepoPath::from(old));
        }
        self.paths.push(RepoPath::from(name));
    }

    /// A complete list of `paths`, for tests.
    #[cfg(test)]
    #[must_use]
    pub fn of(paths: &[&str]) -> Self {
        Self::complete(paths.iter().map(|path| RepoPath::from(*path)).collect())
    }

    /// Whether no file is listed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// The last path listed; none when no file is.
    #[must_use]
    pub fn last(&self) -> Option<&RepoPath> {
        self.paths.last()
    }
}
