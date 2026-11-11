# Zuri's AI Framework

Draft v0, 2026-10-27. Method: Zuri's AI Framework. Implementation: teams 2.0 (the pack) and `zaf` (the binary), both in this repo. Examples are Zurfur's; the pack is generic and every project specific lives in its manifest (`.zaf.toml` or `.zaf/config.toml`). Written from the week's measurements (`~/.claude/projects/-home-zuri/memory/project_claude_budget_strategy.md`) and the rulings of this session. The Engineer rules every open decision in §11; the pre-filled answers are Claude's.

## 1. Principles

1. **Subscription only.** Opus 5.5 and Sonnet 5.5 on the Max plan. Gemini 4 Argon through the `agy` runner. Free GitHub tooling. No API spend, no other paid models.
2. **Claude develops and spawns. Nothing else.** Review, scanning, bookkeeping and dispatch are bots, scripts or Gemini.
3. **State lives on the board.** Never in a session's context. Every role can die and a fresh one resumes from the board.
4. **Every role is short-lived and bounded.** Each has a scope, inputs, outputs, stop conditions and a budget (§3). No role is resumed; its successor reads the board.
5. **Iterate only where the Engineer cares.** Design, visible increments, rulings (§6). Building is autonomous.
6. **Tools before turns.** Looking is done by tools (§7). Models think and change.
7. **Tickets are exact.** A task that needs interpretation is a planning defect, not a programmer's problem.
8. **The zip test.** Zip the project with its configs, unzip it beside a fresh Claude Code with only a login, run `zaf setup`, and the pipeline behaves exactly the same. Nothing outside the project is depended on except declared, pinned binaries, one credential and one named exception, the machine's zaf home `~/.zaf/` (§7.6).
9. **Ask, don't read.** A domain question goes to a short-lived keeper and comes back as a cited answer. No role carries the design in its context; no role guesses.
10. **zaf is project-blind.** No project word in the binary, no assumed language, VCS, forge or design source. A project is its manifest (§7.7).

## 2. Objects

| Object | Where | Meaning |
|---|---|---|
| Release | board milestone `pre-alpha-v0.4.0` + signed `objectives.md` + design documents on the `understand` branch | the unit of shipping; closes with a tag on `main` |
| Epic | issue of type Epic, in the release milestone, signed once approved (`zaf epic sign`, which an edit to its body voids) | one feature or concern; groups its tasks; no code of its own; its stage is derived on read |
| Task | sub-issue of an Epic, type Task or Bug, body in the template (§4.3) | the unit of building; one programmer, one PR |
| Spike | sub-issue of type Task labelled `spike` | the unit of discovery; output is a proposal, never code on a branch |
| Design gap | sub-issue labelled `design-gap` with a proposed corpus diff | the build found the corpus wrong; building continues around it |
| Question | any issue labelled `question`; `door` when it is a one-way decision | waits for the Engineer; the spawner skips what it blocks |
| Instance | one run of a role, numbered on its epic (`instance 3`) | leaves exactly one status comment |

Labels on tasks: `ready`, `claimed`, `blocked`, `question`, `done`. Branches: one integration branch per release, `feature/<version>`; one branch per task, `feature/<version>-<epic-slug>-<task-number>`; PRs target the integration branch; the release merges into `main` by squash and is tagged.

The board is a private GitHub Issues repo in the ZurNetwork org (§11.1), with an Epic issue type added at the org level. ZurFur is public, so its tickets would leak design; when ZurFur goes private the issues transfer into it.

## 2.5 The pipeline: phases
Every release runs the phases in order. Each has one owner, one artifact, one exit criterion and at most one point where the Engineer is in the loop. A patch release runs phases 0 and 1 as a paragraph each.

| Phase | Name | Owner | Input | Artifact | Exit |
|---|---|---|---|---|---|
| 0 | Objectives | Engineer + design session | the corpus, the last release | `objectives.md`, signed | signed |
| 1 | Architecture | Architect instance, then a fresh architecture critique | objectives, the corpus | the **architecture package**: module map (where things go), interface list (traits, ports, types, routes, lexicons and protos, each with an id), data-shape changes with doors flagged, the architecture diagram (layered view + fixture tree), the epic list derived from the modules; filed as corpus pages | the Engineer approves the diagram and the epic list |
| 2 | Planning | planning instance + challenger per epic | the architecture package | tasks that cite module and interface ids | every epic `signed` |
| 3 | Implementation | task phases (§5.2) | tickets, `zaf brief` | merged task PRs on the integration branch | every task `done` |
| 4 | Verification | the review layer, `zaf verify`, mutants | PRs, the integration branch | findings resolved, mutant score above the threshold | no open must-fix |
| 5 | Release | release instance | the milestone | per-must results, release reviews, corpus pages synced to as-built, the squash into `main`, the tag | tagged |

**Adhere, adjust through tickets.** Implementation follows the architecture package. A change within a task's scope that keeps every interface is the programmer's to make. A change that moves a module boundary, alters an interface or touches data shape is a `design-gap` ticket: a short Architect instance rules on it against the package, updates the page, and the planning instance re-cuts the affected tasks. Nothing drifts silently; `/corpus-sync` at phase 5 reconciles the pages to what was built and names any gap that slipped.

**Adherence is checkable.** Because interfaces carry ids, `zaf verify` can check that a task's declared interfaces exist where the package says (the trait, the route, the table), deterministically, before any reviewer reads the diff.

## 3. Roles: scope and lifecycle

Each role is defined by the same eight lines. "Never" is the scope fence.

### 3.1 The Engineer
- **Purpose:** decide, sign, react. Product owner and designer.
- **Model:** attended session, any model, kept small because it holds no build state.
- **Input:** `/my-queue` read from the board; previews; questions.
- **Output:** rulings and signatures as ticket comments; design in the corpus.
- **Lifecycle:** continuous; sessions are short and restartable.
- **Budget:** reads no code, no reports, no briefs (R-247 to R-249 stand).
- **Never:** orchestrates, briefs an agent, reviews a PR.

### 3.2 The Spawner
- **Purpose:** launch instances. The only successor of the Orchestrator.
- **Model:** none. A script (§7, `zaf spawn`), on a timer or on demand.
- **Input:** the board, the slot lock (`build-slot.sh`), the budget guard.
- **Output:** instances, each logged as a comment on its epic.
- **Lifecycle:** one pass per tick: for a signed epic with no tasks, launch a planning instance; for a planned epic, a challenger; for a milestone whose epics are all done, a release instance. It launches no team instance: ready tasks are started by their phases (§5.2).
- **Budget:** two slots; batch size two; the guard (§8) lowers both.
- **Never:** reads a report, writes a brief, answers a question, chooses what to build.

### 3.3 The Planning instance (the Epic Designer)
- **Purpose:** turn a signed epic into exact tasks.
- **Model:** Opus 5.5, effort xhigh.
- **Input:** the epic, the release's objectives and design documents, the corpus pages they cite, `nodes chain` for the areas touched.
- **Output:** Task and Spike sub-issues in the template, with blocked-by links; a comment listing what it could not scope and why.
- **Lifecycle:** spawned when an epic is signed and has no tasks, or when the epic carries a `ruling` comment newer than its last plan (re-plan: close, rewrite or add tasks; never touch a claimed one). Ends when the tasks are filed.
- **Budget:** one instance; 80 turns.
- **Never:** writes code, decides a door, scopes a task larger than M (it splits), leaves a task without acceptance criteria.

### 3.4 The Challenger
- **Purpose:** find the tasks that are not exact.
- **Model:** Opus 5.5, effort xhigh (§11.6), read-only. Once per epic, so the cost is small and the judgment is the best available.
- **Input:** the epic's tasks, the same documents the planner read.
- **Output:** comments on tasks (split, missing interface, untestable acceptance, hidden door), `question` labels where the Engineer must decide.
- **Lifecycle:** spawned after a planning instance; ends when every task has a comment or a pass. The epic is ready for the Engineer's `signed` label after this.
- **Never:** rewrites tasks itself; it argues, the planner re-plans.

### 3.5 The Team instance (retired by #421 ruling 16)
The spawner no longer launches a team instance (#449): a task is built by its phases (§5.2), each phase action starting the worker it needs. What follows is the old design, kept until #429 rewrites this page.

- **Purpose:** build a batch of tasks end to end and merge them.
- **Model:** Opus 5.5 lead, effort xhigh.
- **Input:** `zaf board ready <epic>`; `zaf brief <task>` for each claimed task.
- **Output:** merged task PRs on the integration branch; ticket states; one status comment on the epic.
- **Lifecycle:** claims up to four ready tasks; spawns one programmer per task (parallel within the slot budget); for each PR waits for the review layer (§5.4), runs `zaf findings`, spawns fix rounds; merges clean PRs with `zaf slice merge`; posts its status; ends. Stops early on a question, a door, a third failed round, or its turn cap.
- **Budget:** 200 turns; compaction window 150k; at most two programmers at once.
- **Never:** reads the corpus (it asks), answers a domain question itself (it asks), reviews code itself, merges into `main`, resumes a previous instance's context, answers a design question.

### 3.6 The Programmer
- **Purpose:** one task, green.
- **Model:** Sonnet 5.5 effort high by default; Opus 5.5 when the task is security-flagged, when the ticket says `model: opus`, or on the third round.
- **Input:** `zaf brief <task>`; `zaf ask` for anything beyond it.
- **Output:** commits on the task branch that pass `zaf check <task>` and the light gate; a one-paragraph hand-back.
- **Lifecycle:** started by a task's implement phase (§5.2); ends on hand-back. A fix round is a fresh programmer with the task plus `zaf findings <pr>`.
- **Budget:** 60 turns; two Sonnet rounds, then one Opus round, then the task goes to `question`.
- **Never:** touches a path it does not own, pushes, opens a PR, reads another task, edits the corpus.

### 3.7 The Review layer
- **Purpose:** every finding that is not a door, at zero Claude tokens.
- **Members:** CI gate (exactly CI); CodeQL default setup; Dependabot security updates; CodeRabbit with a tuned `.coderabbit.yaml`; Gemini critique on every PR (two passes: right thing built, built right); Gemini security review on security-flagged tasks; cargo-mutants and Stryker nightly on the integration branch.
- **Opus** reviews code only on doors (§11.2).
- **Lifecycle:** event-driven on PR open and push; mutants on a timer.
- **Never:** blocks a merge on taste; findings are must-fix or notes, as today.

### 3.8 The Release instance
- **Purpose:** close a version.
- **Model:** Opus 5.5 lead; its checks run on Gemini.
- **Input:** the milestone's epics, `objectives.md`, the integration branch, last night's mutant report.
- **Output:** per must, pass or a filed task; the compliance, architecture and security release reviews as comments on the milestone; the squash merge into `main`; the tag; a retrospective comment from board statistics.
- **Lifecycle:** spawned when every epic in the milestone is done; files tasks and ends if anything fails (the spawner sends them through task phases and tries again); merges and tags if everything passes.
- **Never:** edits code, waives a must.

### 3.9 The Design session
- **Purpose:** where iteration lives. Objectives, design documents, corpus pages, canvases, boardroom, design decisions.
- **Model:** the Engineer's attended session; `/teams:objectives`, `/design-decision`, `/corpus-sync`, `/boardroom` as today.
- **Output:** signed objectives, design documents, DD pages, canvases. The planning instance consumes these.
- **Never:** holds build state; a status comment on an epic is the most it reads.

### 3.10 The Domain keeper
- **Purpose:** answer one domain question from the corpus, the settled rulings and project memory, with citations. The single source of domain truth at run time.
- **Model:** the local `zuri` lane (qwen3-coder, zero tokens) first; Haiku 5.5 or Sonnet 5.5 when the local answer fails the citation check (§7). Read-only.
- **Input:** one question, the design index, `rg` over the corpus, the rulings files, the memory index.
- **Output:** at most 150 words, the page ids it rests on, one verbatim quote per claim; or `not settled`, which the asker turns into a `design-gap` or `question` ticket. Every ask and answer is appended to the ask log.
- **Lifecycle:** spawned by `zaf ask` from any role; ends on its answer. Seconds, not minutes.
- **Budget:** 10 turns.
- **Never:** decides, infers beyond the pages, answers without a quote, writes anything but the log.

The ask log is the corpus's own analytics: the pages consulted most, the questions that keep returning (a question that keeps coming back wasn't settled and deserves a DD, as today's rule says), and the gaps. The release instance's retrospective reads it.

### 3.11 The Architect
- **Purpose:** phase 1. Turn signed objectives into the architecture package: where things go, the interfaces, the data-shape changes, the diagram, the epic list.
- **Model:** Opus 5.5, effort xhigh. Once per release, plus short instances for design gaps.
- **Input:** `objectives.md`, the corpus (the architecture pages, the DDs, the conventions), `nodes tree` of the code.
- **Output:** corpus pages for the package, with ids on every module and interface; the diagram as an HTML page, large type, as the diagram ruling says; the epic list with each epic's modules.
- **Lifecycle:** spawned when objectives are signed; a fresh critique instance challenges the package (coupling, missing interfaces, hidden doors, a module with no owner); the Engineer approves; ends. Re-spawned, short, on a `design-gap` ticket.
- **Budget:** 120 turns for the package; 30 for a gap.
- **Never:** writes tasks (that is phase 2), writes code, decides a door.

## 4. Workflow A: creating tickets

### 4.1 Steps
1. **Objectives.** `/teams:objectives <version>`: interview, QA challenge on Gemini, the Engineer signs. Unchanged.
2. **Architecture (phase 1).** The Architect's package replaces the TDD and the delivery plan's module section: module map, interface list with ids, data-shape changes, the diagram, and the **epic list** derived from the modules. The PDD stays the product side of the design documents.
3. **Epic list approval.** The Engineer approves the list as plain questions (iteration point 2 in §6). Approved epics are created on the board in the milestone.
4. **Planning.** Per epic, a planning instance writes the tasks (§3.3).
5. **Challenge.** A challenger comments (§3.4). Questions reach the Engineer.
6. **Sign.** The Engineer runs `zaf epic sign <epic>`, which records the hash of the epic's body as it reads now; an edit voids it until the epic is signed again, and an instance cannot sign. Tasks whose blockers are closed become `ready`.
7. **Re-plan on ruling.** A comment labelled `ruling` on an epic, at any time, triggers a fresh planning instance before the next task phase starts.

### 4.2 Rules of exactness
- One behaviour per task: the Goal is one sentence with no "and". A ticket is XS, S or M: S expects a pull request under 200 lines and M under 400; anything larger is a planning defect and splits. L, XL and E are costs of the size table and of a release launch, not ticket sizes. These are planning targets read against the merged pull request, not gates.
- Every task names its corpus pages by id, and the module and interface ids from the architecture package it implements. A task that needs a page or an interface it does not name is a design gap.
- Acceptance criteria are checkable by `zaf verify`: named tests, named HTTP outcomes, named UI states.
- The security trigger flag follows `.claude/TEAMS.md › Reviews › security triggers`.
- Interfaces between tasks are written down before either task is ready; the one that defines the interface blocks the one that consumes it.
- A migration that changes stored data shape is a door and gets a `door` question before its task is ready.

### 4.3 The task template
```
Goal: <one sentence, one behaviour, the user-visible or system-visible outcome>
Scope: <the capability this task changes, in words; a file or folder only as a place to start>
Out of scope: <what it leaves alone, and the ticket that has it>
Interfaces: <new or changed ports, types, routes; who defines, who consumes>
Design pages: <ids>
Acceptance:
  - <checkable statement>
  - <checkable statement>
Tests: <unit and integration tests to add, by name>
Security trigger: yes <which> | no
Model: sonnet | opus
Size: XS | S | M | L | XL | E
Blocked by: #<n>, #<m>
```

The typed fields take only the values listed; `zaf board file` and `zaf check` refuse any other, naming the field and the allowed values. Sizes are ordered XS to E; a planner files XS, S or M.

## 5. Workflow B: building from tickets

### 5.0 Topology: who is whose parent
- **Instances are processes.** The spawner launches each planning, challenger, team and release instance as its own headless `claude -p` (or `agy -p`) process under the logged-in Max account, with `--model`, `--max-turns`, `--output-format json` and the role's standing brief. The script is the parent. No model holds another instance's output; it goes to the log and the board.
- **A programmer is started by its task's implement phase (§5.2).** There is no team lead: the phase action starts the programmer, and its one-paragraph hand-back is all that comes back. A fix round is a fresh model started by the `on-fail` action.
- **Keepers are processes.** `zaf ask` launches one wherever it is called, so a programmer can ask without nested spawning.
- **Nothing is ever resumed.** A new instance is a new process reading the board.
- **The interface is the only door.** Each role's permissions allow `zaf`, the read-only commands and its editor, and deny raw `jj git push`, `gh pr merge`, workspace commands and the like. The script does those inside its verbs, so a model cannot do them by hand even by mistake.
- **Billing stays on the subscription.** Headless `claude -p` under the Max login counts against the weekly limit like a session. The Agent SDK may be metered differently; settle that before using it for the spawner.

### 5.1 The spawner's tick: the phase scheduler
The spawner is a phase scheduler. A task is an ordered list of phases, each one an action, and the list is declared in the manifest for the task's type. Each tick derives every task's standing from the forge and the phase run folders (the queue is never stored), runs the actions that need no model, launches the waits (review and CI run with no model), and starts at most one model, the ready phase that has waited longest and fits the budget. A tick never waits for the model it starts. Planning, the challenger and the release launch join the same order as epic launches. A task with a pull request is held by its forge facts, not by an instance. A tick runs every five minutes (`zaf spawn --watch`) and whenever a phase ends.

The phases, the kinds, the standing, the claims, the tick's steps, the hooks and the triggers are written once, in `docs/zaf-actions.md` §5.1; read that section, not this one, for how the tick works.

Every model launch is `claude -p` (or `agy -p`) with a one-line prompt naming the role and the epic or task, plus the role's standing brief from the pack. The spawner records the launch in the run folder of the phase, and the phase holds its share of the budget until its process exits.

### 5.2 A task's phases, in place of a team instance's steps
The phase scheduler of §5.1 replaces the team instance's seven steps. Its phase actions do what those steps did: implement starts a programmer on the brief, `ci` and review are waits with no model, a failed wait starts a fresh model on the findings (its `on-fail` action), and `merge` runs `zaf do slice-merge` with no model. Each phase is an action, so the lead that coordinated them (§3.5) is no longer launched. The escalation ladder of §5.4 still decides which model a fix round gets. See `docs/zaf-actions.md` §5.1 for the phases, the kinds and the tick.

### 5.3 Merge conditions
A task PR merges into the integration branch when CI is green, CodeRabbit and Gemini have no open must-fix, and for a security-flagged task the Gemini security review passed. The integration branch merges into `main` only by the release instance, after every must passes and the release reviews are clean, by squash, then the tag.

### 5.4 The escalation ladder, in one place
| Event | Response |
|---|---|
| gate fails | fix round, same ladder |
| two Sonnet rounds fail | Opus round |
| Opus round fails | `question` on the task; instance ends |
| challenger finds a door | `door` question; epic waits |
| build finds the corpus wrong | `design-gap` task; building continues elsewhere |
| release check fails | tasks filed; release instance ends; retried after they merge |
| budget guard trips | Sonnet-only, one slot, batch two |

## 6. Iteration points: the only places the Engineer is in the loop

| # | Point | Form | Everything else |
|---|---|---|---|
| 1 | Objectives | the interview, signed | |
| 2 | Epic list | plain questions on the delivery plan | |
| 3 | Epic sign-off | questions from the challenger, then the `signed` label | task bodies are never read |
| 4 | Doors | a `door` question, answered on the ticket | migrations, data shape, identity, money |
| 5 | Previews | every merge into the integration branch deploys Storybook and staging; the Engineer's reaction is a comment; `ruling` triggers a re-plan | status comments exist but need no reading |
| 6 | Design gaps | ruled on the ticket, applied with `/corpus-sync` | |
| 7 | Release | the release instance's per-must result; the Engineer accepts the version | retros are comments, read if wanted |

Not iterative, by design: task internals, fix rounds, merges into the integration branch, instance boundaries. A design that changes mid-epic goes through point 5, which costs one planning instance, not a conversation.

## 7. Tools: the verb set

One binary, `zaf` (§11.5), one page of help pasted into every standing brief, every verb terse by default with `--json` for the spawner and `--more` for depth.

| Verb | Replaces | Output |
|---|---|---|
| `zaf board ready|claim|report|question|ruling|file` | hand-crafted `gh` queries and JSON | six-line answers; `file` opens a task under an epic in the task template, once per title; `question` sets the `question` label only on a task that is `claimed` or `ready` (on an epic or any other task it posts the comment and leaves labels alone); `ruling` closes the question and returns its task to `ready` unless the task is `done` or an epic (the same ruling again changes nothing) |
| `zaf ask -t <task> "<kind>: <question>"` (up to five) | reading corpus pages into a working context; asking the lead | a keeper's cited answer; the tool checks each quote exists verbatim in the cited page and re-asks on Haiku or Sonnet if the local model fails; a question whose answer failed only because a cited page could not be read prints `q1: check failed — page <id> could not be read (design dir or index misconfigured)` and the verb exits 2, so a misconfigured design source is never read as `not settled` |
| `zaf brief <task>` | the opening burst of rg, cat, ls (the biggest look-around cost) | one context pack: ticket, a `## Rulings` section (every question raised on the task, `#<q> <text>` then `ruling: <text>` or `open`, newest last; `— none` when there are none), named corpus pages, `nodes chain` of each place the scope names to start, the style page, the help page |
| `zaf check <task>` or `zaf check pr:<n>[:<task>]` | reading PR threads turn by turn; hand-run reviews; the LLM QA's checklist | runs the step list configured for the target's kind (task: acceptance checks, run inside the task's slice workspace and refused when the slice is not open; PR: CI, CodeQL, CodeRabbit, Gemini critique, Gemini security when flagged; milestone: objectives check, release reviews, mutant score), waits inside the verb, and returns one deduplicated findings list tagged by source with an exit code: clean, must-fix present, or a step failed. `--status` reads without running. Steps are configured in the manifest with a kind (wait on CI, wait for a bot, run a role on a runner, run a command, read scanner alerts), a `when`, a timeout and a must flag; independent steps run in parallel |
| `zaf changes <task>` | raw `jj diff` (1.2M tokens last week) | stat first, hunks on request |
| `zaf verify <task>` | ad-hoc test runs and reading their logs | the task-kind steps of `check`: pass or fail per acceptance line and per declared interface id, output captured to a file first; `zaf check <task>` is its alias |
| `zaf slice open|pr|merge|drop` | the machinery lane | workspace, branch, PR, merge, cleanup, each one call; `pr` and `merge` publish the workspace's latest commit, hand-made or not, by moving the slice bookmark forward to it before the push; `pr` and `merge` merge the integration branch as the remote has it, after a fetch |
| `zaf spawn --once --version <v>` | the Orchestrator | one tick of §5.1: gives each open epic at most one launch by its stage while a slot is free (planning for a signed epic with no tasks, the challenger for a planned one; nothing for an unsigned or done epic, one with an open question, or a challenged one, whose tasks their phases build and no team instance is launched), sets `ZAF_INSTANCE` and `ZAF_VERSION` for them, records launch and exit on the epic and in the instance's run folder; a slot is one running instance whatever its batch, read from the process each run folder records (the instance's own once it starts, so a killed tick loses nothing); a launch that ended with nobody to record it is recorded `lost`, and a task claimed by an instance that no longer runs returns to `ready`; the loop is a timer's job |
| `zaf asklog` | nothing today | the ask log: top pages, recurring questions, gaps |
| `zaf status` | reading the board and the process list by hand | each open epic with its stage, board, running instances, slots, guard, open questions, one screen |
| `zaf epic list\|sign <epic>` | a `signed` label set by hand | `list`: one line per open epic, `#<n> <stage> <done> of <total>  <title>`, the stage derived on read (unsigned, signed, planned, challenged, done; ruling 48); `sign`: records the hash of the epic's body as it reads now, so an edit voids it, and is refused when a running instance is this process or above it |
| `zaf report` (was `burn`) | the ad-hoc script of 2026-10-27 | the weekly table: tokens per role, findings by source, rounds per task, cycle time |
| `zaf guard` | nothing today | the budget state the spawner reads (`.zaf/guard.json` from `.zaf/local.toml` `[budget] weekly_usd` and the week's spend) |
| `zaf doctor` / `zaf init` / `zaf setup` / `zaf profile init` | hand-checked tool versions; hand-written config; hand-made profiles | the manifest validated and every `[requires]` pin verified; a minimal manifest by asking; `.zaf/` laid out, a git-sibling design cloned, a worksheet for what it cannot install; the hermetic profiles regenerated byte-identical |
| `zaf update [--check]` | downloading the release archive by hand | replaces the running binary with the newest `zaf-v*` release, read from GitHub each time (nothing stored): the platform archive, its sha256 checked, the new binary's `--version` probed, then renamed over the old one, so a failure leaves the old binary; up to date prints nothing; `--check` only says whether a newer release is out (exit 1 when one is); a platform with no archive is told the `cargo install --git … --tag` line |

Rules: deterministic; exit code plus structured output, never a model's reading; a failure stays inside its verb. Exit codes, the same for every verb: `0` ok, `1` refused by the domain (not ready, not claimed by you, conflict), `2` a backend failed or is missing, `64` listed but not built. Environment the verbs read: `ZAF_INSTANCE` (who is running, as the board prints it: `team instance 12 on #16`) and `ZAF_VERSION` (the milestone's version for branch patterns); the spawner sets both on every launch. *As built 2026-10-28 (ways #16).*

### 7.6 Portability: the zip test
The pipeline may depend on nothing outside the project except the binaries it declares, one credential and one named exception: the machine's zaf home (`~/.zaf/`, or `ZAF_HOME`), laid out like a project's `.zaf/`, holding machine-wide defaults in `config.toml`, the machine's actions and the ledger. A project whose steps resolve to a machine action depends on that machine, and `zaf doctor` reports each one, so the dependency is visible (actions §2).
- **Never depended on:** `~/.claude` in any form (global CLAUDE.md, `settings.json`, user-scope plugins, personal or project memory folders under `~/.claude/projects/`); absolute machine paths; the user's installed Claude Code configuration. Today's `~/.cache/zurfur-locks/build-slot.sh`, `~/code/zurfur-workspaces` and `~/code/zurfur-design` all fail this: slots move inside `zaf` with a lock dir under `.zaf/`; workspaces become a declared relative location in the manifest; a design source of kind `git-sibling` is cloned beside the code by `zaf setup` only because it is declared, and a project may declare `repo-dir` or `none` instead.
- **Hermetic profiles.** `.zaf/profiles/<name>/` is a complete Claude home generated by `zaf profile init` from the manifest, never hand-edited: `settings.json` (permissions, model, effort, compaction window, no MCP servers), CLAUDE.md (the interface page plus the project's rules), `plugins/` with the teams pack installed through Claude's own plugin commands (the project root registered as a marketplace for `[pack] path`, the ways marketplace at `[pack] tag` otherwise: `claude plugin marketplace add ProgrammingCheetah/ways` unless `claude plugin marketplace list` already shows `ways`, then `claude plugin install teams@ways`, and the version Claude recorded in `plugins/installed_plugins.json` must be the tag's, else init refuses; a copied folder is invisible to Claude), an empty memory. Instances run with `CLAUDE_CONFIG_DIR` set to it. Deleting `.zaf/` and running `zaf setup` regenerates the profiles.
- **A profile carries its stamp.** `zaf profile init` writes `.zaf-stamp.json` into each profile last: the zaf version that generated it and one digest per input it read (`requires`, `gate`, `forbidden`, `roles`, `pack` and `roles/project.md`). The bytes are deterministic, so a rerun with nothing changed writes nothing. `zaf doctor`, `zaf status` and `zaf spawn` compare the stamp with the live manifest and this zaf. A profile with no stamp, one written by another zaf (`profile build is older than zaf (written by zaf <old>)`) or one made from an older manifest (`profile build is older than the manifest (forbidden changed)`) is named: doctor fails on it, status prints it, and spawn refuses to start (exit 1, `--dry-run` included) until `zaf profile init` has run. A stale profile ran instances with yesterday's deny list; the stamp keeps that from being silent.
- **Pins.** `[requires]` in the manifest: zaf, the teams tag, the Claude Code minimum, jj, gh, nodes, agy, the gate tools, each with a version. `zaf doctor` verifies; `zaf setup` installs what it can and hands the Engineer a worksheet for the rest.
- **On the machine, by nature:** credentials (the Claude login as a long-lived token from `claude setup-token` in the secret store, named in `local.toml`; `gh auth`; the Gemini sign-in) and hardware limits (slots, memory caps) in `local.toml`. To verify: how a `CLAUDE_CONFIG_DIR` profile receives the login on the installed version.

### 7.7 The manifest: zaf is project-blind
No project word in the binary, no assumed language, VCS, forge or design source. Everything zaf knows about a project comes from its manifest, the way nodes learns a tree from `NODE.toml`. A project is its manifest. The manifest is `.zaf.toml` at the project root, `.zaf/config.toml`, or both: when both exist they combine, property by property, and `.zaf.toml` wins on a property they both set; `~/.zaf/config.toml` supplies machine-wide defaults below them. The retired `ZAF.toml` still loads when it stands alone, with a warning, until `zaf setup` moves it to `.zaf.toml`; beside a new file it is an error. `.zaf/local.toml` is the top layer, git-ignored and the machine's: of the manifest it may set only `[spawn] slots` and `[gate] slot`, and any other table or key in it (`[forbidden]`, `[requires]`, `[spawn] guard`, `[spawn] budget`, `[runners.*]`, ...) fails the load, exit 2, naming the file and the key, so a file git does not hold never loosens what the project commits. Its own tables, `[budget]`, `[report.prices]` and `[auth]`, are read by their own readers and never reach the manifest. The file read is the one at the project root, also from a task workspace. `zaf doctor` names the manifest files it read, highest layer first, and warns `unread: .zaf/<name>.toml` for any other `.toml` directly inside `.zaf/`.

```toml
[project]        name = "zurfur"            # only for logs and ticket prefixes
[requires]       zaf = "2.0.0"  teams = "teams-v2.0.0"  claude = ">=2.1"  jj = "*"  gh = "*"
[board]          kind = "github-issues"  repo = "ZurNetwork/zurfur-design"   # or kind = "github-projects" with project = 3 (owner = repo's owner, state_field = "zaf state" by default)
[vcs]            kind = "jj"
[forge]          kind = "github"  repo = "ZurNetwork/ZurFur"
[design]         kind = "git-sibling"  repo = "ZurNetwork/zurfur-design"  path = "../zurfur-design"  index = "docs/design-index.md"   # path is relative to the project root; absent = ../<repo name>
[workspaces]     path = "../zurfur-workspaces"
[branches]       integration = "feature/{version}"  task = "feature/{version}-{epic}-{task}"
[gate]           full = "just gate"  light = "just gate-light"  tests = "cargo test -p {crate} {name}"  slot = "build-slot.sh" # optional: the machine's build-slot command prefix; each gate line runs as `<slot> sh -c '<line>'`
[style]          path = "docs/style.md"      # optional, pasted into every brief
[runners.claude] command = "claude -p {prompt} --model {model} --add-dir {root}"  timeout_minutes = 45
[runners.gemini] command = "agy -p {prompt} --add-dir {root} --model gemini-4-argon-high --print-timeout 60m"
[runners.local]  command = "zuri -p {prompt}"
[roles.programmer]  model = "sonnet"  effort = "high"  profile = "lean"  turns = 60
[roles.team]        model = "opus"    effort = "xhigh" profile = "build" turns = 200  batch = 2
[check]          ci_grace_seconds = 120     # optional: how long after a push an empty check list still counts as pending
[check.pr]       steps = ["ci", "codeql", "bot:coderabbitai", "runner:gemini:critique", "runner:gemini:security?flagged"]
[check.milestone] steps = ["runner:gemini:objectives", "runner:gemini:release-reviews", "command:cargo mutants"]
[ask]            ladder = ["local", "haiku", "sonnet"]  max = 5  cache_hours = 24   # a rung names a runner, else [runners.claude] with {model} = the rung
[spawn]          slots = 2  guard = [70, 85, 95]
[isolation]      kind = "process"           # or "container", image = "…"
[env]            CARGO_TERM_COLOR = "never" # optional: variables every gate, test, check and action run gets; "{root}" and "{version}" expand
```

**`[env]`.** Each key is a variable name and each value a string, which may use `{root}` (the absolute project root) and `{version}` (the epic's version). No name may start with `ZAF_`, whatever its case, because those are zaf's own and a manifest setting `ZAF_INSTANCE` would dodge the action modes; `CLAUDE_CONFIG_DIR` is refused too, because zaf sets it for each profile and a manifest pointing it elsewhere would take an instance out of its profile's denies. The check runs when the manifest loads, so a bad table never reaches a child.

**`[gate] slot`.** `[gate] slot = "<command prefix>"` is optional: the machine's build slot. With it every gate level (`full`, `light` and the named `tests`) runs as `<slot> sh -c '<line>'`, so zaf, not the caller, takes the slot. Inside the slot zaf exports `ZAF_SLOT_HELD=1` to the line and everything it starts; a gate run that finds it set runs its line without the prefix, so a gate nested in a held slot takes no second one (`[env]` cannot set it, being a `ZAF_` name). While a slotted run lasts zaf keeps `.zaf/slots/<pid>` (the holder from `ZAF_INSTANCE`, else `the Engineer`; the level; the start time), and `zaf status` prints `slot: <holder> <level> since <time>` for each live record and drops one whose process is gone. The slot command itself is the machine's; zaf owning the lock is §7.6's open question.

Absent sections take the pack's defaults. `zaf init` writes a minimal manifest by asking; `zaf doctor` validates it against the schema; `.zaf/local.toml` overrides only what belongs to the machine.

**Isolation levels, both declared.** `process`: hermetic profiles, lock dirs under `.zaf/`, nothing from the host's Claude (§7.6). `container`: every instance runs in a declared image with the project mounted, the token passed as env, slots mapped to the container's limits; same manifest, same behaviour, the zip test physically enforced. One Runner adapter away; not needed for Zurfur today.

### 7.0 Per-project isolation
The binary and the pack are the only shared things. Everything else lives in the project, and one project's instances never see another's:
- The manifest (`.zaf.toml` or `.zaf/config.toml`), committed: board repo, branch patterns, gate and light gate, slot script, runner commands, corpus dir and index, trigger list, the `check` step lists per target kind, the ask ladder, batch size, slots, the escalation ladder, previews. Absent keys take the pack's defaults, so two projects run the same binary with different pipelines.
- `.zaf/`, git-ignored: `local.toml` (the manifest's top layer for machine facts only: `[spawn] slots` and `[gate] slot`; plus its own tables, the weekly budget `[budget]`, `[report.prices]` and `[auth]`, which their readers take and the manifest never sees; any other setting fails the load, §7.7), `roles/` (per-role project instructions pasted into briefs, what TEAMS › Config did; `keeper.md` and the planning and challenger briefs override the pack's briefs, `project.md` goes into every profile's CLAUDE.md), `profiles/<name>/`, `asklog.jsonl`, `runs/<instance slug>/` (briefs, asks, check results, instance reports, and the launch record: `launch.json`, the holding process in `pid`, the `exit`), `slices/<task>.pr`, `spawn.state` (the instance counter alone), `spawn.lock`, `guard.json`.
- One spawner timer per project, running `zaf spawn --once` from that project's root with that project's slots, budget and board.
- Nothing secret in the manifest, so a public repo can commit it; everything private sits in `.zaf/`, which a private-papers check can guard.

### 7.1 Spawn rules: where repeated spawning gets expensive
Every spawn pays a fixed tax: its turn-one context (58k tokens median this week: system prompt, tool list, CLAUDE.md chain, memory index, skills, brief) times its turns. A lost instance costs nothing more: it frees its slot the moment its process dies, `zaf status` shows its claims as orphaned at once, and the next tick records `lost` on its epic and releases every task it still held (the `claimed` label and lease go, `ready` returns unless the task carries `question` or `done`, and a comment reads `released from <instance> at <time>`), so the next tick can claim it again. The rules that keep the tax small:
1. **Depth one.** A keeper never spawns; an instance never spawns an instance; a programmer never spawns a programmer. A chain pays the tax per hop while the top asker idles at full context. `not settled` ends a chain as a ticket.
2. **Ask in batches.** One `zaf ask` carries every question the role has at that moment; the keeper answers all of them in one process.
3. **Preload what is named.** The brief pack carries the task's named pages, so the common questions never become asks.
4. **Answer from the log before spawning.** `zaf ask` returns a recent identical answer from the ask log without a process.
5. **Lean profiles for short roles.** Keepers and programmers run `claude -p` with a dedicated settings profile: no MCP servers, no skill catalogue, a one-page CLAUDE.md. Turn-one context near 15k instead of 58k.
6. **Identical prefixes.** Same profile, same CLAUDE.md bytes, the brief as a path in the prompt. A differing prefix is a cache write on every spawn (the $700 night).
7. **Wait inside tools, never in turns.** `zaf check` and `zaf slice merge` block in the shell at zero tokens; a model polling at a 240k context pays a full re-read per poll.
8. **Rounds are the hidden multiplier.** Each fix round is a fresh programmer and a fresh tax; the ladder caps at three, and the challenger exists to make round one the last.

### 7.2 The ask and the answer: one format each
**Ask** (`zaf ask -t <task> "<kind>: <question>" ...`, up to five per call; the tool writes the record):
```
from: team#3 / programmer stoat     # who, for the log
for: #412                            # scopes the search: the task's named pages first
context: Owner renames the handle; implementing the rate window   # two lines at most
questions:
  q1: rule  | Is a second rename inside 30 days rejected, and with which status?
  q2: term  | What is a vacated handle?
  q3: exists| Is there a decision on who may rename, Owner only or any Member?
```
Kinds: `fact` (what the design says), `rule` (allowed, required, forbidden), `term` (an entity or word), `exists` (is there a decision), `where` (which page). One sentence each, no code, under thirty words.

**Answer** (what comes back, per question, verified by the tool before the asker sees it):
```
q1: settled
    answer: A second change inside the 30-day window is refused with 429; the window is per account.
    pages: 27852802 (decided)
    quote: "a second change within thirty days is refused with 429 Too Many Requests"
q2: settled
    answer: A handle released by a change or deletion; *.zurfur.app handles stay unresolvable for 30 days.
    pages: 27852802 (decided), 23003138 (decided)
    quote: "the vacated handle enters quarantine for thirty days"
q3: not settled
    next: question ticket on #412 (door: no)
```
Rules: `settled`, `not settled` or `conflict` (two live pages disagree: both ids, a ticket). Sixty words per answer. One verbatim quote per page cited, which the tool checks against the page; a failed check discards the answer and re-asks on the next model in the ladder. Only pages whose index status is `decided` or `current`; a superseded page is never cited. No advice, no code, no "probably". The log line holds the ask, the answer, the model that answered and the check result. A page the tool cannot read is not a failed quote: when every rung's checked answer failed only on pages the design source could not read, each question prints `q1: check failed — page 21594113 could not be read (design dir or index misconfigured)` in place of an entry, its log line carries the verdict `check failed`, and the verb exits 2; `zaf asklog` lists these under `Check failed:`, apart from `Failed:` (rungs that gave no answer) and `Not settled:`. A `not settled` is only the keeper citing nothing, and exits 0.

### 7.3 Context profiles: a saved context per role
Turn-one context is what the process loads before the brief: system prompt, tool definitions, CLAUDE.md chain, memory index, skill catalogue. A profile fixes all of it, so every spawn of a role starts from the same bytes (a cache hit) and loads only what the role may use. Mechanism: a config directory per profile under the project's `.zaf/profiles/` (`CLAUDE_CONFIG_DIR`), each with its own settings, CLAUDE.md, memory folder and enabled plugins; the spawner sets it per role. `zaf profile init` puts the pack in through Claude's own plugin commands, run with that `CLAUDE_CONFIG_DIR`: `claude plugin marketplace add <project root>` (the marketplace name is read from the root `.claude-plugin/marketplace.json`), then `claude plugin install <pack>@<marketplace>`. In tag mode the marketplace is `ProgrammingCheetah/ways`, added with `claude plugin marketplace add ProgrammingCheetah/ways` unless `claude plugin marketplace list` already shows `ways`, and the install is `claude plugin install teams@ways`; Claude Code has no version pin, so init reads the installed version afterwards and refuses (exit 1, `pack teams is <installed>, manifest asks <tag>`) when it is not the tag's, and writes it to `plugins/.zaf-pack-tag`, which a rerun matches to issue no plugin command. The installed version is read from the profile's `plugins/installed_plugins.json`, by one function that both `zaf profile init` (to decide an update with `claude plugin marketplace update` and `claude plugin update`) and `zaf doctor` (to report each profile's pack) call. `settings.json` and CLAUDE.md are written before the pack step, so a failed install leaves a profile with its denies. Profiles are per project because their CLAUDE.md, memory and skills are. Flag names to confirm on the installed `claude`.

| Profile | Roles | Loads | Target turn-one |
|---|---|---|---|
| lean | keeper, programmer | no MCP servers, no plugins, one skill (the role's), a one-page CLAUDE.md that is the interface (the `zaf` verbs and the fence), no memory index | about 15k |
| build | planning, challenger brief, team, release instances | the `zaf` pack only, the project CLAUDE.md, a memory index trimmed to settled decisions, no MCP | about 30k |
| design | the Engineer's attended session | everything: MCP, skills, memory, canvases | unbounded, attended, short |
| review | Gemini via `agy` | its own settings; inputs copied per runners.md | n/a |

The stable part of a profile (settings, CLAUDE.md, the role's standing brief) is the cached prefix; the variable part (the ticket, the pack) comes after it in the prompt. A profile changes only by a ruling, because changing it invalidates every cached prefix at once.

A profile's allow-list comes from the manifest: `zaf`, each `[requires]` tool, each `[gate]` command verbatim, and one `Bash(<tool> *)` rule for every tool a gate command invokes (the first word of each `&&`, `||`, `;` or `|` segment, skipping `cd`, `env` and shell builtins; a segment that starts with a shell or wrapper tool, such as `sh`, `bash`, `env`, `xargs`, `nohup`, `timeout`, `time`, `nice`, `sudo` or `doas`, gets no tool rule, because a bare rule for it would run anything, and only the gate command's verbatim line covers it), so a programmer can run the gate's tools from its slice workspace, not only the exact line from the project root. Every profile also allows a fixed list of read-only shell basics, one `Bash(<word> *)` rule each (`ls`, `cat`, `head`, `tail`, `wc`, `diff`, `rg`, `grep`, `fd`, `pwd`, `echo`, `printf`, `for`, `while`, `test`, `sort`, `uniq`, `cut`, `stat`, `du`, `which`, `printenv`), so a programmer never stops on a listing or a loop; `sh -c` forms are not added, and neither are `env`, `find`, `sed` and `awk`, which can write or run arbitrary commands (`env <cmd>`, `find -exec`/`-delete`, `sed -i`, `awk` `system()`). When the manifest requires `jj` (a colocated repo), the `git` read verbs `log`, `show`, `diff`, `status` and `blame` are allowed too, and no writing verb (`push`, `commit`, `checkout`, `reset`, ...) ever is. The `[forbidden]` denies keep precedence. Every `[forbidden]` entry `<tool> <sub...>` is denied in four forms, so a global flag before the subcommand cannot walk round it: `Bash(<tool> <sub...>)`, `Bash(<tool> <sub...> *)`, `Bash(<tool> -* <sub...>)` and `Bash(<tool> -* <sub...> *)` (`jj --no-pager git push` and `gh -R owner/repo pr merge 12` are denied when `jj git push` and `gh pr merge` are forbidden; an entry of one word needs no flag form, its `Bash(<tool> *)` deny covers every line). A fixed list of each tool's escape hatches, the subcommands that run any program or rewrite the tool's own config, is denied in every generated profile whenever the profile allows that tool with `Bash(<tool> *)`, from `[requires]` or from a gate line, each in the same four forms: `jj util`, `jj config set` and `jj config edit`; `gh alias` and `gh extension`; `yarn exec`, `yarn node` and `yarn dlx`. A `--config` or `--config-file` flag anywhere on a `jj` line is denied too (`Bash(jj *--config*)`), because it can define an alias. A tool the profile does not allow adds none of its hatches.

### 7.4 Inside `zaf`: ports and adapters
One crate, a library target and a thin binary, layered the way Zurfur is:
- **domain:** the types (Epic, Task, Finding, Ask, Answer, Instance, Profile) and the formats of §7.2, with their parsers and the quote check.
- **app:** one use case per verb, taking Query or Command in and a flat Result out. The verbs see only ports.
- **ports:** narrow traits written from what the verbs need, never from what a backend offers: `Board` (ready, claim, label, comment, report), `Vcs` (open a slice at a base, merge the base in, commit, publish plain, status clean, diff stat, diff hunks), `Forge` (open PR, checks, review comments, merge), `Design` (page by id, search, status; a page may sit in any subfolder of the design folder, the walk skipping `target/`, `.git/`, `node_modules/` and hidden folders, and `zaf doctor` checks the index file is there; adapters `git-sibling` for a corpus repo like Zurfur's, `repo-dir` for a committed `docs/design/` folder, `none`: the ticket is the design, the Architect's package lives in the epic body and `docs/`, and the keeper answers only from the board's epics and rulings, which are always a source), `Runner` (run a role with a profile, capture output), `Gate` (run the gate and light gate from config).
- **adapters:** one per port today: `jj`, `gh` for Board and Forge, `git-sibling` for Design, `claude -p` / `agy -p` / the local `zuri` runners, shell gates. Every external thing is declared with a kind or absent; an absent one removes its `check` steps instead of failing them. The board is the only required port, abstract in kind. A `git` adapter for `Vcs` is a use case the Engineer named for later, not built now.
- **composition root:** the binary reads the manifest (`[vcs] kind = "jj"`, `[board] kind = "github"`, …) and picks the adapters. Nothing else knows which backend is live.
- **tests:** a fake adapter per port, in memory. The fakes are the port's first real consumer, which is what earns a trait under the "traits only when polymorphism is consumed" rule; the second consumer, git, can come later without touching a verb.

The VCS port is expressed in slice terms (open, publish, merge the base in), not VCS terms (branch, worktree, bookmark, stage), because jj and git disagree on the latter and a verb should not care.

## 8. Budget rules
- Two slots; batch two; two programmers per instance at once.
- Programmers: Sonnet, effort high. Leads and planners: Opus, effort xhigh. Instances: compaction window 150k.
- Guard: `zaf guard` reads the weekly usage (mechanism to settle: `/usage` has no CLI form today; the fallback is `zaf burn`'s running total against a weekly budget the Engineer sets in the manifest). Above 70%: Sonnet-only and batch two. Above 85%: one slot. Above 95%: the spawner stops.
- `zaf burn` posts the weekly table as a comment on the milestone.

### 8.1 What leaves Claude and what stays
What leaves is frequent and mechanical: per PR, per commit, per lookup. What stays is rare and judgment-heavy: per epic, per door. Rare work costs little even on Opus, so the budget argument does not reach it and the quality argument decides.

## 9. What retires from the teams way
The Orchestrator; standing build leads; briefs written by hand; Claude critiques; Claude security reviews except on doors; the machinery lane; the LLM QA's planted bugs; the Groundwork plan; claims by hand-back; `queue.md` as the queue (the board is the queue; `/my-queue` reads it). Kept: objectives, design documents, the corpus as truth, `/design-decision`, `/corpus-sync`, `/boardroom`, the Engineer reviewing no code, the two-tier branch model, CodeRabbit on `main`.

## 10. Rollout
| Phase | Work | Tokens |
|---|---|---|
| 0 | board repo + Epic type; CodeQL default setup; Dependabot security updates; `.coderabbit.yaml`; `.zaf.toml` | none |
| 1 | `zaf board`, `brief`, `ask`, `check`, `changes`, `slice` (Rust, in `ways/zaf/`) | built as tickets, dogfooded by hand-spawned instances |
| 2 | `zaf spawn`, the standing briefs per role, the pack `zaf` in the ways marketplace | one trial epic in Zurfur with a known answer |
| 3 | retire the Orchestrator for Zurfur; `/my-queue` reads the board; teams pack stays for the work Mac | |
| 4 | mutants nightly; release instance; `burn` and `guard` | |

## 11. Open decisions
Each line: the options, then Claude's answer.
1. **Board home:** zurfur-design with issues enabled · a new private `zurfur-work` repo · ZurFur public. *Claude: zurfur-design.*
2. **Security standing layer:** Gemini + CodeQL + Semgrep, Opus on doors only · Opus on the whole trigger list as today. *Claude: Gemini layer with a trial: on the first five security-flagged tasks Gemini and Opus both review; Gemini keeps the role only if it finds what Opus finds. Opus on doors regardless.*
3. **Copilot code review:** leave paused · re-enable by ruleset on PRs into `main`. *Claude: leave paused.*
4. **Where the framework lives:** teams 2.0, a major version of the same pack, the work Mac pinned to 1.x until migrated · a new `zaf` pack beside teams. *Claude: teams 2.0 (revised 2026-10-27: it is the next iteration of teams, and the marketplace ruling wants one source). The framework itself is the method, written in the ways README; `zaf` is the binary.*
5. **The tool:** one `zaf` binary in Rust, in the ways repo beside the teams pack (`ways/zaf/`), released with the pack under one tag, linux x86_64 and macOS arm64 archives plus `cargo install --git … zaf` as the fallback; the pack's SessionStart hook pins the version · its own repo like nodes · shell scripts in the pack. *Ruled by the Engineer 2026-10-27: same place as the teams plugin. Claude: beside the pack rather than inside it, so the plugin cache carries no Rust source.*
6. **The challenger:** Opus · Gemini. *Claude: Opus (revised 2026-10-27): rare and judgment-heavy, so quality wins and the budget barely notices.*
7. **Batch size:** two · an epic at a time. *Claude: four, then two (zaf-actions.md ruling 33).*
8. **Previews:** Storybook and staging on every integration merge · on milestone only. *Claude: every merge; it is iteration point 5.*

## 12. Gaps closed on review (2026-10-27)
1. **Dogfood.** zaf is built with the framework: ways carries its own `.zaf.toml` (board: ways issues, public; design: `repo-dir`, `docs/`). ways is a git repo; the Engineer ruled 2026-10-27: jj colocated in ways (`jj git init --colocate`), the git `Vcs` adapter stays a later use case. Items 2 to 10 below were all accepted the same day.
2. **Leases, not claims.** A claim carries a heartbeat (the instance touches its claim comment each turn batch); the spawner reclaims a claim with no heartbeat for the lease window, drops the slice's workspace and returns the task to `ready`.
3. **One instance per epic at a time.** Parallelism is across epics, never within one, so no two instances share an integration state.
4. **`zaf status`.** Board, running instances, slots, guard, open questions, on one screen. `/my-queue` reads it.
5. **Forbidden commands.** `[forbidden]` in the manifest lists commands no profile may run; every generated settings file denies them. Doors protect the design; this protects the machine.
6. **Idempotent verbs.** Re-running any verb on an already-done state succeeds quietly (`slice open` on an existing workspace, `claim` on one's own claim, `merge` on a merged PR). Instances may retry freely.
7. **A fake Runner for end-to-end tests.** Beside the fake adapters per port, a Runner that replays recorded transcripts runs the whole pipeline on a fixture repo in CI at zero tokens. A change to zaf or the pack proves itself there before any project sees it.
8. **Findings by source.** `check` records which source found each finding; the weekly report (`burn` grows into `report`) shows findings by source, rounds per task and cycle time. This is the record the Gemini-versus-Opus security trial needs.
9. **Notifications are the board's.** A question ticket assigns the Engineer; GitHub notifies. No notify code until that proves insufficient.
10. **Migration from teams 1.x.** `queue.md` becomes the board, `TEAMS.md` becomes `.zaf.toml` plus `.zaf/roles/`, rulings files stay in the design source; `zaf init --from-teams` reads the old file. A runner that is unavailable makes its `check` step report "unavailable" and the PR waits; it never merges around a missing review.
