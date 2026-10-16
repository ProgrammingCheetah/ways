---
name: document
description: Add or refresh doc comments on the code that changed, in each language's own style and placement, matching the repo's docs. Docs only, never logic. Use after writing code: /teams:document
argument-hint: "[staged | <range> | path…; default: uncommitted changes plus the branch's commits]"
---

# /teams:document: document what changed, the way each language wants

Add or refresh documentation on the items the diff touched. **Docs only: never change logic.** Match how both the *language* and the *repo* want it done.

## 0. Config

Read the teams file (`.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`) at the root of this checkout; if absent and this is a secondary workspace, at the root of the main checkout (git: the first path `git worktree list` prints; jj: the folder holding the `.jj` directory that this workspace's `.jj/repo` file points to). Use `Repository › main`, `vcs`, `style` and `light gate` / `gate`, and the project's CLAUDE.md. `Repository › style` names the project's style rules, which may set documentation rules: read them.

No teams file: a `.jj/` folder means jj; the default branch comes from `gh repo view --json defaultBranchRef` or `git symbolic-ref refs/remotes/origin/HEAD`. Ask the Engineer once only for what detection can't settle. Run by a Spawn (no Engineer to ask): put the question in the report.

## 1. Scope (the argument)

"Remote main" is `origin/<main>` (jj: `<main>@origin`); with no remote, the local `<main>`.

| Argument | git | jj |
|---|---|---|
| empty | `git diff HEAD` (staged and unstaged) plus `git diff <remote main>...HEAD` (the branch's commits); `git ls-files --others --exclude-standard` lists new untracked files, which are all new | `jj diff -r "<remote main>..@"` (the branch's commits and the working copy together; jj snapshots new files itself) |
| `staged` | `git diff --cached` | no staging area in jj, so it doesn't apply; say so and use the empty form, or `jj diff -r @-` for the last commit |
| a range | `git diff <range>` (e.g. `main..HEAD`, `HEAD~3`) | `jj diff -r "<revset>"` (e.g. `main..@`, `@-`) |
| path(s) | add `-- <path>…` to any form above | add the paths as filesets to any form above (`jj diff -r @ a.txt`) |

An argument that names a revision or range is a range; anything else is a path. Pull the changed files and hunks (add `--name-only` for the file list) and focus on items that were **added or had their signature changed**.

## 2. Before writing: match the house style

- Detect each file's language (by extension).
- **Read the repo's existing docs** for that language and mirror their style, tone and tags. Then read the CLAUDE.md and `Repository › style` for documentation rules (e.g. Rust uses `///` directly above the item, plain `//` for in-body comments). The project's rules win over the table below.
- Don't impose a foreign convention: no Javadoc-style tags in a Google-style Python project.

## 3. What to document

Public, exported or changed items: functions and methods, structs, classes, enums, interfaces and traits, modules and packages, meaningful public constants. For each, capture **what it does and why** (the contract, not a restatement of the name), its parameters, its return, and its errors, panics or exceptions; add a short example only when it earns its keep. Skip noise: don't document trivially self-evident items just to add a comment.

## 4. Placement and syntax, per language

**The big split:** most languages want the doc comment **ABOVE** the signature; **Python** (and a few others) want it **INSIDE** the body as the first statement, right below the signature; **Elixir** uses module attributes above the `def`. Use the right one, and follow each language's tag conventions, which vary by ecosystem and team preference:

| Language | Syntax | Placement | Tags / notes |
|---|---|---|---|
| Rust | `///` (`//!` for module) | **above** | Markdown; `# Examples` / `# Errors` / `# Panics`; doctests must stay valid. `//` in bodies. |
| Python | `"""docstring"""` | **inside**, first statement under the signature | PEP 257; match the repo's Google / NumPy / reST style. |
| JS / TS | `/** … */` (JSDoc / TSDoc) | **above** | `@param` `@returns` `@throws`; in TS let types carry the shapes, document intent. |
| Java | `/** … */` (Javadoc) | **above** | `@param` `@return` `@throws` `@since`. |
| Go | `// Name …` | **above**, a full sentence starting with the identifier | exported identifiers; godoc. |
| C# | `/// <summary>…</summary>` | **above** | XML doc: `<param>` `<returns>` `<exception>`. |
| C / C++ | `/** … */` or `///` (Doxygen) | **above** | `@param` `@return` `\brief`. |
| Kotlin | `/** … */` (KDoc) | **above** | `@param` `@return`. |
| Swift | `///` (markup) | **above** | `- Parameter:` `- Returns:` `- Throws:`. |
| Ruby | `#` (YARD / RDoc) | **above** | `@param` `@return`. |
| PHP | `/** … */` (PHPDoc) | **above** | `@param` `@return` `@throws`. |
| Elixir | `@doc """…"""`, `@moduledoc """…"""` | **above** the def (module attribute) | ExDoc Markdown. |

Unlisted language: follow its idiomatic doc convention and the repo's existing examples.

## 5. Finish

- Edit docs only; keep changes minimal and review-friendly.
- **Check that the docs didn't break anything** where docs are compiled or validated (Rust doctests, C# XML docs, a docs linter). Run the cheap doc or lint step of `Repository › light gate`, or of `gate` when it covers docs (e.g. doctests). Solo: the equivalent step in the CI config (`.github/workflows/*.yml` or similar). Nothing cheap to run: say so; never claim a pass you didn't see.
- Summarize what you documented (by file and item), and **flag anything whose intent was unclear** and you had to guess, so the Engineer can correct it. A Spawn running this puts those guesses in its report.
