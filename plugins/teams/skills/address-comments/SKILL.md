---
name: address-comments
description: Triage an open PR's review comments into a weighted table, apply clear fixes, decline nits on the PR, hand judgment and security calls to the Engineer, re-gate, push. E.g. /teams:address-comments 86
argument-hint: "[PR number or URL; empty: the current branch's PR]"
---

# /teams:address-comments: triage and close out PR review feedback

Once the PR is open (`/teams:prepare-pr`) and reviews have landed (the review bot, if any, and human reviewers), turn the comments into a decision-ready triage: weigh each one honestly, act on the clear ones, hand the judgment calls to the Engineer, **never silently wave off a security comment or break the branch.** It runs before the merge. It doesn't replace the review loop before the PR (QA, code review, the security review when triggered); it closes the loop on what review catches after.

## Input

- **A PR number or URL:** use it.
- **Empty:** the current branch's PR (`gh pr view --json number,title,url,state,isDraft,headRefName,headRefOid,baseRefName`; jj: pass the bookmark, from `jj bookmark list -r ::@`, as the argument). No open PR: say so and stop.

## Configuration

Read from the teams file (`.claude/TEAMS.md`, else `.claude/TEAMS.yaml`, else `.claude/TEAMS.toml`, at this checkout's root; in a secondary workspace without one, the main checkout's root):

- `Reviews › bot`, `Reviews › security triggers`, `Reviews › comment prefix`
- `Repository › gate`, `Repository › light gate`, `Repository › vcs`
- `Branches › engineer reviews`, `Tracker › commit format`
- the **Lanes** table (Model and Effort pins)

**No teams file:** no bot; the security triggers and the comment prefix (`Claude:`) take the template defaults (authentication; authorization and permissions; sessions, tokens, secrets and keys; cryptography; untrusted input; trust boundaries; network egress; personal data storage); the gate is what the CI config runs (`.github/workflows/*.yml` or the forge's equivalent); a `.jj/` folder means jj. Ask the Engineer once, in one round, only for what detection can't settle (a Spawn puts the question in its report).

**`gh` and jj:** in a jj workspace without `.git`, `gh` can't see the repo: pass `-R <owner>/<repo>` to `gh pr` and fill owner and name literally in `gh api`. Another forge: its CLI's equivalent. `gh` absent or failing: say so in one line and stop; never invent PR state.

## Process

1. **Fetch the feedback.** Inline review threads, with resolved state, path and line (`originalLine` when the line is outdated):
   `gh api graphql --paginate -F "owner={owner}" -F "name={repo}" -F number=<N> -f query='query($owner:String!,$name:String!,$number:Int!,$endCursor:String){repository(owner:$owner,name:$name){pullRequest(number:$number){reviewThreads(first:100,after:$endCursor){pageInfo{hasNextPage endCursor} nodes{id isResolved isOutdated path line originalLine comments(first:100){nodes{fullDatabaseId author{login} body url}}}}}}}'`
   Review summaries and conversation comments: `gh pr view <N> --json reviews,comments`. **Skip** resolved threads, threads whose last comment starts with the comment prefix, and summaries or conversation comments already answered by a prefixed reply that quotes or links them. A summary that only restates its inline comments, or an approval with no text, gets no row. The bot's comments are those whose author login matches `Reviews › bot` (its login can differ between REST and GraphQL).
2. **Fetch the branch and record the tip.** You may push here, so key the push to what you fetched.
   - git: `git fetch origin <branch>`, then `git rev-parse origin/<branch>` (the fetched tip; it should equal `headRefOid`), and `git merge-base --is-ancestor origin/<branch> HEAD` (exit 0: your checkout contains it).
   - jj: `jj git fetch --remote origin --branch <bookmark>`, then `jj log -r "<bookmark>@origin" --no-graph -T commit_id`, and `jj log -r "<bookmark>@origin & ::<bookmark>" --no-graph -T commit_id` (non-empty: the bookmark contains it).
   **Stop and report** if the PR is merged or closed, the tip isn't `headRefOid`, or your checkout doesn't contain it (someone else pushed). Never push blind.
3. **Triage every comment** into the table: category, verdict, why, agent. Review comments can be real bugs, valid improvements, nits or false positives; a bot is often noisy.
   - **Category:** security, bug, performance, quality, nit, false positive.
   - **Verdict:** **Fix** (clear and mechanical), **Decline** (nit or false positive), **Propose** (judgment or scope), **Escalate** (a domain call, or security you can't settle).
   - **Security** is any comment that touches a `Reviews › security triggers` item. It is never declined: its verdict is Fix or Escalate only.
   - **Agent,** by the pack's model rule: only programmers may run on Sonnet; security-nature work and every other role are always Opus; nothing at low effort. A Lanes Model pin applies to programmers but never to security work; a Lanes Effort pin applies (never low).

     | Fix kind | Agent |
     |---|---|
     | fully specified, bounded, not security | `teams:builder`, sonnet, high |
     | ambiguous or cross-cutting | `teams:builder`, opus, high |
     | security | `teams:builder`, opus, high, then a security re-review by `teams:designer`, opus, xhigh |
     | Propose, Escalate | the Engineer |
4. **Hold the merge first.** Before any fix lands on the open PR, mark it draft (`gh pr ready <N> --undo`) or, where drafts aren't available, comment ``gh pr comment <N> --body "<prefix> hold merge: review fixes in progress"``. From here on, every report says whether the fix is pushed.
5. **Act by verdict.**
   - **Fix.** Under `/teams:orchestrate`, the fixes are a fix round (its lifecycle step 7): a fresh build lead (`teams:designer`) gets the findings, the branch and the workspace, and spawns fresh programmers per the agent column; the Orchestrator runs the security re-reviews. Solo: apply them directly, or spawn the recommended agent when the `teams:` agents are available.
   - **Decline.** Reply on the thread with a one-line reason starting with the comment prefix (it goes out through the Engineer's account), then resolve the thread:
     `gh api graphql -f query='mutation($t:ID!,$b:String!){addPullRequestReviewThreadReply(input:{pullRequestReviewThreadId:$t,body:$b}){comment{url}}}' -f t=<thread id> -f b="<prefix> <reason>"`
     `gh api graphql -f query='mutation($t:ID!){resolveReviewThread(input:{threadId:$t}){thread{isResolved}}}' -f t=<thread id>`
     A summary or conversation comment has no thread: reply with `gh pr comment <N> --body "<prefix> <reason>"`, quoting or linking it.
   - **Propose.** Ask the Engineer: do it, defer it, or file a follow-up. Don't apply it without a yes.
   - **Escalate.** Hand it to the Engineer with the reasoning. Never decide it.
6. **Re-gate and push, if anything was fixed.** Run the light gate (the gate when unset), green on exit code **and** output. Fixes go in meaningful commits (`Tracker › commit format` when set). Push keyed to the fetched tip:
   - git: `git push --force-with-lease=<branch>:<fetched tip> origin <branch>`
   - jj: `jj git push --bookmark <bookmark>` (it refuses if the remote moved since the fetch)
   A refused push: stop and report; don't re-fetch and retry over someone else's work. Then lift the hold: `gh pr ready <N>` if you made it a draft (leave a PR that was already a draft alone), or comment that the hold is lifted.
7. **One pass.** A push can trigger the bot again. Don't loop re-requesting review; a later round is a new run.

## Output

```
# Addressing review: PR #<N>, <title>
Sources: bot <n>, humans <m>. Gate after fixes: <green / failed / not run>. Pushed: <sha / none>.
```

| Comment (file:line) | Source | Category | Verdict | Why | Agent |
|---|---|---|---|---|---|
| `<path:line>`: <short> | <bot / @login> | <category> | <verdict> | <one clause> | <agent, model, effort / the Engineer / none> |

Then:
- **Applied:** each fix and its commit.
- **Declined:** each with its reason as posted.
- **For the Engineer:** every Propose and Escalate as a clear ask (do, defer, file a follow-up, decide).
- **Net:** one line, e.g. "2 fixed and re-gated green, pushed abc1234; 1 nit declined on the PR; 1 perf comment proposed as a follow-up." Say who merges: a PR into a `Branches › engineer reviews` branch is the Engineer's.

## Quality bar

- **Every comment gets a row** and a verdict. Never drop one silently.
- **Security is never declined:** Fix or Escalate only.
- **Never say "ready to merge"** while fixes are still being pushed: re-gate green and push first, and say whether it's pushed.
- **Weigh honestly both ways:** declining a valid comment to save work is a failure; so is applying a false-positive fix that adds churn. Say why, every time.
