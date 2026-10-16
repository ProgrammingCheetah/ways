---
name: chart
description: Chart a repository (or any directory tree) as a tree of nodes — one NODE.json per directory that carries information beyond its parent — so an agent understands any path by loading the chain of nodes from the root down (`nodes chain <path>`). Two passes like a compiler (understand bottom-up, write top-down), incremental on re-runs, honors .chartignore, writes only through the `nodes` tool so every file is canonical; the project's settings live in a root NODE.toml it seeds. /nodes:familiarize is the read-only counterpart. Use to build or refresh the node tree, or preview it without writing (e.g. /nodes:chart, /nodes:chart src/domain, /nodes:chart --full, /nodes:chart --dry).
---

# /nodes:chart — build the NODE.json tree

Every directory is a node. A node's `NODE.json` describes that directory **at its own scope** and only says what its ancestors haven't already said. Meaning is derived by walking the path root → leaf, so a cold agent understands `src/domain/rules/` by loading four small nodes (`nodes chain src/domain/rules`) instead of the whole tree.

`/nodes:chart` writes the tree. `/nodes:familiarize` reads it. Neither touches `CLAUDE.md`: `NODE.json` is *what/where*, `CLAUDE.md` is *how to behave*.

**The tool.** Every read and write goes through the `nodes` binary, 0.6.0 or later (`nodes --version`). Wherever this skill says `nodes …`, run `[tool].run …` (§0). Missing or older → stop, say so, and give the install: `[tool].install` when set, else a prebuilt binary from https://github.com/ZurNetwork/nodes/releases (Linux, macOS; x86_64 and aarch64) or, anywhere Rust is installed (Windows included), `cargo install --git https://github.com/ZurNetwork/nodes --locked --tag vX.Y.Z` with the latest release tag. Never write or format a node by hand instead.

**Any OS.** `nodes`, `git` and `jj` run the same in Bash or PowerShell; list directories with your file tools (Glob, Read), not shell utilities. Where the shell mangles the JSON in `nodes set` (Windows PowerShell 5.1 drops inner double quotes), edit the file with your file tools, then `nodes fmt <file>`.

## Arguments

| Form | Meaning |
|---|---|
| `/nodes:chart` | chart (or update) from the root |
| `/nodes:chart <path>` | chart (or update) that subtree only, then propagate one level up |
| `/nodes:chart --full` | ignore `charted` dates and rebuild every node |
| `/nodes:chart --dry` | preview: do everything up to the writes, print the plan, write **nothing** (combines with a path, `--full`, `--diff`) |
| `/nodes:chart --diff [base]` | stale set = directories touched since `base` (default: the default branch): `git diff --name-only <base>...HEAD`, or `jj diff --from <base> --name-only`; no scan. Run before opening a PR so node changes ship with the code they describe |

## Dry run (`--dry`)

Run §0–§3 — read config, apply ignore rules, detect mode, understand bottom-up — but **write nothing**: no `NODE.toml`, `.chartignore`, exclude lines or `NODE.json`, no `nodes set` / `classify` / `add-ref` / `touch` / `fmt`, and ask no question. Would-be seeds are held in memory and used as if present. Instead of §4–§5, print:

1. **Seeds**: each file that would be created, with its full contents.
2. **The plan**, one row per directory considered:

```
| path | outcome | short | type | tags |
|---|---|---|---|---|
| . | node (create) | … | … | … |
| src/ | node (create) | … | … | … |
| tools/ | mount (fs line, node: true) | | | |
| assets/ | fs line only | | | |
| .cache/ | ignored (hidden) | | | |
```

   Outcomes: `node (create | update: <fields> | delete)`, `mount`, `fs line only`, `ignored (<why>)`, `unchanged`. Collapse runs of `ignored`/`unchanged` into one counted row each.
3. **Conventions** the root (and any area) would carry, verbatim.
4. One line: `dry run — N nodes would be written, M mounts, nothing written`. Run `[tool].check` only if a tree already exists.

## 0. Project settings — `NODE.toml`

Root `NODE.toml` is this pack's settings file; this skill carries nothing project-specific. Read it first. The pack owns `[tool]`, `[refs]` and `[root]`: if the file, a table or a key is absent, **seed what is missing once** (even when a root `NODE.json` exists) and never overwrite a key that is there. Any other table in the file is not this pack's: leave it untouched.

```toml
# NODE.toml — settings for the nodes pack. Seeded once; edit freely, nothing is overwritten.

[tool]
run = "nodes"            # how the binary is invoked here (e.g. a task-runner recipe that wraps it)
install = ""             # how to install it here; "" = the release or cargo route
check = "nodes check"    # must be green before /nodes:chart reports
fmt_on_write = false     # true when a hook already runs `nodes fmt` after each write

[refs]
index = ""               # a file listing the citable page ids (id → status + title); "" = no refs here
kind = ""                # what a page is here (e.g. "design page", "spec")
on_superseded = ""       # how a ref to a superseded page is re-pointed (a command or a sentence)

[root]
local = false            # true: the tree stays out of version control (§1)
notes = []               # tree-wide conventions, pinned verbatim as the root node's first notes
```

Seed by detection (task-runner files, Claude settings, `CLAUDE.md`), never by asking — `[root].local` excepted (§1). Undetectable keys keep their defaults:

- `[tool]`: a task-runner recipe (Justfile, Makefile, package.json script, …) that wraps `nodes`, runs its check or installs it wins over the default. No check recipe but `[refs].index` set → `check = "nodes check --ref-index <index>"`.
- `fmt_on_write`: `true` if the project's Claude settings carry a hook that runs `nodes fmt`.
- `[refs].index`: an existing file listing citable page ids, one per line (a line's first run of digits is its page), such as a pointer index another job generates from the project's docs. Optional: without one, `refs` stay empty. `kind` / `on_superseded`: from what `CLAUDE.md` or the index says, else `""`.
- `[root].notes`: from an existing root node's tree-wide notes (everything before the reading rule), verbatim; else `[]`.

## 1. Whose repository, and ignore rules

**Ask before the first write.** In a repository with neither `NODE.toml` nor a root `NODE.json`, ask once, your answer pre-filled from the remote's owner: commit the tree (the Engineer's own repo), or keep it local-only (anyone else's). Write nothing before the answer; record it as `[root].local`. For an existing tree, `local` is `true` when `git check-ignore -q NODE.json` succeeds.

**Local-only** works in git, and in jj colocated with git: add `NODE.json`, `NODE.toml` and `.chartignore` to the file `git rev-parse --git-path info/exclude` prints, each line only if missing, with your file tools; re-check them on every run. Nodes stay fully readable to `nodes`; nothing reaches `git status`. With no git directory there is no local-only option: say so plainly, and write only on a yes.

**Ignore rules.**

- Respect `.gitignore` always. Read root `.chartignore` (gitignore syntax; the tool reads `.chartignore` and `.gitignore` at every level, deeper over shallower, and always skips hidden directories, `.git`, `.jj`, `node_modules` and `target`). If absent, **seed it once**, never overwrite it:

```
# /nodes:chart — directories not worth a node. gitignore syntax; .gitignore is also honored.
dist/
build/
__pycache__/
```

- Enumerate candidate directories with `git ls-files` / `jj file list` (tracked only), filtered through both ignore files.
- **Not a repository** (no `.git`/`.jj` at the root — a documents or home directory): walk the disk with your file tools, never following links or crossing filesystems, and let `.chartignore` do a VCS's work. The evidence is the **listing** (names, dates, sizes), not contents: open a file only when its name says nothing and it is small text, never one that looks like a credential, key or personal-data export. Credentials, caches and re-downloadable bulk (installed apps, games, package stores) get an `fs` line at most, never a node. After seeding, print the top-level directories that remain and get a go-ahead before pass 1. `--diff` is unavailable here.
- **A node the walk can't reach is a check error.** `node: true` on a hidden, ignored or mounted-over directory fails `check`. So when a node goes into a hidden directory, add its anchored re-include line (`!path/to/.name/`) to `.chartignore` in the same run; otherwise list it `node: false`.
- **Mounts.** A directory whose own `NODE.json` declares `path: "."` is a separate tree mounted here: never chart into it from the outer root. Writes don't cross a mount (use `nodes --root <mount> …`); its own `NODE.toml` and ignore files govern inside it. Reads do cross, so it gets one `fs` line in its parent with **`node: true`** (role = its own `short` + "own tree, `nodes --root <dir>`"). Don't put a mount in `.chartignore` unless it should vanish from reads too.

## 2. Detect mode

- Root `NODE.json` exists → **update mode**; missing → **full mode**.
- In update mode a directory is **stale** if: it has no `NODE.json` where it earns one; its listing differs from its `fs` names (`nodes --json get <path> fs` vs the listing); a file it owns (beneath it, not under a child node) is newer than its `charted` date (`nodes recent <path>`); or `--full`.
- **Not a repository:** dates don't count, since files there change daily without the map changing. Only the first two tests and `--full` apply.
- With `--diff`, the stale set is exactly the touched directories (plus each one's parent, for `fs`).
- Empty stale set → output `Chart is current.` and stop.
- An unchanged subtree's node (`nodes --json get <path>`) is reused verbatim as its summary in pass 1.
- A tree from before `nodes` 0.6.0 (check fails on `categories`): run `nodes migrate` first. Before 0.4.0 (no `type`): `nodes classify <path> <type> <category>…` per node, from its own `is` and `fs`. Inside a mount, add `--root <mount>`.

## 3. Pass 1 — understand, bottom-up

For each stale subtree produce a summary in the node's own shape, plus `children` (same shape, only for directories that earn a node).

- **Leaves first.** A parent's `is` is synthesized from its children's summaries, never guessed from its name. `short` is the glance version of `is`: a noun phrase, no period.
- **A directory earns a node only if** its children can't be collapsed into one `fs` line of the parent. Three files fully explained by the parent get `node: false`.
- Read listings, manifests, entry points (`mod.rs`, `index.ts`, `__init__.py`, …) and existing `README`/`CLAUDE.md`/`NODE.json`. Skim signatures and headers; never read whole source files.
- `fs` lists **direct children only**: directories and notable files, one role each, no prose.
- `refs` is the **only** place code points at external pages (`[refs].kind`): page id, title, what it governs here. Pointers, never page bodies. With no `[refs].index` there is nothing to cite, so `refs` stay `[]`. Other pointers (tickets, rulings) go in `notes`.

**Fan-out.** Stale top-level subtrees are independent. Four or more → one read-only subagent each (Explore), all in one message, each given the summary shape. Fewer, or not a repository → inline (a not-yet-charted code directory there may still get one subagent). Merging child summaries into the parent is inline work, not an agent.

## 4. Pass 2 — write, top-down

Root first, then descend. A node states only what no ancestor stated: a convention declared at `src/` is not repeated in `src/domain/`; `is` describes the scope, not the project. Siblings are independent once their parent is written.

- **New node:** write the full JSON file (every field, empty arrays allowed), then `nodes fmt <file>` (skip when `[tool].fmt_on_write`).
- **Existing node:** change only what moved — `nodes set <path> <field> '<json>'` per field, `nodes classify` for `type` and categories, `nodes add-ref <path> <page> "<title>" "<governs>"` / `nodes rm-ref <path> <page>` — then `nodes touch <path>`.
- **Parent listing changed:** `nodes set <parent> fs '<json>'`. **Directory gone:** delete its `NODE.json`, drop the parent's `fs` entry.
- Never write `path` (derived from the file's location).

In update mode write only stale nodes **plus** each one's parent `fs` entry (one level up; if that parent's `is` no longer holds, rewrite it and propagate again).

**Routing rules.** In a tree of *things* rather than code, `conventions` are its routing rules — where each kind of thing goes, one per entry: the root's say which area, an area's refine them (naming, year folders, what is never kept). Derive them from what is already there; never invent one.

**Preserve hand-written knowledge:** an existing `notes` entry is carried over unless the fact it states is gone; the same for a `conventions` entry the summary didn't derive.

## The node schema

```json
{
  "path": "src/domain",
  "charted": "2026-01-15",
  "short": "a few words: what this directory is at a glance (what `nodes tree` prints)",
  "is": "one sentence: what this directory is, at this scope",
  "type": "code",
  "tags": ["category:source", "category:schema"],
  "conventions": ["rules that hold from here down, only the deltas from the parent"],
  "entry_points": ["mod.rs"],
  "fs": [
    { "name": "rules/", "role": "validation rules", "node": true },
    { "name": "mod.rs", "role": "re-exports", "node": false }
  ],
  "refs": [{ "page": 1042, "title": "Architecture Overview", "governs": "the use-case shape in rules/" }],
  "notes": ["one fact per entry: what a reader of this directory needs and no file states"]
}
```

- `type` is exactly one term of a closed vocabulary: read it with its meanings from `nodes vocabulary --json` at run time, never from memory. No term fits → the nearest, plus a `notes` entry saying so.
- `tags` are free labels (`word` or `namespace:word`; lowercase letters, digits, `_`, `-`). At least one `category:` tag, ranked most-to-least fitting; author order is kept. Set `type` and categories together with `nodes classify <path> <type> <category>…`.
- Every field is always present. `nodes check` refuses unknown keys, a `path` that disagrees with the file's location, a `node: true` entry without a reachable child node, and a child node its parent doesn't list.
- Terse: one sentence per entry, about 25 entries in all. Prose too big for a doc comment goes in `notes`, with a one-line summary left in the file.
- The **root** node's first `notes` are the tree's own conventions: each `[root].notes` entry verbatim; then the reading rule (to understand a path, load `nodes chain <path>`; `node: false` entries are fully described by their `fs` line); then, when `[refs].index` is set, the pointer rule (code comments state facts plainly; every pointer from code to a page lives in the nearest node's `refs`).

## 5. Validate, then report

Run `[tool].check`; it must be green (warnings allowed) before the report. A warning means a ref cites a page the index marks superseded: re-point it per `[refs].on_superseded` (unset → to the page that replaced it) rather than accept it. Then a short table only:

```
| node | action |
|---|---|
| NODE.json | updated (fs) |
| src/domain/NODE.json | created |
| src/old/NODE.json | deleted (dir gone) |
```

Then one line: `N nodes, M stale re-charted, NODE.toml seeded, .chartignore seeded, local-only, check green` (each clause only when true; or `unchanged`). Nothing else.
