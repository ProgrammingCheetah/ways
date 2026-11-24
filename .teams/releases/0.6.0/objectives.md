---
signed: 2026-11-23T09:55:53Z
hash: a782f9471d5f24aa1d6537ea41573a734a3615d90ca54b1a5d2e476f8b247549
---
# Release 0.6.0 objectives: the plan as a graph of tasks

2026-11-23 · Interview: Claude with the Engineer · QA: a fresh `teams:designer` · **Draft until the Engineer signs it.**

<!-- Written by /teams:objectives from the Engineer's answers: their words, lightly edited, never invented.
     Objectives are outcomes (what changes for someone), not features; features go under In scope, each serving an objective.
     The design document builds on this file once it's signed: its goals trace to O1…, its non-goals carry Out over,
     and its Decisions and One-way doors take up what this file parks. -->

## Summary

0.6.0 makes zaf plan an epic as a graph of tasks and optimise it deterministically. Planners write what each task writes, provides and needs, in a shared vocabulary. zaf refuses an incomplete plan, turns dependencies into interfaces, sizes and orders the nodes, finds the critical path, shows the graph in the chair's epic view, and builds the optimised graph, interfaces first.
There are eight objectives: six musts (O1, O3, O4, O5, O6, O8) and two shoulds (O2 build times, O7 a model's check; O7 is cut first).
The constraint that bites hardest: the optimisation is deterministic, "CHECKED by a model afterwards, but not MADE by it", and anything 0.6.0 breaks it must replace in 0.6.0.
Questions for the Engineer: 13 (Q1 to Q13), all from QA, all agreed as pre-filled on 2026-11-23 ("Agreed") and applied; none open. O1 is checked on a dapple or zabo epic built on a 0.6.0 pre-release, O2 on 0.6.1's epics (Q4), and the challenger moves after optimising as O7's check (Q10). The risk named: derived edges are only as good as the shared vocabulary.

## Why this release

For both zaf's own builds and the Engineer.

The problem: "zaf is getting to epics big enough that we need to optimize them."

The opportunity: "an algorithm that allows for us to reduce and increase the number of tasks deterministically while keeping it consistent."

Why now: the epics are already that big. 0.5.5 was 59 tasks over 8 epics (`plan-graph/real/zaf-0.5.5.json`).

## Objectives

| # | Objective (an outcome) | Achieved when (a check the Engineer could run by hand) | Priority |
|---|---|---|---|
| O1 | "The end product is what is expected, even after optimization" | One dapple or zabo epic is built, optimised, on a 0.6.0 pre-release binary before the release closes (Q4). zaf checks first, pass or fail: the built epic's behaviours equal the planned ones, and each "Done when" of the signed epic passes. Then the Engineer checks the finished product against the signed epic, and it is what the epic asked for (Q11) | must |
| O2 | Build times are reduced | Measured post-implementation and reported on 0.6.1's epics (Q4): the average and the median of an epic's implementation time, start to finish, per planned task (the planner's count, before optimising), leaving out time spent waiting on the Engineer, are both lower for the epics built after 0.6.0 than pre-algorithm, "The 0.5.x already on the board" (Q5) | should |
| O3 | The optimised graph has the right shape | zaf prints pass or fail for an optimised epic's graph, and the Engineer can see it (O6) (Q11): each wait on a name with a signature goes through an interface; every other wait is on a name with no signature, or is a turn on something both write; no body node passes the size limit; there is no cycle; no two tasks that write the same thing are left unordered (Q1); and running the optimiser on its own output changes nothing, ids included (Q3) | must |
| O4 | An incomplete plan is refused | The Engineer gives zaf three plans: one with a need nobody provides, one with a cycle, one with two providers of the same thing. zaf refuses each before any build starts, names the reason, and changes nothing in the plan | must |
| O5 | "Planners should block on tasks, not on epics" | In the graph of a built epic, no dependency names an epic: every order edge names a task or an interface. R15's wait for an epic's signing and challenge stays (Q6) | must |
| O6 | The Engineer sees the graph in zaf | In the chair, the Engineer opens an epic and sees its graph before and after optimisation, with its chain and width, without leaving zaf | must |
| O7 | A model checks the optimised graph | The challenge runs after optimising and is this check (Q10): before the build, the challenger's report on the optimised graph is on record, zaf's hash of the graph is the same before and after it (a difference fails the check), and the checker cannot file tasks | should |
| O8 | "A critical path is discovered through the nodes" | zaf names the critical path's nodes in order and its length; in the chair it is marked on the graph (O6) | must |

A node is a task (the Engineer, 2026-11-22: "I want our graph to be a node graph of tasks"); this file uses both words for the one thing.

O1 and O3 are musts and O2 a should, the Engineer's picks (2026-11-23); their checks are the Engineer's words, sharpened by Claude, and O3's "no cycle" and "no two tasks ... left unordered" are Claude's, accepted ("Yes, those are added too"). O4 to O7 were Claude's suggestions, picked by the Engineer the same day; O8 is the Engineer's. The checks and priorities of O4 to O8 are Claude's suggestions, accepted ("Yes"): O6 is a must because O3's and O8's checks are read off the graph. Cut order, first to go: O7, then O2. Eight objectives stay one release: most of the work is O3 and O6.

## In scope

| # | What | Serves |
|---|---|---|
| S1 | The optimiser in Rust passes the 18 fixtures in `plan-graph/fixtures/` that the reference wrote, as the signed design amends them, and the reference's invariants on random plans ("meaning kept" among them) (Q2) | O1, O3, O4, O8 |
| S2 | Planners write what each task writes, provides and needs, in place of "Blocked by" lines: the planning brief and the task template change, "or implementation cannot continue otherwise". Writes are area names from S9's vocabulary, not files; ruling 31 stands (Q8) | O4, O5 |
| S3 | zaf's scheduler builds the optimised graph itself, interface tasks first by their edges (an interface comes before what needs it); rulings R6 and R20 unchanged (Q12) | O1, O2, O5 |
| S4 | The optimisation can run several times, each run's output the next run's input: "Given a node graph A, f(A) would bring A_1 (optimized once). f(A_1) = f(f(A)) = A_2." 0.6.0 runs a static number of passes: one run reaches the end, so the number is 1 until 0.6.1's quotient (Q3) | O3 |
| S5 | The graph, before and after optimisation, is part of the chair's epic view, "like the dependency tree" | O6 |
| S6 | A model checks the optimised graph, a higher-end one: "I think Fable or Opus are fine? Since it's planning, it has to be the higher end ones". The challenger (Opus) moves after optimising and does it; zaf hashes the graph before and after (Q10) | O7 |
| S7 | zaf computes the critical path, its nodes and its length | O8 |
| S8 | zaf computes O2's measure: the average and median of an epic's implementation time per planned task, leaving out time waiting on the Engineer, from the 0.5.x epics on the board and from the epics built after 0.6.0; start times from zaf's own phase records (Q5) | O2 |
| S9 | A shared vocabulary: for the names tasks write, provide and need, so one task's need matches another's provide ("Let's define a common vocabulary then"), and "for the edges and relationships nodes have between them". It is the names tasks use, apart from zaf's own Vocabulary note; it spans the release (Q7) | O3, O4, O5 |
| S10 | A need another epic provides waits on that epic's providing task, by name; the edge is kept but never optimised (Q7) | O4, O5 |
| S11 | A plan filed with `Blocked by` still reads: each line becomes a need of that task's name. R31's skeleton task becomes a node that provides the names its fillers need (Q9) | O4, O5 |

## Out of scope

| What | Why it's out | Later (a release, or never) |
|---|---|---|
| The CI and the merge (the wait after each run) | "0.6.0's responsibilities are not the CI and merge." | 0.6.1 ("That will go in 0.6.1") |
| An optimisation quotient: a measure of how optimised a graph is, to decide when to stop running passes | "For now, 0.6.0 gets static number of passes" | 0.6.1 ("0.6.1 gets an optimization quotient") |
| Optimising across epics: the whole release as one graph | "Across epics is out" | A future patch version ("That is for a future patch version"), not named |
| The graph on the hub's web page | "WE are waiting for the CLI to settle" | When the CLI settles |

## Constraints

"No other restrictions other than the same 0.5.5" (2026-11-23), so 0.5.5's constraints carry over:

- **No deadline.**
- **Breaking is allowed if it's replaced in this release** (0.5.5: "as long as it is replaced with new behavior, not 'Next version fix'"). Any behaviour 0.6.0 removes or breaks gets its replacement in 0.6.0, for ways, dapple, zabo and the action scripts alike.
- **Abstractions first** (0.5.5: "Heavily favor abstractions and interfaces so that we don't have to worry about breaking stuff").
- **Waits stay inside the call** (0.5.5, Q12): for the instances zaf runs, a verb that waits keeps waiting inside its MCP call.
- **Technology:** petgraph and rustworkx-core are added (the Engineer, 2026-11-22: "I think petgraph and rustworkx-core should be added"; confirmed 2026-11-23).
- **Deterministic, never a model:** "The optimization has to be deterministic, not done by a model. It can be CHECKED by a model afterwards, but not MADE by it."

## Assumptions and risks

- **Derived edges are only as good as the vocabulary (S9).** Edges come from matching one task's needs to another's provides, so a name spelled two ways reads as a need nobody provides, and O4 refuses the plan.
- **No interface task has ever run (Q13).** Their minutes are guesses, and the cost of a wrong signature is not modelled (`plan-graph/README.md`).
- **Conflicts are not modelled (Q13).** The board does not hold what each task writes, so real conflicts would lengthen the optimised plans.
- **Which edges can be lifted is assumed (Q13).** The simulation lifted every edge whose provider is not docs, prompts, roles, shell actions or install.
- **rustworkx-core pulls in `rand` and `rayon` (Q13).** The design must keep off its random and parallel paths (the determinism constraint).
- **O2's gain comes from the chain alone (Q13).** The wait after each run (about 38 of 50 minutes a layer) is 0.6.1's.

## One-way doors seen

Flagged by Claude at the interview's close; named here, decided in the design.

- **The task's shape on the board (S2, S9).** Writes, provides and needs, in the shared vocabulary, become part of every task planners file after 0.6.0. Changing the shape or renaming a word later breaks every stored plan that uses it.
- **zaf rewriting what planners filed (S3, S4).** The optimiser splits, merges and adds tasks (interfaces). Once tickets are filed on the board and built, a split or merge can't be taken back without the board's history showing it.
- **The checking model's rights (O7, the determinism constraint).** The model may check the graph but never make it, so whatever runs the check must have no way to change the graph. Today an instance may still run `board file` (QA C10).
- **Where the graph lives against the signature (QA C12).** An edit to an epic's body voids its signature, so the graph can't live there.
- **Merging filed tickets (QA C12).** "A ticket is never expanded" (zaf's domain document), and merging two filed tickets expands one, which points at optimising before filing.
- **A task shape every board can hold (QA C12).** 0.7.0 is "any board": the writes, provides and needs must fit Jira and zabo too.

## Questions for the design

Parked during the interview. Text in quotation marks is the Engineer's.

- **What weighs a node on the critical path (O8).** Its size (the reference's `measure()` weighs S 1, M 2 and returns only the length) or its predicted minutes (the simulator's ~12 in a lane plus ~38 waiting). O8 needs the path's nodes as well as its length, so S7 needs fixtures of its own (QA C15).
- **Who writes `existing` and `no_signature` (QA C13).** A need marked `existing` passes O4 with no provider, and `no_signature` decides which waits stay, so if the planner writes them O4 can be bypassed. One way: zaf derives `existing` from the code.
- **Where the vocabulary lives and who adds to it (S9).** With the node tree, or a list per epic or per project; added by the planner, the Engineer, or both.
- **What the model's check does with a finding (O7).** It changes nothing in the graph; whether a finding becomes a question for the Engineer, a refusal, or a note.
- **Where the optimised graph lives (S3, S4).** On the board as tickets, in a file, or derived when asked; and how the board shows a task the optimiser split or merged.
- **S8's start and finish.** What starts and ends an epic's implementation. Close times are on the board; start times come from zaf's local phase records (Q5), whose pruning is unverified.
- **Open on the 2026-11-22 draft page** (`.zaf/epic-design-glossary.html`): the size limit (S 1, M 2, limit 2?), one interface task each or one for all, whether the algorithm redraws epics, a closing joining task per epic, and what a conflict that cannot merge means (the draft orders it with a `turn:<name>` edge).
- **Epics planned before 0.6.0.** Settled by Q9 (S11): a `Blocked by` line still reads, as a need. Left for the design: how a line naming a whole epic reads.
- **What a second run changes (S4).** "The function does not know whether a node graph has been optimized or not, it just tries to optimize the current one. By definition, it SHOULD change it unless it is at it's most optimized point or something." The draft says its passes reach their end in one run. QA found (C3, re-checked by Claude) that a rerun gives the same measures, but 4 of the 14 fixtures that aren't refused come back with renamed nodes (`A+B` becomes `A+B.a+A+B.b`): settled by Q3: one run reaches the end. Left for the design: how a rerun keeps the ids.

## Questions for the Engineer

**Q1. O3's check (QA C1). The 18 fixtures S1 requires keep some waits that are not interfaces: a name with no signature (fixture 05) and a conflict too big to merge (fixtures 03, 16). Does O3's check change to match?**

**Claude's answer (2026-11-23; the Engineer adjusts):** Yes. O3 reads: "each wait on a name with a signature goes through an interface; every other wait is on a name with no signature, or is a turn on something both write; no body node passes the size limit; no cycle; no two tasks that write the same thing are left unordered". It is the reference's own invariant, and "sized to whatever they need" becomes a limit you can check.

**Engineer:** Agreed (2026-11-23), applied.

**Q2. S1 and the open design questions (QA C2). The fixtures already answer some open questions (limit 2, the `turn:` order, one interface node per epic, critical path weighed by size). Does S1 read "passes the fixtures as the signed design amends them", plus the reference's invariants on random plans?**

**Claude's answer (2026-11-23; the Engineer adjusts):** Yes. It keeps the fixtures as the tests while the design is still free to change an answer, and the invariants ("meaning kept" among them) are what O1 rests on.

**Engineer:** Agreed (2026-11-23), applied.

**Q3. A second run (QA C3). Rerunning the optimiser on its own output keeps the measures but renames merged nodes, so a static number of passes grows ids every run and churns the board. Which?**

- (a) One run reaches the end: "running the optimiser on its own output changes nothing, ids included" joins O3's check and a fixture; the static number of passes is 1 until 0.6.1's quotient.
- (b) f is one improvement step run N times, and the draft's passes are redesigned around that.

**Claude's answer (2026-11-23; the Engineer adjusts):** (a). It fits the determinism constraint and your "it SHOULD change it unless it is at it's most optimized point": after one run it is at that point, so f(f(A)) = f(A).

**Engineer:** Agreed (2026-11-23), applied.

**Q4. When O1 and O2 are checked (QA C4). 0.6.0 is built on the zaf installed before it, so its own epics never meet the optimiser. Which?**

- (a) O1 runs on one dapple or zabo epic built on a 0.6.0 pre-release binary before the release closes; O2 is reported on 0.6.1's epics.
- (b) Both are checked after the release, on 0.6.1's first epics.

**Claude's answer (2026-11-23; the Engineer adjusts):** (a). O1 is a must, so it has to be checkable before the release merges; O2 is a should and needs builds that only exist after 0.6.0.

**Engineer:** Agreed (2026-11-23), applied.

**Q5. O2's measure (QA C5). The 25 finished 0.5.x epics spread over 13 task counts, the optimiser changes the count, and the board holds no start times (only zaf's local phase records do). Which?**

- (a) Minutes per planned task (the planner's count, before optimising): average and median over every 0.5.x epic against every epic built after 0.6.0, leaving out time spent waiting on you; S8 takes start times from zaf's own phase records.
- (b) Each epic's real time against the simulator's prediction for its as-planned graph.
- (c) As worded.

**Claude's answer (2026-11-23; the Engineer adjusts):** (a). It keeps your "average and median" and "0.5.x on the board", and per planned task stands in for "for a given number of tasks" without groups of one epic.

**Engineer:** Agreed (2026-11-23), applied.

**Q6. O5's check (QA C6). As worded it would forbid R15's wait for an epic's signing and challenge, which the domain document keeps. Does it read "no dependency names an epic: every order edge names a task or an interface; R15's wait for signing and the challenge stays"?**

**Claude's answer (2026-11-23; the Engineer adjusts):** Yes. It is what "block on tasks, not on epics" meant: dependencies, not the signing gate.

**Engineer:** Agreed (2026-11-23), applied.

**Q7. A need another epic provides (QA C7). Across epics is out, yet 0.5.5 had 20 such names over 4 of its 8 epics; within one epic each reads as "a need nobody provides" and O4 would refuse it. Which?**

- (a) It waits on the other epic's providing task, by name: the edge is kept but never optimised, and the vocabulary spans the release.
- (b) It counts as `existing`.
- (c) It is refused, and planners keep needs inside their epic.

**Claude's answer (2026-11-23; the Engineer adjusts):** (a). It keeps O5 (a task, not an epic) and leaves optimising across epics out, as you ruled. (b) is wrong because that code isn't built yet.

**Engineer:** Agreed (2026-11-23), applied.

**Q8. What "writes" names (QA C8). Ruling 31 has tickets state scope in words, never owned files. Which?**

- (a) Writes are area names from S9's vocabulary, not files; ruling 31 stands, and a conflict a planner missed still shows at merge.
- (b) Writes are file paths, and ruling 31 is amended.

**Claude's answer (2026-11-23; the Engineer adjusts):** (a). It keeps the ruling, and S9's vocabulary already gives the names.

**Engineer:** Agreed (2026-11-23), applied.

**Q9. Plans filed with "Blocked by" (QA C9). The scheduler, R31's skeleton task and the planning rules all run on `Blocked by:`, the 4 open 0.5.5 epics use it, and 0.7.0's signed constraint keeps what zaf stored before it readable. Add a scope line: "a plan filed with `Blocked by` still reads: each line becomes a need of that task's name", with R31's skeleton becoming a node that provides the names its fillers need?**

**Claude's answer (2026-11-23; the Engineer adjusts):** Yes. It meets the breaking rule (the replacement ships in 0.6.0) and the open epics build on unchanged.

**Engineer:** Agreed (2026-11-23), applied.

**Q10. O7 and the challenger (QA C10). The challenger already runs on Opus once per epic, after planning, and marks it challenged; if the optimiser runs after it, it approves tasks never built. Which?**

- (a) The challenge moves after optimising and becomes O7's check (one model run, R15 unchanged); zaf records the graph's hash before and after, a difference fails the check, and the checker cannot file tasks.
- (b) A second model check after the challenger.

**Claude's answer (2026-11-23; the Engineer adjusts):** (a). One model run instead of two, and the hash makes "CHECKED ... not MADE" something zaf checks.

**Engineer:** Agreed (2026-11-23), applied.

**Q11. O1's check and "automate, do not read" (QA C11). Does O1 add two zaf checks before your look: the built epic's behaviours equal the planned ones, and each "Done when" of the signed epic passes? And does zaf print O3's shape check as pass or fail?**

**Claude's answer (2026-11-23; the Engineer adjusts):** Yes. It follows your rule that zaf reports pass or fail and you read only failures, and it tells the optimiser's fault from the planner's.

**Engineer:** Agreed (2026-11-23), applied.

**Q12. "Interface tasks first" in S3 (QA C14). Does it mean by their edges (an interface comes before what needs it), with rulings R6 and R20 unchanged, rather than a scheduling priority?**

**Claude's answer (2026-11-23; the Engineer adjusts):** By their edges. That is what the graph already says, and a priority would change a ruling for no gain.

**Engineer:** Agreed (2026-11-23), applied.

**Q13. Five risks QA found (QA C16). You said there were no more, but the README records these: no interface task has ever run (minutes guessed, a wrong signature's cost not modelled); conflicts are not modelled; which edges can be lifted is assumed; rustworkx-core pulls in `rand` and `rayon`; O2's gain comes from the chain alone, since the wait after each run is 0.6.1's. Add them?**

**Claude's answer (2026-11-23; the Engineer adjusts):** Yes. Each has a source, and the first and last bear on O1 and O2.

**Engineer:** Agreed (2026-11-23), applied.

## QA challenges

*From a fresh `teams:designer` (Opus), 2026-11-23. Code read at `main@origin` 8932edf1 (zaf 0.5.4) and `feature/0.5.5@origin` e99208ad; the board read with `zaf epic list`, `epic show` and `report --epics`; the reference run on a scratch copy. Claude re-checked C3 (a scratch rerun of every fixture: fixtures 02, 12, 15 and 17 come back renamed, measures equal) and C4 (`0.5.5/design.md:400`: "The build itself runs on the installed 0.5.4").*

### To the Engineer

C1. **O3's check fails on fixtures that S1 requires the Rust code to pass.** "Each dependency is an interface" is false for fixture 05 (a name with no signature keeps an A→B wait) and fixtures 03 and 16 (a conflict too big to merge gets a `turn:` order). "Sized to whatever they need" is not checkable by hand. Proposal: O3 reads "each wait on a name with a signature goes through an interface; every other wait is on a name with no signature, or is a turn on something both write; no body node passes the size limit", the reference's own invariant. Source: objectives.md:33, :46; `fixtures/05` expects A→B; `fixtures/03` `turn:app/update`; `plan-graph/README.md:41`.

C2. **S1 settles questions the file leaves open.** The 18 fixtures already encode answers to the open questions: limit 2 (`optimize.py:44`), the `turn:` order (fixtures 03, 16), one interface node per epic (fixture 13), and O8's weighting by size (`optimize.py:266`). How did "passes the 18 fixtures" become scope while those are open? Proposal: S1 reads "passes the fixtures as the signed design amends them", plus the invariants `test_reference.py` checks on 400 random plans ("meaning kept" there is what O1 rests on). Source: objectives.md; README.md:9, :33-42.

C3. **A second run is not a no-op, so "f(A_1) = A_1" is false.** On all 14 plans that are not refused, the measures come back equal, but 4 (02, 12, 15, 17) come back with renamed nodes (`A+B` becomes `A+B.a+A+B.b`): split undoes settle's merges and settle redoes them under new ids. With a static number of passes the ids grow every run, and the board churns if those ids reach tickets. S4 serves O3, yet no check of O3 involves a rerun. Options: (a) **(Recommended)** one run reaches the end: "running the optimiser on its own output changes nothing, ids included" joins O3's check and a fixture; the static number is 1 until 0.6.1's quotient. (b) f is one improvement step run N times, and the passes are redesigned around that. Source: scratch script over `fixtures/`; `optimize.py:95-107`, `:225-236`; glossary:211-212.

C4. **O1 and O2 cannot be checked inside 0.6.0.** A release is built on the zaf installed before it, so 0.6.0's own epics are planned with "Blocked by" and built without the optimiser. O1 needs "a real build of an optimised epic"; O2 needs post-implementation data; S8 counts 0.6.0's epics, but they are baseline, not results. Options: (a) **(Recommended)** O1 runs on one dapple or zabo epic built on a 0.6.0 pre-release binary before the release closes; O2 is reported on 0.6.1's epics. (b) Both are checked after the release, on 0.6.1's first epics. Source: 0.5.5/design.md:400; the release role "checks each must".

C5. **O2 can't be measured as worded, and the board holds no start times.** The 25 finished 0.5.x epics fall into 13 task counts; six counts hold one epic each, the rest 2 to 4. The optimiser changes the task count, so "for a given number of tasks" has no fixed group. Close times are on the board (`domain/ticket.rs:201`, feature/0.5.5); no start time is; phase starts exist only in the local, untracked `.zaf/phases/*/pid` records (present for all 29 0.5.x epics); `report --epics` has no time column. Options: (a) **(Recommended)** minutes per planned task (the planner's count, before optimising), average and median over every 0.5.x epic against every epic built after 0.6.0, leaving out time spent waiting on the Engineer. (b) Each epic's real time against the simulator's prediction for its as-planned graph (`replay.py`). (c) As worded. Source: `zaf epic list`; `zaf --json epic show 1002`; `~/.zaf/ledger.db`; `.zaf/phases/implement-instance-1386-on-1002-task-1010/pid`.

C6. **O5's check contradicts R15 and the domain document.** "No task waits on a whole epic … every wait names a task or an interface" leaves out R15's wait on an unsigned or unchallenged epic, the domain document's place 2 "Waiting on its epic", and WaitingOn's question and check waits. Proposal: "no dependency names an epic: every order edge names a task or an interface; R15's wait for signing and the challenge stays." Today an epic number on a `Blocked by:` line is accepted and reads as done once its tasks are (`app/spawn/blockers.rs:77`). Source: scheduler-rulings.md:37; vocabulary.md:22; scheduler.md:38; zuri.domain.md:120.

C7. **No rule covers a need another epic provides.** Across epics is out, yet 0.5.5 had 20 needed names provided in another epic, over 4 of its 8 epics (#1004 9, #1006 7, #1008 3, #1007 1). Within one epic each is "a need nobody provides", so O4 would refuse half of 0.5.5. The README's "takes a third off" was simulated with the whole release as one graph. Options: (a) **(Recommended)** such a need waits on the other epic's providing task, by name; the edge is kept but never optimised, and the names list spans the release. (b) It counts as `existing` (wrong: not built yet). (c) It is refused, and planners keep needs inside their epic. Source: per-epic run of `real/zaf-0.5.5.json`; `replay.py:36-51`; glossary:89.

C8. **S2's "writes" pulls against ruling 31 (scope, not files).** Ruling 31: a ticket states scope in words, names a file only as a place to start, and "the programmer goes where the goal needs". The draft's writes look like paths (`app/update`, fixture 03), and O3's conflict rule works only if they are complete. Options: (a) **(Recommended)** writes are area names from S9's list, not files; ruling 31 stands, and a conflict the planner missed still surfaces at merge (R28's fix). (b) Writes are paths, and ruling 31 is amended. Source: docs/zaf-actions.md:414 (main@origin); `fixtures/03`.

C9. **Replacing "Blocked by" breaks R31, the planning rules, and a signed 0.7.0 constraint.** R31's skeleton task, "Order tasks that touch the same area with `Blocked by`", "the task that defines [an interface] blocks the task that consumes it", and the scheduler itself all run on `Blocked by:` lines. 0.7.0's signed constraint (written when it was 0.6.0) says "what zaf stored before 0.6.0 still reads"; the 4 open 0.5.5 epics have 25 of 31 tasks on `Blocked by`. Proposal: a scope line, "a plan filed with `Blocked by` still reads: each line becomes a need of that task's name, as the replay read 0.5.5"; R31's skeleton becomes a node providing the names its fillers need. Source: scheduler-rulings.md:53; planning.md:108-109, :113 (feature/0.5.5); blockers.rs:17-31; 0.7.0/objectives.md:70.

C10. **O7 repeats the challenger, which runs before the graph exists.** The challenger is already Opus at xhigh, runs once per epic after planning, checks for a "missing interface", and its report marks the epic challenged. If the optimiser runs after it, it approves tasks that are never built. Options: (a) **(Recommended)** the challenge moves after optimising and becomes O7's check: one model run, R15 unchanged. (b) A second model check after the challenger. On "changed nothing": zaf records the graph's hash before and after, and a difference fails the check; today an instance may still run `board file` (not refused in 0.5.5's policy), so a checker could add a task. Source: plugins/teams/agents/challenger.md:1-7; challenger-brief.md:24-25; 0.5.5/design.md:182-190.

C11. **O1's check rests on the Engineer reading the result.** "Automate, do not read" (0.5.5 D24: a script, not reading) points the other way, and as worded O1 can't tell the optimiser's fault from the planner's. Proposal: O1 adds two zaf checks, then the Engineer's look last: the built epic's behaviours equal the planned ones (the reference's `meaning()`), and each "Done when" of the signed epic passes. Likewise zaf prints O3's shape check as pass or fail; the chair's tree draws each ticket only under its first blocker, so some edges never show. Source: memory zaf-automate-not-read; 0.5.5/design.md:229; `optimize.py:278-282`; `cli/chair/panes/detail/tree.rs:1-4`.

### Editorial

C12. **Doors to add.** (a) An edit voids the epic's signature, so the graph must not live in the epic's body. (b) "A ticket is never expanded", so merging tickets already filed expands one: optimise before filing. (c) The task's shape must be one Jira and zabo can hold (0.7.0 is "any board"). (d) The checker's rights (C10). Source: vocabulary.md:16; zuri.domain.md:141; 0.7.0/objectives.md:33-34.

C13. **Design question: who writes `existing` and `no_signature`.** A need listed as `existing` passes O4 with no provider, and `no_signature` decides which waits stay; if the planner writes them, O4 can be bypassed. One way: zaf derives `existing` from code, as `zaf check` finds symbols. Source: `optimize.py:84-88`, `:121`; fixture 05.

C14. **S3's "interface tasks first".** Say it means by their edges, with R6 and R20 unchanged; a priority would change a ruling. Source: scheduler-rulings.md:21, :42.

C15. **Traceability.** S1 also serves O1 (meaning) and O8 (measure); S3 also serves O2 and O5; S4 serves no check (C3); S7 needs fixtures of its own, since the reference returns only the critical path's length (`optimize.py:274`).

C16. **Risks to add.** No interface task has ever run (minutes guessed, a wrong signature's cost not modelled); conflicts are not modelled; which edges can be lifted is assumed (README:61-66); rustworkx-core pulls in `rand` and `rayon`, so the design must avoid its random and parallel paths; O2's gain comes from the chain alone, since the wait after each run (38 of about 50 minutes a layer) belongs to 0.6.1 (README:46, :58).

C17. **Words.** O3 says "node" and O5 "task" for one thing (R1: one word per thing): say once "a node is a task". Say S9's vocabulary is the names tasks use, apart from zaf's Vocabulary note. Source: scheduler-rulings.md R1; vocabulary.md.

C18. **Credits and the Summary.** The credits line names only O1 to O3's priorities, not their checks; the Summary's "none yet" changes once these are routed; the second-run design question needs C3's finding.

### Assumptions checked

- **0.5.5 was 59 tasks over 8 epics:** checked (`real/zaf-0.5.5.json`, `zaf epic list`).
- **The 0.5.x epics hold what S8 and O2 need:** partly. Task counts are there (29 epics, #92 to #1009). Finish: close times on the board, read by zaf, printed by no verb. Start: not on the board; only in local `.zaf/phases` records, whose pruning is unverified.
- **The planning brief and task template carry "Blocked by":** checked: `zaf/assets/spawn/planning-brief.md:9-14` (main), `:14-19` (feature/0.5.5); `plugins/teams/agents/planning.md:108-109, :113, :120, :127`; the parser `zaf/src/domain/task.rs:62-64, :624`; the scheduler `app/spawn/blockers.rs:17-31`.
- **#1004, #1006, #1008 and #1009 are still open:** checked; #1004 and #1006 are blocked on whole epics.
- **f(A_1) = A_1 as drafted:** false for ids (C3); the measures are equal.
- **`measure()` weighs S 1, M 2 and returns the length only:** checked, `optimize.py:256-275`.
- **The reference passes its own tests:** checked, 13 tests OK.
- **petgraph and rustworkx-core:** checked with `cargo info`: rustworkx-core 0.18.1 (Apache-2.0) depends on petgraph 0.8, rand, rand_pcg and rayon; petgraph 0.8.3. Neither is in zaf's `Cargo.toml` yet.
- **The domain may use an outside crate:** `tests/architecture.rs` checks only `crate::` imports plus the terminal and YAML crates; whether the domain names petgraph is the design's.
- **The chair has a dependency tree for S5 to build on:** checked, `tree.rs:1-4`.
- **The checking model "has no way to change the graph":** false today; instances may run `board file` (C10).
- **Forgejo's timeline as a source of start times:** unverified.
- **dapple and zabo have open plans with "Blocked by":** unverified.

## QA resolutions

- **Routed to the Engineer:** C1 → Q1, C2 → Q2, C3 → Q3, C4 → Q4, C5 → Q5, C6 → Q6, C7 → Q7, C8 → Q8, C9 → Q9, C10 → Q10, C11 → Q11, C14 → Q12, C16 → Q13.
- **Resolved in place:** C12 (three doors added under One-way doors seen, the checker's rights widened), C13 (a design question added), C15 (S1 serves O1, O3, O4, O8; S3 serves O1, O2, O5; S7's fixtures noted; S4's missing check goes with Q3), C17 ("a node is a task" said once; S9 set apart from zaf's Vocabulary note), C18 (the credits line names O1 to O3's checks; the second-run design question carries C3's finding; the Summary's count).
- **Rejected:** none.

## Signed

2026-11-23 · Questions answered: "Agreed". zaf takes the Engineer's signature when this session ends.
