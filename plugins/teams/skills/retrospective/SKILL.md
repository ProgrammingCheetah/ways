---
name: retrospective
description: Evidence-based retro: a team lead's after a feature or release, the top-level QA's log after a milestone, or Claude's on its own recent work. E.g. /teams:retrospective api v0.2, /teams:retrospective qa v0.2 m1, or /teams:retrospective last 2 weeks.
argument-hint: "<team> <feature or release>  |  qa <version> m<n>  |  [time range, default: last 2 weeks]"
---

# /teams:retrospective: reflect on the work, from evidence

Ground every point in a commit, PR or file, not in impressions. Honest, constructive, specific: concrete examples over generalities. Three modes, picked from the arguments.

**Config:** read these from the teams file: `Repository › docs`, `vcs`, `main`, and the **Teams** table. It is `.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`, at the root of this checkout; if absent and this is a secondary workspace, at the root of the main checkout (git: the first path `git worktree list` prints; jj: `jj workspace root --name default`). Without a teams file, detect: a `.jj/` folder means jj, else git; the default branch from `gh repo view --json defaultBranchRef` or `git symbolic-ref refs/remotes/origin/HEAD`. `<docs>` is `Repository › docs` from the main checkout's root.

## Which mode

- The first argument is `qa` (the **Teams** table's qa row), followed by a release and a milestone (`m<n>`): **QA mode**.
- The first argument names another team in the **Teams** table (not `qa`: QA writes milestone logs, not team retros), followed by a feature or release: **team mode**.
- Anything else (empty, or a time range such as "last 2 weeks" or "since May 1"): **self mode**.

## Team mode: `/teams:retrospective <team> <feature or release>`

Run by the team's lead (`teams:designer`) when a feature or release ends. Fast: a few lines each.

1. **Gather** (read-only): the feature's commits and PRs (its branch names, `Branches › integration`, `slice` and `fix`, or the PRs the Orchestrator names), their review comments and fix rounds, the release folder `<docs>/releases/<version>/` (plans, reviews, rulings), QA's milestone logs for the release, and the programmers' reported blockers (from your brief).
2. **Write** the lead's part of `${CLAUDE_SKILL_DIR}/../orchestrate/retro-template.md` (the part above its `---`): header (date, `Lead:` your name from **Teams**), then **Went well**, **Cost time or tokens**, **Try next**. Every point cites a commit, PR or file. Programmers' blockers are input; polish is not.
3. **Save** to `<docs>/retros/<YYYYMMDD>-<team>-<feature>.md` without asking. Don't write a QA log: the top-level QA writes one per milestone, in QA mode.

## QA mode: `/teams:retrospective qa <version> m<n>`

Run by the top-level QA (`teams:designer`) at the end of each milestone, after its fix rounds.

1. **Gather** (read-only): the milestone's row in `<docs>/releases/<version>/milestones.md` (its range and fix rounds), its reports (`reviews/m<n>-architect.md`, `reviews/m<n>-qa.md`), the saved slice plans of its timeline phase, and your last milestone's log.
2. **Write** the part of `${CLAUDE_SKILL_DIR}/../orchestrate/retro-template.md` below its `---`: header (date, `QA:` your name from the **Teams** table's qa row), then **What I caught, and how**, **Checks I ran by hand more than once**, **What slipped past me**, **Look at next milestone**. Every point cites a commit, PR or file.
3. **Save** to `<docs>/retros/<YYYYMMDD>-qa-<version>-m<n>.md` without asking. The next milestone's fresh QA is briefed with it.

## Self mode: `/teams:retrospective [time range]`

Claude's own first-person retro. Range: the argument, else the last 2 weeks.

**Where it goes:** with a teams file, `<docs>/retros/<YYYYMMDD>-claude-<range>.md`, with the range slugified (lowercase, spaces to `-`, e.g. `last-2-weeks`); say the path before writing. Without one, ask the Engineer up front for a file path, or a directory (then `retrospective-<YYYY-MM-DD>.md`), and confirm before writing. A Spawn has no Engineer to ask: put the question in its report.

**Evidence** (scoped to the range):

1. **Commits** authored or co-authored by Claude: a `Co-Authored-By:` trailer naming Claude, or an Anthropic noreply address. Sort them into features, fixes, refactors, tests.
   - git: `git log --since="2 weeks ago" -i -E --grep="^Co-Authored-By:.*(claude|noreply@anthropic[.]com)" --name-only --format="%h %s"`. For commits authored as Claude, run `git log --since="2 weeks ago" -i --author=claude --name-only --format="%h %s"` too.
   - jj: `jj log --no-graph -r "description(regex:'(?i)co-authored-by:.*(claude|noreply@anthropic[.]com)') & author_date(after:'2 weeks ago')" -T 'commit_id.short() ++ " " ++ description.first_line() ++ "\n"'`. Use `before:'<date>'` for an end date; `jj log --no-graph --name-only -r '<that revset>' -T 'commit_id.short() ++ "\n"'` lists the files touched (`jj diff -r` fails when the commits aren't contiguous).
2. **Loose ends:** with the Grep tool, search the files those commits touched for `TODO`, `FIXME`, `HACK`, `XXX`. Note any pattern of things left behind.
3. **Feedback:** PRs in range through `gh` (`gh pr list --state all --search "created:>=<YYYY-MM-DD>" --json number,title,url,state,mergedAt,commits,reviews,comments`), then inline review comments with `gh api repos/{owner}/{repo}/pulls/<number>/comments`. Keep PRs whose commits are Claude's. Surface **recurring themes**: tests requested, style nits, architectural pushback, praise. `gh` absent or failing: say in one line that PR feedback wasn't checked, and go on with commits alone.

**Write** these sections, first person, each citing specific commits or PRs:

- **Summary:** what I worked on this period.
- **What went well:** work that landed without revision, good coverage, clear explanations.
- **What could be better:** changes that took several revisions, feedback to learn from, unresolved TODOs, over- or under-engineering, patterns in the corrections I received.
- **What I should change:** concrete, actionable changes to how I approach future work.
- **Path forward:** what's in progress, what needs follow-up, how I can better support the team.

## Lessons (after writing, either mode)

A retro whose lessons never reach the project's memory is write-only. Offer to carry each **durable, new** lesson into the project's memory or `CLAUDE.md`; the Engineer says yes first, lesson by lesson. Update an existing note rather than duplicating it; skip lessons already captured and one-off ones. If nothing is genuinely new, say so instead of inventing one. Run by a Spawn: list the candidate lessons in the report instead of writing them.
