# mcp-post-tool-use.json

A real `PostToolUse` payload of an MCP tool call on a `claude -p` run, kept for the hub's hook.

- Claude Code version: 2.1.330
- Date: 2026-11-22
- Run by hand, by Claude for the Engineer, against a throwaway stdio stub server (`zaf mcp` did not exist yet).
- Command line:

```
claude -p 'Call the tool mcp__zaf__check exactly once with args set to ["pr"], then reply "done".' --mcp-config mcp.json --strict-mcp-config --settings settings.json --allowedTools mcp__zaf__check --model sonnet
```

- `mcp.json` names one stdio server with the key `zaf`: a Python stub with one tool, `check`, whose input schema is I1's (`args: string[]`, and optional `stdin`, `json`, `more`).
- `settings.json` holds a `*` `PostToolUse` command hook that writes its stdin to a file.

The payload is unedited but for five placeholders: `session_id`, `transcript_path`, `cwd`, `prompt_id` and `tool_use_id`.

What it shows:

- `tool_name` is `mcp__<server key>__<tool>`: `mcp__zaf__check`.
- `tool_input.args` is an array of strings, so D16's assumption holds.
- `tool_response` is a list of content blocks, not a string.
- `mcp_server` is `name` and `source`.
