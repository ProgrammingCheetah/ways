# zaf: the interface

One binary for every step of the work. Every verb is terse by default; `--json`
gives structured output and `--more` gives depth. Exit codes: `0` done,
`1` refused by the state (not ready, not yours, a malformed ask), `2` a backend
failed or is missing, `64` listed but not built yet.

| Verb | Use it to |
|---|---|
| `zaf brief <task>` | build this pack: the ticket, its named design pages, the owned paths, the style file, this page |
| `zaf ask -t <task> "<kind>: <question>" ...` | ask the keeper one to five design questions in one call; kinds are `fact`, `rule`, `term`, `exists`, `where` |
| `zaf asklog` | see the pages cited most, the questions that keep returning, the `not settled` answers |
| `zaf board ready` | list the tasks open for a claim |
| `zaf board claim <task>` | claim a task under a lease |
| `zaf board report <task> "<body>"` | write your one status comment on a task |
| `zaf board question <issue> "<text>" [--door]` | raise a question for the Engineer; the issue waits |
| `zaf changes <task> [--path <file>]` | see your change: stat first, hunks of one file on request |
| `zaf check <target> [--status]` | run the configured checks for a task, a pull request or a milestone |
| `zaf verify <task>` | check the task's acceptance lines |
| `zaf slice open\|pr\|merge\|drop <task>` | open, publish, merge or drop the task's slice |

Asking well:
- Ask everything you have at once: one `zaf ask` carries up to five questions.
- One sentence per question, under thirty words, no code.
- An answer is `settled` (with pages and verbatim quotes), `not settled` or `conflict`.
  `not settled` and `conflict` are not yours to resolve: raise a question on the task.
- A question asked recently is answered from the ask log without a run.
