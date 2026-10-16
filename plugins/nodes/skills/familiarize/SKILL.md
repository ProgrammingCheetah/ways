---
name: familiarize
description: Load the project by reading its NODE.inf.json node tree through the `nodes` tool (`nodes tree`, `nodes chain <path>`; offering /nodes:chart first if no tree exists; a worktree or jj workspace of a local-only tree reads the main checkout's), walk the chain of nodes relevant to the current work, note stale nodes, reconcile memory, and get ready. Writes no node itself; it only tends memory, and invokes /nodes:chart when there is no tree. Silent by design — the only reply is "Ready." (plus one line when the `nodes` binary is missing or too old, or the tree needs migrating). Use at the start of a session or after a long gap (e.g. /nodes:familiarize).
---

# /nodes:familiarize — load the project, say "Ready."

Cold-start warm-up. Build a working picture of the project from its **node tree** (one `NODE.inf.json` per directory, committed or local-only, written by `/nodes:chart`, read through the `nodes` tool), reconcile it with what memory believes, and stop. It never writes a node; it writes only memory (§3), and invokes `/nodes:chart` when no tree exists. **No report, no summary, no questions of its own.** The visible output is **Ready.** (§5).

## 1. Read the rulebooks

In order, skipping any that don't exist:

1. Root `CLAUDE.md` and every per-directory `CLAUDE.md` (Glob `**/CLAUDE.md`; Glob ignores `.gitignore`, so drop hits under `node_modules`, `target` and other ignored or dependency directories).
2. Root `NODE.toml`, this pack's settings. Wherever this skill says `nodes …`, run `[tool].run …`; missing → plain `nodes` (this skill never writes the file).
3. The docs the root `CLAUDE.md` names as authoritative (headings only for long ones), and the file at `[refs].index` when set (id + status + title per page, no bodies).
4. The project's memory index (`MEMORY.md`) if it has one, then any memory about the file layout or structure.

## 2. Load the node tree

The tree is a **lookup service**, not context to carry around: load the nodes a path needs, nothing more. Run every `nodes` command with `--root <top>` (the repository's top level, or the working directory outside one): without it `nodes` reads the nearest ancestor's tree instead.

- **A secondary checkout of a local-only tree** has no tree of its own (a committed tree is in every checkout: read it there as usual). It is a linked git worktree (from the top level, `git rev-parse --path-format=absolute --git-common-dir` differs from `… --git-dir`; the main worktree is the common directory's parent) or a jj workspace (`.jj/repo` is a file holding the path of the main checkout's `.jj/repo`, relative to the workspace's own `.jj/` directory) that holds no tree while its main checkout does. There `<top>` is the main checkout, for every read; never chart from it. No tree in the main checkout either → carry on without one.
- **No tree** means neither `NODE.inf.json` nor `NODE.json` at `<top>` (check with your file tools; `nodes` won't say). A clone of a local-only tree's repository is one (nothing of the tree travels); so is a clone whose node files an earlier run kept out of git (`NODE.toml` or `NODE.refs.json` came along, the nodes didn't). Then, if the binary answers with 0.7.0 or later and no memory says the tree was declined, invoke `/nodes:chart` (Skill tool): it asks first (in a new repository, commit or local-only; in a checkout with settings but no nodes, whether to chart it in full), and "not now" leaves you without a tree, which is fine — carry on.
- `nodes tree` is the whole map, one line per node (name + `short`); `nodes get <path> is` gives the full sentence; `nodes get . notes` shows the tree's own conventions.
- Find what's in play: `git status --short` / `jj st`, the current branch or bookmark, `git log --oneline -10` / `jj log`, the work in flight. For each directory touched, `nodes chain <path>` (root → … → node). Only those paths, never the whole tree.
- Later questions: `nodes find <term>` (the chart), `nodes grep <term>` (the files), `nodes resolve <name>` (a half-remembered path), `nodes recent [path]`, `nodes ls --type <T>` / `--category <C>`, `nodes refs <page>`. Reach for them instead of re-reading.
- Manifests and CI config only if a node points at them and the task needs them.
- **Binary missing or older than 0.7.0** (`nodes --version`): read the node files (`NODE.inf.json` or `NODE.json`, plus the root `NODE.refs.json` for refs) along the same chains with your file tools; never hand-edit one, and never fail on it. **A root `NODE.json`** (a tree from before 0.7.0): `nodes` still reads it, with a note on stderr; read as usual.

**Staleness check** (cheap, no re-chart): `[tool].check` (read-only), plus, for each node loaded, `nodes --json get <path> fs` against that directory's listing — stale when an `fs` name is gone or a directory isn't listed. Note stale nodes privately; suggest `/nodes:chart` only if the next task trips over one.

Don't read whole source files: the goal is orientation, not review.

## 3. Reconcile memory

Only when the project has a memory directory. The file-system map lives in the node tree; memory holds only what the tree can't:

- **A memory holding a file-system map** → shrink it to a pointer: "file-system map = NODE.inf.json tree (`nodes tree`, `nodes chain <path>`), charted <root node's date>; /nodes:familiarize to load, /nodes:chart to refresh", plus anything the tree can't give (the gate command, where the roadmap lives). Delete the duplicated tree.
- **None**, and a tree exists → write that pointer as a project memory (why: cold-start orientation; how to apply: load `nodes chain <path>` before guessing at a path) and add its line to the index.
- A memory naming a path, flag or command that no longer exists → correct or delete it.

Don't duplicate what `CLAUDE.md` or the tree already records.

## 4. Get ready

Note privately: the branch and whether it's dirty, the work in flight, the uncommitted files, the gate command, any stale nodes.

## 5. Reply

Output exactly:

```
Ready.
```

Nothing before it. At most one line after it, and only when degraded:

- binary missing or older than 0.7.0 → `nodes 0.7.0+ is not installed:` and the install (`[tool].install` when set, else a prebuilt binary from https://github.com/ZurNetwork/nodes/releases for Linux or macOS, or `cargo install --git https://github.com/ZurNetwork/nodes --locked --tag vX.Y.Z` with the latest release tag);
- the tree predates 0.7.0 → `The node tree needs migrating: run /nodes:chart (it migrates).`

Anything else that's broken surfaces only when the next task hits it.
