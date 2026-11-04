# zaf actions

Draft v4, 2026-11-03, with the Engineer's rulings of this session and of the planning questions (§8). An **action** is a runnable file that `zaf` names, lists and runs through `zaf do <name>`. Actions are how a project or a machine makes zaf its own, and zaf is the abstraction layer over them: it owns the names, the listing, the environment, the log, the modes, the phases and one exit contract; what a name *does* is the action behind it. zaf ships none: whoever sets zaf up writes them, and every place a verb shells out is a name the project or the machine must provide. What Claude assumed beyond the rulings is listed in §9, open to veto.

## 1. Principles

1. **Make it yours.** A project or a machine decides what a name does by putting a file of that name in its actions folder. zaf ships no actions and has no last word on a step that shells out.
2. **zaf is an abstraction over actions.** A verb is a stable name and a composition of named steps; each step resolves to an action the project or the machine provides. `zaf do push` is the project's `push`, or the machine's, and so is every verb that pushes.
3. **Two tiers, resolved by name.** The project's `.zaf/actions/`, then the machine's `~/.zaf/actions/`. The first file found wins; `zaf do` lists every name with its tier. A name no tier provides is a missing backend: exit 2.
4. **Composable.** An action's body may call `zaf do <other>` and any verb: `zaf do something` may be `zaf do something_1; zaf do something_2`. Each call gets its own log; an outer log names the inner ones.
5. **One contract.** Exit 0 is success, non-zero is failure, 1 the canonical one; every non-zero code an action can return is declared in its metadata with its meaning. Nothing else is promised about its output: the log holds it.
6. **Metadata and body.** Every action file has two parts: the metadata, which names it to zaf (description, exit meanings, mode, group, phases, limits), and the body, which runs. A file without metadata is not an action.
7. **Every action has a mode.** Callable by anybody by default, not every action by everybody: a mode like a file's (§3), three digits for the Engineer, the action's group of roles and every other role, bits for seeing, overriding and running.
8. **Phases are the guard that holds.** An action names the Team Phases it may run in; zaf derives a task's phase from the board and its own records, never from the environment, and refuses the action outside them.
9. **State and logic stay in zaf; slices are action language.** The board's leases and labels, the ask ladder and quote check, the spawner's tick, the check engine, the brief, the transcript parser, doctor and setup stay verbs and are not overridable. The slice family is actions: `zaf do slice-open | slice-pr | slice-merge | slice-drop`, written by the project.

## 2. The two homes

**The project's `.zaf/`, committed.** Projects gain `.zaf/` as a folder of their own. The manifest is `.zaf.toml` at the project root, `.zaf/config.toml`, or both: when both exist they combine, and `.zaf.toml` wins on a property they both set. `ZAF.toml` is retired.

```
.zaf.toml                 the manifest at the root (optional; wins on conflicts)
.zaf/config.toml          the manifest inside the folder (optional; the two combine)
.zaf/actions/<name>       the project's actions, committed
.zaf/roles/               per-role project instructions pasted into briefs, committed
.zaf/.gitignore           written by zaf setup, never by hand: an allow-list, so actions and roles stay tracked
.zaf/local.toml           machine-private overrides (ignored)
.zaf/logs/                action logs (ignored)
.zaf/runs/ .zaf/profiles/ .zaf/workspaces/ .zaf/slices/ .zaf/spawn.state .zaf/guard.json .zaf/asklog.jsonl   as today (ignored)
```

**The machine's `~/.zaf/`, the global home.** Laid out like a project's `.zaf/`: `config.toml` (machine-wide defaults every project here inherits, below the project's own manifest) and `actions/` (the machine's actions, seen by every instance on it). Its place is configurable (`ZAF_HOME`); the default is `~/.zaf/`. `zaf setup` creates it, with a commented `config.toml` and an empty `actions/`, and creates the project's `.zaf/` with its `.gitignore`; any verb that finds either missing runs that setup first, non-destructively, so the Engineer may call `zaf setup` from the beginning or let the first verb do it. A `ZAF.toml` still at the root is read as before, with a warning on stderr; only an explicit `zaf setup`, run by the Engineer, moves it to `.zaf.toml`, once, and says so.

**Precedence.** Manifest: `.zaf.toml` > `.zaf/config.toml` > `~/.zaf/config.toml` > the pack's defaults, property by property. Actions: project > machine; a project file shadowing a machine file can never widen its run bits (§3), and `zaf doctor` names every shadowing.

**No built-ins.** zaf ships no actions, so a fresh project has none until written. `zaf doctor` lists the names the project's verbs and steps need (`gate` lines aside) and which tier provides each, and `zaf do --tier machine <name>` runs the machine's when the project's shadows it. The ways repo keeps its own set in its `.zaf/actions/` (§7), isolated from any other project's; a project copies what it wants and makes it its own.

The zip test (framework §7.6) gains one named exception: `~/.zaf/` is the machine's zaf home, like its credentials. A project whose steps resolve to a machine action depends on that machine; `zaf doctor` reports each one, so the dependency is visible.

## 3. The file

Two parts. The **metadata** is the comment block that opens the file, right after the shebang when there is one: every line is the language's comment marker, then `zaf`, then a key, a colon and a value. The **body** is everything after the first line that is not metadata.

```sh
#!/bin/sh
# zaf description: merge the task's pull request and mark the task done
# zaf 1: the merge conditions do not hold, or the forge refused
# zaf 2: a backend failed
# zaf mode: 750
# zaf group: team, release
# zaf target: task
# zaf phases: land
# zaf timeout: 10
set -e
zaf do --tier machine merge-base "$1" "$ZAF_BASE"
zaf do pr-merge "$1"
```

| Key | Required | Meaning |
|---|---|---|
| `description` | yes | one line, what `zaf do` prints beside the name |
| `<n>` (a non-zero integer) | at least `1` | what that exit code means; every code the body can return is declared |
| `mode` | no | three octal digits, as a file's; default `755` |
| `group` | no | the roles the mode's group digit covers, comma-separated; default none |
| `target` | no | what the first argument names: `task` (its phase is checked) or nothing; default nothing |
| `phases` | no | the Team Phases the action may run in, comma-separated; default any; requires `target: task` |
| `timeout` | no | minutes before the run is killed; default `[actions] timeout_minutes` (30) |

A file with no metadata, no description or no declared code is refused by `zaf do` (exit 1, naming the line) and reported by `zaf doctor`. A comment marker is any run of non-alphanumeric characters (`#`, `//`, `--`, `;`, `REM`), so the format is the same in every language.

**Modes.** Three subjects: *owner* is the Engineer (a hand run, no `ZAF_INSTANCE`); *group* is the roles the action names; *others* is every other role. Three bits: `r` (4) the action is listed by `zaf do` and shown by `--show`; `w` (2) it may be overridden by a file in a higher tier, which only the owner digit can say, since the Engineer writes those files (`7xx` can be shadowed, `5xx` cannot); `x` (1) it may be run. A shadowing file never widens the run bits of the file it shadows, whatever mode it declares. The caller's class comes from `ZAF_INSTANCE`'s role and passes through nested `zaf do` calls. A run the mode refuses exits 1, `<name>: not permitted for <role>`; a listing shows only what the caller may read; `zaf doctor` validates every mode and group. Modes guard against mistakes: `ZAF_INSTANCE` is an environment variable a process can set, so the guard that holds is the phase.

**Team Phases.** A task's phase is state zaf derives, never reads from the environment: from the board (labels, the lease) and from zaf's own records (the slice, the check records). An action with `target: task` and `phases:` runs only while the task its first argument names is in one of them; outside, exit 1, `<name>: #<task> is in <phase>, not <phases>`.

| Phase | The task is | Derived from |
|---|---|---|
| `claim` | claimed under a live lease, no slice yet | the `claimed` label and the lease comment |
| `build` | its slice open, a programmer at work (or a fix round) | the workspace and `.zaf/slices/<task>` exist |
| `check` | its task checks have run on the workspace | the last `zaf check <task>` record |
| `publish` | its pull request open, review steps not yet run | `.zaf/slices/<task>.pr` |
| `review` | the PR's steps have run; a must-fix returns it to `build` | the last `zaf check pr` record |
| `land` | the last `zaf check pr` result for the PR and its current head is clean; no result is not clean | that record's head against the PR's head |
| `done` | merged | the `done` label |

**Name.** The file name, as is: `.zaf/actions/land` is `land`. No spaces; hyphens, dots and underscores are fine; no extension is dropped.

**Environment.** Run in the caller's current directory (the project root for a hand run, the slice workspace for a programmer), with `ZAF_ROOT` (the project root), `ZAF_HOME`, `ZAF_ACTION` (the name), `ZAF_DEPTH` (how many `zaf do` calls are above this one), `ZAF_BASE` (the integration branch, from `ZAF_VERSION` and the branch pattern) and `ZAF_INSTANCE` and `ZAF_VERSION` passed through. A manifest `[env]` table applies to every action run but can set none of these. Arguments after the name go to the body as given.

**Handing a value back.** An action that stands in for a verb's step hands its value back as the last non-empty line of its log: `pr-open` a pull request number, `commit` a commit id, `slice-open` the workspace path. `merge-base` exit 1 means conflicted, with the paths in the log; any other non-zero is a backend failure.

**Output and exit.** Everything the body writes, stdout and stderr, goes to `.zaf/logs/<DATETIME>_<ZAF_ACTION_NAME>.log` (`~/.zaf/logs/` outside a project), `<DATETIME>` as `YYYYMMDDTHHMMSSZ`. On exit 0 `zaf` prints nothing. On a non-zero exit it prints `<name>: exit <n>, <its declared meaning>, log <path>` and the last ten lines of the log; an undeclared code prints `exit <n>, undeclared`. The code passes through unchanged; a file that cannot start, or a run past its time bound, is `2`. `--more` prints the whole log after the run; `--json` gives name, tier, exit, meaning, seconds and log. A nested `zaf do` writes its own log and its path into the outer one.

## 4. The verbs, as compositions

Each verb keeps its name, its exit codes and its logic; the steps where it shells out are names the project or the machine provides. A step with no action exits 2, `missing action <name>`.

| Verb | Stays in zaf | Named steps the project provides |
|---|---|---|
| `board ready\|claim\|report\|question\|ruling` | leases, labels, comment markers, through the Board port | none: the board is state, not a step |
| `ask`, `asklog` | the ladder, the quote check, the log | none |
| `brief` | the pack's composition; **it lists the project's actions** | none |
| `check <task>` / `verify` | the engine, the dedupe, the record, the phase it records | `gate-light` and `test <name>` when the manifest names them as actions; a `[check.*]` step may be `action:<name>` beside `command:<cmd>` |
| `check pr:<n>` | waiting on CI, bots and runners; the record with the PR's head | `action:<name>` steps |
| `changes` | the VCS port's diff, stat before hunks | none now |
| `slice open` → `zaf do slice-open <task>` | nothing: the project's action; `zaf slice open` is its alias one release | `workspace-open <slice> <base>` |
| `slice pr` → `zaf do slice-pr <task>` | nothing: the project's action | `commit <slice> <message>`, `push <slice>`, `pr-open <slice> <base> <title> <body>` |
| `slice merge` → `zaf do slice-merge <task>` | nothing: the project's action, `phases: land` | `merge-base <slice> <base>`, `gate-light`, `push <slice>`, `pr-merge <pr>`, `workspace-drop <slice>` |
| `slice drop` → `zaf do slice-drop <task>` | nothing: the project's action | `workspace-drop <slice>` |
| `spawn`, `status`, `guard`, `report` | all of it | none |
| `doctor` | the manifest's validator; **it checks every action** (metadata, modes, phases, runnable, shadowing, each needed name provided) | none |
| `init`, `setup`, `profile init` | they write the project's own files; `setup` makes the two homes (§2) | none |
| `[gate]` | the gate port keeps its levels; a line may be `zaf do gate` | the profile allow-list derives its tool rules from a literal line, so a project that wants both keeps the line |
| `[runners.<name>]` | the template and its placeholders | none now |

The release brief's "merge and tag" is the project's `release` action; a hand-made PR lands with `zaf do land <pr>`; `[check.pr]` names `action:nodes-check` instead of a line. The jj and gh code the slice verbs used leaves zaf for the project's scripts; the Vcs port keeps what `changes` needs.

## 5. The verb added

| Verb | Use it to |
|---|---|
| `zaf do` | list every action the caller may read: name, tier, description; `--json` for the spawner and the briefs |
| `zaf do <name> [args…]` | run one; its exit code, its meaning on failure, its log |
| `zaf do --show <name>` | print the file a name resolves to |
| `zaf do --tier machine <name> [args…]` | run the machine's action when the project's shadows it; modes and phases apply as ever |

Exit codes as every verb: `0` done, `1` refused (no such action, a file without metadata or not runnable, a mode or phase that refuses), `2` the file could not start or timed out, and the action's own declared code otherwise.

## 6. The manifest

```toml
[actions]        timeout_minutes = 30                      # optional; absent = 30
[check.pr]       steps = ["ci", "action:nodes-check"]
[gate]           light = "zaf do gate-light"               # or a literal line, as today
```

The actions folder is not configurable: `.zaf/actions/` in a project, `actions/` in the global home.

## 7. Dogfood: the ways set

Built through the lifecycle on this repo (epic #103, planned, built by team instances on `feature/0.2.0`); 0.1.3 (#99) is folded into it as one release. The ways repo moves first: `ZAF.toml` becomes `.zaf.toml`, the `.gitignore` line `.zaf/` becomes `.zaf/.gitignore`, and `.zaf/actions/` holds the set below, ways' own and nobody else's. Any other project (Zurfur) writes its own; this set is a reference to copy, never a dependency.

| Action | Mode, group | Target, phases | Does |
|---|---|---|---|
| `slice-open` | `750` team, release | task: `claim` | a jj workspace and a same-named bookmark at the base; hands back the workspace path |
| `slice-pr` | `750` team, release | task: `check`, `review` | `commit`, `push`, `pr-open`; hands back the PR number |
| `slice-merge` | `750` team, release | task: `land` | `merge-base`, `gate-light`, `push`, `pr-merge`, `workspace-drop`; marks the task done through `zaf board` |
| `slice-drop` | `750` team, release | task: `build`, `done` | `workspace-drop` |
| `commit`, `push`, `pr-open`, `pr-merge`, `merge-base`, `sync`, `tag`, `workspace-open`, `workspace-drop` | `750` team, release | none | the jj and gh steps the four above compose |
| `land <pr>` | `750` team, release | none | `pr-merge` then `sync main`: the Engineer's `gh pr merge … && jj git fetch && jj new main` |
| `release <version>` | `750` release | none | check `zaf/Cargo.toml` is at `<version>`, `tag zaf-v<version>`; the workflow does the rest |
| `gate-light`, `gate` | `755` | none | the cargo lines of today's `[gate]` |
| `nodes-check` | `755` | none | `nodes check --root "$ZAF_ROOT"`; the `[check.pr]` step |

Not actions: `/nodes:chart --diff` (a model's job, a skill) and the spawner's missing launches (verbs, their own epic).

## 8. Rulings, 2026-11-03

1. **Global home:** a configurable zaf home, default `~/.zaf/`, laid out like a project; `zaf setup` makes it first when it has not run, and may be called from the beginning.
2. **Local home:** projects gain a committed `.zaf/` folder; actions live in `.zaf/actions/`; the file name is the action name, no spaces, hyphens and the like allowed.
3. **Manifest:** `.zaf.toml` at the project root or `.zaf/config.toml`; both may exist and combine, `.zaf.toml` winning on a conflicting property.
4. **Verb:** `zaf do`.
5. **Output:** the log is `.zaf/logs/<DATETIME>_<ZAF_ACTION_NAME>.log`.
6. **Ignore rule:** zaf writes `.zaf/.gitignore` for the machine state.
7. **Actions are the "make it yours" layer, and zaf is the abstraction layer over them:** `zaf do push` may do far more than a push.
8. **Contract:** an action returns 0 for success and non-zero for failure (1 canonical).
9. **Composable:** an action's body may run `zaf do something_1; zaf do something_2`.
10. **Two parts:** metadata (the description and what each non-zero return value means) and the body.
11. **Hand-back (#121):** the last non-empty line of the action's log is its value; `merge-base` exit 1 is conflicted; other non-zero is a backend failure.
12. **Modes and slices (#122):** actions are callable by anybody by default, not every action by everybody: a mode like a file's, owner the Engineer, group the roles named, others the rest, bits r w x, default `755`, the caller's role passed through nesting. The slice family is action language, `zaf do slice-*`, the merge conditions inside `slice-merge`.
13. **Sequencing (#123):** 0.1.3 is folded into 0.2.0: #99's tasks move under #103, one release on `feature/0.2.0`, #94 re-scoped against #105.
14. **Calling past an override (#124):** was `zaf do --builtin <name>`; with no built-ins (16) it is `--tier machine`.
15. **Moving the manifest (#125):** only an explicit `zaf setup` moves a root `ZAF.toml` to `.zaf.toml`; other verbs read it, warn, and make the homes non-destructively.
16. **Overridable, never wider (#127):** landing steps declare `750` group team, release; a shadowing file never widens the run bits of what it shadows.
17. **No shipped actions (#128):** zaf ships none; whoever set zaf up writes them; two tiers; the ways repo writes its own set, isolated from Zurfur's.
18. **Team Phases (#129):** an action's metadata names the phases it may run in; zaf derives a task's phase from the board and its records, never the environment; phases are the guard that holds, modes guard against mistakes.
19. **Merge conditions (#130):** enforced through phases: `slice-merge` runs in `land`, which needs a clean `zaf check pr` on the PR's current head; no result is not clean.

## 9. Assumed by Claude, open to veto

1. The seven Team Phases of §3 and what derives each; `check` records the task check's phase and `check pr` the PR's head, which #131 already files.
2. `target: task` is the only target now; release-level phases (framework §2.5) for `release` come later.
3. `ZAF_BASE` is set from `ZAF_VERSION` and the branch pattern, so slice scripts need not know the pattern.
4. The metadata syntax of §3: `<comment marker> zaf <key>: <value>` lines opening the file.
5. `zaf do` with no name lists; `ZAF_HOME` moves the global home; `ZAF_DEPTH` guards recursion at 8; `--tier machine` is the only tier flag.
6. The ways set of §7, its modes and phases, is Claude's first cut; the planning instance may add names, never drop the four `slice-*`.
7. `~/.zaf/config.toml` is read as machine-wide defaults below the project's manifest; what `.zaf/local.toml` holds today may move there later, not in this epic.
8. `.zaf/roles/` is committed with the project; it holds instructions, not secrets.
9. A root `ZAF.toml` beside a `.zaf.toml` or `.zaf/config.toml` is an error in every verb, not only in doctor.
10. `command:` check steps and the `zaf slice …` verbs stay one release beside `action:` and `zaf do slice-…`, then retire; `changes` stays a verb over a Vcs port shrunk to diffs.
11. The manifest combines table by table, property by property; an array is replaced, not merged. `[env]` applies to every gate, test, check and action run and can set no `ZAF_*` variable.
12. The group digit covers exactly the roles named; a role not in `ZAF_INSTANCE`'s vocabulary is others; programmers share their team's identity until they run as their own process, which no task in this epic needs.
