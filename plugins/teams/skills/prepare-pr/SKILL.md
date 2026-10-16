---
name: prepare-pr
description: Take one branch from work done to an open PR with the gate green (commit, rebase, gate, push with a lease, open or update the PR), then pause. Never merges. E.g. /teams:prepare-pr
argument-hint: "[branch or workspace path] [title]"
---

# /teams:prepare-pr: one branch to an open PR, gate green

Takes one branch from "work done" to "PR open, gate green", then pauses for the merge. It never merges. Runs solo, or in the teams lifecycle at step 8 (programmers committed, the build lead pushed; the Orchestrator runs this from its own checkout against the branch's workspace).

## Inputs

- **Arguments:** `[branch or workspace path] [title]`. Default: the current branch of the current checkout (jj: the bookmark on `@` or `@-`, from `jj bookmark list -r "@ | @-"`; none or several: ask). Given a workspace or a branch with a workspace: run every command there (`cd <workspace> && …`).
- **The teams file:** `.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`, at the root of the current checkout; if absent and this is a secondary workspace, at the root of the main checkout (git: the first path `git worktree list` prints; jj: the folder holding the `.jj` directory that this workspace's `.jj/repo` file points to). It reads `Repository › vcs, main, gate, light gate, style`, `Branches › strategy, integration, slice, engineer reviews, merge slice, merge main`, `Tracker › kind, project, commit format` and `Reviews › bot, security triggers`.
- **No teams file (solo):** detect. A `.jj/` folder means jj. The default branch from `gh repo view --json defaultBranchRef` (or `git symbolic-ref refs/remotes/origin/HEAD`). The gate from the CI config (`.github/workflows/*.yml` or the forge's equivalent). The title convention from recent commit subjects on the default branch. A ticket key from the branch name. Ask the Engineer once, in one round, only for what detection can't settle. A Spawn has no Engineer: it puts the question, with its pre-filled answer, in its report.
- **The base:** with `Branches › strategy` = `integration`, a branch matching `Branches › slice` targets the integration branch with the same `{slug}`; an integration branch, or any other branch, targets `Repository › main`. Solo: the default branch.

## Hard rules

- **Refuse on `Repository › main` or any `Branches › engineer reviews` branch** (solo: the default branch): say "branch first". Never push to them. **Never merge.**
- **Pushes carry a lease** keyed to the remote tip you fetched: `git push --force-with-lease=<branch>:<sha> -u origin <branch>` (`<sha>` from `git rev-parse origin/<branch>` after the fetch; a branch not yet on the remote uses `--force-with-lease=<branch>:`, which requires it to be absent). Never bare `--force`. jj: `jj git push -b <bookmark>`; jj's own safety checks update the remote only if it still matches what jj last fetched (its built-in lease).
- **No interactive terminal:** no `git rebase -i`, `git add -i` or `-p`. Stage by pathspec.
- **Attribution:** commit messages end with the footer the session's attribution guidance gives; the PR body ends with the line it gives for PRs.
- **One PR per branch:** `gh pr list --head <branch> --state open --json number,url,baseRefName,isDraft`. One is open: update it, never open a second. Its base differs from the computed base: stop and ask; don't retarget silently.
- **Outward-facing:** before each push and before opening or editing the PR, say what you're about to do.

## Steps

1. **Survey.** git: `git branch --show-current`, `git status`, `git fetch origin`, `git log --oneline origin/<base>..HEAD`, `git diff --stat origin/<base>...HEAD`. jj: `jj status`, `jj git fetch`, `jj log -r "<base>@origin..<bookmark>"`, `jj diff --from <base>@origin --to <bookmark> --stat`. Find the ticket in the branch name (`Tracker › project` key and number, e.g. `abc-12-…` gives `ABC-12`; `github`: the issue number), and its title from the tracker's tools when present (GitHub: `gh issue view <n> --json title,url`). A step with nothing to do (no changes, not behind) passes on.

2. **Format-write.** Run the writing form of the gate's format check (the check is in `Repository › gate` or CI; its writing form drops the check flag, e.g. a `--check` becomes a write). It may edit files; they're committed next.

3. **Commits.** Keep existing clean commits (lifecycle programmers already committed). Commit only new or formatted changes, granular by pathspec, one coherent story each, titled per `Tracker › commit format`, else the repo's convention. git: `git add <paths>` then `git commit -m "<title>" -m "<footer>"`. jj: `jj commit <paths> -m "<title>" -m "<footer>"`, then `jj bookmark move <bookmark> --to @-` if the bookmark didn't follow.

4. **Rebase if behind.** git: `git log --oneline HEAD..origin/<base>` non-empty, then `git rebase origin/<base>`. jj: `jj log -r "<bookmark>..<base>@origin"` non-empty, then `jj rebase -b <bookmark> -o <base>@origin`. **On conflict, stop:** git pauses the rebase; jj completes it and records conflicts (`jj log -r "conflicts()"` lists them). Show them and resolve with the Engineer, never by guessing; then `git rebase --continue` (or fix the conflicted jj commits). A Spawn reports the conflicts instead (git: `git rebase --abort` first).

5. **The gate.** Run `Repository › gate` exactly (its `light gate` never stands in here), judged on exit code **and** output. Solo: every step the CI config runs, from the directory CI runs it in, with CI's environment variables set and without services CI lacks (a reachable local database or container can hide a failure CI hits). **Never push red.** A step needing a service that isn't reachable: say so and ask (CI will still run it). Then the rest of the definition of done (the project's `CLAUDE.md`, else `/teams:orchestrate` section 8), before the PR:
   - **Docs** on changed signatures, per `Repository › style` (`/teams:document`).
   - **The review loop closed.** In the lifecycle: step 7's reviews passed, every finding applied or listed as deferred. Solo: offer `/teams:critique`.
   - **Security review** when the diff touches a `Reviews › security triggers` item (solo: the template's list: authentication; authorization and permissions; sessions, tokens, secrets and keys; cryptography; untrusted input; trust boundaries; network egress; personal data storage). The project's own review, or Claude Code's `/security-review` when present.
   A red here blocks like a red gate.

6. **Push** (lease as in the hard rules). Already-open PR: say whether this push carries a fix.

7. **Open the PR** with `gh pr create --base <base> --head <branch> --title "<title>" --body-file <file>`, the body file outside the repo (the session's scratch or temp folder). Updating an open PR: `gh pr edit <n> --body-file <file>` when the body no longer fits.
   - **Title:** the argument, else per `Tracker › commit format`, else the repo's convention.
   - **Body:** what it does in plain words; implementation choices; deferred findings; links to reviews; how to check it locally; the ticket link, or a closing keyword when the tracker reads one (GitHub: `Closes #<n>`); the attribution line last.
   - **Review bot:** request `Reviews › bot` (solo: none unless the Engineer names one) only on a PR into an engineer-reviews branch with real review surface. Request it for logic, a security-trigger item (a spec that defines a security boundary counts), or non-trivial size (several files of logic, roughly 50 changed code lines, real branching or error handling). Skip it for docs-only, config-only or mechanical changes (renames, dependency bumps). Unsure: request. `gh pr edit <n> --add-reviewer <login>` (some bots take a special value: `gh pr edit --help` lists them); fallback `gh api --method POST "repos/{owner}/{repo}/pulls/<n>/requested_reviewers" -f "reviewers[]=<login>"`. Confirm with `gh pr view <n> --json reviewRequests`. Say whether you requested it and why.

8. **Pause.** Print the PR URL. Name the merge method that fits: `Branches › merge main` for a base in `engineer reviews`, else `Branches › merge slice`. Solo: what the repo allows (`gh repo view --json mergeCommitAllowed,squashMergeAllowed,rebaseMergeAllowed`), recommending one by how the base's history reads (one commit per PR: squash). **Don't merge.** Wait for word that it's merged.

9. **After the merge** (on the Engineer's or the Orchestrator's word): `/teams:cleanup <branch>` removes the workspace and branch. Solo, when the branch is checked out in the Engineer's own checkout, offer, and only with their yes: git `git switch <base>`, `git pull --ff-only`, `git branch -d <branch>` (a squash merge makes `-d` refuse; with the PR shown merged, `-D`), `git fetch --prune`. jj: `jj git fetch`, `jj new <base>@origin`, `jj bookmark forget <branch>` if it's still listed.

## Notes

- **gh absent or failing:** say in one line what wasn't checked (an open PR, CI state, the reviewer request); never invent PR or CI state. Another forge: its CLI's equivalent of each `gh` command.
- **Workspaces:** with `cd <workspace> && …` on every command, this runs from any checkout without switching the Engineer's.
