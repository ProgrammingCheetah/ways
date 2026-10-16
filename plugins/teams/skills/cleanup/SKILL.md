---
name: cleanup
description: After merges, report every git worktree or jj workspace with its merge evidence, then prune only the proven-safe ones after one confirmation. Use once PRs merge, e.g. /teams:cleanup.
argument-hint: "[--yes] [branch or workspace …]"
---

# /teams:cleanup: remove merged workspaces, branches and stacks

Step 9 of the teams lifecycle as a routine, also usable solo. After merges, it reports every workspace (git worktree or jj workspace) with its merge evidence and cleanliness, then prunes only the unambiguously safe ones. **Destructive, so it reports first, confirms once, and leans to "gray: keep and report" on every doubt.**

**Arguments.** Names (branches or workspaces) limit the sweep to those; the Orchestrator passes the merged branches at step 9. No names: every workspace. `--yes` skips the confirmation for the green-lit set only; gray is never pruned. A Spawn running without `--yes` reports and stops: its report lists the green-lit set for the Orchestrator to confirm.

## Setup

- **The teams file:** `.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`, at the root of the main checkout. Read `Repository › workspaces` (the workspaces folder: `{repo}` is the main checkout's folder name, a relative path is from the main checkout), `Repository › vcs`, `Repository › main`, `Branches › engineer reviews`, `Repository › docs` (for `<docs>/queue.md`) and `Repository › teardown`: commands run inside a workspace before it is removed, with `{name}` (the workspace folder's name) and `{path}` (its absolute path), e.g. `docker compose -p {name} down -v`; — means none.
- **No teams file:** report only and prune nothing; say why (the workspaces folder is unknown, so no workspace is a candidate). Detect the VCS (a `.jj/` folder means jj) and main (`gh repo view --json defaultBranchRef`, else `git symbolic-ref refs/remotes/origin/HEAD`).
- **Run from the main checkout.** git: it is the first entry of `git worktree list`; compare with `git rev-parse --show-toplevel`. jj: it is the `default` workspace; compare `jj workspace root` with default's root. Started inside a secondary workspace: stop and say so. The main checkout is never removed and its working copy never moves (no switch, checkout, `jj new` or `jj edit` there).
- **Candidates** are only the workspaces whose folder lies inside the workspaces folder. Every other worktree or workspace is reported, never touched.

## 1. Inventory and fetch (the only write before the confirmation)

**git.** `git worktree list --porcelain` (path, `branch refs/heads/<b>` or `detached`, `locked`, `prunable`) and `git for-each-ref --format='%(refname:short) %(objectname) %(upstream:remotename) %(upstream:track)' refs/heads` (each branch's tip and remote; remote = the upstream's, else `origin`). Then `git fetch --prune <remote>` for each remote involved.

**jj.** Record before fetching, because the fetch deletes a local bookmark whose tracked remote was deleted: `jj workspace list -T 'name ++ "\t" ++ root ++ "\n"'` (a blank root means the folder is gone or was never recorded), `jj bookmark list --all-remotes -T 'name ++ "@" ++ remote ++ " " ++ normal_target.commit_id() ++ "\n"'`, and per workspace `jj bookmark list -r '::<name>@ & mutable()'` (its branch: exactly one bookmark; none or several is gray). Then fetch with `jj --config git.abandon-unreachable-commits=false git fetch --all-remotes`. Without that flag jj's fetch abandons the commits a deleted remote branch leaves behind and rebases what sits on them, which makes workspaces stale and can move the main checkout's working copy; with it, every abandon goes through step 4's checks.

## 2. Classify

- **Merged** needs positive evidence, against the tip recorded above:
  (a) the forge has a MERGED PR whose head is this branch and whose head commit equals the local tip: `gh pr list --head <branch> --state all --json number,state,headRefOid,baseRefName,commits`; or
  (b) the tip is an ancestor of the remote-tracking ref of main or of the PR's base: git `git merge-base --is-ancestor <tip> <remote>/<base>` exits 0; jj `jj log --no-graph -r '<tip> & ::<base>@<remote>'` prints the commit. Ancestry only proves merge-commit and fast-forward merges. Squash and rebase merges never pass it, so for them ancestry is not evidence either way.
  A local branch with commits beyond the merged head or its remote is never merged. An open PR from the same branch is gray. `gh` absent or failing: say in one line that PR state wasn't checked, and use (b) only.
- **Remote gone:** absent on the remote after the pruning fetch. git: `git ls-remote --heads <remote> <branch>` prints nothing. jj: the bookmark has no `@<remote>` line in a fresh `jj bookmark list --all-remotes` (the local bookmark may be gone too).
- **Clean.** git: `git -C <path> status --porcelain` prints nothing (untracked files count; ignored build output doesn't), the porcelain entry shows the branch (HEAD is its tip, not `detached`), it isn't `locked`, and `git -C <path> submodule status` prints nothing. jj: `jj -R <path> status` (this snapshots that workspace; an error that it is stale makes it gray: the Engineer runs `jj workspace update-stale` there, which keeps unsnapshotted edits in a separate commit), then `jj log --no-graph -r '<name>@' -T 'empty ++ " [" ++ description ++ "]\n"'` prints `true []`, and `jj log --no-graph -r 'mutable() & ::<name>@ & ~empty() & ~::<tip>'` prints nothing.
- **Not in use:** not named as in flight in `<docs>/queue.md`, not a workspace a running Spawn was briefed with (the Orchestrator knows; solo, ask the Engineer when unsure; a Spawn treats every workspace its brief didn't name as possibly in use), not locked. A workspace with no branch (a planning workspace, detached at the base's tip) is gray: "remove on the Engineer's word".
- **Green-lit** = merged + remote gone + clean + not in use. Everything else is gray: kept, with its reason.
- **Folder already gone** (git `prunable`; jj blank root, and `jj workspace root --name <name>` names the missing path): report it. Removing its record is green-lit only when its branch is merged and the remote branch is gone.
- **Stale branches with no workspace:** local branches or bookmarks checked out nowhere (git: in no worktree; jj: in no `::<name>@`). Merged + remote gone is green, else gray. Never `Repository › main`, an engineer-reviews branch, or a branch checked out in any workspace, the main checkout included.

## 3. Report, then confirm

```
# Cleanup: {w} workspaces, {b} branches, {s} stacks; {p} to prune, {g} kept
```

| Workspace | Branch | Merged (evidence) | Remote gone | Clean | In use | Action |
|---|---|---|---|---|---|---|
| {folder} | {branch} | yes: PR #12, head equal / no: open PR | yes / no | yes / untracked files / stale | no / queue.md | prune / keep: {reason} |

| Name | Evidence | Action |
|---|---|---|
| {branch, bookmark or stack tip} | PR #12 merged, remote gone / ahead of remote | delete / abandon / keep: {reason} |

Before any destructive step, list exactly what will be torn down, removed, deleted and abandoned, and ask once (unless `--yes`). Then re-read each tip and cleanliness; anything that changed since the report is kept.

## 4. Prune, per green-lit workspace, in this order

1. **Teardown:** run each `Repository › teardown` command with the workspace folder as the working directory. Any non-zero exit stops that workspace: it is kept and reported.
2. **Remove it through the VCS.** git: `git worktree remove <path>`, never `--force`. Tested: it deletes ignored files (build output) with the folder, and refuses untracked or modified files, a lock and submodules, which clean already excluded; a refusal means something changed, so keep it. A folder already gone: the same command removes just its record (`git worktree prune` would remove every missing one, so don't use it). jj: `jj workspace forget <name>` (always name it: with no name it forgets the current workspace), then delete its folder (bash `rm -rf -- <path>`, PowerShell `Remove-Item -LiteralPath <path> -Recurse -Force`) only after checking the path is the root recorded for that workspace and lies inside the workspaces folder.
3. **Delete its local branch.** git: `git branch -d <branch>`; when it refuses as not fully merged (always after a squash merge), `git branch -D <branch>`, allowed only because evidence (a) or (b) held for this exact tip. jj: `jj bookmark delete <bookmark>` when the fetch hasn't already removed it.
4. **jj only: abandon the proven-merged stack** `mutable() & ::<tip>`. Allowed only when every non-empty change in it (`jj log --no-graph -r 'mutable() & ::<tip> & ~empty()' -T 'commit_id ++ "\n"'`) is one of the merged PR's `commits` and `jj log --no-graph -r 'descendants(mutable() & ::<tip>) ~ (mutable() & ::<tip>)'` prints nothing (`jj abandon` rebases descendants, which would rewrite live work, the main checkout's included). Then `jj abandon '<that set>'`, always with a revset: with none it abandons `@`. One failing change keeps the whole stack.

Stale branches with no workspace get steps 3 and 4. **Never rebase. Never push** (the remote side is already gone, so `jj bookmark delete` has nothing to propagate).

## 5. Final report

**Pruned:** each workspace, branch and stack removed. **Teardown results:** each command and its exit code. **Kept for the Engineer:** each gray item and its reason. No emoji.
