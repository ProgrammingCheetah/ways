---
name: familiarize
description: Load the project by reading its NODE.json node tree through the `nodes` tool (`nodes tree`, `nodes chain <path>`; running /nodes:chart first if no tree exists), walk the chain of nodes relevant to the current work, note stale nodes, reconcile memory, and get ready. Read-only apart from that first chart. Silent by design — the only reply is "Ready." Use at the start of a session or after a long gap (e.g. /nodes:familiarize).
---

# /nodes:familiarize — load the project, say "Ready."

Cold-start warm-up. Build a working picture of the project from its **node tree** (one `NODE.json` per directory, written by `/nodes:chart`, read through the `nodes` tool), reconcile it with what memory believes, and stop. **Read-only**: it never writes a node; the one exception is invoking `/nodes:chart` when no tree exists. **No report, no summary, no questions.** The whole visible output is **Ready.**

## 1. Read the rulebooks

In order, skipping any that don't exist:

1. Root `CLAUDE.md` and every per-directory `CLAUDE.md` (Glob `**/CLAUDE.md`, skipping ignored and dependency directories).
2. Root `NODE.toml`, this pack's settings. Wherever this skill says `nodes …`, run `[tool].run …`; missing → plain `nodes` (note privately that `/nodes:chart` owes the file; this skill never writes it).
3. The docs the root `CLAUDE.md` names as authoritative (headings only for long ones), and the file at `[refs].index` when set (id + status + title per page, no bodies).
4. The project's memory index (`MEMORY.md`) if it has one, then any memory about the file layout or structure.

## 2. Load the node tree

The tree is a **lookup service**, not context to carry around: load the nodes a path needs, nothing more.

- **No root `NODE.json`** and the binary answers → invoke `/nodes:chart` (Skill tool), then continue. This is the only write this skill causes; chart asks first where it must.
- `nodes tree` is the whole map, one line per node (name + `short`); `nodes get <path> is` gives the full sentence; `nodes get . notes` shows the tree's own conventions.
- Find what's in play: `git status --short` / `jj st`, the current branch or bookmark, `git log --oneline -10` / `jj log`, the work in flight. For each directory touched, `nodes chain <path>` (root → … → node). Only those paths, never the whole tree.
- Later questions: `nodes find <term>` (the chart), `nodes grep <term>` (the files), `nodes resolve <name>` (a half-remembered path), `nodes recent [path]`, `nodes ls --type <T>` / `--category <C>`, `nodes refs <page>`. Reach for them instead of re-reading.
- Manifests and CI config only if a node points at them and the task needs them.
- **Binary missing or older than 0.6.0** (`nodes --version`): read the `NODE.json` files along the same chains with your file tools; never hand-edit one, and never fail on it.

**Staleness check** (cheap, no re-chart): `[tool].check` (read-only), plus, for each node loaded, `nodes --json get <path> fs` against that directory's listing. Note stale nodes privately; suggest `/nodes:chart` only if the next task trips over one.

Don't read whole source files: the goal is orientation, not review.

## 3. Reconcile memory

Only when the project has a memory directory. The file-system map lives in the node tree; memory holds only what the tree can't:

- **A memory holding a file-system map** → shrink it to a pointer: "file-system map = NODE.json tree (`nodes tree`, `nodes chain <path>`), charted <root node's date>; /nodes:familiarize to load, /nodes:chart to refresh", plus anything the tree can't give (the gate command, where the roadmap lives). Delete the duplicated tree.
- **None** → write that pointer as a project memory (why: cold-start orientation; how to apply: load `nodes chain <path>` before guessing at a path) and add its line to the index.
- A memory naming a path, flag or command that no longer exists → correct or delete it.

Don't duplicate what `CLAUDE.md` or the tree already records.

## 4. Get ready

Note privately: the branch and whether it's dirty, the work in flight, the uncommitted files, the gate command, any stale nodes.

## 5. Reply

Output exactly:

```
Ready.
```

Nothing before it. One exception after it: when the `nodes` binary is missing or older than 0.6.0, add one line — `nodes 0.6.0+ is not installed:` and the install (`[tool].install` when set, else a prebuilt binary from https://github.com/ZurNetwork/nodes/releases for Linux or macOS, or `cargo install --git https://github.com/ZurNetwork/nodes --locked --tag vX.Y.Z` with the latest release tag). Anything else that's broken surfaces only when the next task hits it.
