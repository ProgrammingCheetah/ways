# zaf actions

Draft v2, 2026-11-03, with the Engineer's rulings of this session (§8). An **action** is a runnable file that `zaf` names, lists and runs through `zaf do <name>`. Actions are how a project or a machine makes zaf its own: zaf is the abstraction layer over them. It owns the names, the listing, the environment, the log and one exit contract; what a name *does* is the action behind it, and every place a verb shells out today is such a name with a built-in default. What Claude assumed beyond the rulings is listed in §9, open to veto.

## 1. Principles

1. **Make it yours.** A project or a machine replaces what a name does by putting a file of that name in its actions folder. Nothing in the binary is the last word on a step that shells out.
2. **zaf is an abstraction over actions.** A verb is a stable name and a composition of named steps; each step resolves to an action. `zaf do push` runs the project's `push` when it has one, else the machine's, else zaf's own, and so does every verb that pushes.
3. **Three tiers, resolved by name.** The project's `.zaf/actions/`, then the machine's `~/.zaf/actions/`, then zaf's built-in defaults. The first file found wins; `zaf do` lists every name with the tier it resolves to.
4. **Composable.** An action's body may call `zaf do <other>` and any verb: `zaf do something` may be `zaf do something_1; zaf do something_2`. Each call gets its own log; an outer log names the inner ones.
5. **One contract.** An action exits 0 on success and non-zero on failure, 1 being the canonical failure; every non-zero code it can return is declared in its metadata with its meaning. Nothing else is promised about its output: the log holds it.
6. **Metadata and body.** Every action file has two parts: the metadata, which names it to zaf (description, exit meanings, limits), and the body, which runs. A file without metadata is not an action.
7. **A verb is not an action.** What holds state or logic (the board's leases and labels, the ask ladder and quote check, the spawner's tick, the transcript parser) stays in zaf and is not overridable; what shells out is.

## 2. The tiers and the homes

**The project's `.zaf/`, committed.** Projects gain `.zaf/` as a folder of their own. The manifest is `.zaf.toml` at the project root, `.zaf/config.toml`, or both: when both exist they combine, and `.zaf.toml` wins on a property they both set. `ZAF.toml` is retired.

```
.zaf.toml                 the manifest at the root (optional; wins on conflicts)
.zaf/config.toml          the manifest inside the folder (optional; the two combine)
.zaf/actions/<name>       the project's actions, committed
.zaf/roles/               per-role project instructions pasted into briefs, committed
.zaf/.gitignore           written by zaf setup, never by hand: the machine state below
.zaf/local.toml           machine-private overrides (ignored)
.zaf/logs/                action logs (ignored)
.zaf/runs/ .zaf/profiles/ .zaf/workspaces/ .zaf/slices/ .zaf/spawn.state .zaf/guard.json .zaf/asklog.jsonl   as today (ignored)
```

**The machine's `~/.zaf/`, the global home.** Laid out like a project's `.zaf/`: `config.toml` (machine-wide defaults every project here inherits, below the project's own manifest) and `actions/` (the machine's actions, seen by every instance on it). Its place is configurable (`ZAF_HOME`); the default is `~/.zaf/`. `zaf setup` creates it, with a commented `config.toml` and an empty `actions/`, and creates the project's `.zaf/` with its `.gitignore`; any verb that finds either missing runs that setup first, so the Engineer may call `zaf setup` from the beginning or let the first verb do it. A `ZAF.toml` still at the root is moved to `.zaf.toml` by setup, once, and said so.

**zaf's built-in defaults.** The bottom tier: one default per named step (§4), shipped in the binary, listed by `zaf do` as tier `zaf`, each with the same metadata as a file. `zaf do --show <name>` prints the resolved action: a file as is, a built-in as its metadata and the steps it takes, so a project copies it into `.zaf/actions/<name>` and edits.

**Precedence.** Manifest: `.zaf.toml` > `.zaf/config.toml` > `~/.zaf/config.toml` > the pack's defaults, property by property. Actions: project > machine > built-in, and `zaf doctor` names every shadowing.

The zip test (framework §7.6) gains one named exception: `~/.zaf/` is the machine's zaf home, like its credentials. A project whose steps resolve to a machine action depends on that machine; `zaf doctor` reports each one, so the dependency is visible.

## 3. The file

Two parts. The **metadata** is the comment block that opens the file, right after the shebang when there is one: every line is the language's comment marker, then `zaf`, then a key, a colon and a value. The **body** is everything after the first line that is not metadata.

```sh
#!/bin/sh
# zaf description: land a pull request and move the working copy onto main
# zaf 1: the forge refused the merge (checks failing or review missing)
# zaf 2: the fetch failed; main was not moved
# zaf timeout: 10
set -e
zaf do merge "$1"
zaf do sync
```

| Key | Required | Meaning |
|---|---|---|
| `description` | yes | one line, what `zaf do` prints beside the name |
| `<n>` (a non-zero integer) | at least `1` | what that exit code means; every code the body can return is declared |
| `timeout` | no | minutes before the run is killed; default `[actions] timeout_minutes` (30) |

A file with no metadata, no description or no declared code is refused by `zaf do` (exit 1, naming the line) and reported by `zaf doctor`. A comment marker is any run of non-alphanumeric characters (`#`, `//`, `--`, `;`, `REM`), so the format is the same in every language.

**Name.** The file name, as is: `.zaf/actions/land` is `land`. No spaces; hyphens, dots and underscores are fine; no extension is dropped.

**Environment.** Run in the caller's current directory (the project root for a hand run, the slice workspace for a programmer), with `ZAF_ROOT` (the project root), `ZAF_HOME`, `ZAF_ACTION` (the name), `ZAF_DEPTH` (how many `zaf do` calls are above this one), and `ZAF_INSTANCE` and `ZAF_VERSION` passed through. Arguments after the name go to the body as given.

**Output and exit.** Everything the body writes, stdout and stderr, goes to `.zaf/logs/<DATETIME>_<ZAF_ACTION_NAME>.log` (`~/.zaf/logs/` outside a project), `<DATETIME>` as `YYYYMMDDTHHMMSSZ`. On exit 0 `zaf` prints nothing. On a non-zero exit it prints `<name>: exit <n>, <its declared meaning>, log <path>` and the last ten lines of the log; an undeclared code prints `exit <n>, undeclared`. The code passes through unchanged; a file that cannot start, or a run past its time bound, is `2`. `--more` prints the whole log after the run; `--json` gives name, tier, exit, meaning, seconds and log. A nested `zaf do` writes its own log and its path into the outer one.

## 4. The verbs, as compositions

Each verb keeps its name, its exit codes and its logic; the steps where it shells out become named actions with a built-in default. The first names:

| Verb | Stays in zaf | Named steps (built-in default, overridable) |
|---|---|---|
| `board ready\|claim\|report\|question\|ruling` | leases, labels, comment markers, through the Board port | none: the board is state, not a step |
| `ask`, `asklog` | the ladder, the quote check, the log | none |
| `brief` | the pack's composition; **it lists the project's actions** | none |
| `check <task>` / `verify` | the engine, the dedupe, the record | `gate`, `gate-light`, `test <name>`; a `[check.*]` step may be `action:<name>` beside `command:<cmd>` |
| `check pr:<n>` | waiting on CI, bots and runners | `action:<name>` steps |
| `changes` | the VCS port | none |
| `slice open` | the slice's naming and record | `workspace-open <slice> <base>` |
| `slice pr` | what to publish, the PR's record | `commit`, `push <slice>`, `pr-open <slice> <base>` |
| `slice merge` | the merge conditions, the task's `done` | `sync <base>`, `gate`, `push <slice>`, `pr-merge <pr>`, `workspace-drop <slice>` |
| `slice drop` | the record | `workspace-drop <slice>` |
| `spawn`, `status`, `guard`, `report` | all of it | none |
| `doctor` | the manifest's validator; **it checks every action** (metadata, runnable, shadowing, each `action:` step naming one that exists) | none |
| `init`, `setup`, `profile init` | they write the project's own files; `setup` makes the two homes (§2) | none |
| `[gate]` | the gate port keeps its levels | its lines may be `zaf do gate`; the profile allow-list still derives its tool rules from a literal line, so a project that wants both keeps the line |
| `[runners.<name>]` | the template and its placeholders | none now |

The release brief's "merge and tag" becomes the project's `release` action; a hand-made PR lands with `zaf do land <pr>`; `[check.pr]` names `action:nodes-check` instead of a line.

## 5. The verb added

| Verb | Use it to |
|---|---|
| `zaf do` | list every action: name, tier, description; `--json` for the spawner and the briefs |
| `zaf do <name> [args…]` | run one; its exit code, its meaning on failure, its log |
| `zaf do --show <name>` | print the action a name resolves to, to copy into the project and edit |

Exit codes as every verb: `0` done, `1` refused (no such action, a file without metadata or not runnable, a declared-code error in its metadata), `2` the file could not start or timed out, and the action's own declared code otherwise.

## 6. The manifest

```toml
[actions]        timeout_minutes = 30                      # optional; absent = 30
[check.pr]       steps = ["ci", "action:nodes-check"]
```

The actions folder is not configurable: `.zaf/actions/` in a project, `actions/` in the global home.

## 7. Dogfood: the first actions

Built through the lifecycle on this repo (an epic on the board, planned, built by a team instance). The ways repo moves first: `ZAF.toml` becomes `.zaf.toml`, the `.gitignore` line `.zaf/` becomes `.zaf/.gitignore`, and `.zaf/actions/` holds the project's own.

| Action | Tier | Does |
|---|---|---|
| `land <pr>` | built-in | `zaf do pr-merge <pr>` then `zaf do sync main`: the Engineer's repeated `gh pr merge … && jj git fetch && jj new main` |
| `release <version>` | project | check `zaf/Cargo.toml` is at `<version>`, `zaf do tag zaf-v<version>`; the workflow does the rest |
| `nodes-check` | machine | `nodes check --root "$ZAF_ROOT"`; the `[check.pr]` step of every charted repo on this machine |

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

## 9. Assumed by Claude, open to veto

1. Built-in defaults are zaf's own code behind a name (today's jj and gh adapters), not scripts; `--show` prints their metadata and steps. Defaults as shipped scripts is the alternative.
2. The metadata syntax of §3: `<comment marker> zaf <key>: <value>` lines opening the file; `timeout` is the only optional key now.
3. `zaf do` with no name lists; `ZAF_HOME` moves the global home; `ZAF_DEPTH` guards recursion at 8.
4. The named steps of §4 are the first set; the planning instance may add or merge names, never remove the five the dogfood needs (`gate`, `push`, `pr-merge`, `sync`, `tag`).
5. `~/.zaf/config.toml` is read as machine-wide defaults below the project's manifest; what `.zaf/local.toml` holds today may move there later, not in this epic.
6. `.zaf/roles/` is committed with the project; it holds instructions, not secrets.
7. `zaf setup` moves a root `ZAF.toml` to `.zaf.toml` once; `zaf doctor` refuses a project that still has both.
8. On exit 0 nothing is printed; a value an action must hand back is read from its log with `--more` or `--json`.
9. `command:` check steps stay one release beside `action:`, then retire.
10. The manifest combines table by table, property by property; an array is replaced, not merged.
