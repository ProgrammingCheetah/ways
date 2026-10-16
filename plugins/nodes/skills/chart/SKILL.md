---
name: chart
description: Chart a repository (or any directory tree) as a tree of nodes — one NODE.inf.json per directory that carries information beyond its parent — so an agent understands any path by loading the chain of nodes from the root down (`nodes chain <path>`). Two passes like a compiler (understand bottom-up, write top-down), incremental on re-runs, honors .chartignore, writes only through the `nodes` tool so every file is canonical; the project's settings live in a root NODE.toml it seeds. Node files are gitignored (`**/*.inf.json`); a tree's refs live in one committed NODE.refs.json at its root. Asks before its first write in a repository whether its settings and refs are committed or kept local-only, and before charting a fresh clone in full; never charts a worktree or jj workspace. /nodes:familiarize is the read-only counterpart. Use to build or refresh the node tree, or preview it without writing (e.g. /nodes:chart, /nodes:chart src/domain, /nodes:chart --full, /nodes:chart --dry).
argument-hint: "[path] [--full] [--dry] [--diff [base]]"
---

# /nodes:chart — build the NODE.inf.json tree

Every directory is a node. A node's `NODE.inf.json` describes that directory **at its own scope** and only says what its ancestors haven't already said. Meaning is derived by walking the path root → leaf, so a cold agent understands `src/domain/rules/` by loading four small nodes (`nodes chain src/domain/rules`) instead of the whole tree.

`/nodes:chart` writes the tree. `/nodes:familiarize` reads it. Neither touches `CLAUDE.md`: `NODE.inf.json` is *what/where*, `CLAUDE.md` is *how to behave*. "The Engineer" below is the person you work for.

**The tool.** Every read and write goes through the `nodes` binary, 0.7.0 or later (`nodes --version`; tested with 0.7.x). Wherever this skill says `nodes …`, run `[tool].run …` (§1), always with `--root <top>` — the repository's top level, or the directory being charted — since without it `nodes` falls back to the nearest ancestor's tree. Missing or older → stop, say so, and give the install: `[tool].install` when set, else a prebuilt binary from https://github.com/ZurNetwork/nodes/releases (Linux, macOS; x86_64 and aarch64) or, anywhere Rust is installed (Windows included), `cargo install --git https://github.com/ZurNetwork/nodes --locked --tag vX.Y.Z` with the latest release tag. Never write or format a node by hand instead.

**Any OS.** `nodes`, `git` and `jj` run the same in Bash or PowerShell. Where the shell mangles the JSON in `nodes set` (Windows PowerShell 5.1 drops inner double quotes), edit the file with your file tools, then `nodes fmt <file>`.

**Secondary checkouts.** A linked git worktree (from `<top>`, `git rev-parse --path-format=absolute --git-common-dir` differs from `… --git-dir`; the main worktree is the common directory's parent) or a jj workspace (`.jj/repo` is a file holding the path of the main checkout's `.jj/repo`, relative to the workspace's own `.jj/` directory) never gets its own tree: it reads the main checkout's. There, before any question or write, reply `Not charted: this is a secondary checkout; chart <main checkout> instead.` and stop.

## Arguments

| Form | Meaning |
|---|---|
| `/nodes:chart` | chart (or update) from the root |
| `/nodes:chart <path>` | chart (or update) that subtree only, then propagate one level up; no root tree yet → chart the root first |
| `/nodes:chart --full` | ignore `charted` dates and rebuild every node |
| `/nodes:chart --dry` | preview: do everything up to the writes, print the plan, write **nothing** (combines with a path, `--full`, `--diff`) |
| `/nodes:chart --diff [base]` | stale set = directories touched since `base`, a branch or a commit (below); no scan. Re-charts what a branch touched (or, given a commit, what changed since it), in this checkout |

`--diff` in git: `git diff --name-only <base>...HEAD`, plus `git diff --name-only HEAD` (uncommitted) and `git ls-files --others --exclude-standard` (new, untracked); `base` defaults to `origin/HEAD`, else `main`, else `master`. In jj: `jj diff --from 'fork_point(<base> | @)' --name-only`; `base` defaults to `trunk()`, or the local `main` bookmark when `trunk()` resolves to `root()` (no remote). `base` may be a commit: after pulling a merge, pass main's commit from before the pull (git: `ORIG_HEAD` straight after the pull; jj: the trunk commit id recorded before fetching), since the default base then shows nothing.

## Dry run (`--dry`)

On a tree from before `nodes` 0.7.0 (a root `NODE.json`), report that it needs migrating (a real run does it) and stop. Otherwise run §1–§4 — read config, apply ignore rules, detect mode, understand bottom-up — but **write nothing**: no `NODE.toml`, `.chartignore`, `.gitignore` or exclude lines or `NODE.inf.json`, no `nodes init` / `set` / `classify` / `add-ref` / `touch` / `fmt` / `migrate`, and ask no question. Would-be seeds are held in memory and used as if present. Instead of §5–§6, print:

1. **Seeds**: each file that would be created, with its full contents. In a new repository add `commit or local-only: asked on the real run`, and the `.gitignore` line (commit) or exclude lines (local-only) each would add.
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

## 0. The first write: whose repository

The node files themselves are never committed: `**/*.inf.json` keeps every `NODE.inf.json` out of version control, so a tree lives in the checkout where it was charted. Its refs are another matter: `nodes` keeps them in one `NODE.refs.json` at the tree's root (§ The node schema). What this section decides is where that line, the refs file and the pack's two settings files (`NODE.toml`, `.chartignore`) go.

In a repository (git or jj) with neither `NODE.toml` nor a root `NODE.inf.json`, `NODE.refs.json` or `NODE.json`, this question comes **before anything else**, and nothing is written before the answer:

> Chart this repository? **commit** its settings / keep it all **local-only** / **not now**

Pre-fill **commit** only when there is no remote, or the remote's owner is the signed-in forge user (e.g. `gh api user --jq .login`); otherwise, or when you can't tell, pre-fill **local-only**. **Not now** → write nothing to the repository, reply `Not charted.` and stop; if the project has a Claude memory directory (it lives outside the repository), save a project memory "node tree declined, <date>: offer /nodes:chart only when asked" so `/nodes:familiarize` stops offering. With no memory directory there is nowhere outside the repository to keep it: say so, and the offer returns next session. Otherwise record the answer as `[root].local`. For an existing tree, `local` is `true` when its settings aren't tracked: `git check-ignore -q NODE.toml` succeeds, or (jj) `jj file list NODE.toml` lists nothing (its "No matching entries" warning is expected); no `NODE.toml` yet → the same test on its root `NODE.json` or `NODE.refs.json`.

**Commit.** Your **first** write is `nodes --root <top> init`: it adds `**/*.inf.json` to `<top>/.gitignore` (only if missing) and seeds no node. Only then seed anything, and run no jj command in between (jj snapshots, and so tracks, any file not yet ignored). `NODE.toml`, `.chartignore`, `NODE.refs.json` (once the tree cites a page) and the `.gitignore` line are the repository's to commit. A tree from before 0.7.0 skips `init`: §3's `migrate` writes the line.

**Local-only.** The `.gitignore` is never written, by you or by `nodes`. The exclude file is `git rev-parse --git-path info/exclude` (git, and jj colocated with git); in jj without colocation it is `info/exclude` under the git directory that `.jj/repo/store/git_target` names, an absolute path or one relative to `.jj/repo/store/`. Can't resolve it → stop before any write. Your **first** write is its lines `**/*.inf.json`, `NODE.refs.json`, `NODE.toml` and `.chartignore`, each only if missing, with your file tools; only then seed anything, and run no jj command in between. Another VCS has no known local-only route: say so plainly, and write only on a yes.

**Every run** in a repository checks that nothing the tree keeps out got tracked: any `*.inf.json` (`git -C <top> ls-files '*.inf.json'` / `jj file list 'root-glob:**/*.inf.json'`, so the current directory doesn't narrow the search), and, local-only, `NODE.toml`, `.chartignore` or `NODE.refs.json` at any depth. Added by this tree (git: not in `HEAD`, so `git ls-tree -r --name-only HEAD -- <f>` prints nothing; jj: `jj file list -r @- <f>` lists nothing) → untrack it (`git rm --cached <f>` / `jj file untrack <f>`) and say so. Already in `HEAD` / `@-` → it is the repository's own: never untrack it; tell the Engineer it exists.

## 1. Project settings — `NODE.toml`

Root `NODE.toml` is this pack's settings file; this skill carries nothing project-specific. Read it first. The pack owns `[tool]`, `[refs]` and `[root]`: if the file, a table or a key is absent, **seed what is missing once** (even when a root `NODE.inf.json` exists; after §0 in a new repository) and never overwrite a key that is there. Any other table in the file is not this pack's: leave it untouched.

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
local = false            # true: NODE.toml, .chartignore and NODE.refs.json stay out of version control too (§0); node files never enter it
notes = []               # tree-wide conventions, pinned verbatim as the root node's first notes
```

Seed by detection (task-runner files, Claude settings, `CLAUDE.md`), never by asking — `[root].local` excepted (§0). Undetectable keys keep their defaults:

- `[tool]`: a task-runner recipe (Justfile, Makefile, package.json script, …) that wraps `nodes`, runs its check or installs it wins over the default. No check recipe but `[refs].index` set → `check = "nodes check --ref-index <index>"`.
- `fmt_on_write`: `true` if the project's Claude settings carry a hook that runs `nodes fmt`.
- `[refs].index`: an existing file listing citable page ids, one per line (a line's first run of digits is its page), such as a pointer index another job generates from the project's docs. Optional: without one, `refs` stay empty. `kind` / `on_superseded`: from what `CLAUDE.md` or the index says, else `""`.
- `[root].notes`: from an existing root node's tree-wide notes (everything before the reading rule), verbatim; else `[]`.

## 2. Ignore rules

- Respect `.gitignore` always. Read root `.chartignore` (gitignore syntax; the tool reads `.chartignore` and `.gitignore` at every level, deeper over shallower, and always skips hidden directories, `.git`, `.jj`, `node_modules` and `target`). If absent, **seed it once**, never overwrite it:

```
# /nodes:chart — directories not worth a node. gitignore syntax; .gitignore is also honored.
dist/
build/
__pycache__/
```

- Enumerate candidate directories with `git ls-files` / `jj file list` (tracked only), filtered through both ignore files.
- **Not a repository** (no `.git`/`.jj` at the root — a documents or home directory): there is no tracked set, so list the disk with the shell where it has one (`find . -xdev -maxdepth 2 -type d`; PowerShell `Get-ChildItem -Directory -Attributes !ReparsePoint`), never following links or crossing filesystems. Glob doesn't follow links but does cross mounted filesystems and ignores `.gitignore`: keep its pattern shallow (`*/`, `*/*/`), and let `.chartignore` do a VCS's work. The evidence is the **listing** (names, dates, sizes), not contents: open a file only when its name says nothing and it is small text, never one that looks like a credential, key or personal-data export. Credentials, caches and re-downloadable bulk (installed apps, games, package stores) get an `fs` line at most, never a node. After seeding, print the top-level directories that remain and get a go-ahead before pass 1 — the one question asked here. `--diff` is unavailable.
- **A node the walk can't reach is a check error.** `node: true` on a hidden, ignored or mounted-over directory fails `check`. So when a node goes into a hidden directory, add its anchored re-include line (`!path/to/.name/`) to `.chartignore` in the same run; otherwise list it `node: false`.
- **Mounts.** A directory whose own `NODE.inf.json` declares `path: "."`, or that holds a `NODE.refs.json`, is a separate tree mounted here: never chart into it from the outer root. Writes don't cross a mount (use `nodes --root <mount> …`, file arguments then relative to the mount); its own `NODE.toml` and ignore files govern inside it. Reads do cross, so it gets one `fs` line in its parent with **`node: true`** (role = its own `short` + "own tree, `nodes --root <dir>`"). Don't put a mount in `.chartignore` unless it should vanish from reads too. Charting a tree that sits inside another charted tree makes it a mount there: tell the Engineer the outer tree must list it.

## 3. Detect mode

- **Old tree first.** A tree from before `nodes` 0.7.0 keeps its nodes in `NODE.json` files; `nodes` reads it (with a note on stderr) but refuses every write but `classify` until `nodes --root <top> migrate`. That renames every node to `NODE.inf.json`, moves every node's `refs` into `NODE.refs.json`, turns pre-0.6.0 `categories` into tags, and adds `**/*.inf.json` to `.gitignore`. Local-only: add the exclude lines (§0) **first**, then run `nodes --root <top> migrate --no-gitignore`. Commit: the old `NODE.json` files leave version control as deletions, with the `.gitignore` line and the new `NODE.refs.json`; say in the report that they are the Engineer's to commit. A node from before 0.4.0 (no `type`) stops `migrate`: `nodes classify <path> <type> <category>…` those first, from each node's own `is` and `fs` (classify writes an old file in place), then migrate. Inside a mount, use `--root <mount>`.
- Root `NODE.inf.json` exists → **update mode**; missing → **full mode**. A fresh clone of a charted repository has `NODE.toml` or `NODE.refs.json` but no nodes: unless the Engineer asked for this chart, ask first, writing nothing before the answer:

  > Chart this checkout in full? **yes** / **not now**

  **Not now** is §0's.
- In update mode a directory is **stale** if: it has no `NODE.inf.json` where it earns one; an `fs` name no longer exists, or a directory appears that `fs` doesn't list (`nodes --json get <path> fs` vs the listing; notable files aside, `fs` is not the whole listing); a file it owns is newer than its `charted` date (`nodes recent <path> --limit N`, ignoring files under a child node); or `--full`.
- **Not a repository:** dates don't count, since files there change daily without the map changing. Only the first two tests and `--full` apply.
- With `--diff`, the stale set is exactly the touched directories (plus each one's parent, for `fs`).
- Empty stale set → output `Chart is current.` and stop.
- An unchanged subtree's node (`nodes --json get <path>`) is reused verbatim as its summary in pass 1.

## 4. Pass 1 — understand, bottom-up

For each stale subtree produce a summary in the node's own shape, plus `children` (same shape, only for directories that earn a node).

- **Leaves first.** A parent's `is` is synthesized from its children's summaries, never guessed from its name. `short` is the glance version of `is`: a noun phrase, no period.
- **A directory earns a node only if** its children can't be collapsed into one `fs` line of the parent. Three files fully explained by the parent get `node: false`.
- Read listings, manifests, entry points (`mod.rs`, `index.ts`, `__init__.py`, …) and existing `README`/`CLAUDE.md`/`NODE.inf.json`. Skim signatures and headers; never read whole source files.
- `fs` lists **direct children only**: directories and notable files, one role each, no prose.
- `refs` is the **only** place code points at external pages (`[refs].kind`): page id, title, what it governs here. Pointers, never page bodies. With no `[refs].index` there is nothing to cite, so `refs` stay `[]`. Other pointers (tickets, rulings) go in `notes`.

**Fan-out.** Stale top-level subtrees are independent. Four or more → one read-only subagent each (Explore), given the summary shape, at most 8 per message. Fewer, or not a repository → inline (a not-yet-charted code directory there may still get one subagent). Merging child summaries into the parent is inline work, not an agent.

## 5. Pass 2 — write, top-down

Root first, then descend. A node states only what no ancestor stated: a convention declared at `src/` is not repeated in `src/domain/`; `is` describes the scope, not the project. Siblings are independent once their parent is written.

- **New node:** write the full JSON file (every field, empty arrays allowed; no `refs`), then `nodes fmt <file>` (skip when `[tool].fmt_on_write`), then `nodes add-ref` each of its refs.
- **Existing node:** change only what moved — `nodes set <path> <field> '<json>'` per field, `nodes classify` for `type` and categories, `nodes add-ref <path> <page> "<title>" "<governs>"` / `nodes rm-ref <path> <page>` — then `nodes touch <path>`.
- **Parent listing changed:** `nodes set <parent> fs '<json>'`. **Directory gone:** move each of its refs to the node that now holds what it governs, else `nodes rm-ref` it; then delete its `NODE.inf.json` and drop the parent's `fs` entry.
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
  "notes": ["one fact per entry: what a reader of this directory needs and no file states"]
}
```

A node's `refs` are not in its file: `nodes add-ref` / `rm-ref` / `set <path> refs` keep a tree's refs in one `NODE.refs.json` at its root, `{"src/domain": [{ "page": 1042, "title": "Architecture Overview", "governs": "the use-case shape in rules/" }]}`, which exists only while the tree cites a page. `get`, `chain`, `find` and `refs` merge them into the nodes they print. Never write that file by hand.

- `type` is exactly one term of a closed vocabulary: read it with its meanings from `nodes vocabulary --json` at run time, never from memory. No term fits → the nearest, plus a `notes` entry saying so.
- `tags` are free labels (`word` or `namespace:word`; lowercase letters, digits, `_`, `-`). At least one `category:` tag, ranked most-to-least fitting; author order is kept. Set `type` and categories together with `nodes classify <path> <type> <category>…`.
- **Category words are free, so keep them few.** Reuse the words the tree already uses (`nodes --json ls`) before coining one, and prefer these: `project`, `source`, `ui`, `schema`, `tests`, `generated`, `tooling`, `infrastructure`, `config`, `docs`, `design`, `identity`, `education`, `finance`, `housing`, `work`, `health`, `legal`, `art`, `media`, `inbox`, `archive`, `index`. A new one is a short common noun.
- Every field is always present. `nodes check` refuses unknown keys, a `path` that disagrees with the file's location, a `node: true` entry without a reachable child node, and a child node its parent doesn't list.
- Terse: one sentence per entry, about 25 entries in all. Prose too big for a doc comment goes in `notes`, with a one-line summary left in the file.
- The **root** node's first `notes` are the tree's own conventions: each `[root].notes` entry verbatim; then the reading rule (to understand a path, load `nodes chain <path>`; `node: false` entries are fully described by their `fs` line); then, only when `[refs].index` is set, `[root].local = false` (the Engineer's own repo: the pack doesn't override another team's comment conventions) and no `[root].notes` entry already says it, the pointer rule (code comments state facts plainly; every pointer from code to a page lives in the nearest node's `refs`).

## 6. Validate, then report

Run `[tool].check`; it must be green (warnings allowed) before the report. A warning means one of two things, and neither is accepted as is: a ref cites a page the index marks superseded — re-point it per `[refs].on_superseded` (unset → to the page that replaced it); or `NODE.refs.json` cites pages for a directory no node charts — move those refs to the node that holds what they govern (`add-ref` there, `rm-ref` the old). Then a short table only:

```
| node | action |
|---|---|
| NODE.inf.json | updated (fs) |
| src/domain/NODE.inf.json | created |
| src/old/NODE.inf.json | deleted (dir gone) |
```

Then one line: `N nodes, M stale re-charted, migrated, NODE.toml seeded, .chartignore seeded, local-only, check green` (each clause only when true; or `unchanged`), plus any file §0 untracked and, after a migration in commit mode, `commit the removed NODE.json files, NODE.refs.json and the .gitignore line`. Whenever this run changed a committed `NODE.refs.json` (any `add-ref` / `rm-ref`), add `NODE.refs.json changed: the Engineer's to commit (a PR)` — a tracked file left modified here blocks the next pull that touches it. Nothing else.
