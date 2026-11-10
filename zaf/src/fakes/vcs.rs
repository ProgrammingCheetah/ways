//! The in-memory VCS.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::ports::{BaseName, CommitId, DiffStat, FileStat, PortError, SliceName, Vcs};

#[derive(Debug, Clone)]
struct Change {
    added: u32,
    removed: u32,
    hunks: String,
}

#[derive(Debug, Clone)]
struct Slice {
    base: BaseName,
    workspace: Option<PathBuf>,
    changes: BTreeMap<String, Change>,
    uncommitted: BTreeSet<String>,
    /// The last commit made; a new slice stands on its base.
    latest: CommitId,
}

#[derive(Debug, Default)]
struct State {
    slices: BTreeMap<SliceName, Slice>,
    next_commit: u64,
}

/// A VCS held in memory. Arrange a slice with [`FakeVcs::open`], [`FakeVcs::place`],
/// [`FakeVcs::edit`] and [`FakeVcs::commit`]: the port reads what they leave.
#[derive(Debug, Default)]
pub struct FakeVcs {
    state: Mutex<State>,
}

impl FakeVcs {
    /// Opens a slice on a base; an open slice stays as it is.
    pub fn open(&self, slice: &SliceName, base: &BaseName) {
        let opened = Slice {
            base: base.clone(),
            workspace: None,
            changes: BTreeMap::new(),
            uncommitted: BTreeSet::new(),
            latest: CommitId::from(format!("fake-base-{base}")),
        };
        self.state().slices.entry(slice.clone()).or_insert(opened);
    }

    /// Records uncommitted work on a file of an open slice.
    pub fn edit(
        &self,
        slice: &SliceName,
        path: &str,
        added: u32,
        removed: u32,
        hunks: &str,
    ) -> Result<(), PortError> {
        let mut state = self.state();
        let slice = state.slice_mut(slice)?;
        let change = Change {
            added,
            removed,
            hunks: hunks.to_owned(),
        };
        slice.changes.insert(path.to_owned(), change);
        slice.uncommitted.insert(path.to_owned());
        Ok(())
    }

    /// Records where an open slice's workspace is, the way a slice record would name it.
    pub fn place(&self, slice: &SliceName, workspace: &Path) -> Result<(), PortError> {
        self.state().slice_mut(slice)?.workspace = Some(workspace.to_owned());
        Ok(())
    }

    /// Commits the slice's uncommitted work; nothing to commit is [`PortError::Rejected`].
    pub fn commit(&self, slice: &SliceName, message: &str) -> Result<CommitId, PortError> {
        let mut state = self.state();
        let next_commit = state.next_commit + 1;
        let slice = state.slice_mut(slice)?;
        if slice.uncommitted.is_empty() {
            return Err(PortError::Rejected(format!(
                "nothing to commit for `{message}`"
            )));
        }
        let commit_id = CommitId::from(format!("fake-{next_commit}"));
        slice.uncommitted.clear();
        slice.latest = commit_id.clone();
        state.next_commit = next_commit;
        Ok(commit_id)
    }

    /// Whether a slice exists.
    pub fn has_slice(&self, slice: &SliceName) -> bool {
        self.state().slices.contains_key(slice)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl State {
    fn slice_mut(&mut self, name: &SliceName) -> Result<&mut Slice, PortError> {
        self.slices
            .get_mut(name)
            .ok_or_else(|| PortError::NotFound(format!("slice {name}")))
    }

    /// The slice placed at a workspace on a base; any other workspace or base is not found.
    fn slice_at(&self, workspace: &Path, base: &BaseName) -> Result<&Slice, PortError> {
        self.slices
            .values()
            .find(|slice| slice.workspace.as_deref() == Some(workspace) && slice.base == *base)
            .ok_or_else(|| {
                PortError::NotFound(format!("workspace {} on {base}", workspace.display()))
            })
    }
}

impl Vcs for FakeVcs {
    fn latest_commit(&self, workspace: &Path) -> Result<CommitId, PortError> {
        self.state()
            .slices
            .values()
            .find(|slice| slice.workspace.as_deref() == Some(workspace))
            .map(|slice| slice.latest.clone())
            .ok_or_else(|| PortError::NotFound(format!("workspace {}", workspace.display())))
    }

    fn diff_stat(&self, workspace: &Path, base: &BaseName) -> Result<DiffStat, PortError> {
        let state = self.state();
        let files = state
            .slice_at(workspace, base)?
            .changes
            .iter()
            .map(|(path, change)| FileStat {
                path: path.clone(),
                added: change.added,
                removed: change.removed,
            })
            .collect();
        Ok(DiffStat { files })
    }

    fn diff_hunks(
        &self,
        workspace: &Path,
        base: &BaseName,
        path: &str,
    ) -> Result<String, PortError> {
        let state = self.state();
        state
            .slice_at(workspace, base)?
            .changes
            .get(path)
            .map(|change| change.hunks.clone())
            .ok_or_else(|| PortError::NotFound(format!("no change to {path}")))
    }
}
