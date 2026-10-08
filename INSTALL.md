# Install (for Claude to follow)

The Engineer asks a Claude session opened in this folder to "install the kit". Follow these steps with your own file tools. Install no programs, run no scripts, and delete nothing.

**The copy rule**, for every file you install: if the target doesn't exist, copy it in. If it's identical, skip it. If it differs, show the Engineer the difference and ask: keep theirs, take the kit's, or merge. Never overwrite silently.

1. **The skills.** Apply the copy rule to every file in `skills/orchestrate/` (into `~/.claude/skills/orchestrate/`) and `skills/how-to/` (into `~/.claude/skills/how-to/`).
2. **The agents.** Apply the copy rule to each file in `agents/` (into `~/.claude/agents/`). A lane's model and effort are the `model:` and `effort:` lines in its agent file; that's where the Engineer changes them.
3. **The roles.** Ask the Engineer which project the Orchestrator will orchestrate, then where the roles go:
   - **(a) `CLAUDE.local.md` in that project. Recommended for a shared work repo.** It's personal: only the Engineer's sessions load it. Git doesn't ignore it by default, so if `git check-ignore -q CLAUDE.local.md` fails in the project, add the line `CLAUDE.local.md` to `.git/info/exclude` (local, never committed). If the repo has an `AGENTS.md` and no `CLAUDE.md`, a `CLAUDE.local.md` stops Claude reading the `AGENTS.md`; tell the Engineer the fix: `/config` → **Project instructions** → `claude-md-and-agents-md`.
   - **(b) `~/.claude/CLAUDE.md`.** Personal, and loaded in every project on this computer.
   - **(c) The project's shared `CLAUDE.md`,** only if the repo is the Engineer's own. Warn first: colleagues' Claude sessions would load it too.

   Show where the snippet would go, and write only once the Engineer says yes. Append `CLAUDE-snippet.md`, then one line saying where the teams live: `**Teams:** the "## Teams" section of this file.` for (a) or (b), or `**Teams:** TEAMS.md beside this file.` for (c). `/orchestrate` fills them in on its first run.
4. **Report** in a short table: each skill file and agent (installed, skipped, kept, replaced or merged), and the roles (where, or skipped). End with the exact start command, using the project's real path: `cd <path> && claude`.
