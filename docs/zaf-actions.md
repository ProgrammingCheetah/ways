# zaf actions

Draft v11, 2026-11-04, with the Engineer's rulings and the answers to the planning questions (§8). An **action** is a runnable file that `zaf` names, lists and runs through `zaf do <name>`. Actions are how a project or a machine makes zaf its own, and zaf is the abstraction layer over them: it owns the names, the listing, the environment, the log, the modes, the phases and one exit contract; what a name *does* is the action behind it. zaf ships none: whoever sets zaf up writes them, and every place a verb shells out is a name the project or the machine must provide. What Claude assumed beyond the rulings is listed in §9, open to veto.

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
.zaf/runs/ .zaf/profiles/ .zaf/workspaces/ .zaf/slices/ .zaf/spawn.state .zaf/guard.json .zaf/asklog.jsonl   as today (ignored)
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

**Team Phases.** A task's phase is state zaf derives, never reads from the environment: from the board (labels, the lease) and from zaf's own records (the slice, the check records). An action with `target: task` and `phases:` runs only while the task its first argument names is in one of them; outside, `NOT_RUN <name>: #<task> is in <phase>, not <phases>`, exit 125.

| Phase | The task is | Derived from |
|---|---|---|
| `claim` | claimed under a live lease, no slice yet | the `claimed` label and the lease comment |
| `build` | its slice open, a programmer at work (or a fix round) | the workspace and `.zaf/slices/<task>` exist |
| `check` | its task checks have passed on the workspace; a must-fix or a failed step stays in `build` | the last `zaf check <task>` record, clean |
| `publish` | its pull request open, review steps not yet run | `.zaf/slices/<task>.pr` |
| `review` | the PR's steps have run; a must-fix returns it to `build` | the last `zaf check pr` record |
| `land` | the last `zaf check pr` result for the PR and its current head is clean, and the workspace's latest commit is that head; no result is not clean | that record's head against the PR's head and the workspace's latest commit |
| `done` | merged | the `done` label |

The first rule that holds decides: the `done` label is `done`; a `check pr` record clean for the PR's current head, with the workspace's latest commit at that head, is `land`; a clean record with the workspace ahead of it, or a must-fix record, is `build`, or `check` when a clean task check is newer; a failed step is `review`; a `.pr` record with no result for the current head is `publish`, or `check` when a clean task check is newer than the `.pr` record, or `build` when the last task check failed; a clean task check is `check`; a workspace record is `build`; the `claimed` label with a live lease is `claim`; anything else is `unclaimed`, which no `phases:` may name.

**The records.** zaf writes them, never a script. When `slice-open` or `slice-pr` exits 0, zaf stores what it handed back in `.zaf/slices/<task>` (the workspace path) or `.zaf/slices/<task>.pr` (the number). When `slice-merge` or `slice-drop` exits 0, zaf forgets both and the task's check records, so a drop returns the task to `claim` and a reopened slice starts clean. `zaf check` and `zaf changes` read the workspace from the record and fill the base from `ZAF_VERSION` and the branch pattern. These are plain files, so this stops a script's mistake, not a process set on forging them.

**Name.** The file name, as is: `.zaf/actions/land` is `land`. No spaces; hyphens, dots and underscores are fine; no extension is dropped.

**Environment.** Run in the caller's current directory (the project root for a hand run, the slice workspace for a programmer), with `ZAF_ROOT` (the project root), `ZAF_HOME`, `ZAF_ACTION` (the name), `ZAF_DEPTH` (how many `zaf do` calls are above this one), `ZAF_BASE` (the integration branch, from `ZAF_VERSION` and the branch pattern) and `ZAF_INSTANCE` and `ZAF_VERSION` passed through. A `target: task` run also has `ZAF_SLICE` (from the task branch pattern), `ZAF_PR` (from the record, when one exists) and `ZAF_TASK_TITLE` (from the board). A manifest `[env]` table applies to every action run but can set none of these. Arguments after the name go to the body as given.

**Handing a value back.** An action that stands in for a verb's step hands its value back as the last non-empty line of its log: `pr-open` a pull request number, `commit` a commit id, `slice-open` the workspace path. `merge-base` exit 1 means conflicted, with the paths in the log; any other non-zero is a backend failure.

**Output and exit.** Everything the body writes, stdout and stderr, goes to `.zaf/logs/<DATETIME>_<ZAF_ACTION_NAME>.log` (`~/.zaf/logs/` outside a project), `<DATETIME>` as `YYYYMMDDTHHMMSSZ`. On exit 0 `zaf` prints nothing. When the body ran and exits non-zero, it prints `FAIL <name>: exit <n>, <its declared meaning>, log <path>` and the last ten lines of the log; an undeclared code prints `exit <n>, undeclared`. The code passes through unchanged. A run past its time bound did run, so it is `FAIL`, exit 2. When the body never ran (§5), it prints `NOT_RUN <name>: <reason>` and exits 125. A body that exits 125 because a nested action did not run is `FAIL`, `exit 125, a nested action did not run`, and the code passes through. `--more` prints the whole log after the run; `--json` gives result (`OK`, `FAIL` or `NOT_RUN`), name, tier, exit, meaning, seconds and log. A nested `zaf do` writes its own log and its path into the outer one. A `zaf check` step that runs a process logs the same way (ruling 30).

## 4. The verbs, as compositions

Each verb keeps its name, its exit codes and its logic; the steps where it shells out are names the project or the machine provides. A step with no action exits 2, `missing action <name>`.

| Verb | Stays in zaf | Named steps the project provides |
|---|---|---|
| `board ready\|claim\|report\|question\|ruling\|done` | leases, labels, comment markers, through the Board port; `done <task>` sets the `done` label, drops `ready` and `claimed`, and refuses unless the task is in `land` or the pull request recorded for it has merged | none: the board is state, not a step |
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

The release brief's "merge and tag" is the project's `release` action; the Engineer's hand-made PR lands with `zaf do land <pr>`; `[check.pr]` names `action:nodes-check` instead of a line. The jj and gh code the slice verbs used leaves zaf for the project's scripts; the Vcs port keeps what `changes` needs.

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

It redraws when a report arrives, not on a timer. zaf verbs report that state changed and a hook in the profiles reports each tool call, so no model spends a turn reporting. Between reports only the clocks move and nothing reads the network. Outside a terminal it draws one frame and exits, so a model cannot hang on it.

## 6. The manifest

```toml
[actions]        timeout_minutes = 30                      # optional; absent = 30
[check.pr]       steps = ["ci", "action:nodes-check"]
[gate]           light = "zaf do gate-light"               # or a literal line, as today
```

The actions folder is not configurable: `.zaf/actions/` in a project, `actions/` in the global home.

## 7. Dogfood: the ways set

Built through the lifecycle on this repo (epic #103, planned, built by team instances on `feature/0.2.0`); 0.1.3 (#99) is folded into it as one release. The ways repo moves before any other project: `ZAF.toml` becomes `.zaf.toml`, the `.gitignore` line `.zaf/` becomes `.zaf/.gitignore`, and `.zaf/actions/` holds the set below, ways' own and nobody else's. Within this epic the `ZAF.toml` rename comes last, because an installed older zaf reads only that name. Any other project (Zurfur) writes its own; this set is a reference to copy, never a dependency.

| Action | Mode, group | Target, phases | Does |
|---|---|---|---|
| `slice-open` | `750` team, release | task: `claim` | a jj workspace and a same-named bookmark at the base; hands back the workspace path |
| `slice-pr` | `750` team, release | task: `check`, `review` | `commit`, `push`, `pr-open`; hands back the PR number |
| `slice-merge` | `750` team, release | task: `land` | `merge-base`, `gate-light`, `push`, `pr-merge`, `zaf board done`, `workspace-drop`; a task already done is refused by its phase |
| `slice-drop` | `750` team, release | task: `build`, `check`, `done` | `workspace-drop` |
| `commit`, `push`, `pr-open`, `pr-merge`, `merge-base`, `sync`, `tag`, `workspace-open`, `workspace-drop` | `750` team, release | none | the jj and gh steps the four above compose |
| `land <pr>` | `750` team, release | none | `pr-merge` then `sync main`: the Engineer's `gh pr merge … && jj git fetch && jj new main` |
| `release <version>` | `750` release | none | check `zaf/Cargo.toml` is at `<version>`, `pr-merge` the integration branch's pull request, `sync main`, `tag zaf-v<version>`; the workflow does the rest |
| `gate-light`, `gate` | `755` | none | the cargo lines of today's `[gate]` |
| `nodes-check` | `755` | none | `nodes check --root "$ZAF_ROOT"`; the `[check.pr]` step |

**What an instance may not run.** The ways manifest's `[forbidden] commands` denies `gh pr merge`, `zaf do pr-merge`, `zaf do land` and their `--tier machine` forms in every instance profile. An instance merges only through `slice-merge`, which the phase guards, or `release`, whose mode names only the release role. Nested calls run inside an action's process, where the permission layer does not see them, so those two still work. The Engineer's own session is under no profile, so `zaf do land <pr>` stays theirs. This is not a wall: `gh api` can still reach the merge endpoint, and the forge's ruleset covers only `main`.

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
14. **Calling past an override (#124):** was `zaf do --builtin <name>`; with no built-ins (16) it is `--tier machine`.
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
12. The group digit covers exactly the roles named, and a role not named is others; programmers share their team's identity until they run as their own process, which no task in this epic needs.
13. The unit and the verb of §10, and the ledger's place and per-run shape, are Claude's design, for the Engineer's ask of 2026-11-04.
14. The ledger uses `rusqlite` with SQLite bundled, zaf's first C dependency. "Every backend is reached by shelling out" still holds, because the ledger is zaf's own state, not a backend, and no `sqlite3` needs installing.
15. Until 0.2.0 is installed, tickets keep the labels installed 0.1.2 requires, because its parser needs them and refuses an unknown field: scope in words under `Owned paths`, `Architecture: none`, and out of scope as the first acceptance line. 0.2.0 reads both forms.
16. With no file lists, tickets that touch the same area are ordered with `Blocked by`. A collision that still happens is met at merge: the base is merged, the gate runs again, and a conflict goes back as a round. No source publishes a conflict rate for this, so ours is read from the rounds per task after the re-plan.
17. `zaf brief` prints a map for each place a ticket names to start and each node once. Today it repeats the chain per path, and 74 to 83% of that section's lines are repeats.
18. The hub's reports go to `.zaf/hub/reports`, git-ignored. That a `PostToolUse` hook fires for a subagent's tools, and its payload's field names, come from Claude Code's documentation and are untested here; the Engineer's session checks a real payload when 0.2.0 is installed.

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
