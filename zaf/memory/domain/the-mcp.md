---
title: The MCP
type: note
tags:
- mcp
- tools
- claude-code
permalink: zaf/domain/the-mcp
---

# The MCP

`zaf mcp` (hidden verb, release 0.5.5) serves zaf's verbs as MCP tools over stdio, so a model calls `mcp__zaf__<verb>` instead of typing `zaf` in Bash. The design is `.teams/releases/0.5.5/design.md` (D1 to D25, "Facts checked"); what is built is the section "5.4 The MCP" of `docs/zaf-actions.md`. This note keeps the facts that are easy to lose.

## Observations

- [rule] The tool contract is external (I1): names `mcp__zaf__<verb>` (the server key `zaf` is part of every name), input `{args, stdin, json, more}`, result one text block (stdout, stderr, `exit <n>` last when not 0), `isError` when not 0, `_meta["zaf/exit"]` always #contract
- [rule] The tools come from clap at run time: every visible verb but `help`; a hidden verb, `mcp` itself, is no tool #catalog
- [rule] A line clap would refuse is not run; its usage message comes back with exit 2 #usage
- [rule] A wait stays inside the call: a `notifications/progress` every 60 s on the call's `progressToken`, `progress` rising by one, `message` the last stderr line or `<verb> waiting, <m> min`; no per-server `timeout`; a cancel kills the verb's process group #waits
- [rule] The verb runs in `ZAF_ROOT`, else `CLAUDE_PROJECT_DIR`, else the server's own directory, because a programmer's session sits in a slice workspace with its own `.zaf.toml` #directory
- [rule] The child never inherits `ZAF_DEPTH` or `ZAF_ACTION_CHAIN`: a call is a new top-level run #nesting
- [rule] After an update the next call already runs the new binary; while its `--version` differs from the server's, each result ends with `zaf was updated to <v>; restart zaf's MCP (/mcp) for the new tool list` before `exit <n>` #update
- [status] The policy table and its refusals (who may call what) belong to epic #1004 and are not in the 5.4 section yet #status
- [fact] Seen on Claude Code 2.1.330 with a probe server, 2026-11-22: a stdio server inherits the session's environment (`ZAF_INSTANCE`, `ZAF_DEPTH`, `CLAUDECODE=1`, `CLAUDE_PROJECT_DIR`) and sits under the session's process tree #probe
- [fact] Seen: Claude Code sends a `progressToken` in `_meta` of every `tools/call`, with `claudecode/toolUseId` #probe
- [fact] Seen: a result with `structuredContent` showed the model only that JSON and dropped the text, so the server sends none #probe
- [fact] Seen: `isError: true` reaches the model as its text inside `<error>…</error>` #probe
- [fact] Seen: the client asks `server/discover` (2026-07-28) first, then `initialize` with `2025-11-25` after a -32601 #probe
- [fact] Seen: with no allow rule for the tool, a lean-profile `claude -p` refuses an MCP call ("you haven't granted it yet") #probe
- [fact] Seen: `CLAUDE_CONFIG_DIR=<dir> claude mcp add-json --scope user …` writes `mcpServers` into `<dir>/.claude.json` #probe
- [fact] Seen: a lean-profile instance sees tools of the claude.ai connectors unless they are switched off #probe
- [fact] Docs: a stdio call that sends no response and no progress for 30 minutes aborts; the per-server `timeout` is a hard wall-clock limit progress does not extend; `MCP_TOOL_TIMEOUT` defaults to about 28 hours #docs
- [fact] Docs: output is capped at 25,000 tokens (`MAX_MCP_OUTPUT_TOKENS`); server instructions at 4,096 characters; a plugin's tools are named `mcp__plugin_<plugin>_<server>__<tool>` #docs
- [fact] Docs: `mcp__<server>__*` in `allow` matches every tool of a server, and a rule with parentheses is skipped #docs

## Relations

- part_of [[zaf overview]]
- described_in `docs/zaf-actions.md` §5.4
