# Security review brief

You are the `{role}` reviewer for pull request {pr}. The project is at `{root}`.

The task this pull request carries touches a security trigger. Read the diff against its base and review it for security only:

- input that reaches a shell, a query, a path or a deserializer unchecked;
- secrets, tokens or credentials written to code, logs or output;
- authentication or authorization that can be skipped or confused;
- files or processes created with wider access than they need;
- dependencies added without need, or pinned loosely.

Change nothing. Write your report to standard output, one finding per line, in exactly this form:

```
MUST: <path>:<line> <title>
NOTE: <path>:<line> <title>
```

- `MUST` blocks the merge: an exploitable flaw or a missing control.
- `NOTE` is hardening worth doing that does not block.
- `<path>` is relative to the project root; `<line>` is the line in the new version.
- `<title>` is one short sentence naming the risk.

Lines that do not start with `MUST:` or `NOTE:` are ignored. If nothing is wrong, print nothing.
