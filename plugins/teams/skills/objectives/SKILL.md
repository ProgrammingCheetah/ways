---
name: objectives
description: Set a release's objectives and scope with the Engineer: an interview one section at a time, written to the release's objectives.md, challenged by a fresh QA, then signed. The lifecycle's first step; works alone too. E.g. /teams:objectives v0.3
argument-hint: "[version, e.g. v0.3, optionally followed by a line on what it's for]"
---

# /teams:objectives: what a release is for, and what it isn't

The lifecycle's first step. **The Engineer drives:** you interview them one section at a time, write their answers up as `objectives.md`, a fresh top-level QA challenges it, and the Engineer signs it. Everything after builds on it: the design document's goals trace to its objectives and its non-goals carry its Out of scope over, and the acceptance worksheet at release end checks its "Achieved when" column.

Run it in the main session, never in a Spawn: an interview can't be delegated. The content is the Engineer's: you sharpen their words, probe for gaps and play each section back; you never supply an objective, a scope line or a priority they didn't give. So the interview's questions aren't pre-filled; every question after it (step 6) is.

## 0. Config

Read the teams file (`.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`) at the root of this checkout; if absent and this is a secondary workspace, at the root of the main checkout (git: the first path `git worktree list` prints; jj: `jj workspace root --name default`). Use `Repository › docs` (`<docs>`, resolved from the main checkout's root), `owner` and `decisions`, `Release › version format`, the **Teams** qa row (the top-level QA's name and Config), the **Runners** row `top-level QA`, and the **Lanes** judgment row's Effort.

No teams file: say so in one line, and ask once where to save it, pre-filled `.teams/releases/<version>/objectives.md` (in a repo that isn't the Engineer's own, say it would show up in `git status`). QA is then a fresh `teams:designer` named QA, with no team instructions.

## 1. The version

- From the argument; else ask, pre-filled with the next version after the newest folder under `<docs>/releases/`, in `Release › version format`. A line after the version is the Engineer's first answer to **Why**.
- `objectives.md` already in that folder and not in its `signoff.md`: resume. Show its Summary and carry on from the first empty section, or the one the Engineer names.
- In `signoff.md`: this is a change to a signed document. Interview only the sections the Engineer wants changed, then QA on the changes (step 5) and a new signature (step 7). If release documents are signed too, list which parts of them rest on what changed: they go back to the Engineer with it.
- A `scope.md` there instead (written before this skill existed): seed each section from it and play it back; keep the file.

## 2. Read before asking (cheap; skip what's absent)

So your probes are specific, not generic: the project's CLAUDE.md and README (what it is), the previous release's `objectives.md` (its Out of scope and Later column) and its design document's `## Parked (later releases)`, the newest `rulings-*.md` and `Repository › decisions` entries, and the Optional items in `<docs>/queue.md`. Never whole source files.

## 3. The interview

One section per message, in this order. Each message is one short open question, plus at most two probes drawn from step 2 or from earlier answers (e.g. "v0.2 parked offline mode: in or out?"). The Engineer answers in their own words, and may answer several sections at once, say "skip" (that section reads "— none stated") or say "done" (every remaining section reads "— none stated"). Objectives can't be skipped.

1. **Why:** who it's for, the problem or opportunity, why now.
2. **Objectives:** the outcomes this release delivers. Once they're listed: which are musts, and if time runs short, which should goes first. More than about seven: ask whether it's two releases.
3. **In scope:** what the release includes, each line tied to an objective.
4. **Out of scope:** what it deliberately leaves out, why, and later or never. Probe the near misses: what a user would expect next to the In scope lines.
5. **Constraints:** a deadline or timebox, what must not break, technology, compatibility, budget.
6. **Assumptions and risks.**

After each answer, **play it back** as it'll be written, sharpened, and ask for a yes or a correction before moving on:
- One outcome per objective. A feature given as an objective goes under In scope, and you ask which outcome it serves.
- Each "Achieved when" is a check the Engineer could run by hand. A vague word ("fast", "simple", "secure") gets a question for the number or the observable.
- Each In scope line serves an objective. One that serves none: ask whether an objective is missing or the line is out.
- A "how" answer (a technology, a schema, an API shape) goes to **Questions for the design**, said so in one line. The interview stays at what and why.
- A one-way door (a security boundary, the shape of stored data, a permission, a published contract) goes to **One-way doors seen**, flagged when first seen.

## 4. Write it

Write `<docs>/releases/<version>/objectives.md` (creating the folder) from `${CLAUDE_SKILL_DIR}/objectives-template.md`, in the Engineer's words, lightly edited, the Summary last. Tell the Engineer in one line where it is and that QA is next.

## 5. QA

Spawn a fresh top-level QA as a subagent (no `name`): `teams:designer`, `model: "opus"`, the judgment Effort when one is set, named from the qa row. When **Runners** assigns `top-level QA` to a runner, run it there instead, as `${CLAUDE_SKILL_DIR}/../orchestrate/runners.md` says. Brief it from `${CLAUDE_SKILL_DIR}/../orchestrate/brief-template.md` as Document QA, leaving out the workspace lines (it reads by absolute path and changes nothing), with the qa team's config file sections for Every role and Top-level QA as its Team instructions (the orchestrate skill's section 7). It reads `objectives.md`, the project's CLAUDE.md, the previous release's objectives and design document, and the rulings and decisions. Its report is `## QA challenges`, each with a source, checking that:

- every objective is an outcome with a priority and an "Achieved when" someone could run by hand;
- every In scope line serves an objective, and every objective has at least one In scope line;
- nothing In scope contradicts Out of scope, a constraint, or a recorded ruling or decision (cited);
- the musts fit the constraints, or the file says what gives;
- each assumption it can check against the repo or a source is checked; the rest are marked unverified;
- one-way doors the release will plausibly touch are named.

A challenge proposes, with why; QA never rewrites the Engineer's words. On a change to a signed file, QA reads the whole file and challenges only what changed and what it touches.

## 6. Resolve

Paste QA's report under `## QA challenges`, and resolve each challenge by number under `## QA resolutions`:
- **Resolved:** wording, a traceability link or a sharper check that doesn't change what the Engineer meant; fix it in place.
- **Routed to the Engineer:** anything that changes an objective, a scope line, a priority or a constraint; it becomes a question under **Questions for the Engineer**, pre-filled `Claude's answer (<date>; the Engineer adjusts)`.
- **Rejected:** with a source.

Check your resolutions against the report before the Engineer sees them.

## 7. The Engineer signs

In one sitting: the Summary, the questions, and where to look (the path and section titles). Write each answer under its **Engineer:** line and apply it. When the Engineer says it's signed, fill `## Signed` (the date, their words), then take `git hash-object <file>` (never `-w`) and record the file name, id and date in `signoff.md` in the release folder, creating it if missing. Any later edit changes the id: it's a change to a signed document (step 1).

Then:
- **Under the Orchestrator:** a status note in `queue.md`; with `Repository › owner` = engineer, the documents go into git as the orchestrate skill's section 4 says; the design document is next, and its author reads this signed file.
- **Alone:** say where the file is, and stop.

## Guardrails

- Never invent an objective, a scope line or a priority, and never decide a design question here: park it.
- Never sign for the Engineer. "Looks good" while questions are open isn't a signature: ask.
- One section per message during the interview; never paste the whole template at the Engineer.
