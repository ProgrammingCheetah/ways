//! The git adapter for [`Vcs`]: what zaf reads of a plain git repository.
//!
//! The repository reads (a remote, the tags) run `git -C <root>` under a fixed environment: no
//! pager, no colour, no prompt, and neither the system nor the user's git config, so nothing
//! the user set changes the output. The repository's own config stays: the remotes live in it.
//!
//! The slice reads (the latest commit, the change against the base) work in a workspace, which
//! may be a linked worktree. The change is read from a temporary index built with `git add -A`,
//! so uncommitted and untracked files show and the workspace's own index is never touched; the
//! `.zaf/` folder stays out, and so does what `.gitignore` says. `holds` reads only what the
//! repository already has and fetches nothing, and so does `tree_of`: a commit's tree id, as
//! `git rev-parse <commit>^{tree}` gives it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ports::{BaseName, CommitId, DiffStat, FileStat, PortError, TreeId, Vcs};

/// The pathspec that keeps zaf's own folder out of a change.
const NOT_ZAF: &str = ":(exclude,top).zaf";

/// What a diff is read with, so the repository's config does not change it: no colour, no
/// external diff driver, no text conversion, rename detection on, and the usual prefixes.
const DIFF_FLAGS: [&str; 6] = [
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--find-renames",
    "--src-prefix=a/",
    "--dst-prefix=b/",
];

/// A temporary index file, removed when it goes out of scope.
struct TempIndex(PathBuf);

impl TempIndex {
    fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let number = COUNTER.fetch_add(1, Ordering::Relaxed);
        let name = format!("zaf-index-{}-{number}", std::process::id());
        Self(std::env::temp_dir().join(name))
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        let mut lock = self.0.as_os_str().to_owned();
        lock.push(".lock");
        let _ = std::fs::remove_file(lock);
    }
}

/// A repository read through `git`.
#[derive(Debug, Clone, Copy, Default)]
pub struct GitVcs;

impl GitVcs {
    /// The adapter. It holds no state: every call names the repository it reads.
    pub fn new() -> Self {
        Self
    }

    /// Runs `git -C <root> <args>` in the fixed environment; its stdout, or
    /// [`PortError::NotFound`] when `root` is no repository, or [`PortError::Unavailable`] when
    /// git cannot be run, or [`PortError::Backend`] carrying its stderr when it fails.
    fn run(root: &Path, args: &[&str]) -> Result<String, PortError> {
        Self::run_with_index(root, args, None)
    }

    /// [`Self::run`] with `GIT_INDEX_FILE` set to `index` when one is given.
    fn run_with_index(
        root: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<String, PortError> {
        let output = Self::output(root, args, index)?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let failure = format!("git {}: {}", args.join(" "), stderr.trim());
            return Err(PortError::Backend(failure));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// A git read whose exit code answers yes (0) or no (1); any other code is the read failing,
    /// and an error, never a `false`.
    fn asks(root: &Path, args: &[&str]) -> Result<bool, PortError> {
        let output = Self::output(root, args, None)?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let failure = format!("git {}: {}", args.join(" "), stderr.trim());
                Err(PortError::Backend(failure))
            }
        }
    }

    /// Runs git in the fixed environment and hands back what it printed and its exit status
    /// whatever that is; [`PortError::NotFound`] when `root` is no repository.
    fn output(
        root: &Path,
        args: &[&str],
        index: Option<&Path>,
    ) -> Result<std::process::Output, PortError> {
        // A repository's own `.git` is a directory, or a file in a linked worktree.
        if !root.join(".git").exists() {
            return Err(PortError::NotFound(format!(
                "repository {}",
                root.display()
            )));
        }
        let mut command = Command::new("git");
        command.arg("--no-pager").arg("-C").arg(root);
        command.args(["-c", "color.ui=never"]);
        command.args(args);
        command.env("GIT_CONFIG_NOSYSTEM", "1");
        command.env("GIT_CONFIG_GLOBAL", "/dev/null");
        command.env("GIT_TERMINAL_PROMPT", "0");
        command.env("GIT_PAGER", "cat");
        command.env("LC_ALL", "C");
        command.env_remove("GIT_DIR");
        command.env_remove("GIT_INDEX_FILE");
        if let Some(index) = index {
            command.env("GIT_INDEX_FILE", index);
        }
        command.env_remove("GIT_WORK_TREE");
        // A process group of its own, so a Ctrl-C meant for zaf does not cut a read short.
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        command
            .output()
            .map_err(|error| PortError::Unavailable(format!("git: {error}")))
    }

    /// A diff of the workspace's whole work against the merge base with `origin/<base>`: the
    /// committed, the uncommitted and the untracked, read through a temporary index. `extra`
    /// goes after the flags (`--numstat -z`, or none for the patch) and `path` limits it.
    fn work_diff(
        workspace: &Path,
        base: &BaseName,
        extra: &[&str],
        path: Option<&str>,
    ) -> Result<String, PortError> {
        let index = TempIndex::new();
        Self::run_with_index(
            workspace,
            &["add", "-A", "--", ".", NOT_ZAF],
            Some(&index.0),
        )?;
        let base = format!("origin/{}", base.as_str());
        let literal = path.map(|path| format!(":(literal,top){path}"));
        let mut args: Vec<&str> = vec!["diff", "--cached", "--merge-base"];
        args.extend(DIFF_FLAGS);
        args.extend(extra);
        args.push(&base);
        args.push("--");
        match &literal {
            Some(literal) => args.push(literal),
            None => args.extend([".", NOT_ZAF]),
        }
        Self::run_with_index(workspace, &args, Some(&index.0))
    }
}

impl Vcs for GitVcs {
    fn latest_commit(&self, workspace: &Path) -> Result<CommitId, PortError> {
        // `HEAD` stays the slice's last commit while a merge is open (`MERGE_HEAD` is set), as
        // the #479 spike found: a merge makes no commit until it is resolved.
        let head = Self::run(workspace, &["rev-parse", "--verify", "HEAD^{commit}"])?;
        Ok(CommitId::from(head.trim()))
    }

    fn holds(&self, workspace: &Path, head: &CommitId) -> Result<bool, PortError> {
        let latest = self.latest_commit(workspace)?;
        // A merge left open (`MERGE_HEAD` is set) makes no commit, so `HEAD` is still the slice's
        // last one while the work is already past it: no head is held until it is committed.
        if Self::asks(
            workspace,
            &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"],
        )? {
            return Ok(false);
        }
        // The head comes from the forge and goes onto a git command line: only a full commit id
        // is let in, and anything else is a head this repository does not hold.
        let head = head.as_str();
        if !is_commit_id(head) {
            return Ok(false);
        }
        // A head the repository does not have is not held, and nothing is fetched to find it:
        // `--quiet` makes a missing commit exit 1 with no output, where a failure exits higher.
        let head_commit = format!("{head}^{{commit}}");
        if !Self::asks(
            workspace,
            &["rev-parse", "--verify", "--quiet", &head_commit],
        )? {
            return Ok(false);
        }
        // Held when the latest commit is the head or one of its ancestors...
        let latest = latest.as_str();
        if !Self::asks(workspace, &["merge-base", "--is-ancestor", latest, head])? {
            return Ok(false);
        }
        // ...and the two commits' trees are equal.
        let tree_of = |commit: &str| {
            let tree = format!("{commit}^{{tree}}");
            Self::run(workspace, &["rev-parse", "--verify", &tree])
        };
        Ok(tree_of(latest)?.trim() == tree_of(head)?.trim())
    }

    fn tree_of(&self, root: &Path, commit: &CommitId) -> Result<TreeId, PortError> {
        // The commit comes from the forge or a run record and goes onto a git command line: only
        // a full commit id is let in, and anything else is a commit this repository does not hold.
        let id = commit.as_str();
        if !is_commit_id(id) {
            return Err(PortError::NotFound(format!("commit {id}")));
        }
        // `--quiet` makes a commit the repository does not have exit 1 with no output, where a
        // failure exits higher. Nothing is fetched to find it.
        let tree = format!("{id}^{{tree}}");
        let args = ["rev-parse", "--verify", "--quiet", &tree];
        let output = Self::output(root, &args, None)?;
        match output.status.code() {
            Some(0) => {
                let printed = String::from_utf8_lossy(&output.stdout);
                Ok(TreeId::from(printed.trim()))
            }
            Some(1) => Err(PortError::NotFound(format!("commit {id}"))),
            _ => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let failure = format!("git {}: {}", args.join(" "), stderr.trim());
                Err(PortError::Backend(failure))
            }
        }
    }

    fn diff_stat(&self, workspace: &Path, base: &BaseName) -> Result<DiffStat, PortError> {
        let numstat = Self::work_diff(workspace, base, &["--numstat", "-z"], None)?;
        Ok(DiffStat {
            files: parse_numstat(&numstat),
        })
    }

    fn has_tag(&self, root: &Path, tag: &str) -> Result<bool, PortError> {
        // Matching is on the exact ref name: `for-each-ref` takes a prefix, and exits 0 either
        // way, so a missing tag is no exit code to tell from a failure.
        let reference = format!("refs/tags/{tag}");
        let listed = Self::run(root, &["for-each-ref", "--format=%(refname)", &reference])?;
        Ok(listed.lines().any(|line| line == reference))
    }

    fn has_remote(&self, root: &Path) -> Result<bool, PortError> {
        let listed = Self::run(root, &["remote"])?;
        Ok(!listed.trim().is_empty())
    }

    fn tags(&self, root: &Path) -> Result<Vec<String>, PortError> {
        let listed = Self::run(root, &["tag", "--list"])?;
        let mut names: Vec<String> = listed.lines().map(str::to_owned).collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    fn diff_hunks(
        &self,
        workspace: &Path,
        base: &BaseName,
        path: &str,
    ) -> Result<String, PortError> {
        let hunks = Self::work_diff(workspace, base, &[], Some(path))?;
        if hunks.is_empty() {
            return Err(PortError::NotFound(format!("no change to {path}")));
        }
        Ok(hunks)
    }
}

/// Whether `text` is a full commit id: 40 (SHA-1) or 64 (SHA-256) lowercase hex digits.
fn is_commit_id(text: &str) -> bool {
    let hex = text
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    matches!(text.len(), 40 | 64) && hex
}

/// Lines added and removed per file in `git diff --numstat -z`, in path order. A record is
/// `<added>\t<removed>\t<path>\0`, and a rename `<added>\t<removed>\t\0<old>\0<new>\0`, where
/// the new path counts; a binary file has `-` for both counts and counts as none.
fn parse_numstat(numstat: &str) -> Vec<FileStat> {
    let mut files = Vec::new();
    let mut fields = numstat.split('\0');
    while let Some(record) = fields.next() {
        let mut parts = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let path = if path.is_empty() {
            let _old = fields.next();
            fields.next().unwrap_or_default()
        } else {
            path
        };
        files.push(FileStat {
            path: path.to_owned(),
            added: added.parse().unwrap_or(0),
            removed: removed.parse().unwrap_or(0),
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

#[cfg(test)]
mod tests;
