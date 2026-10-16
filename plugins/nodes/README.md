# nodes

A repository charted as a tree of `NODE.json` files, one per directory that carries information beyond its parent. Each node says only what its ancestors haven't, so an agent understands any path by loading the chain from the root down (`nodes chain src/domain/rules` loads four small nodes) instead of reading the code or the whole map. `NODE.json` is *what/where*; `CLAUDE.md` stays *how to behave*.

| Part | What it is |
|---|---|
| `skills/chart/` | `/nodes:chart`: builds or refreshes the tree, incrementally; `--dry` previews, `--diff` re-charts only what a branch touched |
| `skills/familiarize/` | `/nodes:familiarize`: loads the tree at the start of a session, silently, and replies `Ready.` |

No scripts, no hooks: the skills run the same on Linux, macOS and Windows. No other plugin is needed, and no other plugin needs this one: `teams` uses it when present.

## Install

With the `ways` marketplace added (`claude plugin marketplace add https://github.com/ProgrammingCheetah/orchestration-kit.git`): `claude plugin install nodes@ways`, or `/plugin install nodes@ways` inside a session. Then the binary.

## The `nodes` binary

Every read and write goes through the `nodes` tool (version 0.6.0 or later), so every file is canonical and the tree validates. Install it once per machine:

- **Linux, macOS** (x86_64, aarch64): the prebuilt binary attached to the latest release at https://github.com/ZurNetwork/nodes/releases, put on your `PATH`.
- **Anywhere with Rust**, Windows included: `cargo install --git https://github.com/ZurNetwork/nodes --locked --tag vX.Y.Z`, naming the latest release tag.

Without it the skills say so and give the install; they never write a node by hand. The tool's own README documents the schema and every command.

## Settings: `NODE.toml`

The pack's whole configuration for a project is the root `NODE.toml`. `/nodes:chart` seeds it once, detecting what it can, and never overwrites a key you set. The pack owns three tables and reads no other file's settings:

| Table › key | Default | Meaning |
|---|---|---|
| tool › run | `nodes` | how the binary is invoked (e.g. a task-runner recipe wrapping it) |
| tool › install | `""` | the project's install command; `""` = the routes above |
| tool › check | `nodes check` | the validation run before every chart report |
| tool › fmt_on_write | `false` | `true` when a hook already formats each written node |
| refs › index | `""` | a file listing citable page ids (id → status + title), e.g. a pointer index generated from the project's docs; `""` = no refs |
| refs › kind, on_superseded | `""` | what a page is here; how a ref to a superseded page is re-pointed |
| root › local | `false` | `true`: the tree stays out of version control |
| root › notes | `[]` | tree-wide conventions, pinned as the root node's first notes |

Optional files: `.chartignore` (gitignore syntax, seeded once) keeps directories out of the tree.

## In someone else's repository

The tree is a set of files in the repo. Before its first write in a repository, `/nodes:chart` asks whether to commit them or keep them local-only. Local-only (git, or jj colocated with git) adds `NODE.json`, `NODE.toml` and `.chartignore` to `.git/info/exclude`: `nodes` still reads every node, and nothing shows in `git status`. A repository with no git directory has no local-only option, and the skill says so before writing.

## The pointer rule

When a project cites external pages (design pages, specs), the citation lives in the nearest node's `refs` — page id, title, what it governs there — and nowhere else: code comments state their facts plainly, and memory or `CLAUDE.md` hold pointers, never copied page bodies. `nodes refs <page>` finds every directory a page governs; `nodes check --ref-index <index>` flags refs to superseded pages.

## For other agents

What any agent (a `teams` Spawn, say) runs, with no skill loaded:

| Command | When |
|---|---|
| `nodes chain <path>` | before reading code in a directory: root → … → node |
| `nodes find <term>` | before searching blind: searches the chart |
| `nodes grep <term>`, `nodes resolve <name>` | full text of the charted files; a half-remembered path |
| `nodes set <path> <field> '<json>'`, then `nodes touch <path>` | after changing what a directory holds |
| `nodes check` | in the gate, after any node edit |

No tree yet: `/nodes:chart`.

## Updating

From the marketplace: `claude plugin marketplace update ways`, then `claude plugin update nodes@ways`, then start a new session. Update the binary by installing the newer release the same way.
