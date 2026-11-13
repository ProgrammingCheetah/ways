---
permalink: zaf/readme
title: README
type: note
---

# zaf's memory

This folder is a [Basic Memory](https://basicmemory.com) project: notes as plain Markdown, kept with the repo.

The repo's `.mcp.json` starts a Basic Memory server limited to the project named `zaf`. On a fresh clone, install the CLI and register this folder under that name once, from the repo root:

```bash
pipx install basic-memory
bm project add zaf "$(pwd)/zaf/memory"
```

The search index lives outside the repo, in `~/.basic-memory/`, and is rebuilt from these files.
