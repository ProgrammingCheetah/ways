# teams

The standing briefs of the instances that build a project, one per role, plus the routines the Engineer runs in the design session. The work itself runs through `zaf`, the project-blind binary of verbs: the board is the queue, the spawner launches the instances, and every project word comes from the project's manifest (`.zaf.toml` or `.zaf/config.toml`) or the ticket. Nothing in this pack names a project.

| Part | What it is |
|---|---|
| `agents/` | Six role briefs: `teams:team`, `teams:programmer`, `teams:keeper`, `teams:planning`, `teams:challenger`, `teams:release`; plus `teams:designer`, the design session's judgment lane |
| `hooks/hooks.json` | At session start in a project with a `.zaf.toml`, `.zaf/config.toml` or the retired `ZAF.toml`, runs `zaf doctor` and prints its table; elsewhere it does nothing |
| `skills/` | `/teams:objectives`, `/teams:my-queue`, `/teams:catch-me-up`, `/teams:document`, `/teams:retrospective` |

## The role briefs

Each brief is an agent of this pack and runs as the session's main thread (`claude --agent teams:<role>`) or, for a programmer, as a team instance's subagent. Each is under a page: purpose, input, output, lifecycle, budget, never-list, and the verbs it runs in order.

| Role | Launched by | Does |
|---|---|---|
| team | the spawner | claims up to four ready tasks of one epic, spawns a programmer per task, checks, publishes and merges each, reports on the epic |
| programmer | a team instance | one task, one round, green in its own workspace; asks the keeper for anything beyond its pack |
| keeper | `zaf ask` | answers one to five design questions with verbatim quotes, or `not settled`; its rules are zaf's own keeper prompt, which this brief points to and never restates |
| planning | the spawner, for a signed epic with no tasks | turns a signed epic into exact tasks in the task template, filed with `zaf board file` |
| challenger | the spawner, once per epic after its first plan | comments on every planned task that is not exact; its report on the epic lets the teams start |
| release | by hand (its spawner launch is not built yet) | checks a milestone's musts, gathers the release reviews and a retrospective, then files tasks or merges and tags |

**One prefix.** Every brief opens with the exact bytes of zaf's interface page (`zaf/assets/interface.md` in the zaf source, also the Interface section of every `zaf brief` pack), so every instance's prompt shares a cached prefix. When that page changes, paste it again into all six, right after the frontmatter, and check:

```
for r in team programmer keeper planning challenger release; do f=agents/$r.md; l=$(awk '/^---$/{c++; if(c==2){print NR; exit}}' $f); tail -n +$((l+1)) $f | head -c $(wc -c < interface.md) | cmp -s - interface.md && echo "$r ok" || echo "$r differs"; done
```

## Launching an instance

**The spawner.** `zaf spawn --once` runs one tick and exits; a timer runs it over and over. Each tick gives every open epic at most one launch, within the slots and the budget guard, by the epic's stage (`zaf epic list`): a planning instance for a signed epic with no tasks, once per signature; the challenger for a planned epic, once per epic; and a team instance for a challenged epic with ready, unclaimed, unblocked tasks. An unsigned epic (`zaf epic sign` is the Engineer's), a done one and one with a question open on it launch nothing. It sets `ZAF_INSTANCE` and `ZAF_VERSION` and starts the role with the model, effort, turns and profile the manifest gives it; planning and the challenger have pack defaults. The release launch is not built yet, and the tick names it as `not built`; until it is, and for a re-plan after a ruling, the Engineer launches those roles by hand, below.

**By hand.** The Engineer runs one the same way, from the project root, with the two variables set:

```
cd <project root>
export ZAF_INSTANCE="team instance <n> on #<epic>" ZAF_VERSION=<version>
claude --agent teams:team "Epic #<epic>."
```

Swap the role and the prompt for the others: `teams:planning` or `teams:challenger` with `"Epic #<epic>."`, `teams:release` with the milestone, its issue and its `objectives.md`. Add `-p` to run it unattended. A programmer by hand needs its task opened first: `zaf slice open <task>`, `zaf brief <task>`, then `claude --agent teams:programmer "Task #<task>, round 1. Pack: <pack path>. Workspace: <workspace path>."`. `zaf board claim` and `zaf board report` refuse to run without `ZAF_INSTANCE`.

**What `zaf setup` shows.** First the plan: what this run will do and why, with `Continue? [Y/n]` only when something is written, asked before anything is. Then its questions, each with what it means and its default; one line per step; and at the end only what failed, each with its command and why, to run `zaf setup` again, or on green `zaf begin --version <next>`. Colour shows only in a terminal, never with `NO_COLOR`.

## A release in four stages

`zaf begin --version <v>` walks a release through four stages and says which one it is at, read from the folder and the board each time, so running it again picks up where the release is:

| Stage | You | zaf |
|---|---|---|
| 1. Objectives | answer Claude's questions about what the version is for, then sign | opens the session (the `objectives` action), checks the file it wrote |
| 2. Design | answer the Designer's questions, then sign | opens the session (the `design` action), checks the file |
| 3. Epics | read each epic (`zaf epic show <n>`) and sign it (`zaf epic sign <n>`) | plans an epic's tickets once it is signed, so an unsigned epic shows no ticket count |
| 4. Build | watch, and answer when asked | builds the signed tickets: `zaf spawn --watch --version <v>`, or the hub's watch |

**Begin's screen.** The four stages, `✓` for each done and `▸` for the one you are at. Before a session it says what the session is for and how to leave it (`/exit`), then `Open it? [Y/n]`. After it: `✓ <file> written` and `Sign it? It says what you mean, and an edit later unsigns it. [Y/n]`; Enter, `y` or `yes` signs, an answer starting with `n` does not, and anything else asks again. At the epics it names each epic with its read and sign commands, then the build command. Once every epic is done it draws all four stages done, opens the release pull request (`release-open`) and prints it with `zaf begin --version <next>`; merging that pull request on the forge ships the version, as zaf tags nothing. With no terminal, or inside an instance, `zaf begin` prints one sentence and exits 2.

**The two questions that start the build.** When every epic of a version is signed, zaf asks `Every epic of <v> is signed. Start the build now? [Y/n]`: from `zaf epic sign`, when the signature you just made is the last one, and from `zaf begin`, when it finds every epic signed. Yes runs `zaf spawn --watch --version <v>` in that terminal; no prints that command for later. Typing the command or pressing the hub's *Start the watch* is itself the asking, so neither asks again.

**What the build start does.** Before its first tick, a scheduler that has just started runs the build start. It refuses, changing nothing, when the objectives (or scope) or the design is unsigned or was edited after it was signed (`zaf begin --version <v>` signs it again), or when the project has no remote. Then it runs two actions of this pack: `base-open` makes the version's integration branch (`[branches] integration`) on the remote at the default branch and pushes it, and `papers-commit` puts the release folder in one commit on that branch and pushes it, so every task reads the papers by path from the branch. Each prints one line when it did something (`made <branch>`, `committed <id>`) and is quiet when the branch and the papers are already there. Neither moves your working copy.

**Answering in the watch.** When a question is open on the board, the view stops redrawing: the frame that shows it is the last, and the question is written under it in full, with its number and `zaf board ruling <n> 'your answer'` for any other terminal. Type your answer at the prompt (`Your answer:`) and press Enter: it is recorded as `zaf board ruling` records it, and the build goes on (`✓ recorded`). An empty line skips the question: it stays open on the board and is not asked again in this run. Ctrl-C ends the watch as ever, rules nothing, and prints `#<n> stays open: answer it with `zaf board ruling <n> '…'``. The rest keeps building while you think. With no terminal or with `--json`, nothing is asked: answer with `zaf board ruling`.

**How a version ends.** When every epic of the version is done, the watch runs `release-open`, the third action of this pack beside `base-open` and `papers-commit`, once: it opens the release pull request from the version's integration branch to the default branch and hands back its number and link; one already open is handed back, and a failure is a warning and tried again after the next tick. In a terminal the view ends with `✓ zaf built <v>: <n> tickets merged in <m> min.`, `Next: merge pull request #<p> on the forge to ship <v>.` and `Then start the next release: zaf begin --version <next>`, and `zaf spawn --watch` exits 0. A piped or `--json` watch, and the hub's, say `zaf built <v>: release pull request #<p>, <link>` and keep running until you stop them. Merging the pull request ships the version; zaf tags nothing.

**What the watch shows in a terminal.** `zaf spawn --watch` draws one view and redraws it in place instead of streaming each tick's text: `zaf is building <v>`, a spinner, how many tickets are merged, the Ctrl-C line, then each epic of the version with its bar and each ticket with its stage in plain words. It draws on the normal screen, so the last frame stays in your scrollback, and it reads the window's size before each frame. It reads the board again after each tick and whenever a verb leaves a report; between those only the spinner turns. Typed keys are not echoed, and the terminal's settings are put back however the watch ends (the end, Ctrl-C, a crash, the restart into a new zaf). Each tick's text and its warnings go to `.zaf/logs/scheduler.log`, and a tick that fails shows one line under the view naming that log. With no terminal (piped or redirected) or with `--json`, the watch streams as before.

**Before the first launch:** `zaf` and this plugin, which the root README's one command gets you (`curl -fsSL https://github.com/ProgrammingCheetah/ways/releases/latest/download/install.sh | sh`, the script `install.sh`: it puts `zaf` on PATH and installs `teams@ways`), then `zaf setup` (which asks for a manifest where there is none and also moves a retired `ZAF.toml`), and a clean `zaf doctor`, which this pack's hook prints at every session start in the project. Claude Code 2.1.296 or later (the team brief sets `autoCompactWindow`).


## The design session's routines

| Skill | What it does |
|---|---|
| `/teams:objectives` | Interviews the Engineer one section at a time into the release's `objectives.md`, has a fresh `teams:designer` challenge it, records the signature |
| `/teams:my-queue` | The Engineer's queue as Ready, Incoming, Questions, Waiting and Optional tables, from the board (`zaf status`) or a queue file, and live PRs. Read-only |
| `/teams:catch-me-up` | Where things stand, why, what's missing, what's next, from the board, the release folder and VCS history. Read-only |
| `/teams:document` | Doc comments on what changed, in each language's style and placement. Docs only |
| `/teams:retrospective` | Claude's own retro over a time range, from commits, PRs and reviews |

They read `.claude/TEAMS.md` (or `.yaml`, `.toml`) when a project still has one from 1.x, and otherwise detect what they need and ask once.

## Retired in 2.0.0

The Orchestrator (`/teams:orchestrate`, its teams file and templates, its first run, runners and planner check), standing build leads, briefs written by hand, the Groundwork, roadmaps and claims by hand-back, the milestone QA and its planted bugs, `queue.md` as the queue, and the lanes the briefs replace (`teams:builder`, `teams:myth`, `teams:planner`) or the design retires (`teams:machinery`). The verbs replace `/teams:prepare-pr` (`zaf slice pr`), `/teams:cleanup` (`zaf slice merge`, `zaf slice drop`), `/teams:critique` and `/teams:address-comments` (`zaf check` runs the review steps and returns the findings; a team instance sends them to a fresh programmer). A project that still runs the 1.x way pins a 1.x version of this pack.

## Updating

From the marketplace: `claude plugin marketplace update ways`, then `claude plugin update teams@ways`, then start a new session. A marketplace added from a local folder loads this plugin in place, so edits apply at the next session or after `/reload-plugins`.
