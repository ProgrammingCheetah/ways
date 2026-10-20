---
name: objectives
description: Set a release's objectives and scope with the Engineer: an interview one section at a time, written to the release's objectives.md, challenged by a fresh QA, then signed. The lifecycle's first step; works alone too. E.g. /teams:objectives v0.3
argument-hint: "[version, e.g. v0.3, optionally followed by a line on what it's for]"
---

# /teams:objectives: what a release is for, and what it isn't

The lifecycle's first step. **The Engineer drives:** you interview them one section at a time, write each answer into `objectives.md` as it's confirmed, a fresh top-level QA challenges the file, and the Engineer signs it. Everything after builds on it: the design document traces to it, the last milestone's QA runs its musts' checks before the release PR, and the acceptance worksheet at release end runs every check.

Run it in the main session, never in a Spawn: an interview can't be delegated. The content is the Engineer's: you sharpen their words, probe for gaps and play each section back; you never supply an objective, a check, a scope line or a priority they didn't give. So the interview's questions aren't pre-filled; every question outside it is.

## 0. Config

Read the teams file (`.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`) at the root of this checkout; if absent and this is a secondary workspace, at the root of the main checkout (git: the first path `git worktree list` prints; jj: `jj workspace root --name default`). Use `Repository › docs` (`<docs>`, resolved from the main checkout's root), `owner`, `vcs`, `main`, `decisions`, `style`, `config dir` and `workspaces`, `Release › version format`, the **Teams** qa row (the top-level QA's name and Config), the **Runners** row `top-level QA`, and the **Lanes** judgment row's Effort.

No teams file: say so in one line. Test whether the repo is the Engineer's own as `${CLAUDE_SKILL_DIR}/../orchestrate/first-run.md` step 2 says, then ask once, in one round: the version (step 1) and the docs folder, pre-filled `.teams` (the teams file's default, so a later first run finds it). In a repo that isn't the Engineer's own, also offer to add the docs folder to the repo's exclude file, pre-selected yes; that line is your first write, before any other file and with no jj command in between, as that file's step 3 says. QA is then a fresh `teams:designer` named QA, with no team instructions.

## 1. The version

- From the argument; else ask, pre-filled with the next version after the newest folder under `<docs>/releases/`, in `Release › version format`. A line after the version is the Engineer's first answer to **Why**.
- **A small release** (the Engineer calls it one, e.g. a patch) gets no interview: write a one-paragraph `scope.md` (version, scope in the Engineer's words, what's out), unsigned and without QA; under the Orchestrator, add a status note in `queue.md`; then stop.
- **`scope.md` already there:** that release runs on it to the end; say so and stop. Its design is checked against `scope.md`'s scope and what's out, and its acceptance against the design's Features' "done when".
- **`objectives.md` there, not in `signoff.md`:** resume where it stopped. One of the six interview sections still holding the template's placeholder: carry on from the first such section. All six filled, the Summary still a placeholder: step 4. Summary written, no QA challenges: step 5. Challenges, no resolutions: step 6. Otherwise: step 7.
- **`objectives.md` in `signoff.md`, its id matching the row:** a change to a signed document. Add `## Earlier rounds` at the end of the file if missing, and move into it, as `### Round <n> (signed <date>)`, the contents of the QA challenges, QA resolutions and Signed sections, leaving those sections empty; copy into that entry the signed text of each section before you first edit it. Interview only the sections the Engineer wants changed, then run steps 5 to 7.
- **In `signoff.md`, its id no longer matching:** with a round open (the newest `### Round` entry and `## Signed` empty), a change under way: ask which sections are still to change and carry on, then no QA challenges: step 5; challenges, no resolutions: step 6; otherwise step 7. With no round open, the file was edited outside this skill: tell the Engineer, then open a round as above, taking the signed text from VCS history where it's there. If release documents are signed too, list which parts of them rest on what changed: they go back to the Engineer with it.

## 2. Read before asking (cheap; skip what's absent)

So your probes are specific, not generic: the project's CLAUDE.md and README (what it is), the previous release's `objectives.md` (its Out of scope and Later column) and its design document's `## Parked (later releases)`, the newest `rulings-*.md` and `Repository › decisions` entries, and the Optional items in `<docs>/queue.md`. Never whole source files.

## 3. The interview

Create `<docs>/releases/<version>/objectives.md` (and the folder) from `${CLAUDE_SKILL_DIR}/objectives-template.md`, header filled, before the first question. Then one section per message, in this order. Each message is one short open question, plus at most two probes drawn from step 2 or from earlier answers (e.g. "v0.2 parked offline mode: in or out?"). The Engineer answers in their own words, and may answer several sections at once, say "skip" (that section reads "— none stated") or say "done" (every remaining section reads "— none stated"). Objectives can't be skipped.

1. **Why:** who it's for, the problem or opportunity, why now.
2. **Objectives:** the outcomes this release delivers, and how you'd check each one by hand. Once they're listed: which are musts, and if time runs short, which should goes first. More than about seven: ask whether it's two releases.
3. **In scope:** what the release includes, each line tied to an objective.
4. **Out of scope:** what it deliberately leaves out, why, and later or never. Probe the near misses: what a user would expect next to the In scope lines.
5. **Constraints:** a deadline or timebox, what must not break, technology, compatibility, budget.
6. **Assumptions and risks.**

After each answer, **play it back** as it'll be written, sharpened, and on a yes or a correction write that section into the file before moving on:
- One outcome per objective. A feature given as an objective goes under In scope, and you ask which outcome it serves.
- Each "Achieved when" is a check the Engineer could run by hand. A vague word ("fast", "simple", "secure") gets a question for the number or the observable.
- Each In scope line serves an objective. One that serves none: ask whether an objective is missing or the line is out.
- A "how" answer (a technology, a schema, an API shape) goes to **Questions for the design**, said so in one line. The interview stays at what and why.
- A one-way door (a security boundary, the shape of stored data, a permission, a published contract) goes to **One-way doors seen**, flagged when first seen.

## 4. Close the interview

Write the Summary, and "— none" into Questions for the design and One-way doors seen if nothing was parked there. Tell the Engineer in one line where the file is and that QA is next.

## 5. QA

Spawn a fresh top-level QA as a subagent (no `name`): `teams:designer`, `model: "opus"`, the judgment Effort when one is set, named from the qa row. When **Runners** assigns `top-level QA` to a runner, run it there instead, as `${CLAUDE_SKILL_DIR}/../orchestrate/runners.md` says. Brief it from `${CLAUDE_SKILL_DIR}/../orchestrate/brief-template.md` as Document QA, leaving out the workspace lines, with the qa team's config file sections for Every role and Top-level QA as its Team instructions (the orchestrate skill's section 7). Say in the brief: it changes nothing; it reads documents by absolute path and code at `origin/<main>` (git: `git show origin/<main>:<path>`; jj: `jj --ignore-working-copy file show -r <main>@origin <path>`), never the Engineer's working copy. It reads `objectives.md`, the project's CLAUDE.md and style rules, the previous release's objectives and design document, and the rulings and decisions. Its report is `## QA challenges`, each with a source, checking that:

- every objective is an outcome with a priority and an "Achieved when" someone could run by hand;
- every In scope line serves an objective, and every objective has at least one In scope line;
- nothing In scope contradicts Out of scope, a constraint, or a recorded ruling or decision (cited);
- the musts fit the constraints, or the file says what gives;
- each assumption it can check against the code or a source is checked; the rest are marked unverified;
- one-way doors the release will plausibly touch are named.

A challenge proposes, with why; QA never rewrites the Engineer's words. On a change to a signed file, the brief pastes each changed section before and after (from `## Earlier rounds`); QA reads the whole file and challenges only what changed and what it touches.

## 6. Resolve

Paste QA's report under `## QA challenges`, and resolve each challenge by number under `## QA resolutions`:
- **Resolved:** wording or a traceability link that doesn't change what the Engineer meant; fix it in place.
- **Routed to the Engineer:** anything that changes an objective, a check (a number or threshold they didn't give included), a scope line, a priority or a constraint; it becomes a question under **Questions for the Engineer** (replacing its "— none"): a `**Q<n>. <the question>**` line, a `**Claude's answer (<date>; the Engineer adjusts):** <answer and why>.` line, and an empty `**Engineer:**` line.
- **Rejected:** with a source.

Refresh the Summary's question count, and check your resolutions against the report before the Engineer sees them.

## 7. The Engineer signs

In one sitting: the Summary, the questions, and where to look (the path and section titles). Write each answer on its **Engineer:** line and apply it. When the Engineer says it's signed, fill `## Signed` (the date, their words), then take `git hash-object <file>` (never `-w`) and record it in the release folder's `signoff.md`, a table `| File | git hash-object | Signed |`, creating it if missing. A re-sign replaces the file's row and moves the old one under the table's `## History`. Any later edit changes the id: it's a change to a signed document (step 1).

Then:
- **Under the Orchestrator:** a status note in `queue.md`; with `Repository › owner` = engineer, the documents go into git as the orchestrate skill's section 4 says; the design document is next, and its author reads this signed file.
- **Alone:** say where the file is, and stop.

## Guardrails

- Never invent an objective, a check, a scope line or a priority, and never decide a design question here: park it.
- Never sign for the Engineer. "Looks good" while questions are open isn't a signature: ask.
- One section per message during the interview; never paste the whole template at the Engineer.
