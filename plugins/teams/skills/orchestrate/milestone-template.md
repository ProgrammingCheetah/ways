# Milestone <n>, release <version>: <what it delivers, in one line>

<date> · Commit `<id>` on `<integration branch>` · Architect: <name> · QA: <name> · **Findings for the Orchestrator; questions reach the Engineer through it.**

<!-- How the Orchestrator runs a milestone (lifecycle step "Milestone QA"). A timeline phase is one row of the design's Timeline.
  1. Add the row to <docs>/releases/<version>/milestones.md:
     | # | Timeline phase | Slices merged | Commit | Architect report | QA report | Fix rounds | Demo |
     A squashed release PR leaves these commits on no branch; GitHub keeps them under the PR's refs/pull/<n>/head.
  2. Point the release's review workspace, `<workspaces>/review-<version>`, at the commit, on no branch (teams:machinery;
     created at milestone 1, reused after). Not clean is gray: stop, a planted bug may remain. QA plants bugs there,
     so no other agent uses it while QA runs; one needed meanwhile gets its own workspace at the commit.
  3. The architect first, fresh, changing nothing: the design's architecture and the Groundwork's seams. Drift a later
     timeline phase would build on is a must-fix; the rest is a request for the next release.
     Then QA, fresh, briefed with the architect's report: each slice against its saved plan; defects, across slices too;
     tests: every rule has a test that fails when it breaks, proven by planting bugs, each reverted before reporting.
     Brief both (brief-template.md) with the signed release documents, the Groundwork, the rulings, every saved slice
     plan of the timeline phase, milestones.md, both reports from the last milestone, and QA's last log. Each reads the
     new range (`<last commit>..<commit>`) whole and uses the cumulative range (git `git diff <main>...<commit>`; jj: the
     equivalent, checked by running it) by --stat and for the hunks a finding needs; the last milestone reads the whole
     release once. QA's question for the architect goes in its section below; route it by resuming the architect.
  4. A finding that touches a security trigger goes to a fresh security reviewer now, whether or not its slice was
     flagged, and its fix round gets a security review.
  5. Fix rounds: one fresh build lead per team with its findings, on `Branches › fix` cut from the integration tip; its
     critique, the gate and, when triggered, a security review; merge it like a slice. After the fix PRs merge, point
     the review workspace at the new integration tip, then resume the architect (for its findings) and, after it
     reports, QA to re-run each proof.
  6. Meanwhile the next timeline phase's slice plans may run, and a team that finishes early plans ahead, read-only, and
     waits. Its builds start once this milestone's must-fixes merge; a plan written meanwhile is checked against the fix
     diff first, and a changed seam sends it back to its lead.
  7. Then the demo, QA's log (/teams:retrospective qa <version> m<n>, from retro-template.md) and the status note in
     queue.md.
  The last milestone is the release end: compliance, security and the architects' release review (with its reviewers,
  standing in for the architect's pass) run first, in parallel; then QA, briefed with all of them, also checks that every
  must-fix is fixed or accepted in writing. Reports: reviews/m<n>-architect.md and reviews/m<n>-qa.md. After the release
  PR merges, remove the review workspace by name, on the Engineer's word.
  With `trunk` there is no integration branch: the range is the release's merged slice PRs (each merge's diff), not a
  commit range, and fixes are PRs into main. Off-cycle work: QA alone, on its branch's diff in its workspace, before
  its PR opens; that workspace must then be clean, as in step 2. -->

## Summary

<One screen, plain words: what this milestone delivers, what was checked and how, the must-fix count, and the questions for the Engineer (count, all pre-filled).>

## Findings

| # | Kind (must-fix, should-fix, next release) | Where (file:line) | What and why | Proof (the planted bug the tests missed, or the failing case) | Team | Status |
|---|---|---|---|---|---|---|

## Questions for <the architect | QA>

## Questions for the Engineer (each pre-answered)

**M1. <question in plain words>**

**Claude's answer (<date>; the Engineer adjusts):** <answer and why>.

**Engineer:**

## Fix rounds

| Round | Team | Branch | Findings | Proofs re-run | Result |
|---|---|---|---|---|---|
