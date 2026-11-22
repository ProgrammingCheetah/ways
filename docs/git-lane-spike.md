# Spike: what git can and cannot do for zaf's lane (#479)

Design pages: design-addendum D27, D28, Q6.

Status: **answered, with the limits named per answer.** Q1 and Q2 rest on the experiments Claude ran for ruling #527 (their output is in the comment on #479; this note carries the results, not the transcripts). Q3 and Q4 rest on the Claude Code documentation, not on an experiment. The instance that wrote this note could not run `git` or `claude` (the `lean` profile refuses them), so the tool versions of the experiments are those recorded in the comment on #479, and the experiments are not re-run here.

| # | Question | Answer | Basis |
|---|---|---|---|
| 1 | A merge left in progress | `HEAD` stays the slice's last commit, `MERGE_HEAD` is set, `git diff --merge-base` works | experiment (#479 comment) |
| 2 | Version floor of `merge-tree --write-tree` | **git 2.38.0**, for `--write-tree` and `--name-only` alike | v2.38.0 manual and release notes; v2.37.0 lacks both |
| 3 | Env prefix against `Bash(git log *)` | an allow rule does not match past it; a deny rule does | permissions docs |
| 4 | `acceptEdits` and `.git/config`, `.git/hooks/` | no on the docs: a protected path prompts | permission-modes docs |

## 1. A merge left in progress in a workspace

Experiment (a scratch repository: both branches change the same line, `git merge` stops with a conflict):

- `HEAD` stays on the slice's last commit; the merge makes no commit. A reader that takes "the slice's latest commit" from `HEAD` (`latest_commit`) still gets the slice's own work, not a merge.
- `MERGE_HEAD` is set. It is the one sign of a merge in progress: `git rev-parse -q --verify MERGE_HEAD` exits 0 with the base's commit, and exits 1 with no output when none is. In a linked worktree the file is under the worktree's own git directory, so read it through `git rev-parse --git-path MERGE_HEAD`, never as `.git/MERGE_HEAD`.
- `git diff --merge-base <base>` works, so the change against the base can be read while the merge is open.

What this means for the lane (reading, built on the above and on zaf's code, not on a further experiment):

- zaf has no git `Vcs` adapter yet. The port (`zaf/src/ports/vcs.rs`) has four reads, `latest_commit`, `diff_stat`, `has_tag` and `diff_hunks`, and only `adapters/jj.rs` implements them; `.zaf/actions/merge-base` reaches git only through `jj git fetch`. So what zaf's own reads report during a merge is for D26's git adapter to define, and it must take `MERGE_HEAD` as "a merge is open" rather than assume `HEAD` moved.
- `slice-pr` and the fix brief can tell that a merge is open from `MERGE_HEAD`. To **name the conflicted paths**, use `git diff --name-only --diff-filter=U` or `git ls-files -u` (git's documented unmerged-path reads); the experiment recorded `MERGE_HEAD` and `diff --merge-base`, so these two are taken from the git manual and the adapter task must check them on its first test.
- `slice-pr` must refuse to commit or push while `MERGE_HEAD` exists unless the programmer has resolved the paths: committing during a merge makes the merge commit.

## 2. The git version floor for `merge-tree --write-tree`

**Floor: git 2.38.0.** Both `--write-tree` and `--name-only` are in the v2.38.0 manual and release notes; v2.37.0 has neither (its `git merge-tree` takes only the old three-argument form, `git merge-tree <base-tree> <branch1> <branch2>`, which writes no tree and has no exit-code contract). One version, not two: `--name-only` did not come later.

The behavior `merge-base --probe` needs, from https://git-scm.com/docs/git-merge-tree:

- "When the merge has conflicts, the exit status is 1." and "For a successful, non-conflicted merge, the exit status is 0."
- Clean: the output is "simply one line: <OID of toplevel tree>".
- Conflicted: the toplevel tree id, then the conflicted file info, then informational messages; with `--name-only` "the mode, object, and stage will be omitted", leaving the path.

Use it as `git merge-tree --write-tree --name-only <slice> <base>`: exit 0 and a tree id on line 1 for a clean merge; exit 1, the tree id on line 1 and the conflicted paths after it for a conflict. Any other exit is a failure of the command, not a conflict. `--merge-base` (an explicit base) is a later addition and is not part of the floor.

The doctor should check `git --version` against 2.38.0 and fail the git lane below it.

Limit: the version before the floor is shown failing by the v2.37.0 manual and release notes, not by a run of v2.37.0 in this note.

## 3. An environment prefix against `Bash(git log *)`

Sourced (https://code.claude.com/docs/en/permissions, "Wrappers"):

- "Claude Code also strips a leading assignment of certain known-safe environment variables, so `Bash(npm test *)` matches `NODE_ENV=test npm test`."
- "An allow rule won't match past an assignment of any other variable."
- "A deny or ask rule matches past any leading assignment, so `Bash(rm *)` in deny still matches `FOO=bar rm -rf tmp/`."

Answer: `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=… git log` is not stripped (the docs do not list `GIT_CONFIG_*` as known-safe), so it does **not** match the allow rule `Bash(git log *)`, and a **deny** rule such as `Bash(git log *)` does catch it. Not run in a real Claude Code.

Same page: `Bash(git push *)` does not catch `git -c push.default=current push`, so the `-c` form needs its own denies, as Q6 already lists.

**Proposal (for the Q6 task, not decided here).** The answer is "no" to the question of whether the prefix slips past a deny, so no rule is needed for it. Keep the Q6 denies and add `Bash(git -c *)` and `Bash(git --config-env*)`. If a run shows `GIT_CONFIG_*` is on the known-safe list, add `Bash(GIT_CONFIG_* *)` to deny.

## 4. `acceptEdits` and `.git/config` or `.git/hooks/`

Sourced (https://code.claude.com/docs/en/permission-modes):

- "`acceptEdits` mode lets Claude create and edit files in your working directory without prompting."
- Auto-approval "applies only to paths inside your working directory or `additionalDirectories`", and "These still prompt: … Writes to protected paths".
- `.git`, `.vscode`, `.idea` and `.claude` are protected paths, prompted in `default` and `acceptEdits`, except Claude's own worktrees under `.claude/worktrees/` (wording taken from a page summary, not checked against the page).

Answer: no on the documentation. An Edit or Write to `.git/config` or `.git/hooks/pre-commit` prompts in `acceptEdits`; an unattended run cannot answer a prompt. Not run in a real Claude Code, and the docs are silent on a zaf worktree (`.zaf/workspaces/…`, not `.claude/worktrees/`), where `.git` is a one-line pointer file and the real config lives in the main checkout's `.git/`, outside the working directory.

**Proposal (for the Q6 task; closes the gap if a run shows a write passing):** `Edit(.git/**)`, `Write(.git/**)`, `Edit(**/.git/**)` and `Write(**/.git/**)` in deny, and `Bash(git config *)`, `Bash(git hook *)`, `Bash(git worktree *)` kept out of the allow list.

## Still owed

Nothing blocks the later tasks. Two checks are cheap and left to them: the unmerged-path reads of Q1 on the git adapter's first test, and a real-Claude run of Q3 and Q4 when the profile task writes its deny rules.
