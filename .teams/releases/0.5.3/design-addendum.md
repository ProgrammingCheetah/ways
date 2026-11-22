---
signed: 2026-11-19T19:33:34Z
hash: 5417e4629fcc1ec5fa20dd962ede81501adba35b53d2811a0df3bc9ec46f1f37
---
# Release 0.5.3 design addendum: what zaf builds after the chair

2026-11-19 · Written by Claude from `design.md`'s D14 and four research reports at main f6df2f88 (zaf 0.5.2). It holds the Engineer's rulings of 2026-11-19: the four nits from the first review go to 0.5.3 ("Lovely! Approved"), and so do the five build-loop gaps ("Yes sure. All five."). It was challenged by a fresh designer (A1 to A17, resolved below), and the Engineer signs it. Its epics are filed only after the signature (D14).

## Summary

The chair is built. This addendum covers the rest of 0.5.3, which zaf builds as six epics:

1. **A security fix found while writing this.** The model profiles allow `git log`, `git show` and `git diff`, and each of those takes `--output=<file>`, which writes any file the user can write. This is live in 0.5.2. Q9 decides when it ships.
2. **Epics that finish, and conflicts named.** A finished epic clears what it blocks, and the fix phase is told when a pull request conflicts with its base.
3. **CI and the merge guard:**
   - a pull request no check will come for passes without asking;
   - an empty commit no longer hides the head;
   - a flaky check gets one fresh run.
4. **The leftovers of 0.5.2's reviews:** eleven small fixes.
5. **Colour and "what waits on you first"** on every other verb. Plain output stays as it is today, apart from `zaf status`'s order, and `--json` doesn't change.
6. **A fast gather.** The chair's read of ways goes from about 20 s and 383 web calls to 3 s, and `zaf status` from 5 s to 2 s.
7. **A plain git repository works** (#229). This is the largest part, at about 15 tasks.

Before the epics are filed, final 0.5.2 (main) merges into `feature/0.5.3`, which doesn't hold #400's fixes yet. The file:line references here are at f6df2f88, and each task finds its lines again on `feature/0.5.3`.

Questions Q1 to Q9 each carry Claude's answer. Signing the addendum as it stands takes all nine answers.

## Questions for the Engineer

**Q1. Should a finished epic also get the `state/done` label?** zaf reads that label back: an open issue labelled `state/done` counts as done (`adapters/forgejo/board.rs:208-213`). If zaf wrote the label and the epic later stopped being done, for example when a task is filed under it, the label would keep clearing its blockers.
**Claude's answer (2026-11-19; the Engineer adjusts):** no label. zaf derives "done" on read (D16). The labels already added by hand stay, as the Engineer's own override.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

**Q2. When is an epic done, for what it blocks and for the release?** Today an epic whose every task was dropped counts as Done (`domain/epic.rs:178,234`). That clears its blockers, and it lets the release pull request open (`spawn/built.rs:17-20`), even though none of its work happened.
**Claude's answer:** one rule for both. An epic is done when no task is open and at least one is done. An epic whose every task was dropped shows in Problems as "every task dropped", so you decide whether to file new work or drop what it blocks.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

**Q3. How does zaf know no check will come for a pull request?** Here are the options:
- (a) Read the CI workflows' `pull_request.paths` and compare them with the pull request's changed files. This needs two new dependencies, a YAML reader and a glob matcher.
- (b) Make a "no check" ruling hold for the whole task rather than for one head. That is one small task, but a later head that touches code would merge with no CI, and nothing would catch it: CI runs on push only for `main` (`.forgejo/workflows/zaf.yml:14-21`).

**Claude's answer:** (a). D17 says when a check counts as expected.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

**Q4. What are the speed targets?** These were measured on ways on 2026-11-19 (D25), with 13 open epics and no open pull request:

| Read | Today | Target, in web calls | Target, in seconds |
|---|---|---|---|
| `zaf status` | 5 s, 87 calls | 6 + 2 per open epic + 5 per open pull request | 2 s |
| The chair's first read of a project | 20 s, 383 calls | 12 + 2 per open epic + 5 per open pull request | 3 s |

**Claude's answer:** those targets. The call counts are tests against a counting fake, so they hold on any machine. The seconds are measured by hand on ways, twice: once with no open pull request, and once with five, where 1 s more is allowed.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

**Q5. How does an action get its git version?** Here are the options:
- (a) The runner chooses. Each action that runs jj says so with a metadata line, `# zaf vcs: jj`. On a git project, the runner looks in each tier in turn, project first: `<name>.git` first, then `<name>` unless it is marked for jj. When neither fits, it refuses by name ("no git version of `merge-base`").
- (b) Each jj action checks `ZAF_VCS` at its top and hands off to `<name>.git`.

**Claude's answer:** (a). Choosing a backend is zaf's job, not a script's. A pack test checks that every action marked `vcs: jj` has a `.git` sibling.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

**Q6. What may models run under git?** Every tool a manifest requires becomes `Bash(<tool> *)` (`app/profile/mod.rs:807-812`), so requiring git would open all of git. Today's git reads are open too (`:753`), and Q9's hole comes from them.
**Claude's answer:**
- Exact verb rules, never `Bash(git *)`: `status`, `diff`, `log`, `show`, `blame`, `add`, `commit`, `restore` and `rev-parse`.
- Denied on every git rule: `--output`. Also denied: git's global options that change where git reads or runs (`-C`, `-c`, `--git-dir`, `--work-tree`, `--exec-path`, `--config-env`), as listed in git(1).
- Pushes and merges go through the actions only, as with jj.

Still unverified, and checked by the task:
- whether Claude Code matches `GIT_CONFIG_COUNT=… git log` against `Bash(git log *)`;
- whether `acceptEdits` lets a model edit `.git/config` or the hooks.

Where the answer is yes, they get deny rules too.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

**Q7. May `zaf status` change the order of its plain output?** Putting what waits on you first moves lines. No line's text or leading word changes, so `rg '^ready'` and similar searches keep working. Models read the plain output: the release instance (`agents/release.md:94`) and the catch-me-up and objectives skills. The scheduler reads `--json`.
**Claude's answer:** yes. The task names each reader of the plain output and checks that none depends on the order.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

**Q8. Should a long-running zaf remember what it read last time?** The chair, the hub and the scheduler could keep the board in memory and, on each read, ask Forgejo only for the issues changed since the last one. That `since=` call works and costs 80 ms, and it would bring a repeat read under 0.5 s. But it goes against your 2026-11-10 rule that zaf calculates everything when asked. The signed D14 says "cached between gathers", so this answer changes D14.
**Claude's answer:** not in 0.5.3, and D14 reads "read once, at the same time" instead. The live reads made fast (D25) already beat the chair's 5 s cadence. Keep the rule, and look again if 0.6.0's Jira reads are slow.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

**Q9. When does the `--output` fix ship?** It is one task (D21, item 11). Here are the options:
- (a) As 0.5.3's first epic. It has no blockers and goes into the lane before everything else, so it ships when 0.5.3 does.
- (b) A patch of its own right away. The chair and this addendum would move to 0.5.4, which means renaming the release folder, epic #327's release line and pull request #350.

**Claude's answer:** (a). The hole needs a model to choose to write outside its workspace, and today zaf's models read only your own board. 0.5.2 has been public for a few hours.
**Engineer:** Claude's answer taken (2026-11-19, signed as is).

## Part A: epics that finish, and conflicts named

**D16. A finished epic counts as done for what it blocks.**
- The rule is derived on read: an open epic that is done by Q2's rule reads as `Standing::Done`, with the close time of its last task. One helper applies it at all four places that read a blocker:
  - `schedule/gather.rs:637-656`;
  - `schedule/labels.rs:290`;
  - `spawn/blockers.rs:94`;
  - `hub/state/line.rs:209` (`:226` on `feature/0.5.3`).
- The release gate, `is_built`, uses the same rule.
- The epic stays open, because `epics()` lists open epics only and the release pull request needs an open epic of its version. No label is written (Q1).

**D19. A conflict with the base is a fact the fix brief states.**
- `PullFacts::mergeable: Option<bool>` comes from the forge. Forgejo's pull request JSON carries `mergeable`. For GitHub, `gh pr view --json mergeable` is used, and the task checks its values.
- When it is false, the fix brief's "Why this run" says the pull request does not merge into its base, and to run `zaf do sync-base <task>` first.
- Placement is unchanged, and sync-base never runs by itself (R28).
- How fast Forgejo works out `mergeable` again after the base moves is unverified, so the task checks it.

## Part B: CI and the merge guard

**D17. A pull request that no check will come for passes CI without a question.** This is Q3's answer (a).
- A new fact, `PullFacts::checks_expected`, comes from a new port, `CiFilter`.
- The Forgejo adapter reads `.forgejo/workflows/*.yml` and `*.yaml`; the GitHub adapter reads `.github/workflows/`. Both read them at the pull request's base, never at its head.
- The base is safe to trust: every change to its workflows came through a pull request that touched a workflow file. Such a pull request always expects a check, so it ran CI and was reviewed.
- The forge adapter lists every changed file, with renames counted under both names (Forgejo's `previous_filename`). Today it keeps only the last file (`adapters/forgejo/forge.rs:313-316`).
- A check is expected, and today's question comes back, when:
  - a changed file matches a workflow's paths;
  - a workflow file changed;
  - the file list may be cut short;
  - the reader isn't sure of a filter (`paths-ignore`, `!` patterns, branch filters).
- When no check is expected, CI counts as done only while no check sits at the head. A check that turns up is waited for.

**D18. The merge guard compares work, not commits.**
- Today `workspace_at_head` asks whether the workspace's latest commit is the pull request's head (`app/actions/phase.rs:117-123`).
- It becomes "the head holds the workspace's work": the latest commit is an ancestor of the head, and the two trees are equal. That is a new Vcs port method, `holds(workspace, head)`, with one meaning for jj and git.
- Reviews stay strictly per head (`domain/schedule/phase.rs:137-140`), so a review never carries over to other work.
- `slice-merge` gets the head Land was derived at from zaf, as `ZAF_HEAD`, and passes it to `--match-head-commit`. Today it uses the working copy's parent (`slice-merge:31-32,47`), and reading the forge's head at merge time would accept whatever was pushed last.
- jj's half is in this epic. git's half comes with the git epic.

**D20. A flaky check gets one fresh run per head.** Forgejo 15's API has no re-run or job-log endpoint (`/swagger.v1.json` on the forge).
- A new pack action, `ci-rerun <task>`, is built from the `commit` and `push` actions, so it needs no git version. It commits an empty "Re-run CI at <short head>" and pushes it forward.
- It is refused (exit 1) unless a check failed at the head. It is also refused when the head is already an empty commit.
- `fix` runs it when its round left the head's tree unchanged. When `ci-rerun` refuses, `fix` exits 0, as today.
- R18, three rounds and then a question, counts rounds by tree, not by head, so an empty commit a model makes itself can't dodge it.
- Both copies, `plugins/teams/actions/` and ways' `.zaf/actions/`, stay identical.

## Part C: the leftovers of 0.5.2's reviews

**D21. The eleven fixes**, each one task:
1. **Pasting after typed text in the live view.** `cli/watch_view.rs:634` finds a paste only at the start of the line.
   - Fix: find the paste markers anywhere in the line. Refuse a line that ends inside a paste; otherwise strip every marker.
2. **The live view cuts lines by characters.** `app/spawn/view.rs:200-219` counts characters, not screen columns, so wide titles wrap and break the redraw in place.
   - Fix: measure display width with `unicode-width`, which is in `feature/0.5.3`'s lock file through ratatui and becomes a direct dependency.
3. **`zaf update` lists the releases 3 + k times** (k is the number of skipped releases), at `adapters/github/public_releases.rs:143,157`. The API allows 60 calls an hour.
   - Fix: list them once per run.
4. **Newtypes.**
   - `End.number`, `ReleasePullRequest.number` and `Opened.number` become `PrNumber`.
   - `Started.branch` becomes `BranchName`.
   - `PluginRow.installed` becomes a version type.
5. **What setup does is always a plan row.** A re-run installs new actions and regenerates profiles without asking, and cloning the design repository (`app/setup.rs:952`) is never a row (`:271-323`).
   - Fix: each of these becomes a row, and a re-run that would change something asks first.
6. **An empty PATH entry for actions.** `app/actions/run.rs:1081` gives `<zaf folder>:` when PATH is empty or unset, which searches the current folder.
   - Fix: append the rest of PATH only when it is not empty.
7. **Only zaf goes first on an action's PATH.** `run.rs:1064` puts zaf's whole folder first, so every other tool in it (often `~/.local/bin`) jumps ahead of the user's PATH.
   - Fix: put first a folder that holds only a `zaf` link to the running binary. There is one folder per binary, named by a hash of its path, under `$ZAF_HOME/state/bin/`, with mode 700, written under a temporary name and then renamed. Two zaf binaries never share one.
8. **A HOME holding `:` fails late.** `install.sh:866` refuses the default folder but carries on with an empty one.
   - Fix: `set_dir "$HOME/.local/bin" || return 1`.
9. **`zaf update` says "Why: not said" for `pr-open`.** 0.5.2 changed it without a `# changed:` line.
   - Fix: add the line. A pack test fails whenever an action differs from its copy at the last release tag and has no new `# changed:` line. CI fetches the tags for it, since the checkout at `.forgejo/workflows/zaf.yml:51` has none.
10. **jj profiles don't block `jj git push`.** The blocked lists at `app/init.rs:409-427` miss it.
    - Fix: add it, with a test.
11. **git's `--output` writes any file** (Q9). Tested on git 2.56: `git log -1 --output=<file> --format=%B` overwrote a file outside the repository with a commit message the model chose. ways' build profile allows `Bash(git log *)`, `show` and `diff` with no deny rule.
    - Fix: Q6's deny rules on the git reads that jj profiles allow, with a test that the generated profile carries them.

## Part D: colour and what waits on you first

**D22. Every verb's text is built as lines with meanings, and the CLI paints them.**
- Render code builds `Line`s (`domain/screen.rs:55`), and the CLI paints them with `Screen::detect()`.
- Without colour (no terminal, `NO_COLOR` set, or `TERM=dumb`), the output is byte for byte what it is today, apart from D23's new order. `--json` never sees a `Line`.
- stderr gets its own terminal check, because `Screen` reads stdout only.
- The meanings are 0.5.2's: waiting on the Engineer lavender, a failure red, done green, ids and paths yellow, commands cyan, zero counts dim, and `zaf` in an error magenta.

**D23. `zaf status` puts what waits on you first.** Today the text of the questions comes last (`app/status.rs:720-750`).
- The new order:
  1. **Needs you**, built with `next_for_you`'s kinds and order;
  2. **Problems:** orphaned claims, dead blockers, cycles, tasks that don't parse, stale profiles and the guard's stop;
  3. **Running**;
  4. **Ready, waiting and claimed**;
  5. **Epics, slots, the machine, budget and guard.**
- One inbox model, as `design.md` D2 and D5 want: `next_for_you`'s item rules move into a function over an input that both `Status` and `ProjectState` can fill. Status and the chair then list the same things in the same order, and nothing is read twice.

**D24. The verbs, one task each:**
1. `zaf status` built as lines;
2. `zaf status` painted;
3. `zaf status` reordered, last, because it is the one task that changes plain output;
4. `zaf doctor`: failures first, `ok` in green, "shadows the machine file" dimmed;
5. `zaf epic list` and `zaf epic show`: the stage word painted, "(edited since signed)" in red;
6. `zaf check`;
7. `zaf forge checks`;
8. `zaf guard`;
9. errors, starting with a magenta `zaf`;
10. `zaf board lint`'s problems in red, and `zaf report`'s headings in bold.

## Part E: the gather made fast

**D25. Read once, read at the same time.** Measured on ways at f6df2f88 (zaf 0.5.2), with nothing written. Most of the cost is the same data read again, not slow reads:
- Every Forgejo call is its own `curl` process, about 45 to 50 ms each (`adapters/forgejo/api.rs:365`). About 25 ms of that is looking up `yagdrassyl.local` again.
- Nothing runs in parallel, and nothing is shared between the parts of one read.
- `zaf status` reads the epics three times (`app/status.rs:372`, `cli/status.rs:43`, `schedule/gather.rs:551`), at 27 calls each, and the questions twice.
- The chair's read calls `board.task` for each of the 155 tasks of the open epics, at two calls each (`hub/state/line.rs:40,185`; `adapters/forgejo/board.rs:383`). That is 312 calls, though the epic's dependency rows already carry each task's body, labels and link (`board.rs:217` drops them).
- Each pull request is fetched four times per read (`adapters/forgejo/forge.rs:310,288,306,193`).
- The 9.6 s in D14 was taken with open pull requests; ways has none right now.

The tasks:
1. The chair takes each task from its epic's dependency rows, with no `board.task` per task.
2. One epics read and one questions read per screen, shared by status, the gather and the hub.
3. One `pulls/N` read per pull request per read, and the page size read once per process.
4. Forgejo's independent reads run at the same time: each epic's comments and dependencies, each task's blockers, each pull request's reads, and the label lists.
   - The pool lives inside `ForgejoApi`, with at most 8 `curl`s per host, on `std::thread::scope`. The ports and the app layer don't change.
   - Tests that check call order become set-based checks.
5. GitHub reads come in batches: one `gh issue list` or GraphQL call instead of `gh issue view` for each task (`adapters/github/board.rs:151`). A GitHub tab is read at most every 20 s, because 5,000 calls an hour is about 1.4 a second and the instances share them.
6. Each verb's call budget (Q4) is a test against a counting fake (`fakes/board.rs`).

## Part F: a plain git repository (#229)

**D26. A git backend behind the same port.**
- `[vcs] kind = "git"` selects a new `GitVcs`, which implements the port's six read methods (`ports/vcs.rs`) and D18's `holds`.
- `kind` stays required inside `[vcs]` (`manifest/sections.rs:143-149`). zaf passes `ZAF_VCS` from it to every action. With no `[vcs]`, actions get no `ZAF_VCS` and run as they do today.
- Older binaries refuse `kind = "git"`, so a git project needs zaf 0.5.3 or newer. Doctor and the release notes say so.
- A contract test, `tests/vcs_contract.rs`, runs the fake, jj and git adapters through the same cases, as `tests/board_contract.rs` does for boards.
- Diffs use `git diff --merge-base`. Untracked files are counted through a temporary index, so briefs see the whole work, as they do with jj.

**D27. Setup and doctor follow the repository.**
- Setup's init step (`app/init.rs`) detects the kind, checking for `.jj` before `.git`, since a colocated jj repository holds both. It writes `[vcs]` and `[requires]` to match.
- Doctor checks that the folder is a repository of the named kind, and offers a fix when it isn't.
- jj stops being required: install.sh already checks git.
- Doctor checks git's version against the oldest one `merge-tree --write-tree` works with. The task finds that version, since 2.38 is unverified.

**D28. The git actions.** Q5 decides how an action finds its git version. The rules:
- Workspaces are `git worktree`s under `.zaf/workspaces/`.
- A slice works from `origin/<base>`, never from a local base branch, because git checks a branch out in one worktree only. The base comes from `ZAF_BASE`, never from stored state.
- jj keeps a conflict as data; git cannot commit a conflicted merge. So git's `merge-base` stops with the conflicting files listed and leaves the merge in progress for the fix phase. A spike checks first how the lane reads a merge in progress.
- `--probe` uses `git merge-tree --write-tree`.
- `papers-commit` uses plumbing (`commit-tree`), so the checkout never moves.
- The work comes in three units:
  - `workspace-open`, `workspace-drop` and `commit`;
  - `push` and `slice-pr`;
  - `merge-base`, `base-open`, `papers-commit` and `release-open`.
- ways' own actions (`base-up`, `ship`, `sync`, `tag`, `release`, `land`) stay jj-only, since ways is a jj repository.

**D29. Models under git**, as Q6's answer: the exact verb rules and the deny rules in the profile git projects get.

## Features and their "done when"

| # | Feature | Done when |
|---|---|---|
| F18 | Finished epics clear their blockers | A ticket blocked by an epic with every task done is in the lane, the epic stays open, and no label is written. An epic with every task dropped shows in Problems, keeps its blockers waiting, and doesn't open the release pull request. |
| F19 | No question when no check comes | A pull request that changes only `docs/x.md` passes CI with no check and raises no question. One that also changes a workflow file, or renames a code file into `docs/`, still asks. |
| F20 | Empty commits keep the head | With CI and review clean at a head that is an empty commit on top of the workspace's latest, the phase derives Land, and `slice-merge` merges at exactly `ZAF_HEAD`. |
| F21 | Conflicts named in the fix brief | A fix brief for a red check on a pull request the forge marks not mergeable names the conflict and the `sync-base` command. |
| F22 | One fresh CI run | A fix that leaves a red head's tree as it was pushes one empty commit. A fix at that empty head pushes nothing, and its round counts toward R18. |
| F23 | The leftovers | Each item of D21 passes its own test. Item 11's test reads the generated profile. |
| F24 | Colour on every verb | For each verb of D24, a test runs it twice, once with colour and once without, and the coloured output with its escape codes removed equals the plain output. |
| F25 | Status puts you first | On a board with a question, `zaf status` prints it in the first block, with its title. The chair's inbox and status's first block list the same items. |
| F26 | Fast gather | Q4's call budgets pass as tests. On ways, by hand: `zaf status` at most 2 s and the chair's first read at most 3 s, each with no open pull request and with five. |
| F27 | A plain git repository | In a new repository made with `git init` and pushed to a forge, `zaf setup` passes and one ticket goes from ready to merged. The Vcs contract test passes for all three adapters. |
| F28 | Models under git | The profile of a git project allows only Q6's verbs. A test shows `git log --output=x` is denied, and so is `git -C /tmp status`. |

## Epics to file once signed

| Epic | Decisions | Tasks, roughly |
|---|---|---|
| git's `--output` (first, no blockers) | D21 item 11 | 1 |
| Epics that finish, conflicts named | D16, D19 | 5 |
| CI and the merge guard | D17, D18 (jj), D20 | 9 |
| The 0.5.2 reviews' leftovers | D21 items 1 to 10 | 10 |
| Colour and you first | D22 to D24 | 10 |
| The gather made fast | D25 | 6 |
| A plain git repository | D18 (git), D26 to D29 | 15, and a spike |

Epic #327, the chair built by hand, is signed with these so the release instance checks it with the rest.

## One-way doors

- **`[vcs] kind = "git"` and `ZAF_VCS`** join the manifest and the actions' contract (D26). dapple and zabo will build on them (`0.6.0/objectives.md:86`).
- **`# zaf vcs:` and `<name>.git`** join the action metadata and naming (Q5).
- **`ZAF_HEAD`** joins the actions' contract (D18).
- **R18 counts rounds by tree** (D20).
- **The git permission rules** for models are a security boundary (Q6, D21 item 11).
- **New dependencies:** a YAML reader and a glob matcher (Q3); `unicode-width` becomes direct (D21).
- **`ci-rerun`** is a new pack action (D20), and `holds` a new Vcs port method (D18).
- **`$ZAF_HOME/state/bin/<hash>`** is a new folder zaf writes (D21, item 7).
- **`zaf status` reorders its plain output** (D23).

## QA challenges

A1 to A17 come from a fresh `teams:designer` (opus), 2026-11-19, reading the code at f6df2f88, `feature/0.5.3` and the board. It checked 31 file:line claims and found 27 exact. In short:
- **A1:** git's `--output` writes any file, and it is live today. The allow list is all of git, and `-C`, `--git-dir` and `--exec-path` are left open.
- **A2:** zaf reads `state/done` back.
- **A3:** "empty commits between" isn't "the same work", and reviews are per head.
- **A4:** reading the forge's head at merge time defeats the guard.
- **A5:** a model's own empty commit dodges R18.
- **A6:** D17 lacks four guards: a check that turns up, renames, a cut-short list, `.yaml` files. It also asks which branch's workflows to read.
- **A7:** the release gate counts an all-dropped epic as done.
- **A8:** keeping the base in git config is stored state.
- **A9:** "unset means jj" is new behaviour.
- **A10:** the lookup order isn't settled, and finding "runs jj" with grep hits comments.
- **A11:** one shared PATH link is fought over and can be planted.
- **A12:** `feature/0.5.3` is behind main.
- **A13:** CI has no tags to compare with.
- **A14:** models read status's plain output.
- **A15:** a second inbox model.
- **A16:** Q8 changes D14 without saying so, and the targets were set with no open pull request.
- **A17:** missing "done when"s, sizing, missing doors, and four wrong references.

## QA resolutions

Each is resolved in the text above:

- **A1:** Q6 rewritten, and Q9 with D21 item 11 added; routed to the Engineer.
- **A2:** Q1's answer becomes no label; routed.
- **A3:** D18's `holds` checks ancestry and equal trees, and reviews stay per head.
- **A4:** D18's `ZAF_HEAD`, added to the doors.
- **A5:** D20 counts rounds by tree, builds `ci-rerun` from `commit` and `push`, and says who runs it and how `fix` exits. R18 is added to the doors.
- **A6:** D17 gets (a) to (c). On (d), the base is read, for the reason D17 gives.
- **A7:** Q2 becomes one rule for blockers and the release; routed.
- **A8:** D28 derives the base from `ZAF_BASE`.
- **A9:** D26 keeps `kind` required and names the version floor.
- **A10:** Q5 gets `# zaf vcs:` and the lookup order; routed.
- **A11:** D21 item 7 gets one folder per binary, written atomically.
- **A12:** the summary says main merges into `feature/0.5.3` before filing.
- **A13:** D21 item 9 fetches the tags in CI.
- **A14:** Q7 names the readers.
- **A15:** D23 uses one inbox model.
- **A16:** Q8 says it changes D14, and Q4's budgets count per epic and per pull request; routed.
- **A17:** F24, F27 and F28 rewritten; D24 split per verb; the build-loop epic split in two, with D18's git half in the git epic; doors added; references fixed.

## Signed

2026-11-19 · Signed by the Engineer: "Sign as is", with Q9 answered "First in 0.5.3". Every question takes Claude's answer.
