# Release 0.5.5 scope: zaf's MCP

2026-11-22 · **Draft, unsigned.** Written by Claude from the Engineer's words of 2026-11-22; the MCP part is a list of questions until `zaf begin --version 0.5.5` has been walked.

**Order.** "0.5.5 is going to be zaf's MCP. I consider it more urgent than 0.6.0." 0.5.5 is built before 0.6.0.

## In

- **`zaf update --yes`.** "Add a patch version for `--yes` on update." Placed inside 0.5.5 as its own epic, built first (the Engineer, 2026-11-22: "Inside 0.5.5"). With `--yes`, `zaf update` takes the default of every question it would ask (the plan, each "Replace it?"), needs no terminal, and ends with one line that says what was replaced and what was kept. Why: on 2026-11-22 an update driven without a terminal took closed input as "no" for one action and left the machine half updated with no line saying so.
- **zaf's MCP.** An MCP server for zaf. Nothing more is settled yet.

## Open, for the objectives

1. Who is the MCP for: the Engineer's own Claude session (asking zaf what waits, ruling, signing), the instances zaf runs (in place of the `zaf` verbs they type in Bash), or both?
2. Which verbs does it carry: the reads only (status, epics, questions, the brief), or the writes too (ruling, signing, filing, starting the build)?
3. Does it replace the Bash verbs in the model profiles? That would make the profile's text denies matter less, which touches 0.6.0's permission boundary.
4. How does it run: `zaf mcp` over stdio started by Claude Code, or served by the hub that already runs?
5. What it must never do: the Engineer's rule is "Zaf is ALWAYS the thing that makes the change".

## Out

- Everything of 0.6.0 (`../0.6.0/objectives.md`), and the items the 0.5.3 reviews left for it.
