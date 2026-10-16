# First run: creating the teams file

Read once per project, when no `.claude/TEAMS.*` exists. Everything here is asked in one round, with every answer pre-filled. Nothing is written until the Engineer answers.

1. **Read** the CLAUDE.md files, CONTRIBUTING, CI config and any design docs. Find where decisions are recorded.
2. **Offer to create the teams file,** and ask, in one round:
   - **Its format:** Markdown (recommended, `teams-file.md`), YAML or TOML (`teams-file.toml`; YAML uses the same keys).
   - **Whether the repo is the Engineer's own.** Pre-fill by comparing the remote's owner with the signed-in host account (`gh api user --jq .login`, if `gh` is present); unsure means `others`.
   - **Every setting,** detected where possible: VCS and workspaces folder, the gate (exactly what CI runs) and a light gate, branch strategy, tracker, docs folder, lanes, limits, release documents and format, and security and compliance triggers fitted to this project (the template's are generic).
   - **The teams:** missions, owned paths, a named lead and QA each (programmers are short-lived, named per task) and the review teams' reviewers, always including the internal team, the security team and the release reviewers unless the Engineer opts out.
   - **nodes:** pre-selected yes in the Engineer's own repo; no in anyone else's, where a tree would be local-only and live only in your checkout.
3. **Exclude first.** Your first write is the repo's exclude file, before any other file, with no jj command in between (jj snapshots, and so tracks, any file not yet excluded). Find it with `git rev-parse --git-path info/exclude` (git, and jj colocated with git). In jj without colocation it is `info/exclude` in the git directory that `.jj/repo/store/git_target` names (an absolute path, or relative to `.jj/repo/store/`; if `.jj/repo` is a file, it names the repo directory to read instead). Add, each only if missing and not already ignored (`git check-ignore -q <path>` succeeds):
   - in every repo: `.claude/settings.local.json`;
   - in a repo that isn't the Engineer's, also: the teams file, `CLAUDE.local.md` and the docs folder.
   Can't find the exclude file: say so, and write nothing else until the Engineer decides.
4. **Write `.claude/TEAMS.<format>`** with your file tools. Opt-outs go in its **Opt-outs** with the date and the Engineer's words. Never regenerate it over the Engineer's edits.
5. **Write the pointer,** this one line, unchanged, into `CLAUDE.md` (the Engineer's own repo) or `CLAUDE.local.md` (anyone else's):
   ```
   **Way of working: teams.** The main session is the Orchestrator: it loads the `teams:orchestrate` skill at the start of each session and again after compaction, and follows it. A Spawn (any subagent) skips this line and follows its brief.
   ```
   If the repo has an `AGENTS.md` and neither a `CLAUDE.md` nor a `CLAUDE.local.md` existed before, put `@AGENTS.md` on the line after the pointer: the new file would otherwise stop Claude reading the `AGENTS.md`.
6. **Hand the Engineer the settings worksheet.** Never edit settings yourself. Read `.claude/settings.local.json` if it exists, then give the full file content to save, keeping every existing key and adding:
   - `permissions.additionalDirectories`: the workspaces folder, as an absolute path;
   - `permissions.allow`: the gate and light gate (e.g. `Bash(npm test *)`), the VCS commands the lifecycle runs (e.g. `Bash(git worktree *)`, `Bash(git status *)`, `Bash(git diff *)`, `Bash(git commit *)`; a push rule only for the slice branches' prefix, never main), using `PowerShell(…)` rules on a machine whose shell is PowerShell.

   Say in the worksheet, above step 1: these rules match the command, not the folder, so they also let any agent run those commands in the Engineer's own checkout without asking. The steps: open the file in an editor; replace its content with the block you give; save; restart Claude in the project. Done when: `/permissions` lists the folder and the rules.
7. **nodes, when chosen:** if `nodes --version` doesn't answer 0.7.0 or later, or the `nodes` plugin's skills (e.g. `/nodes:chart`) are absent, hand a worksheet: `claude plugin install nodes@ways`, then the binary, either from https://github.com/ZurNetwork/nodes/releases (Linux and macOS builds) or `cargo install --git https://github.com/ZurNetwork/nodes --locked --tag <the latest tag, which you look up and write in>`. On Windows only the `cargo` route exists. If the Engineer declines, record the opt-out.
8. **Ask for the first release's scope,** or the off-cycle work, and run the lifecycle.
