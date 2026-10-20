---
name: my-queue
description: The Engineer's queue as five tables (Ready, Incoming, Questions, Waiting, Optional), each row weighted, from the queue file and live PRs. Read-only. Use for "what do you need from me?"
argument-hint: (none)
---

# /teams:my-queue: what's on the Engineer's plate

A read-only snapshot of everything that is the Engineer's to do, decide or watch, as five short tables. It changes nothing: no writes, no merges, no pushes, no posted comments.

## 0. Config

Read the teams file (`.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`) at the root of this checkout; if absent and this is a secondary workspace, at the root of the main checkout (git: the first path `git worktree list` prints; jj: `jj workspace root --name default`). Use `Repository › docs` (`<docs>`, resolved from the main checkout's root), `Branches › engineer reviews` (the branches whose PRs are the Engineer's), `Reviews › bot` and `Reviews › comment prefix` (`<prefix>`, default `Claude:`).

No teams file: say so in one line and build from the forge alone. The Engineer's branch is the default branch (`gh repo view --json defaultBranchRef`, or `git symbolic-ref refs/remotes/origin/HEAD`), so the list is the open PRs into it.

## 1. Gather (cheap; verify, don't trust)

Skip anything absent:
1. `<docs>/queue.md`, the queue the Orchestrator keeps (its shape is `/teams:orchestrate`'s `queue-template.md`: the five headings plus a `## Status` section, newest note first). Struck-through items (`~~…~~`) or items marked done are done; skip them. If it already uses the headings Ready, Incoming, Questions, Waiting, Optional, start from them, still checked against live state.
2. **Open questions:** use the Grep tool for `Claude's answer (` in the current release folder, `<docs>/releases/<version>/`. Current = the release the queue's latest status note names, else the most recently modified folder. A question already answered in a `rulings-*.md` there, or whose **Engineer:** line is filled, is not open.
3. **Live PRs, which win over every file.** GitHub: `gh pr list --state open --json number,title,baseRefName,headRefName,isDraft,mergeable,statusCheckRollup,reviewDecision,latestReviews,reviewRequests,url`, plus `-R OWNER/REPO` for any PR in another repo the queue names. `gh` absent or failing: say in one line what wasn't checked; never invent PR or CI state. Another forge: its CLI's equivalent.
   - A PR into an engineer-reviews branch is the Engineer's to review once its checks are green. While they run, it is Incoming. Draft, marked "hold merge", failing checks or a merge conflict: Waiting.
   - For PRs into engineer-reviews branches, also run `gh pr view <n> --json comments`. A comment starting `<prefix> hold merge` with no later `<prefix> hold lifted` means "hold merge".
   - With `Reviews › bot` set, expect its review only when `reviewRequests` or `latestReviews` names it. Requested and not yet in `latestReviews`: say so in the row. Neither names it: the bot wasn't requested (prepare-pr writes `Review bot: not requested (<reason>)` in the PR body, which is the waiver), so no wait and no flag.
   - A PR into any other branch is never the Engineer's (the Orchestrator merges those): leave it out unless the queue names it.
4. **Agents in flight:** whatever this session knows is running. Never predict a running agent's result.

Reconcile:
- The queue says ready but the forge says merged or closed: drop it.
- An open PR into an engineer-reviews branch that no file mentions: add it.
- Files and forge disagree: one line under the tables, offering to fix `queue.md` (this skill never writes it).

## 2. Classify

| Table | ID prefix | Goes here when |
|---|---|---|
| **Ready** | R1, R2… | The Engineer can act right now (review, merge, run a command, sign, walk a demo) |
| **Incoming** | I1, I2… | It will land on the Engineer, but something else must finish first (checks, an agent, another item) |
| **Questions** | Q1, Q2… | An open ask with the pre-filled answer; one-way doors are marked |
| **Waiting** | W1, W2… | With an agent or a third party; the Engineer only watches |
| **Optional** | O1, O2… | Nice to do, blocks nothing |

Number each table from 1, in descending Weight, and renumber on every run. IDs are labels for this snapshot only, never stable references. When the Engineer answers "R2", map it back to that row's name.

## 3. Columns (every table, this order)

| Column | Content |
|---|---|
| **ID** | `R1`, `I3`, … |
| **Name** | A few words, with the PR number when there is one (`#322 rustls bump`) |
| **What** | One plain sentence: what it is and what the Engineer does. Name the thing, no bare ids |
| **Weight** | 0–10, by the rubric below |
| **Blocker** | `No`, or `Yes: <what it blocks>`, e.g. `Yes: the PR into main` |

**Weight rubric:**
- 10: a security fix, or it blocks a release or every lane.
- 8–9: blocks the critical path or a lane.
- 6–7: needed this release, soon.
- 4–5: needed this release, not urgent.
- 2–3: later releases, or low impact.
- 0–1: FYI.

Ties go to whatever unblocks more.

## 4. Output

- **Header:** one line: `Your queue · <date>`, and how many rows are Ready.
- **The five tables,** in the order Ready, Incoming, Questions, Waiting, Optional. Write `— none` under any empty table instead of an empty grid.
- **After the tables, at most two lines:**
  - **Do next:** R1, as one instruction with the exact command or click path. Numbered steps only if the Engineer asks.
  - **Answer:** "reply with the IDs, e.g. `Q1 yes, Q2 no`", only if Questions isn't empty.

Keep cells to one line. Never pad. Short beats complete.

## Guardrails

- **Read-only,** as above. If the files and the forge disagree, say so in one line and offer to fix `queue.md`.
- **Never fabricate** a running agent's outcome, or a PR's check state you didn't fetch.
- **A question already ruled** (it appears in a `rulings-*.md`) is not open; drop it.
