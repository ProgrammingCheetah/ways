# zaf

A Rust CLI that schedules a project's work deterministically; models work inside a phase and zaf decides everything between phases.

## Memory

`zaf/memory/` is a Basic Memory project (plain Markdown, kept with the repo) and zaf's store of design knowledge. The repo's `.mcp.json` starts its server offline and limited to the `zaf` project; see `zaf/memory/README.md` for the one-time setup on a fresh clone.

- **Before design work, read `zaf/memory/zaf-overview.md`** (or search the `zaf` project). It links to the principle, the vocabulary, the scheduler, the rulings and the open decisions.
- **What is built** is in `docs/zaf-actions.md`. **Where zaf is going** is in `zaf/zuri.domain.md` (the Engineer's own document: change it only when they ask) and in the memory notes.
- **When something is decided or learned, write it there** in the same change: update the note it belongs to, or add one and link it from the overview.
- Notes hold durable knowledge only, never working papers or run status.
