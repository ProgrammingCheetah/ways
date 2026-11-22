//! The in-memory VCS.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::ports::{BaseName, CommitId, DiffStat, FileStat, PortError, SliceName, TreeId, Vcs};

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
    /// A merge left open, as `git merge --no-commit` leaves one: no commit yet, and the slice
    /// holds no head until it is finished.
    merging: bool,
}

/// One commit of the in-memory history: where it stands and what it holds.
#[derive(Debug, Clone)]
struct Node {
    parent: Option<CommitId>,
    /// Two commits with the same tree hold the same files.
    tree: u64,
}

#[derive(Debug, Default)]
struct State {
    slices: BTreeMap<SliceName, Slice>,
    commits: BTreeMap<CommitId, Node>,
    next_commit: u64,
    next_tree: u64,
    tags: BTreeSet<String>,
    remote: bool,
    /// The repository's root, once [`FakeVcs::set_root`] names it; until then any directory is
    /// the repository.
    root: Option<PathBuf>,
}

/// A VCS held in memory. Arrange a slice with [`FakeVcs::open`], [`FakeVcs::place`],
/// [`FakeVcs::edit`] and [`FakeVcs::commit`]: the port reads what they leave. A commit made beside
/// a slice ([`FakeVcs::empty_commit`], [`FakeVcs::changed_commit`], [`FakeVcs::sibling_commit`])
/// is what a pull request's head can be.
#[derive(Debug, Default)]
pub struct FakeVcs {
    state: Mutex<State>,
}

impl FakeVcs {
    /// Opens a slice on a base; an open slice stays as it is.
    pub fn open(&self, slice: &SliceName, base: &BaseName) {
        let mut state = self.state();
        if state.slices.contains_key(slice) {
            return;
        }
        let latest = CommitId::from(format!("fake-base-{base}"));
        if !state.commits.contains_key(&latest) {
            let tree = state.new_tree();
            let node = Node { parent: None, tree };
            state.commits.insert(latest.clone(), node);
        }
        let opened = Slice {
            base: base.clone(),
            workspace: None,
            changes: BTreeMap::new(),
            uncommitted: BTreeSet::new(),
            latest,
            merging: false,
        };
        state.slices.insert(slice.clone(), opened);
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
        let parent = slice.latest.clone();
        slice.uncommitted.clear();
        slice.latest = commit_id.clone();
        state.next_commit = next_commit;
        let tree = state.new_tree();
        let node = Node {
            parent: Some(parent),
            tree,
        };
        state.commits.insert(commit_id.clone(), node);
        Ok(commit_id)
    }

    /// A commit on `parent` that changes nothing: its tree is the parent's. The slice's latest
    /// commit stays where it was. An unknown parent is [`PortError::NotFound`].
    pub fn empty_commit(&self, parent: &CommitId) -> Result<CommitId, PortError> {
        let mut state = self.state();
        let tree = state.node(parent)?.tree;
        Ok(state.add_commit(Some(parent.clone()), tree))
    }

    /// Opens a merge in the slice: the slice holds no head until [`FakeVcs::finish_merge`].
    pub fn open_merge(&self, slice: &SliceName) -> Result<(), PortError> {
        self.state().slice_mut(slice)?.merging = true;
        Ok(())
    }

    /// Commits the slice's open merge: a commit of a tree of its own on top of the latest one,
    /// which is the slice's latest commit then.
    pub fn finish_merge(&self, slice: &SliceName) -> Result<CommitId, PortError> {
        let mut state = self.state();
        let parent = state.slice_mut(slice)?.latest.clone();
        let tree = state.new_tree();
        let merged = state.add_commit(Some(parent), tree);
        let slice = state.slice_mut(slice)?;
        slice.latest = merged.clone();
        slice.merging = false;
        Ok(merged)
    }

    /// A commit on `parent` that changes something: a tree of its own. The slice's latest commit
    /// stays where it was. An unknown parent is [`PortError::NotFound`].
    pub fn changed_commit(&self, parent: &CommitId) -> Result<CommitId, PortError> {
        let mut state = self.state();
        state.node(parent)?;
        let tree = state.new_tree();
        Ok(state.add_commit(Some(parent.clone()), tree))
    }

    /// A commit beside `commit`: the same parent and the same tree, so not a descendant of it.
    /// An unknown commit is [`PortError::NotFound`].
    pub fn sibling_commit(&self, commit: &CommitId) -> Result<CommitId, PortError> {
        let mut state = self.state();
        let node = state.node(commit)?.clone();
        Ok(state.add_commit(node.parent, node.tree))
    }

    /// Records a tag of the repository.
    pub fn tag(&self, tag: &str) {
        self.state().tags.insert(tag.to_owned());
    }

    /// Records that the repository has a remote.
    pub fn add_remote(&self) {
        self.state().remote = true;
    }

    /// Records that the repository has no remote.
    pub fn remove_remote(&self) {
        self.state().remote = false;
    }

    /// Names the repository's root: the reads that take a root then answer
    /// [`PortError::NotFound`] for any other directory, as the real adapters do for a directory
    /// that is no repository.
    pub fn set_root(&self, root: &Path) {
        self.state().root = Some(root.to_owned());
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
    fn new_tree(&mut self) -> u64 {
        self.next_tree += 1;
        self.next_tree
    }

    fn add_commit(&mut self, parent: Option<CommitId>, tree: u64) -> CommitId {
        self.next_commit += 1;
        let commit_id = CommitId::from(format!("fake-{}", self.next_commit));
        let node = Node { parent, tree };
        self.commits.insert(commit_id.clone(), node);
        commit_id
    }

    /// Any root when none was named, else only the named one.
    fn at_root(&self, root: &Path) -> Result<(), PortError> {
        match &self.root {
            Some(named) if named != root => Err(PortError::NotFound(format!(
                "repository {}",
                root.display()
            ))),
            _ => Ok(()),
        }
    }

    fn node(&self, commit: &CommitId) -> Result<&Node, PortError> {
        self.commits
            .get(commit)
            .ok_or_else(|| PortError::NotFound(format!("commit {commit}")))
    }

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

    fn holds(&self, workspace: &Path, head: &CommitId) -> Result<bool, PortError> {
        let latest = self.latest_commit(workspace)?;
        let state = self.state();
        let merging = state
            .slices
            .values()
            .any(|slice| slice.workspace.as_deref() == Some(workspace) && slice.merging);
        if merging {
            return Ok(false);
        }
        let (Some(latest_node), Some(head_node)) =
            (state.commits.get(&latest), state.commits.get(head))
        else {
            // A head the repository does not hold is no head to merge.
            return Ok(false);
        };
        let mut at = Some(head.clone());
        let mut descends = false;
        while let Some(commit) = at {
            if commit == latest {
                descends = true;
                break;
            }
            at = state
                .commits
                .get(&commit)
                .and_then(|node| node.parent.clone());
        }
        Ok(descends && latest_node.tree == head_node.tree)
    }

    fn tree_of(&self, root: &Path, commit: &CommitId) -> Result<TreeId, PortError> {
        let state = self.state();
        state.at_root(root)?;
        let tree = state.node(commit)?.tree;
        Ok(TreeId::from(format!("fake-tree-{tree}")))
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

    fn has_tag(&self, root: &Path, tag: &str) -> Result<bool, PortError> {
        let state = self.state();
        state.at_root(root)?;
        Ok(state.tags.contains(tag))
    }

    fn has_remote(&self, root: &Path) -> Result<bool, PortError> {
        let state = self.state();
        state.at_root(root)?;
        Ok(state.remote)
    }

    fn tags(&self, root: &Path) -> Result<Vec<String>, PortError> {
        let state = self.state();
        state.at_root(root)?;
        Ok(state.tags.iter().cloned().collect())
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
