# zaf actions

Draft v17, 2026-11-10, with the Engineer's rulings and the answers to the planning questions (§8). An **action** is a runnable file that `zaf` names, lists and runs through `zaf do <name>`. Actions are how a project or a machine makes zaf its own, and zaf is the abstraction layer over them: it owns the names, the listing, the environment, the log, the modes, the phases and one exit contract; what a name *does* is the action behind it. zaf ships none: whoever sets zaf up writes them, and every place a verb shells out is a name the project or the machine must provide. What Claude assumed beyond the rulings is listed in §9, open to veto.

## 1. Principles

1. **Make it yours.** A project or a machine decides what a name does by putting a file of that name in its actions folder. zaf ships no actions and has no last word on a step that shells out.
2. **zaf is an abstraction over actions.** A verb is a stable name and a composition of named steps; each step resolves to an action the project or the machine provides. `zaf do push` is the project's `push`, or the machine's, and so is every verb that pushes.
3. **Two tiers, resolved by name.** The project's `.zaf/actions/`, then the machine's `~/.zaf/actions/`. The first file found wins; `zaf do` lists every name with its tier. `zaf do` on a name no tier provides is `NOT_RUN`, exit 125 (§5); a verb's step with no action is exit 2 (§4).
4. **Composable.** An action's body may call `zaf do <other>` and any verb: `zaf do something` may be `zaf do something_1; zaf do something_2`. Each call gets its own log; an outer log names the inner ones.
5. **One contract.** Exit 0 is success, non-zero is failure, 1 the canonical one; every non-zero code an action can return is declared in its metadata with its meaning. Code 125 is reserved for zaf: `NOT_RUN`, the body never ran. What zaf prints on a failure leads with a word, `FAIL` or `NOT_RUN`, because a model reads words; a pass prints nothing. Nothing else is promised about an action's output: the log holds it.
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
.zaf/runs/ .zaf/profiles/ .zaf/workspaces/ .zaf/slices/ .zaf/spawn.state .zaf/spawn.lock .zaf/guard.json .zaf/asklog.jsonl   as today (ignored)
```

**The machine's `~/.zaf/`, the global home.** Laid out like a project's `.zaf/`: `config.toml` (machine-wide defaults every project here inherits, below the project's own manifest) `actions/` (the machine's actions, seen by every instance on it) and `ledger.db` (zaf's record of what every run cost, §10). Its place is configurable (`ZAF_HOME`); the default is `~/.zaf/`. `zaf setup` creates it, with a commented `config.toml` and an empty `actions/`, and creates the project's `.zaf/` with its `.gitignore`; any verb that finds either missing runs that setup first, non-destructively, so the Engineer may call `zaf setup` from the beginning or let the first verb do it. A `ZAF.toml` still at the root is read as before, with a warning on stderr; only an explicit `zaf setup`, run by the Engineer, moves it to `.zaf.toml`, once, and says so.

**Precedence.** Manifest: `.zaf.toml` > `.zaf/config.toml` > `~/.zaf/config.toml` > the pack's defaults, property by property. Actions: project > machine; a project file shadowing a machine file can never widen its run bits (§3), and `zaf doctor` names every shadowing.

**No built-ins.** zaf ships no actions, so a fresh project has none until written. `zaf doctor` lists the names the project's verbs and steps need (`gate` lines aside) and which tier provides each, and `zaf do --tier machine <name>` runs the machine's when the project's shadows it. The ways repo keeps its own set in its `.zaf/actions/` (§7), isolated from any other project's; a project copies what it wants and makes it its own.

The zip test (framework §7.6) gains one named exception: `~/.zaf/` is the machine's zaf home, like its credentials. A project whose steps resolve to a machine action depends on that machine; `zaf doctor` reports each one, so the dependency is visible.

## 3. The file

Two parts. The **metadata** is the comment block that opens the file, right after the shebang when there is one: every line is the language's comment marker, then `zaf`, then a key, a colon and a value. The **body** is everything after the first line that is not metadata.

```sh
#!/bin/sh
# zaf description: merge the task's pull request and mark the task done
# zaf 1: the base conflicts, or the forge refused
# zaf 2: a backend failed
# zaf mode: 750
# zaf group: team, release
# zaf target: task
# zaf phases: land
# zaf timeout: 10
set -e
zaf do --tier machine merge-base "$ZAF_SLICE" "$ZAF_BASE"
zaf do pr-merge "$ZAF_PR"
zaf board done "$1"
```

| Key | Required | Meaning |
|---|---|---|
| `description` | yes | one line, what `zaf do` prints beside the name |
| `<n>` (a non-zero integer, never `125`) | at least `1` | what that exit code means; every code the body can return is declared |
| `mode` | no | three octal digits, as a file's; default `755` |
| `group` | no | the roles the mode's group digit covers, comma-separated; default none |
| `target` | no | what the first argument names: `task` (its phase is checked) or nothing; default nothing |
| `phases` | no | the Team Phases the action may run in, comma-separated; default any; requires `target: task` |
| `timeout` | no | minutes before the run is killed; default `[actions] timeout_minutes` (30) |

A file with no metadata, no description or no declared code, or one that declares `125`, is refused by `zaf do` (`NOT_RUN`, exit 125, naming the line) and reported by `zaf doctor`. A comment marker is any run of non-alphanumeric characters (`#`, `//`, `--`, `;`, `REM`), so the format is the same in every language.

**Modes.** Three subjects: *owner* is the Engineer (a hand run, no `ZAF_INSTANCE`); *group* is the roles the action names; *others* is every other role. Three bits: `r` (4) the action is listed by `zaf do` and shown by `--show`; `w` (2) it may be overridden by a file in a higher tier, which only the owner digit can say, since the Engineer writes those files (`7xx` can be shadowed, `5xx` cannot); `x` (1) it may be run. A shadowing file never widens the run bits of the file it shadows, whatever mode it declares. The caller's class comes from `ZAF_INSTANCE`'s role and passes through nested `zaf do` calls. A run the mode refuses prints `NOT_RUN <name>: not permitted for <role>` and exits 125; a listing shows only what the caller may read; `zaf doctor` validates every mode and group. Modes guard against mistakes: `ZAF_INSTANCE` is an environment variable a process can set, so the guard that holds is the phase.

**Team Phases.** A task's phase is state zaf derives, never reads from the environment: from the board (labels, the claim comment), from the forge (the head of the recorded pull request), from the workspace (its latest commit) and from zaf's own records (the slice, the check records). An action with `target: task` and `phases:` runs only while the task its first argument names is in one of them; outside, `NOT_RUN <name>: #<task> is in <phase>, not <phases>`, exit 125. A board, forge or workspace read that fails while deriving the phase refuses the action the same way, `NOT_RUN <name>: #<task>'s phase cannot be read: <why>`, exit 125, and nothing runs.

| Phase | The task is | Derived from |
|---|---|---|
| `claim` | claimed, no slice yet | the `claimed` label and the claim comment |
| `build` | its slice open, a programmer at work (or a fix round) | the workspace and `.zaf/slices/<task>` exist |
| `check` | its task checks have passed on the workspace; a must-fix or a failed step stays in `build`; the team flow may skip it (ruling 39) | the last `zaf check <task>` record, clean |
| `publish` | its pull request open, review steps not yet run | `.zaf/slices/<task>.pr` |
| `review` | the PR's steps have run; a must-fix returns it to `build` | the last `zaf check pr` record |
| `land` | the last `zaf check pr` result for the PR and its current head is clean, and the workspace's latest commit is that head; no result is not clean | that record's head against the PR's head and the workspace's latest commit |
| `done` | merged | the `done` label |

The first rule that holds decides: the `done` label is `done`; a `check pr` record clean for the PR's current head, with the workspace's latest commit at that head, is `land`; a clean record with the workspace ahead of it, or a must-fix record, is `build`, or `check` when a clean task check is newer; a failed step is `review`; a `.pr` record with no result for the current head is `publish`, or `check` when a clean task check is newer than the `.pr` record, or `build` when the last task check failed; a clean task check is `check`; a workspace record is `build`; the `claimed` label with a claim comment is `claim`; anything else is `unclaimed`, which no `phases:` may name.

A fact zaf could not read, because a board or forge read failed, refuses the action with `NOT_RUN`. A fact that is absent, such as a pull request with no head or a missing workspace, falls to the earlier phase, so it can never reach `land`. A claim has no expiry: the spawner hands back a timed-out instance's claims (ruling 41).

**The records.** zaf writes them, never a script. When `slice-open` or `slice-pr` exits 0, zaf stores what it handed back in `.zaf/slices/<task>` (the workspace path) or `.zaf/slices/<task>.pr` (the number). When `slice-merge` or `slice-drop` exits 0, zaf forgets both and the task's check records, so a drop returns the task to `claim` and a reopened slice starts clean. `zaf check` and `zaf changes` read the workspace from the record and fill the base from `ZAF_VERSION` and the branch pattern. These are plain files, so this stops a script's mistake, not a process set on forging them.

**Name.** The file name, as is: `.zaf/actions/land` is `land`. No spaces; hyphens, dots and underscores are fine; no extension is dropped.

**Environment.** Run in the caller's current directory (the project root for a hand run, the slice workspace for a programmer), with `ZAF_ROOT` (the project root), `ZAF_HOME`, `ZAF_ACTION` (the name), `ZAF_DEPTH` (how many `zaf do` calls are above this one), `ZAF_BASE` (the integration branch, from `ZAF_VERSION` and the branch pattern) and `ZAF_INSTANCE` and `ZAF_VERSION` passed through. A `target: task` run also has `ZAF_SLICE` (from the task branch pattern), `ZAF_PR` (from the record, when one exists) and `ZAF_TASK_TITLE` (from the board). A manifest `[env]` table applies to every action run but can set none of these. Arguments after the name go to the body as given.

**Handing a value back.** An action that stands in for a verb's step hands its value back as the last non-empty line of its log: `pr-open` a pull request number, `commit` a commit id, `slice-open` the workspace path. `merge-base` exit 1 means conflicted, with the paths in the log; any other non-zero is a backend failure.

**Output and exit.** Everything the body writes, stdout and stderr, goes to `.zaf/logs/<DATETIME>_<ZAF_ACTION_NAME>.log` (`~/.zaf/logs/` outside a project), `<DATETIME>` as `YYYYMMDDTHHMMSSZ`. On exit 0 `zaf` prints nothing. When the body ran and exits non-zero, it prints `FAIL <name>: exit <n>, <its declared meaning>, log <path>` and the last ten lines of the log; an undeclared code prints `exit <n>, undeclared`. The code passes through unchanged. A run past its time bound did run, so it is `FAIL`, exit 2. When the body never ran (§5), it prints `NOT_RUN <name>: <reason>` and exits 125. A body that exits 125 because a nested action did not run is `FAIL`, `exit 125, a nested action did not run`, and the code passes through. `--more` prints the whole log after the run; `--json` gives result (`OK`, `FAIL` or `NOT_RUN`), name, tier, exit, meaning, seconds and log. A nested `zaf do` writes its own log and its path into the outer one. A `zaf check` step that runs a process logs the same way (ruling 30).

## 4. The verbs, as compositions

Each verb keeps its name, its exit codes and its logic; the steps where it shells out are names the project or the machine provides. A step with no action exits 2, `missing action <name>`.

| Verb | Stays in zaf | Named steps the project provides |
|---|---|---|
| `board ready\|claim\|report\|question\|ruling\|done\|file` | leases, states (labels, or a Project field), comment markers, through the Board port; `done <task>` sets the `done` state, drops `ready` and `claimed`, and refuses unless the task is in `land` or the pull request recorded for it has merged; `file <epic> <title>` opens a task under the epic in the task template, ready or `--blocked`, and files a title the epic already counts only once | none: the board is state, not a step |
| `ask`, `asklog` | the ladder, the quote check, the log | none |
| `brief` | the pack's composition; **it lists the project's actions** | none |
| `check <task>` / `verify` | the engine, the dedupe, the record, the phase it records | `gate-light` and `test <name>` when the manifest names them as actions; a `[check.*]` step may be `action:<name>` beside `command:<cmd>` |
| `check pr:<n>` | waiting on CI, bots and runners; the record with the PR's head | `action:<name>` steps |
| `changes` | the VCS port's diff, stat before hunks | none now |
| `slice open` → `zaf do slice-open <task>` | nothing: the project's action; `zaf slice open` is its alias one release | `workspace-open <slice> <base>` |
| `slice pr` → `zaf do slice-pr <task>` | nothing: the project's action | `commit <slice> <message>`, `push <slice>`, `pr-open <slice> <base> <title> <body>` |
| `slice merge` → `zaf do slice-merge <task>` | nothing: the project's action, `phases: land` | `merge-base <slice> <base>`, `push <slice>`, `pr-merge <pr>`, `workspace-drop <slice>`; no `gate-light`, CI is the gate (ruling 39) |
| `slice drop` → `zaf do slice-drop <task>` | nothing: the project's action | `workspace-drop <slice>` |
| `spawn`, `status`, `guard`, `report` | all of it | none |
| `doctor` | the manifest's validator; **it checks every action** (metadata, modes, phases, runnable, shadowing, each needed name provided) | none |
| `init`, `setup`, `profile init` | they write the project's own files; `setup` makes the two homes (§2) | none |
| `[gate]` | the gate port keeps its levels; a line may be `zaf do gate` | the profile allow-list derives its tool rules from a literal line, so a project that wants both keeps the line |
| `[runners.<name>]` | the template and its placeholders | none now |

The release brief's "merge and tag" is the project's `release` action, which squash-merges with `pr-merge <pr> --squash`; the Engineer's hand-made PR lands with `zaf do land <pr>`. `pr-merge` takes the pull request and one option, `--squash`: with none it merges with a merge commit, and any other option is refused, exit 1. `[check.pr]` may name `action:<name>` instead of a line; ways' own is `["ci"]` (ruling 39). The jj and gh code the slice verbs used leaves zaf for the project's scripts; the Vcs port keeps what `changes` needs.

## 5. The verb added

| Verb | Use it to |
|---|---|
| `zaf do` | list every action the caller may read: name, tier, description; `--json` for the spawner and the briefs |
| `zaf do <name> [args…]` | run one; its exit code, its meaning on failure, its log |
| `zaf do --show <name>` | print the file a name resolves to |
| `zaf do --tier machine <name> [args…]` | run the machine's action when the project's shadows it; modes and phases apply as ever |

`zaf do <name>` exits `0` when it is done (`OK`). It exits `125` when the body never ran (`NOT_RUN`): no such action, a file without metadata or not runnable, a mode or phase that refuses, or a file that could not start. Otherwise the body ran (`FAIL`): the action's own declared code, or `2` for a timeout. No action may declare `125`, and `zaf doctor` rejects a file that does, so a caller never reads a refusal as a declared code such as `merge-base`'s `1`, conflicted. Every other verb keeps `0`, `1` and `2`, and a verb's step with no action stays exit 2, `missing action <name>` (ruling 17).

**`zaf hub`.** The Engineer's live view of what is happening. It only reads: it starts nothing, changes nothing and costs no model turns.

```
epic #103                     [######------------------------]  17 of 82       $39.83
  1 of 2 slots free · 63 ready · no questions

group: ways-steps             [##########--------------------]  3 of 9
  #143  sonnet programmer     [#################-------------]  build    testing        40s
  #156  opus team             [##########################----]  review   waiting on CI   2m
```

An epic and a group count their tasks done. A ticket's bar is its Team Phase, one of seven, because a model has no honest percent. A group is a task's `group:<name>` label. A ticket's word is general, from its last tool call: reading, writing, testing, waiting on CI. The time is since its last report and keeps ticking, so silence shows.

It redraws when a report arrives, not on a timer. zaf verbs report that state changed and a hook in the profiles reports each tool call, so no model spends a turn reporting its tool calls. Between reports only the clocks move and nothing reads the network. Outside a terminal it draws one frame and exits, so a model cannot hang on it.

**Model phases.** Each model declares its own phases when it starts, `zaf progress plan <task> <phase>…`, and reports each change, `zaf progress at <task> <phase>`. Phases may repeat: a model that goes back from `test` to `write` reports `write` again, and its row shows the iteration, `write ×2`. The ticket row keeps its Team Phase bar, and each model reporting on the ticket gets a row under it with a bar over its declared phases. Each phase report is a hub report, so the frame redraws. It costs the model one turn per phase change, which ruling 44 accepts.

**Dashboard mode.** `zaf hub --port <n>` serves the same frame as an HTML page on every interface at that port, for anyone on the LAN. It answers GET only, asks for no login and changes nothing, and it prints the page's LAN URL when it starts. The page holds a server-sent events stream and redraws when a report arrives, so it is never staler than the terminal. It runs until it is stopped.

### 5.1 The phase scheduler

Designed for 0.3.0 by epic #421, ruled by the Engineer on 2026-11-10 (§8, rulings of #421 and of the questions raised on it). zaf schedules a task's phases and a model is a worker for one stretch of a task: it runs until the first wait on the outside, then exits and frees its budget, and zaf starts a fresh model for the next phase later. zaf decides and an action does. The tick, the choice of what runs, the budget and the lock stay in zaf; what a phase does is an action, and some start a model while others do not. The team instance goes away: each phase action launches the worker it needs directly (a programmer to implement, a reviewer to review), and zaf does the coordinating. These phases are a task type's own, and are not the Team Phases of §3, which still guard `zaf do` and are derived as before; nor the phases a model declares to the hub (§5).

**A task's phases.** A task type has an ordered list of action names, its phase list, and zaf owns it. The project manifest declares one list for each task type, in a `[phases]` table keyed by the type's name, and a ticket's Type (the forge's issue type, such as Task or Bug) picks the list. A higher manifest layer replaces a type's whole list, as §6 and assumption 11 say of an array. The load refuses an empty list and a name listed twice, and `zaf doctor` names a listed action that no tier provides, one with no kind and one with no `ends`. A task with no type, or a type with no list, is named by the tick and passed over. Phases of one task with no wait between them stay in one model run: a task that reads its brief and then implements is one model, not two (ruling 2), so adjacent model actions form one run whose standing names the first.

**Kind, ends and on-fail.** Each phase action declares three things in its metadata (§3), and zaf reads them and guesses nothing (ruling 23).

| Key | Value | Meaning |
|---|---|---|
| `kind` | `model` | running it starts a model and costs the task's size |
| | `wait` | it waits on the outside, with or without a process of its own, and costs nothing |
| | `zaf` | zaf runs it alone in the tick, with no model, and it costs nothing |
| `ends` | `pr-open`, `review-posted`, `ci-done`, `merged` or `exit` | the fact that ends the phase: one of four the forge answers, or `exit`, the phase's process exited 0; required whenever `kind` is set, refused without it |
| `on-fail` | an action name | only on a `wait`, optional: the model action the tick starts as the task's phase when the wait fails, at the task's size; when it exits, the wait applies again to the new head |

A review can be either a reviewer model or a wait on the outside, by its action's kind, and zaf does not choose. A wait is failed when a check failed at the pull request's head or when its own run at that head exited non-zero. A failed wait that declares no `on-fail` raises a question on its task (the ruling on #425), so the Engineer decides what follows. The key was proposed as `on-red` and ruled as `on-fail`.

**Review, CI and merge.** Review is a wait the launcher runs with no model: it runs `zaf check pr` at the pull request's head as its own detached process, with no budget, ends at `review-posted`, and its `on-fail` is the fix action, a model (the ruling on #452; the challenger's proposal of a model review is declined). `ci` is a wait that ends at `ci-done`. `merge` is a `zaf`-kind action that runs `slice-merge`, which needs a clean `zaf check pr` at the head. A model is started only when CI is red or the review has findings to fix (ruling 6), so ci and merge never start one.

**The standing.** Each tick derives every claimed or ready task's standing from the forge and the phase run folders alone, and nothing about the queue is stored (ruling 5). The current phase is the first listed one whose end fact does not hold for the pull request's current head (the ruling on #424), so a push that moves the head reopens a review posted at the old head. The standing is one of: running (a live phase process, whatever the forge says), ready since a time, waiting, failed (with its `on-fail`, or none, since a time) and done (the pull request merged). `since` is when the previous phase's fact came to hold, or when the task became ready for its first phase, and it is the task's weight (ruling 20). A wait is ready until it has run at the current head, then waiting until its fact holds, and a wait already run at the current head is not launched again. A forge read that fails makes the standing unreadable, named, and the task gets none; zaf never guesses one.

The `exit` fact holds when the action's latest run for the task, started after the previous phase's fact, recorded exit code 0. The launcher that started the phase writes the code into the phase's run folder when the process ends, and the standing reads it (the ruling on #451). The code is a recorded act that ruling 47 keeps alongside the launch record, and the queue is still derived from it and never stored. A phase run folder holds the task, the action, the pull request's head when it started (empty without one), the pid and, once the process has ended, the exit code; a dead pid with no exit code reads as no exit code, never as 0.

**Claims.** Ruling 47's claim, which counts only while its instance runs, would release a task whose model exited at a wait. A task with a recorded pull request is held by its forge facts, not by an instance, so its claim stays until the task is done; the orphan release applies only to a claimed task with no pull request and no running phase (the ruling on #426). A running phase holds its task too. `zaf status` shows an orphan by the same rule.

**The tick.** A tick derives, it does not store, and it never waits for a phase. Each tick, in this order:

1. The `pre-tick` hook runs; a non-zero exit fails the tick (ruling 11).
2. It derives every task's standing, and every epic's stage (§8, ruling 48).
3. It runs every ready `zaf`-kind action, such as merge, one after another with `zaf do`, at no cost to the budget; one that fails is logged and its task is left for the next tick, and the others still run (rulings 6 and 22).
4. It launches every ready `wait` action detached, as its own process, at no cost to the budget, and does not wait for any; one that fails to launch is logged and the others still go (rulings 6 and 22).
5. It raises one question on each task whose wait failed with no `on-fail`, unless the task already has an open question, and charges nothing.
6. It starts at most one model, and never waits for it (ruling 19). The candidates are the ready model phases, the failed waits with an `on-fail` (whose model action is the phase to start) and the epic launches (the planning instance for a signed epic with no tasks, the challenger for a planned one), all in one order (the ruling on #427). An item's weight is how long it has been ready, read from the forge; a signed epic's is the time since its signature and a planned epic's the time since planning ended. The tick walks the order by weight, starts the first item that fits the budget and has its prerequisites met, and the items it passes are pushed back, their weight growing as in priority aging (ruling 20). A phase blocked on an earlier phase is passed over (ruling 4).
7. The `post-tick` hook runs, idle ticks included.

The model the tick starts runs as its own process, so phases of different tasks run at the same time, as many as the budget holds, and phases of one task never overlap (ruling 19). The `zaf`-kind and `wait` actions are not the tick's one start (ruling 22). Planning and the challenger are charged XS and a release E, though the tick does not launch a release yet; the release instance's launch, the zaf service (#353) and the split of `zaf slice`, `zaf check` and `zaf ask` into a decision and an action (#422) are not in this epic.

The budget is that of #419: a number of units for the machine, an absolute cost for each size, a table for each project, and live use read from the process table. It is charged for a phase only while the phase runs, so a task waiting on review or CI holds nothing, and each phase must fit again, at the task's size, when its turn comes (ruling 14). After 30 minutes a passed item holds the budget for itself, the starvation rule of #419, read from the forge and never counted or stored. E costs the budget minus a reserve, so one E runs at a time and small work keeps running beside it; the reserve is configurable and defaults to 6 units (ruling 24). A resumed phase's brief holds only what that phase needs, which zaf builds from the task, the pull request's diff and the new findings (the review comments or the CI failure); the exiting model may leave one hand-off comment on the pull request, and zaf includes it (rulings 3 and 15).

**Hooks.** A hook is an action in a `hooks` folder inside an actions folder, `.zaf/actions/hooks/` in a project and `~/.zaf/actions/hooks/` on a machine, and the project's wins as an action does. It is named after its event, one file for each, and there are three events (rulings 9, 10 and 13). A hook cannot be run as a normal action: `zaf do hooks/pre-tick` is `NOT_RUN`, exit 125, and `zaf do` lists none. The hook's file carries the metadata of §3, and `zaf doctor` names a file whose name is no event.

| Event | Runs | Gets | A failure |
|---|---|---|---|
| `pre-tick` | before each tick | nothing | fails the tick; a hook that fails and does not care returns 0 after its own error handling (ruling 11) |
| `post-tick` | after every tick, idle ones included | the tick's outcome as JSON on stdin: the model started (task, phase action, units) or none, the waits launched, the `zaf` actions run with their exit codes, the questions raised, or that nothing was ready; no exit code of the model, because the tick never waits (ruling 12) | reported, and changes nothing |
| `post-phase` | when a phase's process exits, run by the launcher that started it | the task, the phase action and the process's exit code, as JSON on stdin (ruling 21) | logged, and changes nothing |

**Triggers.** A tick runs in at least three cases (rulings 25 and 26). Every five minutes: `zaf spawn --watch` runs a tick in the foreground each `[spawn] tick_minutes` minutes (five by default, in any project manifest layer) until it is stopped, a tick that fails is logged and the loop goes on, and zaf writes no system timer; the zaf service (#353) replaces it later (the ruling on #428). Every time a phase ends: a task completing is its last phase ending, so the one event covers both, and the launcher that runs `post-phase` then asks for a tick, so freed budget is reused at once. The ask is `zaf spawn --once --no-wait`, which does nothing and exits 0 when another tick holds `spawn.lock`, since that tick reads the same forge; plain `--once` still waits, and `zaf spawn` with neither flag still refuses. A webhook is the third trigger, for waits that end outside zaf: when a review is posted or CI finishes, the forge's webhook asks for a tick at once through the same entry point. A webhook carries no state, because the tick still reads everything from the forge, so a missed one costs at most five minutes, and a wait that ends outside zaf has no process to exit and is found by the five-minute tick. The listener is the zaf service (#353), not this epic.

## 6. The manifest

```toml
[actions]        timeout_minutes = 30                      # optional; absent = 30
[check.pr]       steps = ["ci"]                            # or action steps: ["ci", "action:nodes-check"]
[gate]           light = "zaf do gate-light"               # or a literal line, as today
```

The actions folder is not configurable: `.zaf/actions/` in a project, `actions/` in the global home. The scheduler of §5.1 adds a `[phases]` table, one list of action names for each task type, and `[spawn] tick_minutes`, the minutes between the ticks of `zaf spawn --watch` (five by default); the machine budget and the reserve are `[spawn]` settings in the zaf home, as #419 rules.

## 7. Dogfood: the ways set

Built through the lifecycle on this repo (epic #103, planned, built by team instances on `feature/0.2.0`); 0.1.3 (#99) is folded into it as one release. The ways repo moves before any other project: `ZAF.toml` becomes `.zaf.toml`, the `.gitignore` line `.zaf/` becomes `.zaf/.gitignore`, and `.zaf/actions/` holds the set below, ways' own and nobody else's. Within this epic the `ZAF.toml` rename comes last, because an installed older zaf reads only that name. Any other project (Zurfur) writes its own; this set is a reference to copy, never a dependency.

| Action | Mode, group | Target, phases | Does |
|---|---|---|---|
| `slice-open` | `750` team, release | task: `claim` | a jj workspace and a same-named bookmark at the base; hands back the workspace path |
| `slice-pr` | `750` team, release | task: `build`, `check`, `review` | `commit`, `push`, `pr-open`; hands back the PR number |
| `slice-merge` | `750` team, release | task: `land` | `merge-base`; when the base moved the head, `push` and exit 1, so the team runs `check pr` again (CI is the gate, ruling 39); otherwise `pr-merge`, `zaf board done`, `workspace-drop`; a task already done is refused by its phase |
| `slice-drop` | `750` team, release | task: `build`, `check`, `done` | `workspace-drop` |
| `commit <slice> <message>`, `push <slice>`, `pr-open <slice> <base> <title> <body>`, `pr-merge <pr> [--squash]`, `merge-base <slice> <base>`, `sync <base>`, `tag <name> [rev]`, `workspace-open <slice> <base>`, `workspace-drop <slice>` | `750` team, release | none | the jj and gh steps the four above compose; `pr-merge` retries a merge the forge refuses for a moved base, up to three times, 15 seconds apart, and one already merged is quiet; `tag` pushes the tag and hands back the tagged commit (`rev` defaults to the parent of the working copy) |
| `land <pr>` | `750` team, release | none | `pr-merge <pr>` then `sync main`: the Engineer's `gh pr merge … && jj git fetch && jj new main` |
| `release <version>` | `750` release | none | check `zaf/Cargo.toml` is at `<version>`, `pr-merge <pr> --squash` the integration branch's pull request, `sync main`, `tag zaf-v<version>`; the workflow does the rest |
| `gate-light`, `gate` | `755` | none | the cargo lines of today's `[gate]` |
| `nodes-check` | `755` | none | `nodes check --root "$ZAF_ROOT"`; not a `[check.pr]` step since ruling 39 |
| `ci-wait <pr> [--minutes <n>]` | `755` | none | waits for the pull request's checks at its head and hands back the head commit; a failed check is named last, its job's failed log lines above it; still running after `--minutes` (else `CI_WAIT_MINUTES`, else 20) is exit 3; no checks after `CI_WAIT_GRACE` seconds (90) passes, since `zaf.yml` runs only when `zaf/`, `.zaf/`, `.zaf.toml` or the workflow change (ruling 46) |
| `task-pr <task>` | `755` | none | hands back a task's pull request: the slice record, else an open one, else the latest merged, else the latest closed, matched by a `(#<task>)` title or a `-<task>` branch (ruling 46) |
| `issue-file <epic> <title> <body-file> [<label>...]` | `755` | none | files an issue on the board with the epic's milestone and links it as the epic's sub-issue, handing back the number; a sub-issue with the same title is handed back and nothing filed; filed but unlinked is exit 3. `755` because the planning instance is not a manifest role a group can name (ruling 46) |
| `base-up <branch> [<base>]` | `750` release | none | brings a branch up to its base (`main`) as the remote has them and pushes: a fast-forward, or a merge commit; a conflict lists the paths and moves nothing; the working copy stays (ruling 46) |
| `ship <bookmark> <base> <title> [<body>]` | `700` | none | the Engineer's change in the root checkout onto a base: describe, bookmark and push, `pr-open`, `ci-wait`, `pr-merge --squash` at the head CI passed, `sync <base>`; hands back the pull request number (ruling 46) |

**What an instance may not run.** The ways manifest's `[forbidden] commands` denies `gh pr merge`, `zaf do pr-merge`, `zaf do land`, `zaf do ship` and their `--tier machine` forms in every instance profile. An instance merges only through `slice-merge`, which the phase guards, or `release`, whose mode names only the release role. Nested calls run inside an action's process, where the permission layer does not see them, so those two still work. The Engineer's own session is under no profile, so `zaf do land <pr>` and `zaf do ship` stay theirs. Every `[forbidden]` entry `<tool> <sub...>` is denied in four forms, so a global flag before the subcommand does not get round it: `Bash(<tool> <sub...>)`, `Bash(<tool> <sub...> *)`, `Bash(<tool> -* <sub...>)` and `Bash(<tool> -* <sub...> *)` (the `zaf do` entries keep their own flag forms too). A built-in list of each tool's escape hatches is denied in the same four forms in every generated profile whenever the profile allows that tool with `Bash(<tool> *)`, from `[requires]` or from a gate line: `jj util`, `jj config set`, `jj config edit`, `gh alias`, `gh extension`, `yarn exec`, `yarn node` and `yarn dlx`; and a `--config` or `--config-file` flag anywhere on a `jj` line is denied, because it can define an alias. A tool the profile does not allow adds none. This is not a wall: `gh api` can still reach the merge endpoint, and the forge's ruleset covers only `main`.

Not actions: `/nodes:chart --diff` (a model's job, a skill) and the spawner's missing launches (verbs, their own epic).

## 8. Rulings

Rulings 1 to 19 are of 2026-11-03; 20 to 30 of 2026-11-04.

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
14. **Calling past an override (#124):** was a flag that called the shipped action past an override; with no built-ins (16) it is `--tier machine`.
15. **Moving the manifest (#125):** only an explicit `zaf setup` moves a root `ZAF.toml` to `.zaf.toml`; other verbs read it, warn, and make the homes non-destructively.
16. **Overridable, never wider (#127):** landing steps declare `750` group team, release; a shadowing file never widens the run bits of what it shadows.
17. **No shipped actions (#128):** zaf ships none; whoever set zaf up writes them; two tiers; the ways repo writes its own set, isolated from Zurfur's.
18. **Team Phases (#129):** an action's metadata names the phases it may run in; zaf derives a task's phase from the board and its records, never the environment; phases are the guard that holds, modes guard against mistakes.
19. **Merge conditions (#130):** enforced through phases: `slice-merge` runs in `land`, which needs a clean `zaf check pr` on the PR's current head; no result is not clean.
20. **Slice records (#134):** zaf writes them from the hand-back of `slice-open` and `slice-pr` on exit 0 and forgets them on `slice-merge` and `slice-drop`; `check` and `changes` read the workspace from them.
21. **Merging outside `land` (#135):** `[forbidden]` denies `gh pr merge`, `zaf do pr-merge` and `zaf do land` for instances; they merge through `slice-merge`, and the release instance through the `release` action. The depth guard was rejected because it keys on the environment (principle 8), and leaving the hole was rejected because the forge protects only `main`.
22. **Marking done (#136):** `zaf board done <task>`, refused unless the task is in `land` or its recorded pull request has merged.
23. **A slice action's slice (#137):** a `target: task` run gets `ZAF_SLICE`, `ZAF_PR` and `ZAF_TASK_TITLE`; `[env]` cannot set them.
24. **Result words and `NOT_RUN` (#138):** exit codes stay numbers, because the OS carries only an integer and a script branches on it, and what zaf prints gains words, because a model reads words. A body that never ran is `NOT_RUN`, exit 125, reserved. Verbs keep `0`, `1` and `2`.
25. **`land` needs the workspace at the PR head (planning decision 1):** a workspace ahead of the PR head is not `land`, otherwise `slice-merge`'s `push` merges commits nobody reviewed.
26. **A failed task check stays in `build` (planning decision 2):** only a clean one moves the task to `check`, so no pull request opens on red and `slice-drop` still works.
27. **Drop and reopen (planning decision 4):** `slice-open` is refused past `claim`; forgetting a slice also removes the task's check records; `slice-drop` also runs in `check`.
28. **Accepted as planned:** `slice-merge` on a done task refuses (decision 3); a base merge pushed by `slice-merge` followed by a failed `pr-merge` needs a new `check pr` (5); doctor finds needed names by scanning bodies for a literal `zaf do` (6); the `ZAF.toml` rename comes last (7); `ZAF_BASE` arrives in #140 (8).
29. **The ledger (#147):** zaf holds an SQLite ledger. It is zaf's own state, not a committed file, so no row has to reach `main` and the release has no step for it.
30. **Logs are kept (#158):** a check step's whole output goes to `.zaf/logs/<DATETIME>_<name>.log`, git-ignored, as an action's does (§3). What the step returns is short: a word and the log's path.
31. **Tickets state scope, not files:** a ticket's `Owned paths` becomes `Scope`, the capability it changes in words, with a file or folder named only as a place to start, and `Out of scope`, what it leaves alone and the ticket that has it. The programmer goes where the goal needs and stops at `Out of scope`. `Architecture` goes: `zaf check` takes its symbols from `Interfaces` and looks for them in the whole tree. The reason: 5 of the 8 questions raised during this build came from a closed file list (#149, #157, #158, #166, #173), and each one stops the loop for the Engineer. Tickets written for people state scope as behaviour with explicit exclusions; agent vendors treat a named file as a hint; no measurement supports a closed list.
32. **One behaviour per ticket:** the Goal is one sentence with no "and". Size S expects a pull request under 200 lines and M under 400; anything larger splits. These are planning targets read against the merged pull request, not gates. The numbers come from review research on people and are unproven for agents.
33. **Batch two, limit 45:** `[roles.team] batch = 2`, which matches the two programmers an instance runs at once. The runner limit stays 45 minutes: instances are short-lived by design, so one that reaches the limit is carrying too much work.
34. **Phases and modes, from the build (#234 to #238):** a failed task check with a `.pr` record and no result for the current head is `build`. A mode refusing the owner prints `the Engineer`. A blank `ZAF_INSTANCE` does not parse, so it is refused; `zaf setup` keeps treating blank as none. There is no fixed list of roles: the role a well-formed `ZAF_INSTANCE` names is the caller's and `group` decides its digit, so a project's own role works without a zaf release.
35. **`zaf hub` (the Engineer, 2026-11-04):** the live view of §5 is in 0.2.0, so it is there for the first real dogfood. No tail: it redraws when a report arrives. A bar for each epic, each group and each model's ticket, and a general word for what the model is doing. Groups are `group:<name>` labels, set from the re-plan's map; the planning instance labels what it files, and Unit of Work (#232) can build on them later.
36. **Shadowing, from the build (#264):** a project file shadowing a machine file with invalid metadata is refused, `NOT_RUN` naming both, since the machine file's write bit cannot be read. A refused shadow is refused for running only; `--show` and the listing still show the file, which is how the Engineer sees why. A run of a shadowing file needs both files to permit the caller, each under its own group, because masking digits would let a longer `group` widen it.
37. **The design source (#266):** `[design]` is `repo-dir` on `docs/`, and `docs/index.md` lists `zaf-actions.md` as page `zaf` and `zuri-ai-framework.md` as page `zuri`. With no source `zaf ask` rejects every citation, so all 52 answers up to this ruling were `not settled` with no page cited. Tickets keep naming docs by path, so `brief` stays small and programmers get the design through `zaf ask`.
38. **The release role and slice records, from the build (#272, #273):** the ways manifest has a `[roles.release]` table (opus, xhigh, profile `build`, 200 turns) so `release` can stay in §7's groups; the spawner does not launch it yet. A slice record's workspace is an absolute path to an existing directory, a symlink to a directory passes, and it need not be inside the project. A PR number is ASCII digits above 0 fitting u64: `#12`, padded values and `0` are refused, and `007` is stored as `7`. A missing or malformed task is `FAIL`, exit 2, and writes nothing; `#140` and `140` are the same record. A slice name exiting 0 outside a project is `FAIL`, exit 2.
39. **CI is the only gate (the Engineer, 2026-11-05):** only CI stands between a task's pull request and its merge. The team instance does not run `zaf check <task>` again after the programmer's round, and `[check.pr]` is `["ci"]`, with no per-PR model review. Reviews are paid once per release, x.y.z, on the integration branch's pull request. That review covers the merge-path tickets that were security-flagged (#132, #139, #203, #204, #217, #218, #269). The programmer still ends its round on a clean `zaf check <task>`. `slice-pr` runs from `build`, and `slice-merge` drops `gate-light` (§7). The reason: the 23 tasks merged that day averaged about 80 lines of code each, at a median 9 minutes from claim to merge, and most of that was fixed cost.
40. **Units of work (the Engineer, 2026-11-05):** the rest of 0.2.0 is grouped into units of 2 or 3 tickets. Each unit has one programmer, one workspace and one pull request. Units are chosen so they depend little on each other, and so the work within one is similar. zaf has no unit yet, so each unit is one ticket that folds the others, and the folded tickets are closed pointing at it. `[roles.team] batch = 1` replaces ruling 33's batch, and `[roles.programmer] turns = 150` gives a unit room, so an instance carries one unit and stays inside 45 minutes. The four lanes (`[spawn] slots = 4`) run four units at once. #132 and #195 to #198 move to 0.2.1, and #230 is dropped along with the team's task check. This supersedes ruling 32 for units and brings #232 forward.
41. **Phase facts (#285 to #287):** a board or forge read that fails while deriving a phase refuses the action, `NOT_RUN`. A fact that is absent, such as a pull request with no head or a missing workspace, falls to the earlier phase. A claim has no expiry, because the spawner hands back a timed-out instance's claims (#95), and §3 says `claimed` where it said `live lease`.
42. **Two rulings from the build (#214, #309):** a failed `gh pr merge` exits 1, since the other actions also put forge refusals under 1. #110's CLI wiring is within its scope. A malformed `ZAF_INSTANCE` fails `zaf brief`, and with no machine home the brief lists the project's actions only.
43. **Dashboard mode (the Engineer, 2026-11-05):** `zaf hub --port <n>` serves the hub on every interface for anyone on the LAN, GET only and with no login. The ways board is public on GitHub, so the page adds only the costs and what each model is doing. It redraws on each report through server-sent events (§5). It ships in 0.2.0 as unit U13, after U7.
44. **Models report their phases (the Engineer, 2026-11-05):** each model defines its phases when it starts and reports when its phase changes, and phases may be iterative. A phase report is what redraws the hub and the dashboard. Unlike the tool-call hook, it costs one turn per change. It ships in 0.2.0 as unit U12, and the role briefs (U10) tell each role to declare and report.
45. **The documents say what was built (#325):** the manifest is `.zaf.toml` or `.zaf/config.toml`, and a `ZAF.toml` standing alone still loads, with a warning, until `zaf setup` moves it. `[env]` refuses `ZAF_` names and `CLAUDE_CONFIG_DIR`. `pr-merge` takes `--squash`, which `release` uses. The profile allow-list gains `printenv` and gives no tool rule to a gate segment that starts with a shell or wrapper tool. §4 and §7 name each action's arguments as built, and assumption 2 lists the optional keys.
46. **Actions from the transcripts (the Engineer, 2026-11-08):** the multi-step procedures models ran by hand in this repo, mined from about 130,000 tool calls in the zaf ledger's transcripts, become ways actions (§7): `ci-wait` (checks polled, then failed logs read: about 30 runs), `ship` (describe, bookmark, push, PR, wait, squash-merge, sync: every design-document PR), `issue-file` (create, read the id, link the sub-issue, label: 58 creates by planning), `task-pr` (an issue read, then a PR search: 12 runs) and `base-up` (fetch, move or merge the integration branch, push: 4 runs). The fmt, build, clippy and test chain and `jj commit` recur too, but `gate`, `gate-light` and `commit` already are those actions.
47. **zaf derives its state on read (the Engineer, 2026-11-09, #390):** state that a live source can answer is read from it on every read, never kept in step by hand, because a persisted copy goes stale. Whether an instance runs comes from its run folder and the process table: `.zaf/runs/<slug>/pid` holds the holding process as its pid and start time, the tick's from reservation and then the instance's own, written by the runner, so killing a tick leaves a running instance its slot and its claims, and a pid the OS has handed to another process reads as dead. Slots are the instances that run. A claim counts only while its instance runs; a claim by an instance this machine launched that no longer runs reads as orphaned in `zaf status` and the next tick releases it, while a claim by an instance with no run folder here is left alone. A blocker is done once its issue closes as completed, label or not; closed as not planned or as a duplicate, it is dead, so a task folded into a unit or dropped is closed as not planned. Only what no live source can answer is saved: the instance counter in `spawn.state`, the launch record and exit in each run folder, the guard's spend and the ask log. `spawn.lock` keeps two ticks from planning the same number or slot.
48. **zaf tickets over pluggable backends (the Engineer, 2026-11-09, #393):** a ticket is zaf's own shape, the domain's `Ticket` (id, epic, title, body, state, link), whichever backend holds it; how a backend saves it (labels, Project fields, comments) never crosses the Board port, and every `--json` prints a ticket as one summary (`number`, `epic`, `title`, `state`, `url`). A ticket id is a number shown as `#n`, and the issues backend maps it straight to the issue number, so ids, `Blocked by:` lines, branches and run folders don't change. The Projects backend (`[board] kind = "github-projects"`) uses real issues from the `[board] repo`, added to the Project, and no draft items: the state lives in a single-select Project field, the epic link is the sub-issue parent (the Project's built-in `Parent issue` field shows it), and the release goes in a Project field when U3 first reads it. One contract suite (`zaf/tests/board_contract.rs`) checks every backend the same way: always on the in-memory board, and on a backend's scratch board when its variables name one (`ZAF_CONTRACT_REPO` for issues, with `ZAF_CONTRACT_PROJECT` too for Projects). An epic's states are derived on read: signed (the hash `zaf epic sign` stored still matches its body; the verb refuses inside an instance, judged by process ancestry), planned (it has tasks), challenged (the challenger's report is on it, once per epic, after the first plan) and done (every task done). A task is ready when its epic is signed and challenged, no question is open on it or its epic, and its blockers are done. The release instance never merges into main: it runs every check, opens the release PR and asks a door question, and the Engineer's yes merges and tags. Built on `feature/0.3.0`; the domain names no outer layer (`zaf/tests/architecture.rs`), and its values are newtypes.

### Rulings of #421, the phase scheduler (the Engineer, 2026-11-10)

These are cited as `#421 ruling <n>`, apart from the numbers above. §5.1 says what they build.

1. **A task is an ordered list of phases (#421 ruling 1, 2026-11-10):** zaf owns the list.
2. **A model runs to the first wait (#421 ruling 2, 2026-11-10):** it runs until the first async wait (review, CI), then exits and frees its budget. Phases with no wait between them, such as read brief and implement, stay in one model run.
3. **A fresh model for the next phase (#421 ruling 3, 2026-11-10):** zaf later starts a fresh model for the task's next phase, and its brief holds only what that phase needs.
4. **A blocked phase is passed over (#421 ruling 4, 2026-11-10):** a phase blocked on an earlier phase is passed over.
5. **The queue is derived each tick, never stored (#421 ruling 5, 2026-11-10):** zaf reads every task's current phase from the forge (PR open, review posted, CI green, merged) and runs the ready phase that has waited longest.
6. **zaf runs `ci` and `merge` alone, with no model (#421 ruling 6, 2026-11-10):** a model is started only when CI is red or the review has findings to fix.
7. **The weights are the size budget of #419 (#421 ruling 7, 2026-11-10):** absolute units per size, a table per project, live usage and a starvation rule.
8. **A phase is an action, and the tick stays in zaf (#421 ruling 8, 2026-11-10):** the tick derives each task's phase, picks, checks the budget and holds the lock. What a phase does is an action: some start a model (implement, fix review findings) and some do not (`ci`, `merge`). A task type's phase list is a list of action names.
9. **A tick has a pre hook and a post hook (#421 ruling 9, 2026-11-10):** the Engineer made this the condition for ruling 8: "Add pre and post hooks to ticks and I'll agree with you."
10. **A hook is an action in a `hooks` folder (#421 ruling 10, 2026-11-10):** inside the actions folder (`.zaf/actions/hooks/`). `zaf do` refuses to run one as a normal action.
11. **A failing pre hook fails the tick (#421 ruling 11, 2026-11-10):** "A non-zero fails the tick. A file that fails and doesn't care should return 0 after its own error handling."
12. **The post-tick hook gets the tick's outcome (#421 ruling 12, 2026-11-10):** JSON on stdin of what the tick launched (task, phase, action, units) and the no-model actions it ran, or that nothing was ready. It carries no exit code, because the tick never waits. It runs after every tick, idle ones included, and a failing one is reported and changes nothing.
13. **A hook file is named after its event (#421 ruling 13, 2026-11-10):** `hooks/pre-tick` and `hooks/post-tick`, one file per event; ruling 21 adds `hooks/post-phase`.
14. **The budget is charged per phase, only while the phase runs (#421 ruling 14, 2026-11-10):** a task waiting on review or CI holds nothing, and each phase must fit the budget again when its turn comes, at the task's size.
15. **zaf builds a resumed phase's brief (#421 ruling 15, 2026-11-10):** from the task, the PR's diff and the new findings (review comments or the CI failure). The exiting model may leave one hand-off comment on the PR, and zaf includes it.
16. **The team instance goes away (#421 ruling 16, 2026-11-10):** each phase action launches the worker it needs directly, a programmer for implement and a reviewer for review. zaf does the coordinating.
17. **Phase lists are declared in the manifest, per task type (#421 ruling 17, 2026-11-10):** a ticket's Type picks its list.
18. **`zaf slice`, `zaf check` and `zaf ask` split the same way (#421 ruling 18, 2026-11-10):** zaf decides, an action does, in a later epic (#422), not this one.
19. **A tick starts at most one model, and never waits for it (#421 ruling 19, 2026-11-10):** "a tick grabs the next item in queue and either starts it (if the budget is enough and the dependencies or prerequisites are met) or pushes it back, assigning weight or the like (such as in a scheduler in a computer)." The started phase runs as its own process, so phases of different tasks run at the same time, as many as the budget holds, and phases of one task never overlap.
20. **The weight is derived (#421 ruling 20, 2026-11-10):** it is how long the item has been ready, read from the forge. Each tick orders the items by weight, walks that order, and starts the first one that fits the budget and has its prerequisites met. The items it passes are pushed back, and their weight keeps growing, as with priority aging. Nothing about the queue is stored. After 30 minutes the starvation rule of #419 holds the budget for a passed item.
21. **`hooks/post-phase` runs when a phase's process exits (#421 ruling 21, 2026-11-10, after the simulation):** it gets the task, the phase, the action and the exit code, as JSON on stdin. The launcher that started the phase runs it, so nothing is stored to remember the launch.
22. **Actions that need no model do not count as the tick's start (#421 ruling 22, 2026-11-10):** a tick runs every ready no-model action (`merge`), at no cost to the budget, and may also start one model.
23. **Each phase action declares its kind (#421 ruling 23, 2026-11-10):** it starts a model (costs the task's size), waits on the outside (costs nothing), or is run by zaf alone (costs nothing). zaf reads the kind and guesses nothing; a review can be either a reviewer model or an outside wait, by its action.
24. **E leaves a reserve (#421 ruling 24, 2026-11-10):** E costs the budget minus a reserve, and one E runs at a time, so small work keeps running beside it. The reserve is configurable and defaults to 6 units ("The reserve is configurable. I agree on 6."). This amends #419, where E took the whole budget.
25. **A tick runs in at least two cases (#421 ruling 25, 2026-11-10):** every five minutes, and every time a phase ends ("a tick happens in two places at LEAST: every five minutes or every time a task is completed. Likewise, a phase change informs of it."). A task completing is its last phase ending, so the one event is a phase ending. The launcher that runs `hooks/post-phase` then asks for a tick, so freed budget is reused at once. A wait that ends outside zaf has no process to exit, so the five-minute tick finds it.
26. **A webhook is a third tick trigger (#421 ruling 26, 2026-11-10):** for waits that end outside zaf. When a review is posted or CI finishes, the forge's webhook asks for a tick at once ("Regarding the review or CI, that's where I wanted to introduce webhooks."). The webhook carries no state: the tick still reads everything from the forge, so a missed webhook costs at most five minutes. The listener is the zaf service (#353), not this epic; this epic ships the two triggers of ruling 25 and one entry point that asks for a tick, which the service calls.

Rulings on the questions raised under #421, each the Engineer's, 2026-11-10:

- **#419, the size budget (2026-11-10):** sizes cost absolute units, about 1 GB of allowed RAM each; the defaults are XS 1, S 2, M 4, L 8 and XL 16, with E the whole budget until ruling 24; a project may override the table; zaf charges live use read from every zaf process tree, and a launch fits when current use plus the new size stays inside the budget; once a waiting launch has been ready for 30 minutes and still does not fit, nothing smaller launches until it does, the time read from the forge and never counted or stored; a team instance takes its largest task's size; planning and the challenger are XS and release is E; the budget is set by hand as `[spawn] budget` in `~/.zaf/config.toml`, and unset means no machine cap; a project's `[spawn] slots` stays its ceiling, counted in instances.
- **#424, how a task's phase is derived (2026-11-10):** accepted as proposed. Each phase action declares `zaf ends: <fact>` from a closed set the forge answers (`pr-open`, `review-posted`, `ci-done`, `merged`) or `exit` (its process exited 0). The current phase is the first whose end fact does not hold for the PR's current head, and its weight is the time since the previous phase's fact.
- **#425, what starts when a wait fails (2026-11-10):** accepted, with the key renamed from `on-red` to `on-fail`. A wait action declares `zaf on-fail: <action>`, a model action the tick then starts as the task's phase, at the task's size; when it exits, the wait applies again to the new head. Without `on-fail`, a failed wait raises a question on the task.
- **#426, claims and the orphan release (2026-11-10):** accepted as proposed. A task with a recorded pull request is held by its forge facts, not an instance, and the orphan release applies only to a claimed task with no pull request and no running phase.
- **#427, planning, challenger and release in the one order (2026-11-10):** accepted as proposed. Planning, challenger and release launches count as the tick's one model start: every model launch is one item in the same order, an epic's weight being how long its stage has waited, and each is charged its size (planning and challenger XS, release E).
- **#428, shipping the five-minute tick (2026-11-10):** accepted as proposed. `zaf spawn --watch` ticks in the foreground every `[spawn] tick_minutes` (default 5) until stopped; the zaf service (#353) replaces it later; zaf writes no system timer.
- **#451, reading the exit end fact (2026-11-10):** accepted as proposed. The launcher of #438 writes the phase's exit code into the phase's run folder, and `derive_standing` reads it. The exit code is a recorded act, which #409 keeps alongside the launch record; the queue is still derived from it and never stored.
- **#452, who runs `zaf check pr` for merge (2026-11-10):** review is a wait action that the launcher runs detached as its own process, with no model and no budget: it runs `zaf check pr` at the PR's head, ends at `review-posted`, and `fix` is its `on-fail`. A model starts only when the check fails (ruling 6). The challenger's proposal of a model review phase is declined.

## 9. Assumed by Claude, open to veto

1. The seven Team Phases of §3 and what derives each; `check` records the task check's phase and `check pr` the PR's head, which #131 already files.
2. The optional metadata keys, as built, are `mode` (three octal digits, default `755`), `group` (default none), `target` (only `task`; omit it for none), `phases` (needs `target: task`; default any) and `timeout` (whole minutes, at least 1; default `[actions] timeout_minutes`, 30). A key declared twice, an unknown key and an unknown phase are refused. Release-level phases (framework §2.5) for `release` come later.
3. `ZAF_BASE` is set from `ZAF_VERSION` and the branch pattern, so slice scripts need not know the pattern.
4. The metadata syntax of §3: `<comment marker> zaf <key>: <value>` lines opening the file.
5. `zaf do` with no name lists; `ZAF_HOME` moves the global home; `ZAF_DEPTH` guards recursion at 8; `--tier machine` is the only tier flag.
6. The ways set of §7, its modes and phases, is Claude's first cut; the planning instance may add names, never drop the four `slice-*`.
7. `~/.zaf/config.toml` is read as machine-wide defaults below the project's manifest; what `.zaf/local.toml` holds today may move there later, not in this epic.
8. `.zaf/roles/` is committed with the project; it holds instructions, not secrets.
9. A root `ZAF.toml` beside a `.zaf.toml` or `.zaf/config.toml` is an error in every verb, not only in doctor.
10. `command:` check steps and the `zaf slice …` verbs stay one release beside `action:` and `zaf do slice-…`, then retire; `changes` stays a verb over a Vcs port shrunk to diffs.
11. The manifest combines table by table, property by property; an array is replaced, not merged. `[env]` applies to every gate, test, check and action run and can set no `ZAF_*` variable (whatever its case) and not `CLAUDE_CONFIG_DIR`, which zaf sets for each profile so that no manifest points an instance away from its profile's denies.
12. The group digit covers exactly the roles named, and a role not named is others; programmers share their team's identity until they run as their own process, which no task in this epic needs.
13. The unit and the verb of §10, and the ledger's place and per-run shape, are Claude's design, for the Engineer's ask of 2026-11-04.
14. The ledger uses `rusqlite` with SQLite bundled, zaf's first C dependency. "Every backend is reached by shelling out" still holds, because the ledger is zaf's own state, not a backend, and no `sqlite3` needs installing.
15. Until 0.2.0 is installed, tickets keep the labels installed 0.1.2 requires, because its parser needs them and refuses an unknown field: scope in words under `Owned paths`, `Architecture: none`, and out of scope as the first acceptance line. 0.2.0 reads both forms.
16. With no file lists, tickets that touch the same area are ordered with `Blocked by`. A collision that still happens is met at merge: the base is merged, the gate runs again, and a conflict goes back as a round. No source publishes a conflict rate for this, so ours is read from the rounds per task after the re-plan.
17. `zaf brief` prints a map for each place a ticket names to start and each node once. Today it repeats the chain per path, and 74 to 83% of that section's lines are repeats.
18. The hub's reports go to `.zaf/hub/reports`, git-ignored. That a `PostToolUse` hook fires for a subagent's tools, and its payload's field names, come from Claude Code's documentation and are untested here; the Engineer's session checks a real payload when 0.2.0 is installed.
19. The phase verb is `zaf progress plan <task> <phase>… [--role <role>]` and `zaf progress at <task> <phase> [--role <role>]`. The role defaults to the one `ZAF_INSTANCE` names, and a programmer passes `--role programmer`, because it runs under its team's `ZAF_INSTANCE`. A phase the plan did not declare is refused. The dashboard serves with the standard library's `TcpListener`, so zaf gains no dependency.
20. The Projects board (ruling 48, U4 of #393): the state field is named `zaf state` unless `[board] state_field` names another, and its options are matched to states without regard to case. zaf creates the field, with Ready, Claimed, Blocked, Question and Done, on the first write to a Project that lacks it; a field missing an option refuses that state and says to add it. `[board] project` is the Project's number and `owner` its owner, the repository's owner by default. The kind labels (`task`, `epic`, `question`, `door`, `group:<name>`) stay on the issues, so `gh issue list` still finds a ticket's kind; only the state moves. GitHub's own `Status` field is left alone. A GraphQL call GitHub calls transient (two writes to one Project at once) is sent again up to three times. `zaf board file` is how a planning instance files a task on any backend; it is idempotent by title within the epic.
21. The spawner's stage branches (U3 of #393): a planning instance is launched for a signed epic with no tasks once per signature (a planning run reserved after the signature answers it; to plan again, the Engineer edits the epic and signs it again), and the challenger once per epic, ever (F3). While a planning instance runs on an epic, nothing else launches on it. A question open on the epic holds every launch on it. A run with no task holds a slot like any other. Planning (80 turns) and the challenger (60) get pack defaults: opus, xhigh, profile `build`. Both list an epic's tasks with `zaf epic show <epic>`, so no brief needs the board's own tool. A re-plan after a ruling and the release launch are not launched by the spawner yet. How epics already in flight on upgrade are treated is the Engineer's call, asked 2026-11-10; until it is answered, they wait unsigned like any other.
22. The phase scheduler's names (epic #421): the section 5.1 words `standing`, `ready since`, `waiting`, `failed` and `done` are Claude's terms for what the rulings derive, and the manifest keys `[phases]` (a table of ticket Type to a list of action names) and `kind`, `ends` and `on-fail` (the rulings on #424 and #425) are the ones the tasks of the epic build. The `ends` value `exit` and the exit code in the phase run folder are the ruling on #451. The three hook names are ruling 21's and rulings 9 to 13's. What the rulings leave open is assumed here: the post hooks get one JSON object per event; a phase list that names an action with no file is refused by `zaf doctor`. Each of these is open to veto, and the tasks of the epic may settle them. A phase action that declares no `kind` is refused by `zaf doctor`, never read as `model`, and an action outside every phase list needs no `kind` (the Engineer, 2026-11-10, vetoing the default first assumed here; ruling 23: zaf guesses nothing).

## 10. Is it saving tokens

Raw totals cannot answer this, because they move with how much work was done. The question is whether the same work costs less than it did.

**The unit.** Estimated dollars per merged task, per epic, because dollars weight the token kinds by price. Each epic also shows tokens by kind, turns per task and context at turn one and at the end. The figure to compare is the median by size (S, M), since one large task skews a mean. Everything the epic cost counts: planning and re-plans, team instances, programmers and keepers.

**The verb.** `zaf report --epics` prints one row per epic with its share per role, from the ledger, a subagent counted through its parent session, and the tasks done from the board. It also prints one line for what ran outside any epic, because that spend is invisible otherwise. This is the tokens-per-role table the framework's verb list promises and `zaf report` does not yet print.

**The ledger.** Transcripts expire (30 days by default), so zaf holds the numbers itself: an SQLite database at `~/.zaf/ledger.db`, one row per run, with its project, role, epic, tasks, model, turns, tokens by kind, context at turn one and at the end, and times. `zaf report` and each `zaf spawn` tick ingest the transcripts they find, updating a run while its transcript still grows, and a row outlives its transcript. Dollars are computed on reading, so a price change cannot look like a saving. The ledger is machine state, never committed, so a rebuilt machine starts empty unless it is backed up.

**Reading it.** An epic saved tokens when its median per size is lower than the last epic's on at least five tasks of that size; with fewer, the row says `too few`, not a verdict. Different work still differs, so a change that claims a large saving gets a benchmark: one old task re-run from its parent commit under the current pack, the only direct comparison, at the cost of that run.

**First reading, 2026-11-04,** from `zaf report --hours 1000 --match "#<epic>"`, so by prompt text, unsplit by size and without planning runs whose prompts name no epic:

| Epic | Tasks done | Est. $ | $ per task | Turns per task | Programmer context at turn 1 |
|---|---|---|---|---|---|
| #37, zaf 0.1.1 | 14 | 17 | 1.2 | 49 | 26k |
| #76, zaf 0.1.2 | 6 | 9 | 1.5 | 56 | 26k |
| #103, zaf 0.2.0 | 0 | 8 | planning only, three runs | | |

Turn-one context has fallen, from a 58k subagent median in the 2026-10-27 baseline to 26k. Per-task cost is not yet a verdict: 0.1.2 cost more per task than 0.1.1, on too few tasks and different work. Over the same week, `zaf report --all` shows $214, of which $33 is this project's instances; the other $181 ran outside them, so instances are the smaller part of the spend.
