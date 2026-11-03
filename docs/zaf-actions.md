# zaf actions

Draft v1, 2026-11-03, with the Engineer's rulings of this session (§8). An **action** is a runnable file that `zaf` names, lists and runs. It is how a project, or the machine, gives the instances a step that is not a verb: the tag that releases a binary, the landing of a hand-made PR, a check the forge does not run. Verbs stay what they are (§4); actions carry the project-specific *how* out of the briefs and out of the Engineer's hands, so the pack stays project-blind and nothing is typed twice. What Claude assumed beyond the rulings is listed in §9, open to veto.

## 1. Principles

1. **Anything runnable.** A file with a shebang or the executable bit, in any language. `zaf` adds nothing to it but a name, a listing, an environment and a log.
2. **Two homes, one listing.** *Global* actions live in the machine's zaf home, `~/.zaf/`, and every instance on the machine sees them; *local* actions live in the project's `.zaf/actions/`, are committed, and travel with it. `zaf do` lists both, with the source.
3. **Named through zaf, called through zaf.** The file name is the action name: `.zaf/actions/land` is `land`. No spaces; hyphens, dots and underscores are fine; no extension is dropped. Instances never call the file; they call `zaf do <name>`, which runs under the profile's `zaf` allow rule, so no action needs a permission of its own.
4. **Terse like a verb.** Everything an action prints goes to its log; a pass prints nothing and exits 0; a failure prints the exit and the log path. Depth stays in the log.
5. **A verb is not an action.** A verb goes through a port (board, VCS, forge, design, runner, gate) and is backend-blind; an action is project-specific or machine-specific by nature. Nothing that needs a port becomes an action (§4).

## 2. The two homes

**The project's `.zaf/`, committed.** Projects gain `.zaf/` as a folder of their own. The manifest is `.zaf.toml` at the project root, `.zaf/config.toml`, or both: when both exist they combine, and `.zaf.toml` wins on a property they both set. `ZAF.toml` is retired.

```
.zaf.toml                 the manifest at the root (optional; wins on conflicts)
.zaf/config.toml          the manifest inside the folder (optional; the two combine)
.zaf/actions/<name>       local actions, committed
.zaf/roles/               per-role project instructions pasted into briefs, committed
.zaf/.gitignore           written by zaf setup, never by hand: the machine state below
.zaf/local.toml           machine-private overrides (ignored)
.zaf/logs/                action logs (ignored)
.zaf/runs/ .zaf/profiles/ .zaf/workspaces/ .zaf/slices/ .zaf/spawn.state .zaf/guard.json .zaf/asklog.jsonl   as today (ignored)
```

**The machine's `~/.zaf/`, the global home.** Laid out like a project's `.zaf/`: `config.toml` (machine-wide defaults every project here inherits, below the project's own manifest) and `actions/` (global actions). Its place is configurable (`ZAF_HOME`); the default is `~/.zaf/`. `zaf setup` creates it, with a commented `config.toml` and an empty `actions/`, and creates the project's `.zaf/` with its `.gitignore`; any verb that finds either missing runs that setup first, so the Engineer may call `zaf setup` from the beginning or let the first verb do it. A `ZAF.toml` still at the root is moved to `.zaf.toml` by setup, once, and said so.

**Precedence.** Manifest: `.zaf.toml` > `.zaf/config.toml` > `~/.zaf/config.toml` > the pack's defaults, property by property. Actions: a local name shadows a global one, and `zaf doctor` says so.

The zip test (framework §7.6) gains one named exception: `~/.zaf/` is the machine's zaf home, like its credentials. A project that depends on a global action depends on that machine; `zaf doctor` reports every `action:` step and gate line that names a global action, so the dependency is visible.

## 3. The object

| Field | Where it comes from |
|---|---|
| name | the file name in `actions/`, as is |
| source | `global` or `local` |
| description | the first comment line after the shebang (`# <one line>`); empty when there is none |
| timeout | `[actions] timeout_minutes` (default 30); an action may set its own with a `# zaf: timeout <minutes>` header line |

**Environment.** Run in the caller's current directory (the project root for a hand run, the slice workspace for a programmer), with `ZAF_ROOT` (the project root), `ZAF_HOME`, `ZAF_ACTION` (the name), and `ZAF_INSTANCE` and `ZAF_VERSION` passed through. Arguments after the name go to the action as given.

**Output.** Everything the action writes, stdout and stderr, goes to `.zaf/logs/<DATETIME>_<ZAF_ACTION_NAME>.log` (`~/.zaf/logs/` when run outside a project), `<DATETIME>` as `YYYYMMDDTHHMMSSZ`. On exit 0 `zaf` prints nothing. On a non-zero exit it prints `<name>: exit <n>, log <path>` and the last ten lines of the log. `--more` prints the whole log after the run; `--json` gives name, source, exit, seconds and log. The action's exit code passes through, mapped to `2` when the file cannot start or the time bound passes.

## 4. The verbs, scrutinized

| Verb | Stays a verb? | Why |
|---|---|---|
| `board ready\|claim\|report\|question\|ruling` | yes | leases, labels and comment markers through the Board port; backend-blind |
| `ask`, `asklog` | yes | the ladder, the quote check, the log: logic, not a script |
| `brief` | yes, and **lists the local actions** in the pack | composed from ports; the pack is where an instance learns the project's steps |
| `check <task>` / `verify` | yes; its `command:<cmd>` step gains a sibling **`action:<name>`** | the engine, the dedupe and the record are logic; the step itself is a script |
| `check pr:<n>` | yes; `action:<name>` steps apply here too | CI, bots and runners wait inside the verb |
| `changes` | yes | the VCS port, stat before hunks |
| `slice open\|pr\|merge\|drop` | yes | VCS, forge and board together, idempotent in slice terms |
| `spawn`, `status`, `guard` | yes | the tick, the board snapshot and the budget are state and logic |
| `report` | yes | the transcript parser with golden tests; generic across projects |
| `doctor` | yes, and **checks the actions**: each file runnable, each `action:` step and each gate line naming one that exists, local names that shadow global ones | the manifest's validator |
| `init`, `setup`, `profile init` | yes; `setup` also makes the two homes (§2) | they write the project's own files from the manifest |
| `[gate] full\|light\|tests` | stays a command line | the profile allow-list derives its tool rules from the gate's words; a project may write `full = "zaf do gate"` when it prefers a script, at the price of that derivation |
| `[runners.<name>] command` | stays a template | placeholders (`{prompt}`, `{model}`) are the runner's shape, not a script's |

No verb moves. What changes is where a project's *how* lives: the release brief says "merge and tag" and the project's `release` action does it; `[check.pr]` names `action:nodes-check` instead of a line; the Engineer's repeated `gh pr merge … && jj git fetch && jj new main` is `zaf do land <pr>`.

## 5. The verb added

| Verb | Use it to |
|---|---|
| `zaf do` | list the actions: name, source, description; `--json` for the spawner and the briefs |
| `zaf do <name> [args…]` | run one; its exit code and its log |

Exit codes as every verb: `0` done, `1` refused (no such action, a local file that is not runnable), `2` the file could not start or timed out, and the action's own code otherwise.

## 6. The manifest

```toml
[actions]        timeout_minutes = 30                      # optional; absent = 30
[check.pr]       steps = ["ci", "action:nodes-check"]
```

The actions folder is not configurable: `.zaf/actions/` in a project, `actions/` in the global home.

## 7. Dogfood: the first actions

Built through the lifecycle on this repo (an epic on the board, planned, built by a team instance). The ways repo moves first: `ZAF.toml` becomes `.zaf.toml`, the `.gitignore` line `.zaf/` becomes `.zaf/.gitignore`, and `.zaf/actions/` holds the local ones.

| Action | Home | Does |
|---|---|---|
| `land` | global | `land <pr>`: squash-merge a PR with branch deletion, fetch, and start a new change on the main bookmark |
| `release` | local | `release <version>`: check `zaf/Cargo.toml` is at `<version>`, tag `zaf-v<version>` on main and push the tag; the workflow does the rest |
| `nodes-check` | global | `nodes check --root "$ZAF_ROOT"`; the `[check.pr]` step of every charted repo |

Not actions: `/nodes:chart --diff` (a model's job, a skill), the gate (a line, §4), the spawner's missing launches (verbs, their own epic).

## 8. Rulings, 2026-11-03

1. **Global home:** a configurable zaf home, default `~/.zaf/`, laid out like a project; `zaf setup` makes it first when it has not run, and may be called from the beginning.
2. **Local home:** projects gain a committed `.zaf/` folder; actions live in `.zaf/actions/`; the file name is the action name, no spaces, hyphens and the like allowed.
3. **Manifest:** `.zaf.toml` at the project root or `.zaf/config.toml`; both may exist and combine, `.zaf.toml` winning on a conflicting property.
4. **Verb:** `zaf do`.
5. **Output:** the log is `.zaf/logs/<DATETIME>_<ZAF_ACTION_NAME>.log`.
6. **Ignore rule:** zaf writes `.zaf/.gitignore` for the machine state.

## 9. Assumed by Claude, open to veto

1. `zaf do` with no name lists; `--json` for the briefs.
2. `ZAF_HOME` moves the global home; nothing else configures it.
3. `~/.zaf/config.toml` is read as machine-wide defaults below the project's manifest; what `.zaf/local.toml` holds today (budget, prices) may move there later, not in this epic.
4. `.zaf/roles/` is committed with the project; it holds instructions, not secrets.
5. `zaf setup` moves a root `ZAF.toml` to `.zaf.toml` once; `zaf doctor` refuses a project that still has both.
6. On exit 0 nothing is printed; a value an action must hand back is read from its log with `--more` or `--json`.
7. `command:` check steps stay one release beside `action:`, then retire.
8. Every instance may run global actions; the brief lists local actions only.
9. The manifest combines table by table, property by property; an array is replaced, not merged.
