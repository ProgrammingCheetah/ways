---
name: critique
description: Critique a diff in two passes (right thing built? built right?) against the plan or ticket and the codebase's own patterns; flag reinvention, duplication, quality; fix or ask. E.g. /teams:critique
argument-hint: "[staged | <range> | <path>…; default: uncommitted changes plus the branch's commits]"
---

# /teams:critique: judge the diff against what was asked and what the codebase already does

Reviews changed code for **fit with the request, fit with existing patterns, quality and duplication**. Not bug-hunting (that is `/code-review`). The stance is a senior reviewer who knows the codebase: prefer reusing what exists, weigh quality honestly, and **be willing to find nothing**. Runs solo, or in the teams lifecycle as the build lead's own pass on its slice diff before it pushes (**Build**).

## Inputs

- **The teams file:** `.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`, at the root of the current checkout; if absent and this is a secondary workspace, at the root of the main checkout (git: the first path `git worktree list` prints; jj: `jj workspace root --name default`). It reads `Repository › main, vcs, style, decisions`, `Repository › light gate, gate`, `Tracker › kind` and `<docs>` (`Repository › docs`, resolved from the main checkout's root).
- **No teams file (solo):** detect. A `.jj/` folder means jj. The default branch from `gh repo view --json defaultBranchRef` (or `git symbolic-ref refs/remotes/origin/HEAD`). The gate from the CI config (`.github/workflows/*.yml` or the forge's equivalent). Ask the Engineer once, in one round, only for what detection can't settle. A Spawn has no Engineer: it puts the question, with its pre-filled answer, in its report.
- **Conventions to judge against:** the project's `CLAUDE.md`, the files in `Repository › style`, and the recorded decisions in `Repository › decisions` (an ADR folder or similar) plus the release folder's rulings files. **A deliberate, recorded choice is not a mistake:** cite it and move on.

## Scope (the argument)

Critique only the changed code, but read freely across the repo as **reference**. Remote main below is `origin/<main>` (jj: `<main>@origin`); no remote ref: the local `<main>`.

| Argument | git | jj |
|---|---|---|
| **empty**: uncommitted changes plus the branch's commits against remote main | `git diff --merge-base origin/<main>` (tracked files, staged or not), then `git status --short` for untracked files | `jj diff -r "<main>@origin..@"` (every commit since main, plus the working copy) |
| **`staged`** | `git diff --cached` | Doesn't apply: jj has no staging area, the working copy is a commit. Treat `staged` as the current change: `jj diff -r @` |
| **a range or revision** | `git diff <range>`, e.g. `git diff main..HEAD`, or `git diff HEAD~3` (that commit to the working tree) | `jj diff -r "<revset>"` (all commits in the revset, summed), or `jj diff --from <a> --to <b>` |
| **paths** | add `-- <path>…` to any form above | add the paths (filesets) after any form above |

Add `--stat` or `--name-only` (both tools accept both) to list the files before reading hunks. An empty scope: say so and stop.

## Step 0: what "the right thing" is measured against

- **Teams lifecycle:** the slice plan for this branch, `<docs>/releases/<version>/plans/<team>-<slice>.md` (match the branch name or slice; the version is the one the plan or brief names, else the newest folder under `<docs>/releases/`), and the signed release documents (`Release › documents`, with `signoff.md`). Several plans fit: ask the Engineer; a Spawn names the one it assumed. A milestone fix round: the findings it fixes, in `<docs>/releases/<version>/reviews/m<n>-*.md`.
- **Solo:** the ticket (the tracker's tools when present; `Tracker › kind` = `github`: `gh issue view <n> --comments`, the number from the branch name or the PR), else the PR description (`gh pr view --json title,body`), else the Engineer's ask in this conversation. `gh` absent or failing: say in one line what wasn't read.
- **Nothing found:** run phase 1 against the Engineer's ask, or skip it and say so. Never invent requirements.

## Two phases: Product Owner, then Tech Lead

Two ordered passes with distinct lenses. Phase 2 reads phase 1's findings, so engineering judgment accounts for product intent. A finding can surface in both ("this criterion isn't met" and "here is the technical reason").

1. **Product Owner (phase 1): did we build the *right thing*?** Judge the change against the plan or ticket's intent and acceptance criteria, product value and scope, not code style. Every criterion met and mapped to real behaviour; no scope creep and no silent gaps; the edge cases that matter to users; consistency with the signed documents; a coherent, shippable increment. Flags: a missing or partial criterion, scope drift, a product-level edge case, a value gap, and **a decision that should have been the Engineer's** (an entity's shape, a name, an invariant, a boundary, an API contract, a schema, a trade-off with two defensible answers). That last flag is never fixed here: it goes to the Engineer as a question.
2. **Tech Lead (phase 2): did we build the thing *right*?** Judge the engineering: pattern fit, maintainability, the right abstractions, tech debt introduced, long-term risk, testability, and design-level security and performance. Steps 1 to 4 below do most of their work here.

Present **two labelled sections**, Product Owner findings first, then Tech Lead findings. Steps 1 to 4 apply within each phase (phase 1 weighs each region against the criteria, phase 2 against the code and architecture). Either phase may return nothing.

## Step 1: learn the codebase's patterns first

Before judging, map what the repo already does that is relevant. For each new function, type or abstraction in the diff, hunt for **prior art**:

- Existing helpers, utilities or abstractions that already do this (Grep and Glob, or a search Spawn: "does anything already do X?").
- Established idioms: error handling, naming, layering and module boundaries (whatever architecture rules the project's `CLAUDE.md` or style files state), test patterns.
- The conventions and recorded decisions listed under Inputs.

Every flag must be anchored to something that exists or an established norm.

## Step 2: weight each changed region

Give each meaningful region a verdict, with evidence:

| Verdict | Meaning |
|---|---|
| Solid | fits the codebase, clear, no action |
| Minor | small nits; optional |
| Should-fix | a clear quality or consistency problem |
| Reinvents | re-implements something already solved, or notable duplication |

## Step 3: flag only what is anchored

Three flag types; each MUST cite the pattern or norm it is measured against:

- **Already solved (reinvention):** the new code re-implements an existing abstraction. Cite it (`file:line`) and propose using it.
- **Duplication:** logic repeated within the diff, or against existing code. Propose extracting or reusing.
- **Quality could improve:** readability, naming, wrong altitude, a missed idiom, inconsistency with house style. Propose the concrete improvement.

**Verify before you flag.** Confirm the existing abstraction *actually* fits (same semantics, not just surface similarity) before calling something a reinvention or duplicate: a wrong refactor suggestion is worse than none. Unsure: lower the verdict and raise a question, not a fix.

## Step 4: present, then fix or ask

Show a short summary (table of regions and verdicts) and the detailed findings, each with its cited prior art and a concrete proposed change.

For each actionable finding, **offer to fix it or ask why it isn't already that way**: the author may have a reason you can't see. The Engineer chooses per finding (the AskUserQuestion tool when present, else a numbered list in chat). Batch trivial nits into one question; ask substantial refactors individually.

- **Fix it:** apply the change, **behaviour-preserving** (reuse, dedupe, clarity; never a feature change). Then re-run the light gate (`Repository › light gate`, else `Repository › gate`; solo with neither: the CI config's format, lint and test steps) on what you touched, judged on exit code **and** output.
- **Keep (the author's reason):** record the rationale in the summary and offer a brief code comment or decision note so it isn't flagged again.
- **Skip.**

**Run by a Spawn** (no Engineer to ask): apply nothing that isn't behaviour-preserving and clearly inside its brief; leave everything else untouched and put each ask, with a pre-filled answer, in the report. Any fix it does apply gets the light gate as above.

## May return: nothing

If the diff already fits the request and the codebase and reads well, say so plainly: **No changes needed**, with a one-line why. Do not manufacture findings. A clean pass is a valid, valuable result.

## Relationship to the other skills

- `/teams:critique`: fit with the request and the existing patterns, quality, duplication; dialogic (fix, or ask why not).
- `/code-review`: correctness and bug hunting (Claude Code built-in, when present).
- `/simplify`: applies reuse and simplification cleanups without asking (when present).
- `/teams:document`: adds doc comments to what changed.
- `/teams:prepare-pr`: commits, gate, push and the PR.

A reasonable pre-PR order: critique, document, code review, prepare-pr.
