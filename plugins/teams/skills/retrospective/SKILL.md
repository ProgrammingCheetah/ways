---
name: retrospective
description: Evidence-based retro of Claude's own recent work, from commits, PRs and review feedback. A release's retro is the release instance's, from board statistics. E.g. /teams:retrospective, /teams:retrospective last 2 weeks.
argument-hint: "[time range, default: last 2 weeks]"
---

# /teams:retrospective: reflect on the work, from evidence

Ground every point in a commit, PR or file, not in impressions. Honest, constructive, specific: concrete examples over generalities. A release's retrospective is not this skill's: the release instance writes it from board statistics (`zaf report`, `zaf asklog`).

**Config:** read `Repository › docs`, `vcs` and `main` from the teams file when one exists (`.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`, at the root of this checkout; if absent and this is a secondary workspace, at the root of the main checkout: git, the first path `git worktree list` prints; jj, `jj workspace root --name default`). Without one, detect: a `.jj/` folder means jj, else git; the default branch from `gh repo view --json defaultBranchRef` or `git symbolic-ref refs/remotes/origin/HEAD`. `<docs>` is `Repository › docs` from the main checkout's root.

## Claude's retro: `/teams:retrospective [time range]`

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

## Lessons (after writing)

A retro whose lessons never reach the project's memory is write-only. Offer to carry each **durable, new** lesson into the project's memory or `CLAUDE.md`; the Engineer says yes first, lesson by lesson. Update an existing note rather than duplicating it; skip lessons already captured and one-off ones. If nothing is genuinely new, say so instead of inventing one. Run by a Spawn: list the candidate lessons in the report instead of writing them.
