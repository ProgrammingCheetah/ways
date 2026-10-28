//! The in-memory VCS.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::ports::{
    BaseMerge, BaseName, CommitId, DiffStat, FileStat, PortError, Publication, SliceName,
    SliceOpening, Vcs,
};

#[derive(Debug, Clone)]
struct Change {
    added: u32,
    removed: u32,
    hunks: String,
}

#[derive(Debug, Clone)]
struct Slice {
    base: BaseName,
    behind_base: bool,
    conflicts: Vec<String>,
    changes: BTreeMap<String, Change>,
    uncommitted: BTreeSet<String>,
    commits: u32,
    published: u32,
}

#[derive(Debug, Default)]
struct State {
    slices: BTreeMap<SliceName, Slice>,
    next_commit: u64,
}

/// A VCS held in memory. Arrange work with [`FakeVcs::edit`], [`FakeVcs::advance_base`]
/// and [`FakeVcs::conflict_on_merge`].
#[derive(Debug, Default)]
pub struct FakeVcs {
    state: Mutex<State>,
}

impl FakeVcs {
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

    /// Moves a base ahead, so every slice on it is behind.
    pub fn advance_base(&self, base: &BaseName) {
        let mut state = self.state();
        for slice in state
            .slices
            .values_mut()
            .filter(|slice| slice.base == *base)
        {
            slice.behind_base = true;
        }
    }

    /// Makes the next base merges into a slice stop on conflicts in these paths.
    pub fn conflict_on_merge(&self, slice: &SliceName, paths: &[&str]) -> Result<(), PortError> {
        let mut state = self.state();
        state.slice_mut(slice)?.conflicts = paths.iter().map(|path| (*path).to_owned()).collect();
        Ok(())
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
}

impl Vcs for FakeVcs {
    fn open_slice(&self, slice: &SliceName, base: &BaseName) -> Result<SliceOpening, PortError> {
        let mut state = self.state();
        if let Some(existing) = state.slices.get(slice) {
            if existing.base == *base {
                return Ok(SliceOpening::AlreadyOpen);
            }
            return Err(PortError::Conflict(format!(
                "slice {slice} is open on {}",
                existing.base
            )));
        }
        let opened = Slice {
            base: base.clone(),
            behind_base: false,
            conflicts: Vec::new(),
            changes: BTreeMap::new(),
            uncommitted: BTreeSet::new(),
            commits: 0,
            published: 0,
        };
        state.slices.insert(slice.clone(), opened);
        Ok(SliceOpening::Opened)
    }

    fn merge_base(&self, slice: &SliceName) -> Result<BaseMerge, PortError> {
        let mut state = self.state();
        let slice = state.slice_mut(slice)?;
        if !slice.conflicts.is_empty() {
            let conflicted = BaseMerge::Conflicted {
                paths: slice.conflicts.clone(),
            };
            return Ok(conflicted);
        }
        if slice.behind_base {
            slice.behind_base = false;
            return Ok(BaseMerge::Merged);
        }
        Ok(BaseMerge::UpToDate)
    }

    fn commit(&self, slice: &SliceName, message: &str) -> Result<CommitId, PortError> {
        let mut state = self.state();
        let slice = state.slice_mut(slice)?;
        if slice.uncommitted.is_empty() {
            return Err(PortError::Rejected(format!(
                "nothing to commit for `{message}`"
            )));
        }
        slice.uncommitted.clear();
        slice.commits += 1;
        state.next_commit += 1;
        let commit_id = CommitId::from(format!("fake-{}", state.next_commit));
        Ok(commit_id)
    }

    fn publish(&self, slice: &SliceName) -> Result<Publication, PortError> {
        let mut state = self.state();
        let slice = state.slice_mut(slice)?;
        if slice.published == slice.commits {
            return Ok(Publication::AlreadyPublished);
        }
        slice.published = slice.commits;
        Ok(Publication::Published)
    }

    fn is_clean(&self, slice: &SliceName) -> Result<bool, PortError> {
        let mut state = self.state();
        Ok(state.slice_mut(slice)?.uncommitted.is_empty())
    }

    fn diff_stat(&self, slice: &SliceName) -> Result<DiffStat, PortError> {
        let mut state = self.state();
        let files = state
            .slice_mut(slice)?
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

    fn diff_hunks(&self, slice: &SliceName, path: &str) -> Result<String, PortError> {
        let mut state = self.state();
        state
            .slice_mut(slice)?
            .changes
            .get(path)
            .map(|change| change.hunks.clone())
            .ok_or_else(|| PortError::NotFound(format!("no change to {path}")))
    }

    fn drop_slice(&self, slice: &SliceName) -> Result<(), PortError> {
        self.state().slices.remove(slice);
        Ok(())
    }
}
