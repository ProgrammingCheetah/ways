---
signed: 2026-11-20T01:12:30Z
hash: 92cf07c9d6066f66451133c82735664e59e4171fa4f7b6c9ebc73d80bf99a7a4
---
# Release 0.5.3 design: merges in groups

2026-11-19 · Written by Claude on the Engineer's word ("Merge in groups"), taken over the full merge queue. That queue is parked, with its two reviews, as `.teams/releases/0.6.0/merge-queue-draft.md`. Facts are at `feature/0.5.3` b1f77f94. QA was a fresh `teams:designer`. **Signed by the Engineer, 2026-11-19.**

## Summary

Today a tick lands at most one merge, and the next merge waits for CI on the commit that merge made (R29). That costs 10 to 13 minutes per task. On 2026-11-19, 8 finished tasks waited about 2 hours behind each other, partly on stale questions, which #577 fixes.

With groups, **one tick lands every task that is ready to merge, up to 8, then waits once for CI**:

```
tick:  #452 merges ─► #461 merges ─► #480 merges ─► ...   (up to 8, each with today's merge action)
                                                          │
next tick:  waits for the branch's CI at the group's last commit, as R29 does today
            green ─► the next group      red ─► today's question; one at a time after it is ruled
```

- **Each merge is today's.** It is the same `merge` action: the base probe, the forge's merge at `ZAF_HEAD`, `zaf board done`, then dropping the workspace. A task whose base now conflicts goes to `fix` and `sync-base`, as today, and the forge refuses a merge that conflicts in any case.
- **One CI wait per group, not per task.** A later merge in the group doesn't wait for checks at a commit the group itself just made. Any other new commit, such as a hand push or another tick's merge, is waited for as today.
- **Order.** Tasks fresh from a fix go first (R30), and the tasks held behind them follow in the same tick.
- **A red branch** raises today's question, which says the head may hold up to 8 merges. Once it is ruled, merges go one at a time until the head is no longer red.
- **Off is today.** `[merge] per_tick = 1` is the default. ways sets 8 once the installed zaf can read it.

There is no new branch, verb, action, forge call or repository setting. Today's merges already land combinations nobody tested, because a base that merges in cleanly gets no new CI run (R28), and that stays the same. What changes is that a red branch has up to 8 suspects instead of 1. The full queue, which keeps the branch always green, is 0.6.0's. On ways, CI cancels a run when a newer push arrives (`concurrency` in `zaf.yml`), so a group really does cost one CI run.

Questions Q1 to Q4 each carry Claude's answer. Signing as it stands takes all four.

## Questions for the Engineer

**Q1. How many merges may one tick land?**
**Claude's answer (2026-11-19; the Engineer adjusts):** `[merge] per_tick`, default 1, which is today's behaviour, for every project. ways sets 8. A bigger group has more suspects when the branch goes red. The key isn't called `batch`, which `[roles.team]` already uses, or `group`, which is a task label.
**Engineer:** Claude's answer taken (2026-11-19, "I do agree with all four and sign it").

**Q2. What happens when the integration branch goes red?** "Red" means a check failed at the branch's head, as zaf reads it today. A red head already raises a question on each epic, and that question holds every ticket, merges included, until it is ruled.
**Claude's answer:** that stays. In the tick that first sees red, and after the ruling, merges go one at a time until the head is no longer red, so a fix can land and nothing piles up faster than today. Holding every merge would stop the fix itself from landing, which is why R29 never held merges on red. A head with no check at all (a docs-only commit) reads as not red, as today.
**Engineer:** Claude's answer taken (2026-11-19, "I do agree with all four and sign it").

**Q3. What does the red-branch question say?**
**Claude's answer:** today's question, plus one sentence: "Up to <per_tick> merges may have landed in one tick, so the suspects are the branch's latest merges." Naming each suspect would take a new forge read of a commit's checks and a new Vcs read, in jj, git and the fake. This patch doesn't take those on, and 0.6.0's queue makes suspects rare. The question's marker stays its last sentence.
**Engineer:** Claude's answer taken (2026-11-19, "I do agree with all four and sign it").

**Q4. Does a group stop when the forge refuses a merge?** After the base moves, Forgejo works out each pull request's `mergeable` again, and how fast it does that isn't verified (addendum D19). `pr-merge` retries "not mergeable yet" three times, 15 s apart, then exits 4, which counts toward R18's three at one head.
**Claude's answer:** yes. The first refusal (exit 4) ends the group for that tick, and the rest go next tick. Pushing on would add more retries and more counts toward a question nobody needs. F34 records the retries and how long the tick took, so the next release can measure it.
**Engineer:** Claude's answer taken (2026-11-19, "I do agree with all four and sign it").

## The design

<details>
<summary><b>D30. One tick, up to <code>per_tick</code> landed merges</b></summary>

- **Counting.** The tick's step 3 (`docs/zaf-actions.md` §5, `app/spawn/tick.rs`:392-433) runs the lane's `zaf`-kind actions as today, but stops counting at `per_tick` merges that landed, not at one.
  - A merge that landed but exited 2 because `board done` failed (`slice-merge`:54) counts as landed: the branch moved.
  - The first refusal ends the group (Q4).
  - Every merge left over is named as skipped, as today.
- **The group's base.** Before each merge, the gate is asked again under `spawn.lock`, with the ticket gathered afresh, as today (`app/spawn/phase.rs`:388-423). That gather reads the branch again, and R29 is judged at the head it finds (`domain/schedule/place.rs`:130-136).
  - After each landed merge, the tick reads the branch's head once and keeps it in memory for that tick only: the group's own last commit.
  - The re-check judges R29 at the group's base (the head at the tick's start) only while the fresh head is that group's own last commit.
  - Any other head, such as a hand push or another tick's merge, is judged as today. Nothing is written down.
- **Order.** The lane's order, with R30's tasks fresh from a fix first. A task whose only wait is R30's hold behind a fresh task merging in this same tick joins the tick's list right after it. Its re-check runs after the fresh task has merged, so the hold is already over.
- **Conflicts.** The re-check doesn't read `mergeable`. The base probe catches a conflict instead (`merge-base --probe` fetches the base from the remote, `merge-base`:110, and `slice-merge`:45-49 runs it). So each later merge probes against the base the earlier ones moved.
- **A tick that dies mid-group** leaves only whole merges: a merge's process runs in a group of its own and outlives the tick (`docs/zaf-actions.md`:257).
</details>

<details>
<summary><b>D31. After the group</b>: the next wait, and a red branch</summary>

- The next tick's merges wait for the branch's checks at its new head, as R29 says, with the clock from #577.
- **A red head:** today's question on each epic, with Q3's sentence. In the tick that first sees red, merges already in step 3 go one at a time, because merges (step 3) run before questions (step 5). After the ruling, `per_tick` counts as 1 until the head is no longer red, read from the forge on each tick. Nothing is stored.
</details>

<details>
<summary><b>D32. The pieces</b></summary>

- **Manifest:** `[merge] per_tick`, a whole number of at least 1, default 1. The manifest refuses 0 when it loads.
- **Tick:**
  - step 3 counts landed merges up to `per_tick`, or 1 while red, and stops at the first refusal;
  - the group's own last commit is kept for the tick's re-checks (D30);
  - R30's held tasks follow the fresh one;
  - the skipped line says how many landed: `#4 skipped: 8 merged in this tick`.
- **Red-branch question:** Q3's sentence.
- **Docs:**
  - R29's "one merge at a time" becomes "up to `per_tick` merges a tick, judged at the group's base; one while red";
  - R30 says that its held tasks follow in the same tick;
  - R28 is unchanged.
- **ways:** `[merge] per_tick = 8` is set in `.zaf.toml` with 0.5.3's release, not by a task in the build. An older zaf refuses a manifest that has a `[merge]` section (`manifest/document.rs`:19, `deny_unknown_fields`).
</details>

## Features and their "done when"

| # | Feature | Done when |
|---|---|---|
| F29 | A group lands in one tick | With three tasks ready to merge and `per_tick = 8` on the fake forge, one tick lands all three. With `per_tick = 2`, it lands two and names the third as skipped. |
| F30 | One wait per group, and only for the group | Checks pending at the group's own first commit don't stop its second merge. Checks pending at a commit someone else pushed mid-group do stop it, as today. The next tick's merges wait for the checks at the group's last commit. |
| F31 | Fresh fixes first, the held ones after | A task fresh from a fix lands first, and a task held only behind it lands after it in the same tick. |
| F32 | Red goes one at a time | With a failed check at the branch's head and the question ruled, a tick with three ready tasks lands one, and the question carries Q3's sentence. |
| F33 | A refusal ends the group | When the forge refuses the second of three merges, the tick lands one and skips the third. |
| F34 | On ways, by hand | With `per_tick = 8` on 0.5.3, five tasks ready to merge land in one tick, and `zaf status` shows them done. The tick's log shows each merge's retries and the tick's length. |
| F35 | Off is today | With `per_tick = 1`, every existing merge test passes unchanged. |

## Epic to file once signed

| Epic | Decisions | Tasks, roughly |
|---|---|---|
| Merges in groups | D30 to D32 | 5 |

It is blocked by #577, the base clock it reads. It needs nothing from the git epic.

## One-way doors

- **`[merge] per_tick`** joins the manifest, and an older zaf refuses a manifest that sets it.
- **R29 changes:** a tick may land up to `per_tick` merges, judged at the group's base, and a red branch has up to that many suspects.
- **The red-branch question** gains Q3's sentence.

## QA challenges

A fresh `teams:designer` read the draft at `feature/0.5.3` b1f77f94. It made read-only calls to the forge: the statuses at five integration commits, and #577.

**What holds:**
- one landed merge per tick today (`tick.rs`:392-433);
- the re-check under the lock (`phase.rs`:388-423);
- `--probe` fetching (`merge-base`:110);
- a tick that dies mid-group (`docs`:257);
- R18 unchanged;
- the skipped line in only two places;
- nothing stored, the domain untouched, and models never writing the VCS;
- CI cancelling superseded runs on ways.

<details>
<summary>The nine challenges (C1 a blocker)</summary>

- **C1 (blocker).** The group's base contradicts the re-check before each merge. That re-check reads the branch again and judges R29 at the new head (`gather.rs`:513-525, `place.rs`:130-136). On ways, a check is pending within about 1 s of a merge (#494 merged at 16:52:06, and its check was pending at 16:52:07), so every second merge would come back NOT_RUN. Proposal: judge at the group's base only while the fresh head is the group's own last commit. Any other head is judged as today, since ticks can overlap and hand pushes happen.
- **C2.** R30's held tasks can't follow in the same tick, because the lane is sorted once (`tick.rs`:451-457). Proposal: a question, re-sort or wait.
- **C3.** Q2 described the red branch wrongly. A red head raises a question on each epic, and that question holds every ticket (`ask.rs`:735-760, :584-588). A docs-only head has no check, so it reads as not red. Proposal: define red as a failed check at the head, and reword.
- **C4.** Q3's reads don't exist:
  - `checks_at` reads a pull request's checks (`ports/forge.rs`:342-345);
  - walking the commits needs a new Forge read and a new Vcs read in jj, git and the fake;
  - on ways, runs cancelled by a newer push read as failures.
- **C5.** An older zaf refuses a manifest with an unknown section (`manifest/document.rs`:19). Proposal: add it to the doors, and set ways' 8 only once the installed zaf reads it.
- **C6.** The forge may refuse mid-group: `pr-merge` retries three times, 15 s apart (`pr-merge`:64-95), and exit 4 counts toward R18. Proposal: a question on whether a group stops at its first refusal, and a measurement in F34.
- **C7 (nit).** Wording:
  - the re-check doesn't catch a conflict, the probe does;
  - cite `merge-base`:110;
  - a landed merge that exits 2 must still count.
- **C8 (nit).** GitHub's merge message differs from Forgejo's, and the question's marker must stay its last sentence.
- **C9.** The size is likely 5 to 6, the git epic's order matters if a Vcs read is added, and the doors list is incomplete.

Two things it couldn't verify: where the head's `(pull_request)` statuses come from, and the "2 hours" figure. Claude checked the first on 2026-11-19: the forge's runs at those commits belong to release pull request #350 (feature/0.5.3 into main).
</details>

## QA resolutions

- **C1:** resolved in D30 ("the group's base") and F30, as proposed.
- **C2:** resolved without a question. A task held only behind a fresh task merging in this same tick joins the tick's list right after it (D30, F31). Neither of the review's options is needed: no second gather, and no CI wait per fix.
- **C3:** resolved. Q2 is reworded, with red defined as today; it was not yet answered.
- **C4:** resolved by changing Q3's answer (not yet answered) to one sentence with no new reads, so no Forge or Vcs read is added.
- **C5:** resolved in D32's ways line and in the doors.
- **C6:** routed as Q4.
- **C7:** resolved in D30.
- **C8:** resolved: no message is parsed now (C4), and Q3 keeps the marker last.
- **C9:** resolved: 5 tasks, no tie to the git epic, and the doors completed.

## Signed

2026-11-19, the Engineer: "I do agree with all four and sign it though." Q1 to Q4 are taken as Claude answered them.
