---
name: catch-me-up
description: Brief the Engineer on where the project stands: what we were doing, the context, what's missing, what's next. Read-only, starts no work. Use after a gap or on "where were we?"
argument-hint: (none)
---

# /teams:catch-me-up: where we are, how we got here, what's next

A **spoken briefing** of the project's current state. Read-only: it writes nothing, changes nothing, starts no work. The briefing is the deliverable.

## 0. Config

Read the teams file (`.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`) at the root of this checkout; if absent and this is a secondary workspace, at the root of the main checkout (git: the first path `git worktree list` prints; jj: `jj workspace root --name default`). Use `Repository › docs` (`<docs>`, resolved from the main checkout's root), `vcs`, `main`, `decisions` and `workspaces`, and `Release › version format`.

No teams file: say so in one line and brief from the project's CLAUDE.md, its decisions folder if one is obvious, the VCS history and the open PRs. A `.jj/` folder means jj; the default branch comes from `gh repo view --json defaultBranchRef` or `git symbolic-ref refs/remotes/origin/HEAD`.

## 1. Gather (cheap; skip what's absent)

Skip anything already loaded this session. Never read whole source files or whole documents, only the parts that explain the current work.

1. **The queue:** `<docs>/queue.md`. Its latest status note (the newest entry under its `## Status` heading) is "what we were just doing"; its top items are "what's next".
2. **The current release folder,** `<docs>/releases/<version>/`. Current = the release the latest status note names, else the most recently modified folder. Read `objectives.md` (a release older than it has `scope.md`), then name the phase by the last of these that holds:
   - `objectives.md` only, or not yet in `signoff.md`: objectives (interview, QA or signature);
   - `objectives.md` in `signoff.md`, no release documents: design next;
   - release documents not yet in `signoff.md`: design review;
   - release documents in `signoff.md`: signed;
   - `roadmaps/` and `claims.md`: roadmaps and claims;
   - `groundwork.md`: Groundwork;
   - `plans/`: slice plans, so build is next or under way;
   - `milestones.md`: milestone QA; its last row says which milestone and whether its fix rounds closed;
   - `reviews/` with `compliance.md`, `architects.md` or `security.md`: release reviews.

   Then the newest `rulings-*.md` there.
3. **Signed files:** for each id in `signoff.md`, run `git hash-object <file>` and compare. Read-only: never pass `-w`. It works in a jj repo too. A changed signed file is a finding.
4. **Decisions:** the newest entries under `Repository › decisions`, only as far back as needed to explain the current work. The newest file in `<docs>/retros/`, if it touches the current work.
5. **VCS:**
   - git: `git status --short`, `git branch --show-current`, `git log --oneline -15 origin/<main>`, `git worktree list`.
   - jj: `jj status`, `jj log -r "ancestors(<main>@origin, 15)"`, `jj workspace list`, `jj bookmark list`.
   - No remote: use the local `<main>`. This skill doesn't fetch, so the remote view is as of the last fetch; say so in the briefing.
   - Open PRs: `gh pr list --state open` (another forge: its CLI's equivalent). `gh` absent or failing: say in one line that PRs weren't checked; never invent PR state.

   Workspaces under `Repository › workspaces` are work in flight.

## 2. Reconcile

Cross-check the clocks before speaking:

- **Status note vs VCS:** merged work past the note means the note is behind; say so.
- **Plan vs VCS:** planned work merged but not marked, or marked but not merged.
- **Signed files:** any changed since signoff.
- **Dirty state:** uncommitted files, a non-main branch, or in-flight workspaces mean work in progress; name it.

Discrepancies are findings, not errors to fix. They go under "Missing".

## 3. Brief

Short prose and short lists, no padding, no headers beyond these five:

1. **Where we are:** one or two sentences: branch, last merged work, the release and its phase, the slice in flight.
2. **What we've been doing:** the last few units of work (PRs or commits), newest first, one line each.
3. **Context:** the why behind the current work: the ruling or decision that shaped it, one or two sentences. Cite the ruling file or decision record by path and title, not ids alone. Don't re-explain settled history.
4. **Missing:** the findings from step 2, open questions without a ruling, claims or plan items not yet resolved, anything the status note flags as open.
5. **Next steps:** concrete actions in order. The first is what to do the moment the briefing ends. Name a fitting skill where one applies: `/teams:my-queue` for the full queue, `/teams:orchestrate` to resume the lifecycle, `/teams:cleanup` for merged workspaces.

Then stop. **Do not start the work.** The Engineer decides what happens next.
