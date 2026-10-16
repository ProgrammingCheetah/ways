# First run: creating the teams file

Read once per project, when no `.claude/TEAMS.*` exists. Everything here is asked in one round, with every answer pre-filled.

1. **Read** the CLAUDE.md files, CONTRIBUTING, CI config and any design docs. Find where decisions are recorded.
2. **Offer to create the teams file,** and ask, in one round:
   - **Its format:** Markdown (recommended, `teams-file.md`), YAML or TOML (`teams-file.toml`; YAML uses the same keys).
   - **Whether the repo is the Engineer's own.** Pre-fill by comparing the remote's owner with the signed-in host account (`gh api user --jq .login`, if `gh` is present); unsure means `others`.
   - **Every setting,** detected where possible: VCS and workspaces folder, the gate (exactly what CI runs) and a light gate, branch strategy, tracker, docs folder, lanes, limits, release documents and format, and security and compliance triggers fitted to this project (the template's are generic).
   - **The teams:** missions, owned paths and named members, always including the internal team, the security team and the release reviewers unless the Engineer opts out.
   - **nodes:** pre-selected yes in the Engineer's own repo, no in anyone else's (its `NODE.json` files would land in colleagues' repo).
3. **Write `.claude/TEAMS.<format>`** with your file tools. Opt-outs go in its **Opt-outs** with the date and the Engineer's words. Never regenerate it over the Engineer's edits.
4. **Write the pointer,** this one line, unchanged, into `CLAUDE.md` (the Engineer's own repo) or `CLAUDE.local.md` (anyone else's):
   ```
   **Way of working: teams.** The main session is the Orchestrator: it loads the `teams:orchestrate` skill at the start of each session and again after compaction, and follows it. A Spawn (any subagent) skips this line and follows its brief.
   ```
   If the repo has an `AGENTS.md` and the file you're creating is new, put `@AGENTS.md` on the line after the pointer: a new `CLAUDE.md` or `CLAUDE.local.md` otherwise stops Claude reading the `AGENTS.md`.
5. **Not the Engineer's repo:** add the teams file, `CLAUDE.local.md` and the docs folder to `.git/info/exclude`, each only if missing, with your file tools.
6. **Hand the Engineer the settings worksheet.** Never edit settings yourself. Read `.claude/settings.local.json` if it exists, then give the full file content to save, keeping every existing key and adding:
   - `permissions.additionalDirectories`: the workspaces folder, as an absolute path;
   - `permissions.allow`: the gate and light gate (e.g. `Bash(npm test *)`), the VCS commands the lifecycle runs (e.g. `Bash(git worktree *)`, `Bash(git status *)`, `Bash(git diff *)`, `Bash(git hash-object *)`, `Bash(git commit *)`; a push rule only for the slice branches' prefix, never main), using `PowerShell(…)` rules on a machine whose shell is PowerShell.

   The worksheet: open the file in an editor; replace its content with the block you give; save; restart Claude in the project. Done when: `/permissions` lists the folder and the rules.
7. **nodes, when chosen:** if `nodes --version` doesn't answer 0.6.0 or later, or the `nodes` plugin's skills (e.g. `/nodes:chart`) are absent, hand a worksheet: `claude plugin install nodes@ways`, then the binary, either from https://github.com/ZurNetwork/nodes/releases (Linux and macOS builds) or `cargo install --git https://github.com/ZurNetwork/nodes --locked --tag <the latest tag, which you look up and write in>`. On Windows only the `cargo` route exists. If the Engineer declines, record the opt-out.
8. **Ask for the first release's scope,** or the off-cycle work, and run the lifecycle.
