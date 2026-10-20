# Runners: roles outside Claude

Read when the teams file's **Runners** assigns any role to a runner: a model outside Claude, run through its own command-line tool. A role with no runner, a missing section, a missing `runs` key or `—` runs on Claude, as the skill says. A role on a runner replaces Claude in that role, and the skill's model rules don't apply to it.

**Assignable roles:** lead, build lead, programmer, security reviewer, release reviewer, top-level QA, architect, document author. Never the glue lane: it pushes, opens and merges PRs, and removes workspaces. A role naming a runner the file doesn't define: tell the Engineer, and run it on Claude.

## 1. Setup, before a runner's first run in the project

1. **Someone else's repo** (`Repository › owner` = others): first ask whether the repo's owners allow its code to go to the runner's provider, pre-filled no. Until the Engineer says yes, the runner's roles run on Claude; record a no in **Opt-outs** (`runners`).
2. **The Engineer's worksheet.** Never edit the tool's settings yourself. Say plainly:
   - the runner works under its own tool's permissions, not Claude Code's: the deny rules don't reach inside it;
   - with main's protection off, only those permissions keep it off main;
   - its settings may be global (agy: `~/.gemini/antigravity-cli/settings.json`, for every use in every project);
   - everything it can read goes to its provider: its working folder, every folder its command adds, and files its tool loads on its own (agy: `AGENTS.md`, `GEMINI.md`).

   The steps:
   1. Install the tool and sign in.
   2. Check the runner's model is listed (agy: `agy models`).
   3. In the tool's own settings, allow every command its roles run, as narrow prefix rules, and deny what must never run. A command allowed by no rule and denied by none ends a headless run with no report (agy, tested), so the allow list must be complete:
      - every role: the read-only commands its lane uses (e.g. `rg`, `ls`, `cat`, `head`), and `nodes` when in use;
      - git: `status`, `diff`, `log`, `show`, `add`, `commit`, `fetch`, `merge` (a branch behind its base merges it in); jj: `st`, `diff`, `log`, `show`, `file list`, `describe`, `new`, `commit`, `git fetch`;
      - reviewers and QA: the project's targeted test commands; writing roles: also the gate and light gate as written, and `git rm` (or jj's equivalent) for deleting files;
      - file writes in the folder a run works in (agy: `write_file(./)`, which the trial showed allows writes there and refuses them outside; writes aren't auto-allowed);
      - deny: pushes (`git push`, `jj git push`), `gh`, `sudo`, `rm -rf`, `git reset --hard`, and writes into `.git/`, `.jj/` and `~/.ssh`. A denied command doesn't end the run: the runner reports the refusal.

      agy: `"allow": ["command(git status)", …]` and `"deny": ["command(git push)", …]` under `permissions`. A `command(…)` rule matches word by word as a prefix, and deny beats allow. Never a bare tool rule (`command(git)` or `command(jj)` also allows a push). Never an allow-everything flag (agy: `--dangerously-skip-permissions`).

   Done when: the model is listed and the rules are saved.
3. **Your trial,** in the exact form a run uses (section 2), so it also tests auto mode on your `cd … &&` command:
   1. Make a throwaway git repository `<workspaces>/.scratch/runner-trial/repo`, with a bare remote beside it.
   2. Put a probe file in the run folder's `inputs/`.
   3. Write a brief that does what its roles do: quote the probe's first line, run `git status`, plant a bug by editing a tracked file and `git restore` it; for writing roles, also commit one new file. Then try `git push`.

   Passed when all of these hold:
   - `report.md` quotes the line and reports every step, the push refused;
   - the workspace ends clean (writing roles: the commit exists);
   - the bare remote is unchanged;
   - a second run doesn't know the first (runs are fresh);
   - a third, with the tool's timeout set very short (agy: `--print-timeout 5s`) on a long task, fails as a timeout.

   Record in `<docs>/runner-trials.md`: the date, the result, each run's exit code, and the exact text the tool wrote for the refused push (its **refusal notice**) and to `errors.log` for the timeout and any run that ended early (its **failure markers**). agy, tested: every case exits 0; a tool no rule covers writes `no output produced — a tool required the` and leaves no report; a timeout writes `print timeout after` and may leave a partial one; a deny rule's refusal shows in the report (`Matches user-configured deny rule.`). Failed: show the Engineer, and its roles stay on Claude until a trial passes.

## 2. A run

- **Its folder:** `<workspaces>/.scratch/<its name>/<n>/`, `n` the attempt number from 1, never reused. Copy into `inputs/` every file the brief names outside the role's workspace, and name the copies in the brief. That covers release documents, plans, templates and style rules. A skill the brief names: copy the plugin's whole `skills/` folder to `inputs/skills/`, keeping its layout so a skill's `${CLAUDE_SKILL_DIR}/..` paths resolve, and say in the brief that `${CLAUDE_SKILL_DIR}` means `<run folder>/inputs/skills/<that skill>`. Write the brief there as `brief.md`.
- **Where it works:** its workspace. A build lead on a runner with several workspaces works one per run: each round runs it once per workspace. A role with none (a document author, a roadmap lead, document QA, a retrospective) works in its run folder and writes its document there, or returns it as its report. You review it and copy it into `<docs>`. Nobody commits in the Engineer's checkout.
- **The command:** substitute `{prompt}` with `Read your brief at <run folder>/brief.md and follow it. Your last message is your report.` and `{dir}` with the run folder, both single-quoted. A path holding `'`: don't start it; run the role on Claude. The command must start a fresh conversation, never resuming or attaching to one (agy: no `--continue`, `--conversation` or `--project`).
- **Running it:**
  - For a writing role, record its workspace's commit first.
  - Then run in the background `cd <where it works> && <command> > <run folder>/report.md 2> <run folder>/errors.log`.
  - Name the runner and model at launch.
  - A run counts as a Spawn for the limits, ownership and your review. With the agent board, you hold its locks.
- **Failed** when any of these holds:
  - it exits non-zero;
  - `report.md` is empty;
  - `errors.log` holds one of the runner's failure markers from `<docs>/runner-trials.md` (a timeout's partial report is never a report);
  - a refused tool (its refusal notice) kept the role from its work, as its report says. Other refusals don't fail the run: note them in your review. A run that ended on a tool no rule covers: hand the Engineer the rule to add, or the deny;
  - its command asks for structured output and the status isn't success (agy `--output-format json`: `status` = `SUCCESS`).

  Retry once as attempt `n+1`. Failed again: run the role on Claude in its lane as usual, and tell the Engineer, naming both run folders. A failed writing run's leftovers stay in its workspace: the next brief names them (the status, and the diff from the recorded commit).
- **Roles that don't commit, in a workspace** (every role but programmers and build leads):
  - Before each run, record the workspace's status and commit. After it, check both are unchanged: every reviewer reverts its planted bugs and proof files (an exploit test, a failing case) before reporting, so its workspace ends as it began. The brief says to plant them by editing tracked files only and revert with the VCS (`git restore`, `jj restore`): agy has no delete tool, and deleting a new file would take `rm`.
  - Planner leads and plan security challenges also get `planner-check.md` around every run. Keep its marker outside the run folder and the workspace, and also check the run folder for changes beyond `report.md` and `errors.log`.
  - Any change stops that piece and goes to the Engineer, as the check says.
  - A separate read-only runner for these roles is safer (agy: `--mode plan`; whether it holds in headless runs is unverified).

## 3. Its brief

- **The brief template as usual,** with its "On a runner" lines.
- **Your lane:** under `## Your lane`, paste the body of its lane's agent file (`${CLAUDE_SKILL_DIR}/../../agents/<agent>.md`, without the frontmatter). The runner line overrides its Claude-only parts: subagents, the hand-back tool, the `cd` start.
- **Team instructions** as usual.
- **Never resumed.** Wherever the lifecycle or a template says to resume a lead, QA or the architect, run it fresh instead. Its brief carries its last report plus the new input: the claims addressed to it, a ruling, QA challenges, or a fix round's reports.
- **A top-level QA at a milestone:** its Done when also asks for the log's material, with the part of `retro-template.md` below its `---` pasted. You save that as its log (`<docs>/retros/<YYYYMMDD>-qa-<version>-m<n>.md`).

## 4. Build leads and programmers in rounds

When the build lead or the programmer role is on a runner, a slice's build runs in rounds. A Claude build lead with Claude programmers is unchanged.

1. **First round:** the build lead's report is the task split, each task's model and one programmer brief per task.
2. **Check each brief** against the template: owned paths, no push or force-push, nothing that loosens the skill's rules. Set each model by the policy, whatever the split says: Opus for security-nature work, or a Lanes pin; a programmer on a runner runs there. Then run or spawn them within the budget, independent ones in parallel.
3. **Each later round:** a fresh build lead gets the reports, plus each task's fix-round count and model. It reviews them, runs the critique by path, applies and commits the findings it keeps, lists the ones it declines, and runs the gate. Work for a programmer comes back as more briefs, and another round. Done when none is left and the gate is green.
4. **The push:** a Claude build lead pushes in its last round, as usual. A build lead on a runner never pushes. First check its changed files against the owned paths. Then a `teams:machinery` Spawn, briefed with the commit it reported, checks the branch tip is that commit, fetches (remote commits it lacks: gray, stop) and pushes plain.
