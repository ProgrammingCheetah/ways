---
name: how-to
description: Hand the Engineer something to DO as a very short worksheet — numbered steps, one action each, exact commands or click paths, a "done when" per step, and one line saying what to report back. Soft skill Claude applies whenever it asks the Engineer to run a command, click through a UI, review something, or make a change (e.g. /handoff:how-to push the release branch, "run these three commands").
---

# /handoff:how-to — instructions as a short worksheet

Whenever Claude asks the Engineer to do something, the ask takes this shape. Nothing else: no narrative around the steps, no options inside the steps (options are decided before the worksheet exists).

## Shape

```
**Goal:** <one line: what is true when this is done>

1. **<Verb> <thing>.**
   `<exact command, one line, cd baked in>`  — or —  <App> → <Menu> → <Item> → <what to type/click>
   Done when: <observable result>.
2. **<Verb> <thing>.**
   …
Tell me: <the one or two things to paste back — exit codes, a table, "done">.
```

## Rules

1. **Numbered steps, in execution order.** One action per step. Split a step that has an "and" in it.
2. **Bold verb first** in every step: Run, Open, Paste, Click, Merge, Push, Tell.
3. **Exact, copy-pasteable commands** in a fenced block or inline code, one line each, working directory included, chained the way the Engineer's shell chains commands (`cd … && …` in bash, zsh and PowerShell 7; `cd …; …` in Windows PowerShell 5.1), flags complete. Never a placeholder the Engineer has to fill unless it is a secret; then name where it comes from.
4. **UI paths as click chains:** `Settings → Permissions → Edit`. Name the button as it is labelled.
5. **Done when** on every step that isn't obviously self-evident: the exit code, the text that appears, the file that exists.
6. **Tell me** is the last line and lists exactly what to paste back. Nothing that could contain a secret.
7. **Short.** Up to ~7 steps; more means it is two worksheets. No explanations inside the worksheet — one line of context above the goal at most; reasons live in the chat before it.
8. **Normalized formatting:** `**Goal:**` line, `1.` numbers, `**Verb**`, `Done when:`, `Tell me:` — always these words, always this order. Markdown-as-syntax: `inline code` for files, commands, keys and labels; fences for multi-command blocks.
9. **Safety in the steps, not around them:** a destructive or outward-facing step says so in the step itself ("this publishes…", "this cannot be undone").
10. If the Engineer must decide something first, ask first, then hand the worksheet. A worksheet never contains a fork.

## Example

**Goal:** the `release` branch is on GitHub and its tests pass.

1. **Run** the tests.
   `cd ~/code/app && npm test`
   Done when: the run ends with no failures.
2. **Push** the branch. This publishes it to everyone with access to the repo.
   `cd ~/code/app && git push -u origin release`
   Done when: the output ends with `branch 'release' set up to track 'origin/release'.`
3. **Open** GitHub → the repo → `Branches`.
   Done when: `release` is listed.
Tell me: "done", or the step that failed and its last line of output.
