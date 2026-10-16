# nodes

A repository charted as a tree of `NODE.inf.json` files, one per directory that carries information beyond its parent. Each node says only what its ancestors haven't, so an agent understands any path by loading the chain from the root down (`nodes chain src/domain/rules` loads four small nodes) instead of reading the code or the whole map. `NODE.inf.json` is *what/where*; `CLAUDE.md` stays *how to behave*. The node files are gitignored (`**/*.inf.json`): a tree lives in the checkout where it was charted.

| Part | What it is |
|---|---|
| `skills/chart/` | `/nodes:chart`: builds or refreshes the tree, incrementally; `--dry` previews, `--diff` re-charts only what a branch touched |
| `skills/familiarize/` | `/nodes:familiarize`: loads the tree at the start of a session, silently, and replies `Ready.` |

No scripts, no hooks: the skills run the same on Linux, macOS and Windows. No other plugin is needed, and no other plugin needs this one: `teams` uses it when present.

## Install

With the `ways` marketplace added (`claude plugin marketplace add https://github.com/ProgrammingCheetah/ways.git`): `claude plugin install nodes@ways`, or `/plugin install nodes@ways` inside a session. Then the binary.

## The `nodes` binary

Every read and write goes through the `nodes` tool (version 0.7.0 or later), so every file is canonical and the tree validates. Install it once per machine:

- **Linux, macOS** (x86_64, aarch64): the prebuilt binary attached to the latest release at https://github.com/ZurNetwork/nodes/releases, put on your `PATH`. Each archive has a `.sha256` beside it (`sha256sum -c`): it checks the download is intact, not who published it.
- **Anywhere with Rust**, Windows included: `cargo install --git https://github.com/ZurNetwork/nodes --locked --tag vX.Y.Z`, naming the latest release tag.

Without it the skills say so and give the install; they never write a node by hand. The skills are tested with 0.7.x. A tree charted before 0.7.0 keeps its nodes in `NODE.json` files: `/nodes:chart` migrates it (`nodes migrate`) on its next run. The tool's own README documents the schema and every command.

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
| root › local | `false` | `true`: `NODE.toml` and `.chartignore` stay out of version control too (node files always do) |
| root › notes | `[]` | tree-wide conventions, pinned as the root node's first notes |

Optional files: `.chartignore` (gitignore syntax, seeded once) keeps directories out of the tree.

## Version control

The node files never enter version control: `**/*.inf.json` keeps them out, so a fresh clone or a new worktree has no tree until `/nodes:chart` runs there. What stays a choice is the pack's settings. Before its first write in a repository, `/nodes:chart` asks: **commit** its settings, keep it all **local-only**, or **not now**. It pre-selects commit only when there is no remote or the remote's owner is the signed-in forge user; in a repository that isn't yours, or when it can't tell, it pre-selects local-only. "Not now" writes nothing to the repository; it is remembered in Claude's project memory, outside the repository, when the project has one, so `/nodes:familiarize` stops offering.

Commit runs `nodes init` first, which adds `**/*.inf.json` to the root `.gitignore`; `NODE.toml`, `.chartignore` and that line are the repository's to commit.

Local-only leaves `.gitignore` alone and writes `**/*.inf.json`, `NODE.toml` and `.chartignore` into the repo's exclude file before anything else: git's `info/exclude` (`git rev-parse --git-path info/exclude`, which also serves jj colocated with git), or, in jj without colocation, `info/exclude` in the git directory `.jj/repo/store/git_target` names (absolute, or relative to `.jj/repo/store/`). `nodes` still reads every node; nothing shows in `git status` or `jj st`. Either way, each run untracks any node file (and, local-only, either settings file) that the tree itself got tracked by mistake; files of those names the repository already tracks are its own: they are left alone and reported. Know two things about an ignored tree:

- Excluded files are ignored files: `git clean -x` or `-X` deletes the whole tree, and nothing backs it up.
- It is one copy, in that checkout only, shared by every branch. Other worktrees and clones don't have it.

## The pointer rule

When a project cites external pages (design pages, specs), the citation lives in the nearest node's `refs` — page id, title, what it governs there — and nowhere else: code comments state their facts plainly, and memory or `CLAUDE.md` hold pointers, never copied page bodies. `nodes refs <page>` finds every directory a page governs; `nodes check --ref-index <index>` flags refs to superseded pages.

## For other agents

What any agent (a `teams` Spawn, say) runs, with no skill loaded. Always pass `--root <repo top level>`: without it `nodes` falls back to the nearest ancestor's tree, and there is no tree when the top level has no `NODE.inf.json`.

| Command | When |
|---|---|
| `nodes chain <path>` | before reading code in a directory: root → … → node |
| `nodes find <term>` | before searching blind: searches the chart |
| `nodes grep <term>`, `nodes resolve <name>` | full text of the charted files; a half-remembered path |
| `nodes set <path> <field> '<json>'`, then `nodes touch <path>` | after changing what a directory holds |
| `nodes check` | in the gate, after any node edit (the project's `[tool].check` may add `--ref-index`; plain `check` skips only the ref rules) |

No tree yet: `/nodes:chart`.

**Every tree lives only in the checkout where it was charted**, since its node files are gitignored. Agents working in other worktrees or clones read it from the main checkout, read-only (`nodes --root <main checkout> chain <path>`), and don't edit nodes; after each merge, the main session re-charts in the main checkout (`/nodes:chart --diff`).

## Updating

From the marketplace: `claude plugin marketplace update ways`, then `claude plugin update nodes@ways`, then start a new session. Update the binary by installing the newer release the same way.
