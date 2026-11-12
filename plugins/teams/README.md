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

**Before the first launch:** `zaf` on PATH, `zaf setup` (which asks for a manifest where there is none and also moves a retired `ZAF.toml`), and a clean `zaf doctor`, which this pack's hook prints at every session start in the project. Claude Code 2.1.296 or later (the team brief sets `autoCompactWindow`).

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
