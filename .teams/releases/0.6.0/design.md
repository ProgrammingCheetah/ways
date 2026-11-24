---
signed: 2026-11-23T21:01:35Z
hash: b972acd3ca3c23c561b5df12015a17b0c5c4996ce9398acf3765c0a071dd6d30
---
# Release 0.6.0 design: the plan as a graph of tasks

2026-11-23 · Written by Claude (the Designer) from the signed objectives.md and zaf's code at `feature/0.5.5@origin` 7a0e213b (change ptxwlxsx). **Draft until the Engineer signs it.**

This is the second draft, with the Engineer's answers. A fresh QA raised 12 challenges against the first, and each is resolved under "QA resolutions" at the end. On 2026-11-23 0.5.5 merged to `main` at f2c3886e. `git diff --stat 7a0e213b f2c3886e` over zaf's source, tests, assets, `Cargo.toml` and the pack touches only version numbers, so every line cited here still holds at `main`. The first draft read 4a06d983. Between that revision and 7a0e213b only the profile, setup, update and doctor code changed, and a diff shows no line this design cites moved. The crate facts were read from crates.io and the published source on 2026-11-23, and the fixtures were run with the reference in `plan-graph/`. "Facts checked" says where each fact came from.

## Summary

The planner stops filing tasks one at a time. It writes the whole plan for its epic, and for every task what it **writes**, **provides** and **needs**, in names from the release's vocabulary. It hands the plan to zaf in one call, `zaf plan file <epic>`. Then zaf, with no model:

- **refuses** a plan that is not complete: a need nobody provides, a cycle, two providers, an unknown name. It names why and files nothing;
- **optimises** the rest, by the passes the reference already runs: split, lift each wait on a name with a signature into the epic's interface task, merge or order tasks that write the same area, drop implied waits, measure;
- **files** the result as tickets. Before the first ticket it posts one comment on the epic that holds the plan as written, and after the last it posts a short mark, so a plan stopped halfway can be finished.

The graph is never stored. zaf reads it from the tickets each time it needs it: the scheduler, the chair and the checks all read it the same way. A ticket filed before 0.6.0 still reads, each `Blocked by` line as a need. A re-plan adds tickets beside the filed ones and may drop a ticket that has not started. It never changes a filed ticket.

The rest of the release:

- **The check (O7).** The challenger runs on the optimised graph, as it already runs after planning. It can file nothing: zaf refuses it every filing verb, and its agent loses the shell. zaf compares the graph's hash at its launch and at its report.
- **Seeing it (O6, O8).** In the chair, `d` draws the graph in layers, before and after (`v` flips), with its chain, width and critical path.
- **Shape and results (O3, O1, O2).** `zaf plan verify` prints O3's pass or fail. `zaf epic check` gives O1's two zaf checks. `zaf report --times` gives O2's minutes per planned task.

**Epics.** Eight, E1 to E8, filed on 2026-11-23 as #1155 to #1162, unsigned. All eight land before O1's run. That run is the release's must check, done by hand at the close on a dapple epic the Engineer picks (Q17); it is not an epic.

**Fixtures.** The 18 stay; each now also names its critical path's nodes. Eleven are added. The reference's tests now make random plans of seven kinds, and all 20 pass.

**Questions.** There are 24, and none is open. Claude pre-filled Q1 to Q23 overnight on the Engineer's word. On 2026-11-23 the Engineer agreed to each as pre-filled and picked Q17's (a), a small new dapple epic at the close. Q24, raised in the design session, keeps "CI by tree" in 0.6.1.

<details>
<summary><b>Picture: one plan, from the planner to the board</b></summary>

```
 planner (model)          zaf (no model)                board
 writes the plan ──▶ plan check ──▶ refused: back
                     plan file  ──▶ optimise
                                     ├─▶ plan record ───▶ epic comment
                                     ├─▶ file tickets ──▶ tickets
                                     └─▶ filing mark ───▶ epic comment
 challenger ◀── plan show ◀── graph read ◀──────────── tickets
    └─ report ──▶ zaf adds the hash ──────────────────▶ challenged
 scheduler, chair, checks ◀── graph read ◀──────────── tickets
```

Only zaf writes to the board here; the models read and report.

</details>

## Calls Claude made that the Engineer did not speak to

The Engineer's instruction tonight, verbatim: "When 0.5.5 finishes building, can you run 0.6.0's design and answer the questions with sensible answers as they come? I can run the build in the morning" (in conversation, 2026-11-23). Each call below is Claude's under that instruction. Each points at its question. The Engineer agreed to all 23 as pre-filled in the design session (2026-11-23).

1. The planner hands zaf the whole plan, and zaf optimises before anything is filed. The graph is read from the tickets; the plan as written lives in a zaf comment on the epic (Q1).
2. Writes, provides and needs are plain lines in the ticket's body, so any board that holds text holds them (Q2).
3. The vocabulary is a block in the release's signed design. A planner may add names, but never one the code is said to provide, and zaf checks each such name's path (Q3).
4. A merged task splits back into its parts on a rerun, so ids never grow (`A+B` stays `A+B`) (Q4).
5. XS and S weigh 1, M weighs 2, and the limit is 2 (Q5).
6. Each plan of an epic gets one interface task. zaf writes it for `sonnet`, sized S, or M from three signatures, while the critical path weighs it 1 a signature (Q6).
7. The optimiser redraws no epic and adds no closing task (Q7).
8. Two tasks too big to merge take turns: the one more tasks wait on goes first, the lower id on a tie (Q8).
9. The critical path is weighed by size and follows the heaviest chain, the lower id on each tie (Q9).
10. A need on a name another epic will provide waits on a new kind of line, `Waits on: epic #<n> for <name>`. The line names an epic until zaf resolves it to that epic's task, so O5 holds on the built graph (Q10).
11. A `Blocked by` line that names an epic reads as a need of each task in it (Q11).
12. A finding at the check becomes a question for the Engineer and changes nothing in the graph (Q12).
13. A change to the graph after the challenge voids it, as an edit voids a signature. Only tickets that have not started a phase wait for the new check (Q13).
14. For S8, an epic's work starts at its first phase run and ends when its last task closes; overlapping question time is counted once and left out. The baseline is the 0.5.x epics and the result is the epics planned as a graph; 0.6.0's own epics are in neither (Q14).
15. rustworkx-core is added, as signed, but only two of its functions are used. Its `longest_path` is banned because its ties change between runs (Q15).
16. A re-plan only adds, and may drop a ticket that has not started. Its signatures go into a new interface ticket, and its tasks take turns after filed tickets that write the same area (Q16).
17. O1's run is done by hand at the release's close as the release's must check, not as an epic. Which epic it builds is the Engineer's (Q17).
18. The challenger's agent loses Bash, WebFetch and WebSearch (Q18).
19. A wait on another epic's name with a signature is on that epic's interface ticket. An epic lifts every name other epics need, and a kept wait enters the hash as written (Q19).
20. Under an epic planned as a graph, `board file` is refused to everyone, the Engineer included. The release instance files fix tasks with `plan file` (Q20).
21. A design signed through `signoff.md` counts as signed. A design with no vocabulary block plans from the plan's declared names, and a block in an unsigned design refuses the plan (Q21).
22. `plan file` posts the plan record first and a filing mark last. zaf's tick finishes a half-filed plan from the record (Q22).
23. A "Done when" line is checkable when it names tests in backticks. `epic check` refuses an epic with none, and the designer prompt asks for such lines (Q23).

## Questions for the Engineer

Each question has its options, Claude's pick first, then Claude's answer. The Engineer answers on the **Engineer:** line.

**Q1. Where does the optimiser run, and where do the graphs live?** "A ticket is never expanded" (zaf's domain document), and an edit voids an epic's signature.

- (a) **(Recommended)** Before filing. The planner hands zaf the whole plan (`zaf plan file <epic>`, the plan in `stdin`); zaf refuses it or optimises it, then files the tickets. The optimised graph is read from the tickets each time it is needed. The plan as written goes into a comment zaf writes on the epic (the plan record).
- (b) After filing. The planner files as today and zaf rewrites the tickets, merging some and closing others.
- (c) The graph lives in a file in the repository.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Why:

- (b) merges filed tickets, which expands one and leaves the board's history showing every rewrite.
- (c) is a second copy of what the tickets say, and at work it would be a change to the workplace's repository.
- A comment does not touch the epic's body, so the signature holds (`domain/epic.rs:228-233`).
- The plan as written is a fact zaf cannot work out again, like a report, so storing it keeps to "derive state on read".

**Engineer:** Agreed (2026-11-23).

**Q2. Where do writes, provides and needs live on a ticket?** It is a one-way door: every ticket filed after 0.6.0 carries them, and 0.7.0 is "any board".

- (a) **(Recommended)** As lines in the body, beside the template's fields: `Node`, `Writes`, `Provides`, `Implements`, `Needs`, and on tickets zaf makes, `Turn` and `Waits on`. A ticket that holds two behaviours has one section for each.
- (b) As labels.
- (c) As the board's own links (Forgejo dependencies, Jira issue links).

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Why:

- Every board holds a body of text: Forgejo and GitHub issues, Jira descriptions, zabo cards.
- Labels would be hundreds of names, and Jira labels take no spaces.
- Links are edges, and edges are derived from the names, so storing them would say one fact twice.

**Engineer:** Agreed (2026-11-23).

**Q3. Where does the vocabulary live, and who adds to it?** This also answers who writes `existing` and `no_signature` (QA C13 of the objectives).

- (a) **(Recommended)** It is a `vocabulary` block in the release's `design.md`, signed with the design. It holds the areas, then each name with its kind: a signature with its shape, or no signature. A name the code already provides gives a path, and zaf checks that the path is in the tree. Any other name gives the epic that provides it. A planner may add areas and names in its plan, never one the code provides, and the challenger is shown the added ones. A release with no block makes the plan declare every name it uses (Q21). The planner never writes `existing` or `no_signature`.
- (b) With the node tree, one list per project across releases.
- (c) One list per epic.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Why:

- The design's modules and interfaces are already the names, and the Engineer signs them.
- One list per release lets another epic's names be checked (objectives Q7).
- A planner who cannot mark a name `existing` cannot get past O4.

**Engineer:** Agreed (2026-11-23).

**Q4. How does a rerun keep its ids?** The objectives' QA (C3) found `A+B` coming back as `A+B.a+A+B.b`.

- (a) **(Recommended)** A merged node splits back into the nodes it was made of (`A+B` into `A` and `B`), and settle merges them again under the same id. An id with `+` must hold one behaviour per part. Fixture 19 checks it, and so does a test over every fixture and every random plan.
- (b) zaf numbers nodes afresh on every run.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). It gives f(f(A)) = f(A), ids included, which is O3's rerun check. The reference passes it on every fixture and random plan, re-plans included.

**Engineer:** Agreed (2026-11-23).

**Q5. The size limit.**

- (a) **(Recommended)** XS and S weigh 1, M weighs 2, and the limit is 2. A task sized L, XL or E is refused as too large.
- (b) XS weighs less than S.
- (c) The limit is 3.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Why:

- It is what the fixtures encode (`plan-graph/reference/optimize.py:49`, the `limit` default).
- A planner already files only XS, S or M (`domain/task.rs:82-86`).
- A merged ticket is then never above M.

**Engineer:** Agreed (2026-11-23).

**Q6. One interface task per epic, or one per signature?**

- (a) **(Recommended)** One per epic, and one more for each re-plan (Q16). It holds every signature that the epic's tasks, or other epics' tasks, need from it. zaf writes it for `sonnet`. Its size is S, or M from three signatures, which is what the board and the scheduler read for any ticket. The critical path weighs it 1 a signature, so a large one shows on the path, though no body node may weigh more than 2.
- (b) One per signature.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Why:

- The hours are the same: on 0.5.5 replayed, one interface per epic gives 67 nodes, chain 6 and 6.99 hours at 4 lanes; one per signature gives 95 nodes, chain 6 and 6.92 hours. Both rows optimise the whole release as one plan.
- zaf will optimise each epic alone. The replay of that, with waits on other epics' names kept on the providing epic's interface (Q19), gives the same 67 nodes, chain 6 and 6.99 hours. Had the 17 names needed across epics never been lifted, it would be chain 9 and 7.61 hours.
- The board holds fewer tickets.
- The stand-ins for a shared file all land in one change, which was R31's skeleton's job.

**Engineer:** Agreed (2026-11-23).

**Q7. May the optimiser redraw epics, or add a closing task to each?**

- (a) **(Recommended)** Neither. Across epics is out, and an epic's end is derived from its tasks. O1's `epic check` runs when the epic is done, not as a task.
- (b) It adds a closing "joining" task per epic.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). A joining task would be a node that builds nothing and lengthens every chain by one.

**Engineer:** Agreed (2026-11-23).

**Q8. Two tasks write the same area, have no order, and are too big to merge.**

- (a) **(Recommended)** They take turns: a `turn:<area>` wait. The one more tasks wait on goes first, the lower id on a tie (fixtures 03, 14, 16, 18).
- (b) The plan is refused, and the planner splits the work or adds a skeleton.
- (c) They stay unordered, and the merge finds the conflict.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). It keeps O3's "no two tasks that write the same thing are left unordered" without sending the planner back. The rule is deterministic, and the challenger sees every turn.

**Engineer:** Agreed (2026-11-23).

**Q9. What weighs a node on the critical path (O8)?**

- (a) **(Recommended)** Its size: XS 1, S 1, M 2, and an interface task 1 a signature. zaf names the path's nodes in order. The path ends where the heaviest chain ends, and each step back stays on that chain; every tie goes to the lower id.
- (b) Its predicted minutes from the simulator.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Why:

- It is deterministic and testable.
- The minutes are guesses, since no interface task has run.
- Every fixture now names its path, and fixture 20 pins both tie rules.

**Engineer:** Agreed (2026-11-23).

**Q10. A need on a name another epic will provide, when that epic is not planned yet.** The objectives' Q7 kept such waits; this is the case where the providing ticket does not exist yet.

- (a) **(Recommended)** It is accepted when the vocabulary gives the name to another open epic of the release. The ticket carries `Waits on: epic #<n> for <name>`, a new kind of wait. Until that epic is planned the line names an epic, and the ticket waits. Once planned, zaf resolves the line to the providing ticket: the interface ticket for a name with a signature (Q19), the task for one with none. When that epic is planned without the name, its plan is refused. If that epic closes without it, the wait can never end, and zaf raises a question.
- (b) It is refused until the providing epic is planned.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Why:

- With (b), epics would have to be planned in an order zaf does not know.
- O5 is checked "in the graph of a built epic". By the time a ticket can start, its wait has resolved to a task, so O5 holds there. The stored line still names the epic, and the Engineer should know that.

**Engineer:** Agreed (2026-11-23).

**Q11. A `Blocked by` line that names a whole epic (objectives, "Epics planned before 0.6.0").**

- (a) **(Recommended)** It reads as a need of each task in that epic that counts (open or done), and thinning drops what is implied. An epic with no task yet keeps the wait on itself, as today.
- (b) It stays a wait on the epic.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Why:

- It is how the replay read 0.5.5.
- It is what today's code means: an epic blocker is done once the epic is finished (`app/spawn/blockers.rs:77-104`).
- It makes O5 hold on old plans too.

**Engineer:** Agreed (2026-11-23).

**Q12. What does the model's check do with a finding (O7)?**

- (a) **(Recommended)** It becomes a question for the Engineer, with a proposed answer, as the challenger's findings do today. Nothing in the graph changes. If the ruling asks for a change, a re-plan files it (Q16).
- (b) A finding refuses the plan.
- (c) A finding is a note only.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). "It can be CHECKED by a model afterwards, but not MADE by it": a refusal by a model would be a model making the graph.

**Engineer:** Agreed (2026-11-23).

**Q13. Does a change to the graph after the challenge void it?**

- (a) **(Recommended)** Yes, but only tickets that have not started a phase wait for the new check. The epic is challenged while the challenger's report carries the hash of the graph as it reads now, the way a signature holds while the body is unchanged. A ticket closing changes nothing. An edited ticket body, or a re-plan, voids the challenge, and the challenger runs again, once for each graph. A ticket already in its phases goes on, since a re-plan never changes a filed ticket (Q16). An unsigned epic still stops every ticket, as today.
- (b) Yes, and every ticket that is not running waits, as `domain/schedule/place.rs:104` does today for an unchallenged epic. That includes tickets between phases, so a ticket whose CI passed stops before its merge.
- (c) No. Once challenged, always challenged.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). (c) lets a re-plan's tasks build unchecked. (b) stops work the new check cannot change. The cost of (a): the epic's unstarted tickets wait until the new challenger reports.

**Engineer:** Agreed (2026-11-23).

**Q14. S8's start and finish, and its two groups (objectives, "S8's start and finish").**

- (a) **(Recommended)** An epic's work starts at the first phase run of any of its tasks (the run folder's `task` file, `app/schedule/record.rs:10-11`). It ends when its last task closes, at the board's close time. Time while a question was open on the epic or on a task is left out; overlapping questions are counted once. Minutes are divided by the planned count: every plan record's count, less what was dropped, for a graph-planned epic, and the tasks that count for an older one. The baseline row is the epics of a 0.5.x release ("The 0.5.x already on the board", O2). The result row is the epics planned as a graph. 0.6.0's own epics were built the old way, but they are not 0.5.x, so they sit in neither row and are named as left out. An epic with no phase record is left out and named too.
- (b) It starts at signing or at the challenge.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). It measures implementation time. No 0.5.x epic's phase records have been pruned: no code removes them, and 1,303 folders from 2026-11-12 on are on this machine. The rows keep to O2's and S8's signed words; the Engineer may widen the baseline to 0.6.0's own epics.

**Engineer:** Agreed (2026-11-23).

**Q15. rustworkx-core.** The Engineer: "I think petgraph and rustworkx-core should be added." What was read on 2026-11-23:

- It has no feature flags, so `rand`, `rayon` and `ndarray` always compile in, though they never run unless called.
- Its `longest_path` breaks ties through a hash map seeded at random, so the path it returns can differ between runs.
- It has no transitive reduction; petgraph has one.

The options:

- (a) **(Recommended)** Keep it, as signed. Use only its `lexicographical_topological_sort` and `layers`, inside one module. An architecture test bans `longest_path`, `rand` and `rayon` everywhere. zaf writes the critical path itself.
- (b) Drop it and use petgraph alone.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). It is the Engineer's pick, and it is fenced into one file, so (b) stays a one-file change later.

**Engineer:** Agreed (2026-11-23).

**Q16. How does a re-plan work? (QA C1)** Today the planning role re-plans by "close, rewrite or add tasks; a claimed task is never touched" (`plugins/teams/agents/planning.md:101`). With the first draft's rules, three things broke:

- a closed task failed `epic check` for good;
- a re-plan's interface got the filed one's id, so it was skipped and its signatures got no ticket;
- a new task that writes a filed task's area was never ordered against it.

The options:

- (a) **(Recommended)** A re-plan is another `plan file` on the epic, and it adds. Filed tickets are fixed. The plan may drop a ticket no phase has started: zaf closes it as not planned, and the record names it. It never rewrites a filed ticket ("A ticket is never expanded"); to change an unstarted one, drop it and add a new one. Its signatures go into a new interface ticket, `I@<epic>.<k>` for the epic's k-th plan. A new task that writes an open filed ticket's area takes turns after it and is never merged with it. A new task may not wait on an open filed task for a name with a signature that no interface holds. That is refused as `fixed-provider`: drop and re-add that task, or wait until it is done. A done ticket's names count as existing code. `epic check` compares against every record's plan, less what was dropped.
- (b) A re-plan rewrites unstarted tickets in place.
- (c) A re-plan replaces the graph: zaf closes every unstarted ticket and files afresh.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). It keeps "A ticket is never expanded", and filed tickets stay as they were challenged. Fixtures 27 and 28 check it, and so do the random re-plans, where a filed ticket never changes. (b) edits filed bodies. (c) throws away work already challenged.

**Engineer:** Agreed (2026-11-23).

**Q17. Which epic does O1's run build? (The Engineer's alone.)** O1 says "one dapple or zabo epic … on a 0.6.0 pre-release binary before the release closes". On 2026-11-23:

- neither board has an open epic (`zaf epic list` printed nothing in either);
- zabo's v0.1 design is not signed;
- dapple's v0.1 design is signed through `signoff.md`;
- neither design has a "Done when" line.

The options:

- (a) **(Recommended)** A small new dapple epic that the Engineer picks and signs at 0.6.0's close, with "Done when" lines that name tests (Q23).
- (b) A zabo v0.1 epic, once the Engineer signs zabo's design.

How the run happens is Claude's call: by hand at the release's close, as the release's must check. It is not a filed epic of 0.6.0, which would get a planner and never finish.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a), as a placeholder only. The epic, its scope and its signature are the Engineer's to pick.

**Engineer:** (a), a small new dapple epic, picked and signed at 0.6.0's close (2026-11-23).

**Q18. Can the challenger file through the shell? (QA C7)** O7 says "the checker cannot file tasks", and zaf refuses the challenger every filing verb (D22). But the challenger keeps Bash. In ways, its profile allows `node`, a `[requires]` tool, and `cat`, a read-only basic (`app/profile/mod.rs:785-788`, `:1197-1210`; `.zaf.toml:7-11`). The forge token is on disk. Together these could reach the forge. This was not tried.

- (a) **(Recommended)** The challenger's agent file also disallows Bash, WebFetch and WebSearch, beside Agent, Edit, Write and NotebookEdit (`plugins/teams/agents/challenger.md:6`). It reads with Read, Grep and Glob and works through zaf's MCP tools. Its brief uses nothing else: the `progress`, `epic`, `brief`, `ask` and `board` tools, and Read for the pack `brief` writes (`zaf/assets/spawn/challenger-brief.md`). Profiles already load no MCP server but zaf's (#1053, at d15a6778). The hash stays as the detection.
- (b) Keep Bash; the hash alone detects a change.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Nothing in the brief needs the shell, and (b) leaves "cannot file" true only through zaf. A ways test checks the agent file.

**Engineer:** Agreed (2026-11-23).

**Q19. A need on another epic's name with a signature (QA C6).** O3 is signed: "each wait on a name with a signature goes through an interface". A wait kept on the providing task breaks that, and the first draft's fixture 22 did.

- (a) **(Recommended)** When the providing epic is planned as a graph, the wait is on its interface ticket. To make sure that ticket holds the name, an epic lifts every name with a signature that another epic needs, even when none of its own tasks needs it (fixture 29). Those are the names the vocabulary gives it, and the names a filed ticket elsewhere needs from it. A name the vocabulary gives an epic planned before 0.6.0 is refused, with the way on: need that epic's task as `#<n>`, which has no signature. A kept wait enters the hash as written (`epic #<n> for <name>`), so another epic's planning never voids this epic's challenge.
- (b) Kept waits stay on the providing task, and O3 gains an exception for them.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). O3 holds as signed. The replay gives the same hours as lifting across the whole release (6.99 at 4 lanes, chain 6).

**Engineer:** Agreed (2026-11-23).

**Q20. Who may still use `board file`? (QA C2)** The release instance files fix tasks "with the board's own tool, in the task template, as the planner files them" (`plugins/teams/agents/release.md:110`).

- (a) **(Recommended)** Under an epic planned as a graph, nobody, the Engineer included. The refusal says "this epic is planned as a graph; re-plan with `plan file`". Under an older epic, the Engineer and the release instance keep it, as today. Planning and the release instance file with `plan file`, and the challenger with nothing.
- (b) The Engineer keeps `board file` everywhere.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). A ticket filed beside the graph carries no names: the graph cannot place it, and O3 cannot be checked. The release instance's fix tasks go through the same door as the planner's.

**Engineer:** Agreed (2026-11-23).

**Q21. Which designs give a plan its names? (QA C3)** dapple and zabo sign through `signoff.md`, a `git hash-object` per file, not through zaf's signature block. zaf leaves such folders alone today (`app/release.rs:68-70`).

- (a) **(Recommended)** A design counts as signed either way: through zaf's block, or through a `signoff.md` row whose hash matches the file. A design with no vocabulary block, or no design at all, plans from the plan's declared names (D6), so no signed design is edited to add one. A block in a design that is not signed refuses the plan, with the way on.
- (b) Only zaf's signature block counts, and other projects add one.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Adding a block to a signed design would void its signature, and dapple and zabo keep planning as they are.

**Engineer:** Agreed (2026-11-23).

**Q22. A plan filed halfway (QA C5).** The spawner does not launch planning twice for one signature (`app/spawn/tick.rs:152-194`).

- (a) **(Recommended)** `plan file` posts the plan record first, holding the whole plan as written, and a filing mark last, holding the graph's hash. An epic is planned once its last record has its mark. A record with no mark is finished from the record itself, by `zaf plan file <epic>` with nothing in stdin. zaf's tick runs that when no planning instance is on the epic. If the design changed since the record was posted, zaf refuses to finish and raises a question.
- (b) The record is posted last, as the first draft had it, and the Engineer runs the planner again by hand.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). Finishing needs no model, and the record holds everything needed.

**Engineer:** Agreed (2026-11-23).

**Q23. What makes a "Done when" line checkable (O1)? (QA C3, C10)**

- (a) **(Recommended)** It names one or more tests in backticks, and zaf runs each through the manifest's `[gate] tests` template. A line that names none fails as "not checkable". An epic with no "Done when" line is refused, never passed. The designer prompt and the design action ask for such lines in every epic's parts.
- (b) zaf runs the gate only; the lines are the Engineer's to read.

**Claude's answer (2026-11-23, on the Engineer's word "answer the questions with sensible answers as they come"; the Engineer adjusts):** (a). It follows "automate, do not read", and passing an epic with no lines would be a false pass.

**Engineer:** Agreed (2026-11-23).

**Q24. CI by tree (raised in the design session, 2026-11-23).** Claude's memory note says that on 2026-11-23 the Engineer agreed "CI by tree" (a tree that already passed CI is never built again) goes into 0.6.0's design. The signed objectives say "0.6.0's responsibilities are not the CI and merge" and move them to 0.6.1, and the draft left it out without asking.

- (a) **(Recommended)** It stays out and goes to 0.6.1 with the CI and the merge.
- (b) It joins 0.6.0 as a ninth epic, which widens the signed scope.

**Claude's answer (2026-11-23; the Engineer adjusts):** (a). The signed objectives name no objective it would serve, and 0.6.1 already holds the wait after each run, which is what it shortens.

**Engineer:** (a), out: 0.6.1 (2026-11-23).

## Goals and non-goals

| Objective | Met by |
|---|---|
| O1 the end product is what is expected | D8, D9, D25, D27, D29 (E1, E8, O1's run) |
| O2 build times are reduced | D26 (E8); D11 and D12 shape the chain it measures |
| O3 the graph has the right shape | D8 to D16, D24, D29 (E1, E5) |
| O4 an incomplete plan is refused | D1, D2, D5 to D8 (E1, E3) |
| O5 tasks block on tasks, not epics | D4, D13, D19 (E2, E3, E4) |
| O6 the graph in the chair | D23 (E5) |
| O7 a model checks the graph | D21, D22 (E6, E7) |
| O8 the critical path | D15, D23, D24 (E1, E5) |

Not in 0.6.0, carried over from the objectives:

- the CI and the merge ("0.6.0's responsibilities are not the CI and merge"), so "CI by tree" is not in this design (Q24: it goes to 0.6.1);
- the optimisation quotient (0.6.1);
- optimising across epics (a later patch);
- the graph on the hub's web page (when the CLI settles).

The number of passes is 1 (S4).

<details>
<summary><b>The questions for the design, answered</b></summary>

| Parked in the objectives | Answered by |
|---|---|
| What weighs a node on the critical path | D15 (Q9) |
| Who writes `existing` and `no_signature` | D6, D7 (Q3) |
| Where the vocabulary lives and who adds to it | D5, D6 (Q3, Q21) |
| What the model's check does with a finding | D21 (Q12) |
| Where the optimised graph lives; how the board shows a split or merge | D3, D17, D18 (Q1, Q2) |
| S8's start and finish | D26 (Q14) |
| The size limit | D10 (Q5) |
| One interface task each, or one for all | D11 (Q6) |
| Whether the algorithm redraws epics; a joining task | D14 (Q7) |
| A conflict that cannot merge | D12 (Q8) |
| A `Blocked by` line naming a whole epic | D4 (Q11) |
| What a second run changes; how a rerun keeps ids | D9 (Q4) |

</details>

<details>
<summary><b>Decisions D1 to D29</b> (the design itself)</summary>

### The plan

**D1. Three passes, three owners.** The Engineer: "create an epic (make it exist), make sure it is complete (make it work) and then optimize it with an algorithm (make it pretty)". The quote is from Claude's memory note of the 2026-11-22 talk (`zaf-epic-design-algorithm.md`), not from the objectives.

- *Exist:* the planner drafts every task of its epic as a node.
- *Work:* the planner runs `zaf plan check` until zaf no longer refuses the plan. zaf's refusals are the completeness check, so the planner never judges completeness itself.
- *Pretty:* `zaf plan file <epic>` runs the optimiser and files what it gives.

Only the last pass writes. "The optimization has to be deterministic, not done by a model."

**D2. The plan's shape (I1).** It is the task template the planner already writes, one section per task, each opened by `Task: <title>`:

- `Node: <key>`, the planner's name for the task: letters, digits, `-` and `_`. `.`, `+`, `:`, `@` and `#` are zaf's.
- The template's fields as today, except `Blocked by`.
- Three new lines, each a list of names: `Writes` (areas), `Provides` and `Needs`.
- An optional `Group` and `Type` per task, which were `board file`'s flags.

The plan may open with two lists. `New names:` adds names, one item each: `area <name>`, `signature <name>: <shape>` or `no signature <name>`. `Drop:` names filed tickets a re-plan drops (D29). Nobody writes an edge. One task is one behaviour, as ruled today. `--blocked` is gone, because zaf places tickets from facts.

**D3. A ticket's shape on the board (I2; Q2; a one-way door).** A ticket filed from a plan holds, in its body:

- `Node: <id>`. It is the planner's key, `A+B` for a merge, `A.a` for a part, or `I@<epic>.<k>` for the interface task of the epic's k-th plan.
- One section per behaviour. Each section is the template's fields without `Blocked by`, plus `Writes`, `Provides`, `Implements` and `Needs`. `Implements` names what the interface task gave a signature to: it reads as a need for the graph, and the programmer sees it as "build the body of this stand-in". A ticket of one behaviour has no `Behaviour:` line. A merged ticket opens each section with `Behaviour: <key>`.
- Lines only zaf writes: `Turn: after <node> on <area>`, and `Waits on: #<n> for <name>` (a task named by number) or `Waits on: epic #<n> for <name>` (another epic's name, Q10, Q19).
- On the interface ticket: `Kind: interface`, and `Signatures:` with one item `<name>: <shape>` for each name.

Its size is derived from its sections: weight 1 is S (XS when its one behaviour is XS), and weight 2 is M. The interface ticket is S, or M from three signatures. Its model is `opus` if any section says so.

**D4. Plans filed before 0.6.0 still read (S11; Q11).** A body with no `Node:` line parses as today's template (`domain/task.rs:530-645`). In the graph it is one node, whose id is its number:

- it provides `#<n>`;
- each `Blocked by` line is a need of `#<m>`;
- a line naming an epic is a need of each task of that epic that counts, and an epic with no task yet stays a wait on itself;
- `#` names have no signature, so nothing old is lifted; old tickets are read, never optimised.

R31's skeleton task is then a node that provides the name its fillers need (fixture 21). A new plan may also need `#<n>` for a filed task of the release: that is a kept wait (D13).

### The vocabulary

**D5. Where it lives (S9; Q3, Q21; a one-way door).** In the release's `design.md`, in the one fenced block opened by ```` ```toml vocabulary ```` (I3). It is signed with the design, so the Engineer signs the names when signing the design.

- `[areas]`: every area a task may write, each with a note. An area is a part of the system in words, never a file (objectives Q8; ruling 31 stands).
- `[names."<name>"]`: each name a task may provide or need. It has `signature = "<shape>"` or `no_signature = true`, then either `epic = <n>` (the epic that provides it) or `existing = "<path>"` (the code already provides it).

zaf reads the block when a plan is checked or filed. The release is the version on the first line of the epic's body (`Release: <version>`, `domain/epic.rs:27-45`, `:221-226`), and the manifest gives its folder (`app/release.rs:41`). The design counts as signed through zaf's signature block (`domain/document_signature.rs`) or through a `signoff.md` row whose `git hash-object` matches the file, the way dapple and zabo sign. A block in a design signed neither way refuses the plan, with the way on, and so does a block that does not parse. A design with no block, or no design, plans from the plan's own names (D6). A name holds lowercase letters, digits, `.`, `-`, `/` and `_`. `#<n>`, `sig:` and `turn:` are zaf's.

S9 also asks for words "for the edges and relationships nodes have between them". Those are zaf's and fixed, not per release: *needs* (an order), *implements*, *turn* and *waits on* (a kept wait). They go into zaf's Vocabulary note (D28), apart from the names.

**D6. Who adds names (Q3).** The plan's `New names:` may add areas and names, with or without a signature, never one the code provides. A name it adds must be provided in the plan if anything needs it. `plan show` lists the added names, so the challenger sees them. A release whose design has no block works the same way: the plan declares every name it uses, and nothing is existing. A need met by existing code makes no edge, so a planner simply leaves it out. So dapple and zabo plan without touching their signed designs.

**D7. What the planner can never write (QA C13 of the objectives).** `existing` and `no_signature` come from the signed vocabulary or from the plan's declared kinds; the plan format has no field for either list. An existing name's path must be in the tree at the integration branch's head (`Vcs::path_at`, I11), or the plan is refused naming the path. With the vocabulary present, a name in neither list is refused as `unknown-name` (fixture 24), and a plan that tries to add an existing name is `planner-list` (fixture 26).

### The optimiser

**D8. f is the reference, in Rust (S1).** It lives in `domain/plan/`, is pure, and passes every fixture in `zaf/tests/data/plan-graph/`, a copy of `plan-graph/fixtures/` as this design amends them. The passes run once each, in order:

- **Refuse.** It names every problem. The first one is the code the fixtures name: `duplicate-id`, `no-behaviour`, `bad-size`, `too-large`, `needs-itself`, `bad-id`, `unknown-name`, `planner-list`, `two-providers`, `unprovided`, `fixed-provider` or `cycle`. Nothing is repaired.
- **Split, lift, settle, thin, measure,** as the reference does them.

The reference's random plans come in seven kinds: plain, with the vocabulary, with other epics' names, one epic's plan, with names other epics need, filed with `Blocked by`, and a re-plan beside filed tickets. Each runs in the four option sets. Twelve of the properties `test_reference.py` checks on them become Rust property tests over the same generator:

1. the meaning is kept (what O1 rests on);
2. no cycle, every wait has both ends, and ids are unique;
3. no body node passes the limit;
4. the same plan gives the same result however it is listed;
5. no two writers of one area are left unordered;
6. no wait is left that another path implies;
7. after lifting, a body waits on a body only on a name with no signature, or on a turn;
8. a rerun changes nothing, ids included;
9. the critical path is a path of waits whose sizes add up to its length;
10. lifting alone, with no conflicts, gives a chain of 2 or less;
11. a kept wait on a name with a signature comes from an interface;
12. a re-plan never changes a filed ticket.

The reference's thirteenth check, that the simulator finishes every plan, stays in Python, since the simulator is not ported.

**D9. A rerun changes nothing, ids included (S4; Q4).** Split gives each behaviour its id: a node of one behaviour keeps its own; a merged node `A+B` splits back into `A` and `B`; any other node with several behaviours gives `<id>.a`, `<id>.b`, … An id with `+` must hold one behaviour per part (`bad-id`). Settle then meets the same pairs in the same order and merges them under the same ids. `plan verify` runs f on the graph read from the board and fails if anything differs, ids included.

**D10. Sizes and the limit (Q5).** The weights are XS 1, S 1, M 2; L, XL and E are refused as `too-large`. The limit is 2 and applies to body nodes. An interface node weighs 1 a signature and has no limit (the reference).

**D11. One interface task per plan of an epic (Q6).** Production runs with `interfaces: one`; `each` stays for the fixtures. The interface ticket's id is `I@<epic>.<k>`. zaf writes its body from the vocabulary:

- one `Signatures:` item per name, with the shape the vocabulary or the plan gave;
- Goal: "the signatures of …: stand-ins that build, with no behaviour";
- Acceptance: "the gate passes with every stand-in";
- model `sonnet`, size S, or M from three signatures; its security trigger is `yes` when a task implementing one of its names says so.

Its size is what the board and the scheduler read, as for any ticket. Its weight on the critical path is 1 a signature (D15). The two differ on purpose, so a large interface shows on the path. The brief of a task that implements or needs a name quotes its shape from this ticket.

**D12. Conflicts (Q8).** Settle as the reference does it. Two unordered bodies that write the same area merge when their weights fit the limit. Otherwise they take turns: a `turn:<area>` wait, the one with more tasks waiting on it first, the lower id on a tie. In a re-plan, a filed ticket always goes first (D29). A conflict the planner did not write still shows at merge (R28), as objectives Q8 accepted.

**D13. Another epic's names (S10; Q10, Q19).** A need that this plan does not provide is not optimised when its provider is one of these:

- a filed task of another epic of the release;
- a filed task named as `#<n>`;
- a name the vocabulary gives another open epic.

It makes no edge inside the plan and is never lifted or thinned. It is kept as a `Waits on:` line, the reference's `kept` (fixtures 22 and 23). For a name with a signature in an epic planned as a graph, the wait is on that epic's interface ticket, so O3 holds as signed. So that the interface ticket holds the name, an epic lifts every name with a signature that another epic needs, even when none of its own tasks needs it (the reference's `shared`, fixture 29). Those are the names the vocabulary gives it, and the names a filed ticket elsewhere needs from it.

Four refusals guard this:

- A plan that provides another epic's name has two providers.
- A name the vocabulary gives an epic planned before 0.6.0 is refused, with the way on: need that epic's task as `#<n>`.
- When an epic files its plan, every name the vocabulary gives it that a filed ticket elsewhere needs must be among its provides, or the plan is `unprovided`.
- `Waits on` lines are written as the plan meant them, by epic and name or by number, never by the ticket they resolve to.

**D14. No redraw, no closing task (Q7).** The optimiser adds interface tasks and nothing else, and every node stays in its epic.

**D15. Measure and the critical path (O8; Q9).** Layers come by longest path: the chain is their number, the width the size of each, and the narrow layers those under the lanes. The work is the sum of the weights. The critical path is weighed by size. It ends at the node where the heaviest chain ends, and each step back stays on that chain: it goes to a node it waits on whose own heaviest chain is shorter by exactly this node's weight. Every tie goes to the lower id (`plan-graph/reference/optimize.py`, `critical_path()`). zaf names its nodes in order and its length (fixture 20). For example, 0.5.5 as it was planned has a chain of 13 and a critical path of 19, from #1012 to #1058 through 13 tasks.

**D16. Determinism, and what the libraries do (Q15).** `domain/plan/graph.rs` is the only file that names petgraph or rustworkx-core (an architecture test, as globset is fenced into `domain/ci`, `tests/architecture.rs:350`). Its graph is a petgraph `Graph`, built afresh from lists sorted by natural id and never changed by removal, since removal reorders indices. What it uses:

- petgraph `algo::is_cyclic_directed`, and `algo::kosaraju_scc` to name a cycle. It is iterative where `tarjan_scc` recurses. Each component and the list are sorted before use.
- petgraph `algo::tred::dag_to_toposorted_adjacency_list` and `dag_transitive_reduction_closure` for thin (the reduction) and for settle's "is there an order" and "how many wait" (the closure). Edges come out sorted. Between two nodes one wait stays, labelled with the greatest of its names, which is what the reference does (fixture 13).
- rustworkx-core `dag_algo::lexicographical_topological_sort`, keyed by natural id, for the one order every later step walks. Its length is checked against the node count, since a cycle shortens it silently.
- rustworkx-core `dag_algo::layers`, from the sources in natural order, each layer sorted.

zaf writes the critical path itself (D15). Banned everywhere by the architecture test: rustworkx-core's `longest_path`, `rand` and `rayon`. The plan module uses no `HashMap` or `HashSet`. The tests check the same plan listed in any order gives the same result.

### Filing and reading

**D17. `zaf plan file <epic>` (Q1, Q22).** In order:

1. It parses the plan and reads the vocabulary. With the epic's filed tickets and the release's, it builds f's input: the lists, the names other epics provide, the names other epics need (`shared`), and for a re-plan the filed tickets (D29).
2. It runs f. A refusal prints every problem, one line each (the first is the fixtures' code), exits 1 and writes nothing. `zaf plan check` stops here and prints the measures before and after instead.
3. It posts the plan record (I8): the whole plan as written, its number k, the dropped tickets, and the design's hash.
4. It files each node in natural order. A node whose `Node:` id is already filed under the epic is not filed again.
5. It closes each dropped ticket as not planned (D29).
6. It reads the epic back and posts the filing mark: the record's number and the graph's hash.
7. It prints one line, for example: `filed 7 tickets for 9 planned tasks (1 interface, 1 merge): chain 5 → 3, critical 8 → 5`.

A record with no mark is a half-filed plan. `zaf plan file <epic>` with nothing in stdin finishes it from the record, repeating steps 4 to 6. zaf's tick runs that itself when no planning instance runs on the epic. A design changed since the record refuses the finish and raises a question naming both hashes. While a record has no mark, a new plan is refused: finish that one first. A planner that stops before step 3 has filed nothing, and the epic waits for a planner as today.

**D18. The graph is read from the tickets (Q1).** `PlanGraph::read` (I6) takes an epic's tickets, closed ones included, plus the release's open epics:

- each ticket is a node;
- each need is an edge from the ticket that provides the name, or from the interface ticket for an implemented name;
- `Turn` lines are edges, and `Waits on` lines are kept waits, resolved by name to the providing epic's interface ticket or task (D13);
- a wait on a ticket that was done before the waiting ticket's record was posted is not a wait, since the name was code by then (D29);
- the edges are thinned.

A need with no provider and no `Waits on` line is one the code provides; zaf checked that when it filed. A provider closed as not planned is a dead wait. The hash (I6) is SHA-256 over the canonical form: each node's id, kind, ticket number and body hash (`BodyHash`, `domain/epic.rs:114-126`), then the edges inside the epic, then the kept waits as written, sorted by natural id. A state change never moves it, and neither does another epic's planning. An edit to any of the epic's ticket bodies does. Nothing is stored but the tickets, the records and the marks. That is "derive state on read".

**D19. The scheduler builds the graph (S3).** Each reader of `Blocked by:` reads the ticket's waits from the graph read instead:

- the gather (`app/schedule/gather.rs:728`) and the label sync (`labels.rs:285`);
- the tick's cycles (`app/spawn/tick.rs:249`);
- the hub's line (`app/hub/state/line.rs:213`) and the chair's squares (`app/chair/build.rs:388`).

Interface tasks go first by their edges, since nothing they need comes after them; rulings R6 and R20 are unchanged (objectives Q12). A wait that can never end raises a question at once, by today's dead-dependency path (`domain/schedule/wait.rs:264-288`). That covers a provider dropped, a cycle, and another epic planned or closed without the name. The questions' way on fits the ticket (`app/schedule/ask.rs:546-561`). For an older ticket it stays "leaves its Blocked by: line". For a graph ticket it is "a re-plan drops the ticket or provides the name", and for a cycle across epics, "a re-plan of one of the epics breaks the cycle". An epic is planned once its last plan record has its filing mark; an older epic once it has tasks (`domain/epic.rs:240-253`). An epic whose last record has no mark is half filed: the chair shows it, with the way on, until the tick finishes it.

**D20. Who files (Q20).** The planning role loses `board file` and `epic file` and gets `plan file` (I10). The release instance gets `plan file` for its fix tasks under an epic planned as a graph; those are a re-plan (D29). Under an epic that has a plan record, `board file` is refused to everyone, the Engineer included: "this epic is planned as a graph; re-plan with `plan file`". Under an older epic, the Engineer and the release instance keep `board file`, as today.

### The check (O7)

**D21. The challenger checks the optimised graph (objectives Q10; Q12).** It already runs after planning (`app/spawn/tick.rs:186-191`), and planning now ends with the optimised graph filed, so it moves by itself. Its brief changes:

- it reads `plan show <epic>` before and after, and each ticket's brief;
- it checks each task as today, and also: each interface's signatures, each merged ticket, the meaning kept, the names the planner added, and for a re-plan what was dropped.

It does not judge the optimiser's choices, which are deterministic. Each finding is a question for the Engineer with a `Proposed answer:` line. The epic waits on it (place 3 of the domain document), and nothing in the graph changes.

**D22. The checker cannot change the graph, and zaf checks that it did not (Q13, Q18; a one-way door).**

- *Refused:* `plan file`, `board file` and `epic file`, to the challenger. The MCP refuses them by role (a new kind of row, I10). Each verb also refuses them itself when `ZAF_INSTANCE` names a challenger, since the MCP is not a boundary (0.5.5 D21).
- *No shell:* the challenger's agent file disallows Bash, WebFetch and WebSearch, so zaf's MCP tools are its only way to act (Q18).
- *Hashed:* the spawner writes the graph's hash into the challenger's launch record (`launch.json`, I14). When the challenger reports on its epic, `board report` reads the hash again. If it is equal, zaf appends `graph sha256:<hash>` as the report's last line. If not, the report is refused and zaf raises a question on the epic naming both hashes. That is O7's "a difference fails the check".
- *Derived:* an epic is challenged while a challenger's report carries the hash of the graph as it reads now (Q13). An older epic is challenged once any challenger report is on it, as today (`adapters/notes.rs:260`). The challenger runs once per graph: it is launched when no earlier challenger's launch record holds the current hash.
- *What waits:* today a ticket of an unchallenged epic waits whenever it is not running (`domain/schedule/place.rs:104`). For a challenge that a change voided, only the tickets with no phase started wait. A ticket in its phases goes on. An unsigned epic still stops every ticket.

### Seeing it (O6, O8, O3)

**D23. The chair draws the graph (S5).** `d` draws the selected epic's graph in place of the tree (`cli/chair/panes/detail/tree.rs`, which draws each ticket only under its first blocker). There is one row per layer group. Each ticket shows its square, id and title, and each of its waits, so no edge is hidden. `v` flips between after (tickets, with their states) and before (the records' nodes, no states). The header says, for example, `chain 3 · width 2 3 1 · critical 5: I@1004.1 → C → D · shape: pass`. The critical path's nodes are marked ★. An older epic shows one view, "planned before 0.6.0". The palette lists `v` (`cli/chair/palette.rs:70`).

**D24. The verbs behind it.** `zaf plan show <epic> [--before]` prints the same layers, measures and critical path, and `--json` the graph. `zaf plan verify <epic>` is O3's check, pass or fail: a pass prints nothing and exits 0, and each failure is one line and exit 1. It checks O3 as signed:

- each wait on a name with a signature goes through an interface, kept waits included (D13);
- every other wait is on a name with no signature, or is a turn;
- no body node passes the limit;
- there is no cycle;
- no two writers of one area are unordered;
- f on the graph changes nothing, ids included;
- the graph's hash equals the last filing mark's ("as zaf filed it").

### Checks and measures (O1, O2)

**D25. `zaf epic check <epic>` (O1, objectives Q11; Q23).** It makes two checks and prints only failures:

1. **The behaviours.** The behaviours of the body tickets closed as completed equal those of every plan record, less what was dropped, by key and goal (the reference's `meaning()`). An older epic prints "not planned as a graph".
2. **The Done when lines.** Each `Done when:` line of the signed epic runs the tests it names in backticks through the manifest's `[gate] tests` template, as `zaf check` runs a task's tests (`app/check/task.rs:1-8`). A line that names no test fails as "not checkable". An epic with no `Done when:` line is refused, never passed.

The Engineer's look comes after zaf's checks pass.

**D26. Build times (S8, O2; Q14).** `zaf report --epics` gains these columns: planned count, minutes of work, minutes on questions, and minutes per planned task. `zaf report --times` prints the average and median minutes per planned task in two rows: the epics of a 0.5.x release, and the epics planned as a graph. It says whether the second is lower in each, and names every epic left out, with why. The inputs:

- starts from the phase run folders;
- close times from the board (`EpicTask.closed_at`, `domain/epic.rs:84-86`);
- questions through the board port as it is: `questions_on(issue)` lists an issue's questions open and closed (`ports/board.rs:327`), each with its `opened_at` (`domain/ticket.rs:91-92`), and `standing_at` gives its close (`ports/board.rs:165`, `domain/ticket.rs:196-202`).

Question time is the union of the intervals, so two questions open at once count once. O2 is reported on 0.6.1's epics (objectives Q4).

**D27. O1's run (objectives Q4; Q17).** O1's run is the release's must check, done by hand at the release's close. It is not an epic of 0.6.0. Once E1 to E8 have merged into `feature/0.6.0`, the integration branch is built and installed as a pre-release with its pack. Then:

1. The Engineer picks and signs one epic (Q17), with "Done when" lines that name tests (Q23).
2. It is planned with `plan file`, from its vocabulary block or its plan's declared names (D6), then challenged and built.
3. `zaf plan verify` passes on it.
4. `zaf epic check` passes on it.
5. The Engineer checks the product against the signed epic.

ways' own build is over by then, so replacing the machine's zaf stops nothing.

### Prompts and records

**D28. The pack and the notes, in the same changes.**

- *Planning brief and role* (`zaf/assets/spawn/planning-brief.md`, `plugins/teams/agents/planning.md`): the plan format, names from the vocabulary, `plan check` then `plan file`, and re-plans by D29 in place of "close, rewrite or add". R31 becomes "the skeleton provides the stand-ins its fillers need". `Blocked by`, `--blocked` and `gh issue close` go.
- *Challenger brief and role*: D21 and D22, and the agent file's disallowed tools (Q18).
- *Release role* (`plugins/teams/agents/release.md:110`): fix tasks under an epic planned as a graph go through `plan file`.
- *Designer and the design action*: the vocabulary block in every design, and each epic part's "Done when" naming a test in backticks (Q23).
- *The interface page* (`zaf/assets/interface.md`): the new verbs.
- *Docs:* `docs/zaf-actions.md` gets a section on the plan.
- *zaf's memory* (`zaf/memory/`): the Vocabulary note gains the relationship words and Plan, Node, Behaviour, Name, Area, Interface task, Turn, Kept wait, Chain, Width, Critical path, Plan record, Filing mark and Re-plan. The scheduler note says waits come from the graph, and R31's line gets S11's wording.

Every pack file the planner, challenger, release instance and designer read changes, and so do the action scripts that quote them. dapple and zabo get the changes with the pack.

### Re-plans

**D29. A re-plan adds (Q16).** A re-plan is another `plan file` on a planned epic, launched by hand as today, or by the release instance for its fix tasks. Its input to f is:

- its new tasks;
- the epic's open filed tickets, as fixed nodes with their `Turn` lines;
- the names of done tickets, as existing.

The rules:

- *Fixed means fixed.* A filed ticket is never split, merged, lifted or rewritten ("A ticket is never expanded"). Two filed tickets were settled when filed and are left alone.
- *Turns.* A new task that writes an open filed ticket's area takes turns after it: the `Turn` line goes on the new ticket. They are never merged.
- *A new interface.* The plan's signatures go into `I@<epic>.<k>`, never into a filed interface ticket.
- *No unlifted signature.* A new task may not wait on an open filed task for a name with a signature that no interface holds (`fixed-provider`). The way on: drop and re-add that task if no phase has started, or re-plan once it is done.
- *Drop.* The plan's `Drop:` list names filed tickets with no phase run and no pull request. zaf closes them as not planned once the new tickets are filed, and the record names them. Their names must be provided again by the new tasks if anything still needs them, or the plan is `unprovided`.
- *Checks.* `epic check` compares against every record's plan, less the dropped tickets. `plan verify` runs f on the whole graph read, filed tickets fixed, and the result must equal it.

Fixtures 27 (a re-plan beside filed tickets) and 28 (`fixed-provider`) pin it, and so do the reference's random re-plans: a filed ticket never changes, and a rerun changes nothing.

</details>

<details>
<summary><b>Architecture: modules, interfaces, data shapes</b></summary>

zaf stays ports and adapters. The plan's rules are pure and live in the domain, which imports no outer layer (`tests/architecture.rs:14-26`). The two graph crates are fenced into one domain file, as globset is.

### Modules

| Id | Where | What it owns | New or changed |
|---|---|---|---|
| M1 | `domain/plan.rs` | the plan's types: `NodeKey`, `NodeId`, `Name`, `Area`, `Weight`, `Behaviour`, `Node`, `NodeKind`, `Filed`, `PlanInput`, `Options`, `Order`, `KeptWait`, `Refusal` (newtypes, BTree collections) | new |
| M2 | `domain/plan/graph.rs` | `Dag` (I12): the only file naming petgraph or rustworkx-core | new |
| M3 | `domain/plan/optimise.rs` | f: refuse, split, lift, settle, thin, fixed nodes for re-plans (I4) | new |
| M4 | `domain/plan/measure.rs` | layers, chain, width, work, the critical path (I5) | new |
| M5 | `domain/plan/vocabulary.rs`, `app/release.rs` | the vocabulary block's shape, the name check, the plan's added names; the design's signature through zaf's block or `signoff.md` (D5 to D7) | new, changed |
| M6 | `domain/plan/body.rs`, `domain/task.rs` | the planned ticket body, parsed and rendered; `TaskBody`, planned or template (I2, I7) | new, changed |
| M7 | `domain/plan/read.rs` | the graph read from tickets, old ones too; kept waits by name; the hash (I6) | new |
| M8 | `domain/plan/shape.rs` | O3's check (I13) | new |
| M9 | `domain/plan/record.rs`, `adapters/notes.rs`, `domain/epic.rs` | the plan record and the filing mark (I8); `Epic` gains its records; planned, half filed and challenged derived (D17, D19, D22) | new, changed |
| M10 | `app/plan/` | `check`, `file`, `finish`, `show`, `verify` use cases (D17, D24, D29) | new |
| M11 | `cli/plan.rs`, `cli/epic.rs`, `cli/report.rs` | the verbs (I9) | new, changed |
| M12 | `domain/mcp.rs`, `app/board.rs` | rows by role, the verbs' own refusals, `board file` under a graph epic, the challenger report's hash (D20, D22) | changed |
| M13 | `app/schedule/gather.rs`, `labels.rs`, `ask.rs`, `app/spawn/tick.rs`, `blockers.rs`, `app/hub/state/line.rs`, `app/chair/build.rs`, `domain/schedule/place.rs` | waits from the graph; the dead-wait questions' way on; what a voided challenge holds (D19, D22) | changed |
| M14 | `app/spawn/runs.rs`, `app/spawn/tick.rs` | the challenger's launch record holds the hash; once per graph; the tick finishes a half-filed plan (D17, D22) | changed |
| M15 | `cli/chair/panes/detail/graph.rs` (replaces `tree.rs`), `palette.rs`, `model.rs` | the graph view (D23) | new, replaced |
| M16 | `app/epic/check.rs` | O1's checks (D25) | new |
| M17 | `app/report/times.rs`, `app/report/epics.rs` | S8 (D26) | new, changed |
| M18 | `ports/vcs.rs`, `ports/board.rs`, their adapters | `path_at` (D7); `drop_task` (D29) | changed |
| M19 | `zaf/assets/spawn/*`, `zaf/assets/interface.md`, `plugins/teams/agents/{planning,challenger,release,designer}.md`, `plugins/teams/actions/design` | the prompts and the challenger's tools (D28) | changed |
| M20 | `docs/`, `zaf/memory/` | the records (D28) | changed |
| M21 | `tests/architecture.rs`, `tests/plan_graph.rs`, `tests/data/plan-graph/` | the crate fence and bans; the fixtures as tests; the properties (D8, D16) | new, changed |

### Interfaces

| Id | Interface | Shape |
|---|---|---|
| I1 | **The plan** (external: what a planner writes) | optional `New names:` list (`area <name>`, `signature <name>: <shape>`, `no signature <name>`) and `Drop:` list (`#<n>`), then sections opened by `Task: <title>`: `Node`, optional `Group` and `Type`, the template's fields but `Blocked by`, and `Writes`, `Provides`, `Needs` (D2) |
| I2 | **The ticket body** (external; a one-way door) | ticket lines first: `Node`, `Kind: interface` on the interface ticket, `Turn: after <node> on <area>`, `Waits on: #<n> for <name>` or `epic #<n> for <name>`; then one section per behaviour (`Behaviour: <key>` when more than one), each the template's fields but `Blocked by`, plus `Writes`, `Provides`, `Implements`, `Needs`; the interface ticket's `Signatures:` items `<name>: <shape>` (D3) |
| I3 | **The vocabulary block** (external; a one-way door) | in the release's `design.md`, the fence ```` ```toml vocabulary ````: `[areas]` with `"<area>" = "<note>"`; `[names."<name>"]` with `signature = "<shape>"` or `no_signature = true`, and `epic = <n>` or `existing = "<path>"` (D5) |
| I4 | `optimise` | `optimise(&PlanInput, Options) -> Result<Optimised { nodes, orders, kept }, Refusals>`; `PlanInput` carries the record number, the shared names and the filed nodes; `Options { lift: All \| None, interfaces: Each \| One }`; production is `All`, `One` |
| I5 | `measure` | `measure(&Optimised, lanes) -> Measures { nodes, interfaces, chain, width, narrow_layers, work, critical, critical_path }`, the fixtures' keys |
| I6 | `PlanGraph` | `PlanGraph::read(epic: &Epic, release: &[Epic]) -> PlanGraph { nodes, orders, kept, half_filed }`; `PlanGraph::before(&[PlanRecord])`; `hash() -> GraphHash` |
| I7 | `TaskBody` | `TaskBody::parse(&TicketBody) -> Result<TaskBody, TaskTemplateError>`, `Planned(PlannedTicket)` or `Template(TaskTemplate)`; `size()`, `model()`, `behaviours()`, `acceptance()`, `tests()`, `interfaces()`, read by every caller of `Ticket::template()` today |
| I8 | **The plan record and the filing mark** (external) | two comments zaf marks as its own, as signatures are. The record: `plan record <k>`, `design sha256:<hex>`, `dropped: #<n> …`, and a fenced block holding the whole plan as written. The mark: `plan filed <k>`, `graph sha256:<hex>` |
| I9 | **New verbs** (MCP tools by 0.5.5's D2) | `zaf plan check` (plan in stdin); `zaf plan file <epic>` (plan in stdin; empty stdin finishes the last record); `zaf plan show <epic> [--before]`; `zaf plan verify <epic>`; `zaf epic check <epic>`; `zaf report --times` |
| I10 | Policy rows by role | `Refused::Roles(&[...])` and `Refused::AllInstancesBut(&[...])` beside `Everyone` and `Instance`: `plan file` refused to every instance but planning and release; `board file` and `epic file` to planning and the challenger, each with its own way on; `board file` under a graph epic refused by the verb to every caller |
| I11 | Ports | `Vcs::path_at(root, revision, path) -> Result<bool, PortError>`; `Board::drop_task(task) -> Result<(), PortError>`, closing it as not planned. The question times need no new port (D26) |
| I12 | `Dag` | `Dag::new(&[NodeId], &[(NodeId, NodeId)])`; `cycle() -> Option<Vec<NodeId>>`; `reduction() -> BTreeSet<(NodeId, NodeId)>`; `closure() -> BTreeMap<NodeId, BTreeSet<NodeId>>`; `order() -> Vec<NodeId>` by natural id; `layers() -> Vec<Vec<NodeId>>` |
| I13 | `check_shape` | `check_shape(&PlanGraph, &Vocabulary, &[PlanRecord]) -> Vec<ShapeFailure>` |
| I14 | Launch record | `launch.json` gains `graph`, for a challenger's launch only |

### Data shapes

- **Ticket body (I2).** It gains lines; a body with none still parses as today (D4).
- **Epic comments.** Each plan record and its filing mark (I8), and a last line `graph sha256:<hex>` on the challenger's report. The epic's body is never touched, so its signature holds.
- **Design document.** The vocabulary block (I3).
- **Run folder.** `launch.json` `graph` (I14).
- **No new store.** The graph, the stages and the measures are worked out on every read.

</details>

## One-way doors

From the objectives, each decided:

- **The task's shape on the board: decided by D3 and D5 (Q2, Q3).** Writes, provides, needs and the vocabulary's names go into every ticket filed after 0.6.0. Renaming a word later breaks every stored plan that uses it. Plain body lines keep the shape board-neutral.
- **zaf rewriting what planners filed: decided by D17 and D29 (Q1, Q16).** zaf rewrites the plan before it is filed and never after. A re-plan adds tickets and may drop unstarted ones; it never merges, splits or edits a filed one.
- **The checking model's rights: decided by D22 (Q18).** Its refusals are rows of the policy table, where a model meets zaf, and its agent file has no shell. A row removed or a tool given back later widens every challenger at once. The hash is what makes "CHECKED … not MADE" something zaf checks, whatever path a model takes.
- **Where the graph lives against the signature: decided by D18 (Q1).** In the tickets, read on demand, with the plan as written in a comment. It is never in the epic's body.
- **Merging filed tickets: decided by D17 and D29 (Q1, Q16).** Never; optimising happens before filing.
- **A task shape every board can hold: decided by D3, carried to 0.7.0.** Lines of text fit Forgejo and GitHub. That Jira's description and a zabo card give the same lines back is unverified, and 0.7.0's board strategies check it.

New doors this design opens:

- **The vocabulary block's format (I3)** and **the plan record's and filing mark's format (I8)**: every later zaf reads them back.
- **The new verbs (I9).** By 0.5.5's D2 they are MCP tools, part of the tool contract.
- **Node ids on the board** (`A+B`, `A.a`, `I@<epic>.<k>`, `#<n>`): reruns, re-plans and the before view read them.
- **`board file` refused under a graph epic (Q20).** It takes a verb away from the Engineer.

<details>
<summary><b>Facts checked</b> (read in the code, read at a source, or run here)</summary>

**Read in the code** at `feature/0.5.5@origin` 7a0e213b. The first draft read 4a06d983; `git diff --stat 4a06d983 7a0e213b` touches only profile, setup, update and doctor code and their tests, and no line cited here moved.

| Fact | Where |
|---|---|
| The task template: one `Field: value` per line; `Blocked by` required, issue numbers or `—`; Size `XS` to `E`, a planner files XS, S or M | `domain/task.rs:25-68`, `:82-113`, `:530-645` (`:624` takes `Blocked by`), `:726-742` |
| The planner drafts each task and files it with `board file`, one call a task, `--blocked` when its blocker is open; there is no moment where zaf holds the whole plan | `zaf/assets/spawn/planning-brief.md:14-22`; `plugins/teams/agents/planning.md:120-127`; `app/board.rs:284-305` |
| A re-plan today: "close, rewrite or add tasks; a claimed task is never touched", by hand, closing with `gh issue close` | `plugins/teams/agents/planning.md:101` |
| The release instance files fix tasks "with the board's own tool, in the task template, as the planner files them" | `plugins/teams/agents/release.md:110` |
| R31 and the interface rule run on `Blocked by` | `plugins/teams/agents/planning.md:108-113`; `planning-brief.md:28-32` |
| Forgejo files a task as an issue with `task`, `state/`, `type/`, `group:` labels, and makes the epic depend on it | `adapters/forgejo/board.rs:1-8`, `:340-380` |
| The scheduler's waits come from the `Blocked by:` line; an epic number reads as done once the epic is finished | `app/schedule/gather.rs:728-748`; `app/spawn/blockers.rs:17-32`, `:77-104`; `domain/schedule/place.rs:116-119` |
| Other readers of `Blocked by:` | `app/schedule/labels.rs:285`, `app/spawn/tick.rs:249`, `app/hub/state/line.rs:213`, `app/chair/build.rs:388`, `app/chair.rs:352` |
| The dead-wait and cycle questions tell the Engineer to edit a `Blocked by:` line | `app/schedule/ask.rs:546-561` |
| Twelve callers parse the template (gather, phase, board lint and file, report, ask, brief, hub, check, status) | `app/schedule/gather.rs:721`, `app/spawn/phase.rs:183`, `app/board.rs:87`, `:288`, `app/report/epics.rs:204`, `app/ask/mod.rs:301`, `app/brief/mod.rs:215`, `app/hub/state.rs:905`, `cli/check.rs:156`, `:254`, `app/hub/state/line.rs:210`, `app/status.rs:419` |
| An epic's release is the `Release: <version>` first line of its body | `domain/epic.rs:27-45`, `:221-226` |
| An epic's stage: signed while the signature's hash matches the body; planned once it has tasks; challenged once a challenger's report is on it | `domain/epic.rs:114-126`, `:228-253`; `adapters/notes.rs:251-261` |
| The spawner launches planning for a signed epic with no tasks, once per signature, waits while planning runs, then the challenger once per epic | `app/spawn/tick.rs:152-194` |
| A ticket that is not running and not merged waits on its epic while the epic is unsigned or unchallenged, between phases too | `domain/schedule/place.rs:89-106` |
| The MCP's caller carries its role; the rows refuse by "everyone" or "instance"; no row refuses `board file` | `domain/mcp.rs:16-20`, `:215-222`, `:275-332` |
| The challenger never files, by its brief only; its agent file disallows Agent, Edit, Write and NotebookEdit, not Bash | `zaf/assets/spawn/challenger-brief.md:18-28`; `plugins/teams/agents/challenger.md:6`, `:102` |
| A profile allows read-only shell basics (`cat`, `printenv` among them) and each `[requires]` and gate tool; ways requires `jj`, `node` and `npm`; profiles load no MCP server but zaf's | `app/profile/mod.rs:781-788`, `:890-896`, `:1197-1210`; `.zaf.toml:7-11` |
| The chair's tree draws each ticket under its first blocker inside the epic; `d` opens it | `cli/chair/panes/detail/tree.rs:1-5`, `:85-111`; `cli/chair/model.rs:619`; `cli/chair/palette.rs:70` |
| Phase run folders: `task` written when the run starts, `exit` when it ends; no code removes a folder under `.zaf/phases/` | `app/schedule/record.rs:4-50`, `:64`; a search for removals finds only `app/spawn/runs.rs:222` (a launch's own folder) |
| `report --epics` prints cost per role and median dollars per done task; no time | `app/report/epics.rs:1-14` |
| A question is an issue labelled `question` whose body says `Raised on #<task>`; `questions_on` lists an issue's questions open and closed; a ticket carries `opened_at`, and `standing_at` gives a close time | `adapters/forgejo/board.rs:583-590`; `ports/board.rs:165`, `:327`; `domain/ticket.rs:91-92`, `:196-202`; `adapters/forgejo/rows.rs:31-34` |
| The Board port has no verb that closes a task as not planned | `ports/board.rs` (its operations listed) |
| `zaf check <task>` runs the tests named on `Tests:` through `[gate] tests` | `app/check/task.rs:1-8` |
| The release folder comes from the manifest; release documents carry a signature block; a folder with `signoff.md` was made the old way and zaf leaves it alone | `app/release.rs:41`, `:68-70`; `domain/document_signature.rs:1-13` |
| The domain names only `domain` and `newtype`; a crate can be fenced into one module | `tests/architecture.rs:14-26`, `:296`, `:325-345`, `:350` |
| zaf is edition 2024 with no `rust-version`; CI uses the stable toolchain; neither graph crate is a dependency yet | `zaf/Cargo.toml`; `.forgejo/workflows/zaf.yml:56`, `:85` |

**Seen on this machine, 2026-11-23.**

- There are 1,303 phase run folders under `.zaf/phases/`, from 2026-11-12 on. The local toolchain is rustc 1.99.0.
- `zaf epic list` in dapple and in zabo printed nothing: neither board has an open epic, and no release is due.
- dapple's and zabo's release folders are `.teams/releases/v0.1/`, each with a `signoff.md`. dapple's design is signed there (2026-11-11); zabo's `signoff.md` signs only its objectives, and its design says "**Not signed.**" Neither design has a "Done when" line.

**Read at a source** (a research agent, 2026-11-23, from crates.io's API, docs.rs and the published crate source, never built):

- **petgraph 0.8.3**, published 2025-09-30; `rust-version` 1.64.
  - Default features: `std`, `graphmap`, `stable_graph`, `matrix_graph`. `rayon` is optional and off.
  - Hard dependencies: `fixedbitset`, `hashbrown`, `indexmap`.
  - These exist: `algo::toposort`, `is_cyclic_directed`, `tarjan_scc` (recursive), `kosaraju_scc` (iterative), `has_path_connecting`, and `algo::tred::{dag_to_toposorted_adjacency_list, dag_transitive_reduction_closure}` (for `Graph`, not `StableGraph`). There is no longest-path function.
  - Docs: neighbours of a directed `Graph` are "listed in reverse order of their addition"; "Removing nodes or edges may shift other indices"; `toposort` promises only a topological order, with no tie rule.
  - Sources: https://crates.io/api/v1/crates/petgraph, https://docs.rs/petgraph/0.8.3/petgraph/algo/index.html, https://github.com/petgraph/petgraph/tree/petgraph@v0.8.3.
- **rustworkx-core 0.18.1**, published 2026-07-29; `rust-version` 1.85, edition 2024.
  - No feature flags. Hard dependencies include `petgraph ^0.8`, `rand ^0.10`, `rand_distr`, `rand_pcg`, `rayon`, `rayon-cond`, `ndarray`, `hashbrown`, `indexmap` and `priority-queue`.
  - These exist: `dag_algo::longest_path` (edge weights from a closure), `longest_path_length`, `layers` (the caller gives the first layer), `lexicographical_topological_sort` (a key closure, then the node id, on ties). There is no transitive reduction in this crate; only the Python binding has one. The `dag_algo` functions use no rayon.
  - Read in the source, not run: `longest_path` picks its end through a `hashbrown` map seeded at random, so on ties its path can differ between runs. `lexicographical_topological_sort` with no initial set drops the nodes of a cycle without an error.
  - Sources: https://crates.io/api/v1/crates/rustworkx-core, https://docs.rs/rustworkx-core/0.18.1/rustworkx_core/dag_algo/index.html, https://github.com/Qiskit/rustworkx/blob/0.18.1/rustworkx-core/src/dag_algo.rs.

**Run here.**

- `python3 -m unittest test_reference` in `plan-graph/reference`: 20 tests OK (13 before the design, 16 after its first draft).
- The rerun test passes on every fixture that is not refused, and on 400 random plans of seven kinds in four option sets.
- `replay.py ../real/zaf-0.5.5.json`: the first five rows are as before. Two rows are added, each epic optimised alone. Kept waits on the providing epic's interface give 67 nodes, chain 6 and 6.99 hours at 4 lanes. Kept waits on the providing task give 67, 6 and 6.58, but break O3. With the 17 names needed across epics never lifted (QA's reading, run as a scratch scenario), it is 65 nodes, chain 9 and 7.61 hours.
- 0.5.5 as planned: chain 13, critical 19, path #1012 → #1020 → #1024 → #1029 → #1033 → #1013 → #1019 → #1022 → #1043 → #1047 → #1053 → #1057 → #1058.

**Not verified, and checked by the task that needs it:**

- how long a comment Forgejo, GitHub and Jira accept, which bounds the plan record now that it holds the whole plan (E3 refuses a record over 60,000 characters);
- whether Jira's description and a zabo card give back plain lines unchanged (0.7.0);
- whether `rand 0.10` and `rayon` build for musl and macOS (E1 adds both compile checks, as 0.5.5 did for `rmcp`);
- `hashbrown` 0.15 and 0.16 seeding (only 0.17.2 was read);
- whether the challenger could reach the forge through `node` and the token on disk (Q18: not tried), and whether the instance's environment holds `FORGEJO_TOKEN`;
- that an agent file's `disallowedTools` removes Bash, WebFetch and WebSearch as it removes Edit and Write today (E7 checks it on a spawned challenger);
- which crate or port computes `git hash-object` for `signoff.md` (SHA-1 of git's blob form; E3 picks it);
- whether dapple's `[gate] tests` template runs one named test (O1's run).

</details>

<details>
<summary><b>Amendments to the fixtures (S1)</b></summary>

S1 lets the signed design amend the fixtures. Each amendment below is in `plan-graph/`, and `reference/optimize.py` stays the oracle.

| Amendment | Files | Decision |
|---|---|---|
| Every fixture's `before` and `after` gain `critical_path`, the path's nodes first to last. No other value in fixtures 01 to 18 changed (checked by diff). | `fixtures/01` to `18` (not the refused 07 to 10) | D15 |
| `measure()` names the path, by the tie rule | `reference/optimize.py` `critical_path()` | D15 |
| A merged node splits back into its parts; an id with `+` holds one behaviour per part (`bad-id`) | `optimize.py` `part_ids()`, `check()` | D9 |
| The vocabulary: `vocabulary` and `new_names` give the lists; `unknown-name`, `planner-list` | `optimize.py` `normalise()`, `check_names()` | D5 to D7 |
| Another epic's names: `outside`; the output's `kept`; providing one is `two-providers` | `optimize.py` `check()`, `derive()`, `kept()` | D13 |
| Plans filed with `Blocked by`: `"filed_with": "blocked_by"`, `epics` | `optimize.py` `read_blocked_by()` | D4 |
| Names other epics need: `shared`, lifted even when the plan does not need them | `optimize.py` `lift()` | D13 |
| Re-plans: `record`; `"filed": "open"` nodes fixed, with their `turns`; `"filed": "done"` nodes' names existing; a filed ticket first in a turn, never merged; `fixed-provider`; the interface id `I@<epic>.<record>` | `optimize.py` `normalise()`, `check()`, `split()`, `derive()`, `interface_id()`, `lift()`, `settle()` | D11, D29 |
| New: 19 a rerun keeps ids (fixture 12's output in, the same out) | `fixtures/19-rerun-keeps-ids.json` | D9 |
| New: 20 the critical path's nodes, with a tie before C and a tie of two ends | `fixtures/20-critical-path-nodes.json` | D15 |
| New: 21 `Blocked by` read as needs; R31's skeleton; an epic's tasks kept | `fixtures/21-blocked-by-reads-as-needs.json` | D4, D13 |
| New, amended after QA: 22 a need on another epic's name with a signature, kept on that epic's interface ticket `I@1004.1`, beside a lifted one | `fixtures/22-need-another-epic-provides.json` | D13 |
| New, amended after QA: 23 providing another epic's name is refused (the provider now `I@1004.1`) | `fixtures/23-refuse-providing-another-epics-name.json` | D13 |
| New: 24 an unknown name is refused | `fixtures/24-refuse-unknown-name.json` | D7 |
| New: 25 the vocabulary gives the lists; the plan adds a name | `fixtures/25-vocabulary-gives-the-lists.json` | D5, D6 |
| New: 26 a plan may not add a name the code provides | `fixtures/26-refuse-a-planner-written-existing.json` | D7 |
| New: 27 a re-plan beside filed tickets: a done ticket's name makes no wait, filed tickets go first in turns, a dropped ticket's name provided again, new signatures in `I@1004.2` | `fixtures/27-replan-adds-beside-filed.json` | D29 |
| New: 28 a new task may not wait on an open filed task for an unlifted signature | `fixtures/28-refuse-a-signature-from-an-open-filed-task.json` | D29 |
| New: 29 a name other epics need is lifted though nothing in the plan needs it | `fixtures/29-a-name-other-epics-need-is-lifted.json` | D13 |
| Tests: compare `kept` and the lists; a rerun of every fixture changes nothing; every fixture's kept waits on signatures come from interfaces; random plans of seven kinds; on them, a rerun changes nothing, the critical path is a path of its length, a kept wait on a signature comes from an interface, a filed ticket never changes | `reference/test_reference.py` | D8, D9, D13, D15, D29 |
| The replay: two rows with each epic optimised alone | `reference/replay.py` `per_epic()` | D13, Q6 |

The README's "What always holds" gains four lines, and its "Amended by the 0.6.0 design" section lists these amendments.

</details>

## Epics

| Id | Filed | Epic | Serves | Waits on |
|---|---|---|---|---|
| E1 | #1155 | The optimiser, deterministic | O3, O4, O8, O1 | nothing, first in the lane |
| E2 | #1156 | The ticket's shape and the graph read from the board | O1, O3, O5, O7 | E1 |
| E3 | #1157 | The vocabulary, `plan check`, `plan file` and re-plans | O4, O5, O1 | E2 |
| E4 | #1158 | The scheduler builds the graph | O1, O2, O5 | E2 |
| E5 | #1159 | Seeing the graph: `plan show`, `plan verify`, the chair | O6, O8, O3 | E2, and E3 part 1 (the vocabulary `plan verify` reads) |
| E6 | #1160 | The check after optimising | O7 | E3, E5, and E4 part 4 (the voided state) |
| E7 | #1161 | The pack's prompts and the records | O4, O5, O7 | E3, E5, E6 |
| E8 | #1162 | `epic check` and build times | O1, O2 | E3 |

All eight land in `feature/0.6.0` before O1's run (D27). That run needs every one of them: E5 because the challenger reads `plan show` and the run ends with `plan verify`, and E6 because R15 waits on the challenge. The run itself is not an epic: it is the release's must check, done by hand at the close on the pre-release.

The build itself runs on the installed 0.5.5, with `Blocked by`, so the order guards the release, not the run. Each epic's body on the board carries its parts with a "Done when" each, and points back here by D and Q number.

<details>
<summary><b>Each epic's parts</b></summary>

**E1. The optimiser, deterministic** (D8 to D16, D29, Q4 to Q9, Q15, Q16)

1. *The graph module.* petgraph 0.8 and rustworkx-core 0.18 are added, and `domain/plan/graph.rs` holds `Dag` (I12). Done when: the architecture test fails on any second file that names either crate, and on any file that names `longest_path`, `rand` or `rayon`; the musl and macOS compile checks pass.
2. *The plan's types and its refusals.* Done when: fixtures 07, 08, 09, 10, 23, 24, 26 and 28 give their codes, each naming the node and the name.
3. *Split, lift, settle, thin.* Done when: fixtures 01 to 06, 11 to 19, 21, 22, 25, 27 and 29 give their nodes, waits and kept waits.
4. *Measure and the critical path.* Done when: every fixture's `before` and `after` are equal, fixture 20 included.
5. *Properties.* The reference's random generator is ported, all seven kinds. Done when: the twelve properties of D8 hold on 400 seeded plans in the four option sets.

**E2. The ticket's shape and the graph read from the board** (D3, D4, D18, Q1, Q2, Q11)

1. *The planned body, parsed and rendered.* Done when: rendering every fixture's output nodes and parsing them back gives the same nodes; `board lint` names a bad name and a missing `Size`.
2. *`TaskBody`.* The twelve callers move to it. Done when: a merged ticket's brief lists both behaviours' acceptance, `zaf check` runs both behaviours' tests, and every test of an older body still passes.
3. *The older read.* Done when: fixture 21's tickets, on the fake board, read as fixture 21's graph, and an epic with no task yet stays a wait on itself.
4. *The graph read, the records, the hash.* Done when: fixture 12's output filed on the fake board reads back as fixture 12's graph; editing one body changes the hash; closing a ticket, or planning another epic it waits on, does not.

**E3. The vocabulary, `plan check`, `plan file` and re-plans** (D1, D2, D5 to D7, D11, D13, D17, D20, D29, Q1, Q3, Q10, Q16, Q19 to Q22)

1. *The vocabulary block, the signatures and `path_at`.* Done when: an existing name whose path is gone is refused naming the path; a block in an unsigned design is refused with the way on; a design signed through `signoff.md` is read; a release with no block plans from the plan's declared names.
2. *The plan format and `plan check`.* Done when: the three plans of O4 (a need nobody provides, a cycle, two providers) each exit 1 naming the reason, and the fake board is untouched.
3. *Another epic's names.* Done when: fixtures 22, 23 and 29 hold with the providers filed on the fake board; a name another open epic will provide waits on `epic #<n>`, then reads as a wait on that epic's interface ticket once it is filed.
4. *`plan file` and its finish.* It files the interface ticket from the vocabulary's shapes, and gives each merged ticket its title, size, model, group and type. Done when: two runs on the fake board file each ticket once and post one record and one mark; a run stopped after the record is finished by `plan file` with empty stdin, and by the tick.
5. *Re-plans.* Done when: fixture 27's filed tickets and plan, on the fake board, file `I@1004.2` and the new tickets with their `Turn` lines, close the dropped ticket as not planned, and leave every filed body unchanged.
6. *Who files.* Done when: a planning instance's `board file` is refused with the way on, by the MCP and by the verb; `board file` under a graph epic is refused to an outside caller; the release instance's `plan file` is accepted, and the programmer's refused.

**E4. The scheduler builds the graph** (D19, D22, Q10, Q11, Q13)

1. *Waits from the graph.* Done when: on the fake board, fixture 12's tickets run the interface ticket first and C and D side by side; every scheduler test of an older epic still passes.
2. *Waits that never end.* Done when: a provider closed as not planned, a cycle, and another epic planned without the name each raise one question naming the ticket and the name, with the way on that fits a graph ticket.
3. *Stages.* Done when: an epic whose last record has no mark is not planned, and the chair shows it as half filed with its way on.
4. *A voided challenge.* This part adds the voided state to the epic's gate and what it holds. E6 part 3 works out when an epic is in it, so E4 need not wait on E6. Done when: on an epic built in the voided state, a ticket with no phase started waits on the epic, and a ticket between two phases does not.

**E5. Seeing the graph** (D23, D24, Q9)

1. *`plan show`.* Done when: fixture 20's epic prints `critical 4: A → C → E` and its three layers; `--before` prints the records' nodes.
2. *`plan verify`.* Done when: a fake epic built to break each of O3's checks gets that one failure line and exit 1, a kept wait on a body for a name with a signature among them; a good one prints nothing.
3. *The chair's graph.* Done when: the draw tests show fixture 12's epic before and after, with ★ on the critical path, and the palette lists `v`.

**E6. The check after optimising** (D21, D22, Q12, Q13, Q18)

1. *The checker's rights.* Done when: `plan file`, `board file` and `epic file` are refused to a challenger by the MCP and by each verb.
2. *The hash at the check.* Done when: a fake challenger that files a ticket mid-check gets its report refused, and the epic gets a question naming both hashes.
3. *Challenged, derived.* It puts the epic in E4's voided state while no challenger's report carries the graph's current hash. Done when: editing a ticket's body after the challenge launches a new challenger and holds the unstarted tickets; closing a ticket does neither.

**E7. The pack's prompts and the records** (D28, Q18, Q20, Q23)

1. *Planning brief and role.* Done when: a ways test finds the `plan` tool's `check` and `file` and the re-plan rules in both, and no `Blocked by`, `--blocked` or `gh issue close`.
2. *Challenger brief and role.* Done when: a ways test finds `plan show` and no filing tool in both, and Bash, WebFetch and WebSearch among the agent file's disallowed tools; a spawned challenger's tool list shows no Bash.
3. *Release role.* Done when: a ways test finds `plan file` for fix tasks under a graph epic in `release.md`.
4. *Designer and the design action.* Done when: both ask for the vocabulary block, and for a test named in backticks in each "Done when".
5. *The interface page, docs and memory.* Done when: the interface page lists every verb of I9, and zaf's Vocabulary note has the relationship words.

**E8. `epic check` and build times** (D25, D26, Q14, Q23)

1. *`epic check`.* Done when: a fake epic with one behaviour left unbuilt fails naming it; a dropped ticket's behaviour is not asked for; a line with no test fails as not checkable; an epic with no "Done when" line is refused; a good epic prints nothing.
2. *Times.* Done when: a fake epic with two questions open over the same 30 minutes reports its minutes less 30; an epic with no phase record, and a 0.6.0 epic, are left out and named; `report --times` prints both rows.

</details>

<details>
<summary><b>Risks</b></summary>

- **The vocabulary decides the edges (objectives).** A name spelled two ways now reads as `unknown-name` when the vocabulary is present, which is a clearer refusal. A release with no block falls back to the plan's own names, and loses that guard.
- **No interface task has ever run.** The stand-in approach assumes a language where a signature with an empty body builds. That holds for Rust; for dapple, O1's run finds out. A wrong signature's cost is not modelled.
- **One interface task per plan starts every epic with one task alone.** The critical path weighs it at one per signature, so a big one dominates the path. The simulation says the hours are the same. Lifting names other epics need adds stand-ins nobody in the epic uses.
- **Conflicts are not modelled.** Turns may lengthen real plans beyond the simulation.
- **The epic gates are not modelled.** The replay does not make an epic's interface ticket wait for that epic's signing and challenge, so a wait across epics may cost more than it shows.
- **rustworkx-core's weight.** `rand`, `rayon` and `ndarray` compile into zaf, unused. The fence keeps dropping it to a one-file change (Q15).
- **The tie label.** Between two nodes, one wait stays, labelled with the greatest name, a quirk kept from the reference. The chair shows every name a node needs from another, so nothing is hidden.
- **A planner that stops before its record** has filed nothing, and the spawner does not launch planning twice for one signature. The epic waits for the Engineer, as today.
- **A re-plan can stall on a started provider.** `fixed-provider` holds a new task back until a filed task that has started is done.
- **S8's baseline lives on this machine.** Phase run folders are local and untracked, and epics before 2026-11-12 have none.
- **A challenge voided mid-build (Q13)** holds back the epic's unstarted tickets until the new check ends.
- **`board file` is gone under a graph epic (Q20).** A quick fix by hand now needs a plan.

</details>

## QA challenges

*From a fresh QA read of the first draft, 2026-11-23, relayed by the design session. Claude checked each against the code, the boards and the reference before acting on it; what was checked is listed under "Assumptions checked".*

### To the Engineer

C1. **Re-plans break.** D17 said a re-plan only adds, but the planning role re-plans by "close, rewrite or add" (`plugins/teams/agents/planning.md:101`). (a) Closing a dropped task fails D25's behaviours check for good. (b) The reference gives a re-plan's interface the same id (`I@1004`), so D17 step 3 skips it as filed, the new signatures get no ticket, and D18 reads their needs as provided by code. (c) Filed tickets are fixed, so settle never orders new writers of the same area against them, and `plan verify` fails. Proposal: the interface id is per record (`I@<epic>.<k>`); settle orders new nodes after open filed tickets with a turn, never a merge; a re-plan may add tickets and may drop one no phase has started (zaf closes it, and the record names it dropped, so D25 compares against the records less what was dropped); it never rewrites a filed ticket. A question, with re-plan fixtures and the reference updated.

C2. **The release instance files fix tasks with `board file`** (`plugins/teams/agents/release.md:110`), and D20 and I10 leave it no way. Proposal: the release role may call `plan file`; add `release.md` to D28 and E7.

C3. **O1 has no epic to run on.** dapple and zabo have no open epic; zabo's v0.1 design is unsigned; both sign through `signoff.md` with no zaf block, so D5 refuses them, and adding a vocabulary block would void a signature; neither design has a "Done when" line, and D25 passes zero lines vacuously. Proposal: D5 accepts the project's own signing; a design with no block plans from the plan's declared names; `epic check` refuses an epic with no "Done when" line. Which epic O1 runs on is the Engineer's: a question, pre-filled "a small new dapple epic the Engineer picks and signs at 0.6.0's close".

C4. **Epic order.** E7 waits on E5, D21's challenger reads `plan show`, and E9 part 2 needs `plan verify`, so "E5 can land beside them and is not needed for O1" is wrong. And E9 as a filed epic would get a planner and never finish. Proposal: E1 to E8 all precede the O1 run, and the run is the release's must check, not an epic.

C5. **A half-filed plan cannot be recovered**, since the record is posted last and the spawner will not launch planning again. Proposal: post the plan as written first, mark it complete last; a rerun reads the plan from the record.

C6. **Kept waits against the signed O3.** Fixture 22 keeps a wait on a name with a signature that goes through no interface. Proposal: when the other epic is planned as a graph, the wait is on that epic's interface ticket, so O3 holds as signed; a kept wait enters the hash as written, by name, so another epic's planning never voids this epic's challenge. Amend fixture 22 and the reference.

C7. **O7's "cannot file tasks" holds only through zaf.** The challenger keeps Bash, and the forge token is on disk. Question, pre-filled: the challenger's profile loses Bash (it reads with Read, Grep, Glob and zaf's MCP), and the hash stays as the detection. Check whether the brief needs Bash for anything else.

C8. **Q9 and D15 describe the critical path wrongly.** The code follows the heaviest chain (`optimize.py:358-372`; fixture 20 ends at E). Proposal: it ends where the heaviest chain ends, each step back stays on it, the lower id on ties.

C9. **E1 part 5 and D8 name fewer properties than `test_reference.py` checks (10)**, and the random generator never exercises the vocabulary, `outside`, `Blocked by`, or `one` mode with an epic. Proposal: name them all and widen the generator.

C10. **Calls missing from the list:** D20 (`board file` refused to the Engineer too), D17 (re-plans), D25 (Done-when lines name tests in backticks; the designer prompt changes), D24 (O3's check, as C6 settles it), D11 (the interface ticket's model and size against its weight on the path), D5 (an unsigned design refuses every plan), D26 (the baseline grouped by format while O2 says "0.5.x"; keep 0.6.0's own epics out of the "after", and in the baseline only if built the old way), D19 (`Waits on: epic #n` as a new kind of wait), and the O1 run done by hand. Quote the Engineer's instruction tonight exactly; D1's three-pass quote comes from memory `zaf-epic-design-algorithm.md`, not the objectives.

C11. **Q13's cost.** `place.rs:104` sends every ticket that is not running to wait on its epic, so tickets between phases stop too. Say so.

C12. **Three smaller points.** `app/schedule/ask.rs:548-561` tells the Engineer to edit a `Blocked by:` line: add it to D19 and M13 with a way on that fits graph tickets. Q6's "same hours" comes from lifting across the whole release; with the names needed across epics kept, the replay gives chain 9 and 7.61 hours at 4 lanes against 6 and 6.99. D26 should take the union of overlapping question times. Re-pin the revision read (origin was d15a6778) and check nothing cited moved.

### Assumptions checked

- **C1:** true. The reference's `one` mode named the interface `I@<epic>` (`optimize.py:262`, first draft), and a re-plan reused it; `planning.md:101` re-plans by "close, rewrite or add".
- **C2:** true, `release.md:110`.
- **C3:** true. `zaf epic list` printed nothing in dapple and zabo; zabo's design says "**Not signed.**"; both have `signoff.md`; `rg 'Done when'` finds nothing in either design; zaf leaves `signoff.md` folders alone (`app/release.rs:68-70`).
- **C4:** true. E7 waited on E5, and D21 reads `plan show`.
- **C5:** true. The spawner launches planning once per signature (`app/spawn/tick.rs:152-194`).
- **C6:** true. Fixture 22's `mcp.catalog` had a signature and was kept on `#1012`.
- **C7:** true, with detail. The challenger's agent file disallows Agent, Edit, Write and NotebookEdit only (`challenger.md:6`). Its profile allows `cat`, `printenv` and each `[requires]` tool, `node` among them in ways (`app/profile/mod.rs:781-788`, `:1197-1210`; `.zaf.toml:7-11`). Not tried. Its brief needs no shell: it uses only zaf's tools and reads the pack.
- **C8:** true. The text said "heaviest node"; the code follows the heaviest chain.
- **C9:** true, and there were 11 property tests, not 10; the simulator check is the eleventh.
- **C10:** true. The quote is in this session's brief; D1's quote is in `zaf-epic-design-algorithm.md:15` and not in the objectives.
- **C11:** true, `domain/schedule/place.rs:89-106`.
- **C12:** true. The replay with the 17 names needed across epics never lifted gives 65 nodes, chain 9 and 7.61 hours at 4 lanes. Origin had moved on again, to 7a0e213b; no cited line moved.

## QA resolutions

- **Routed to the Engineer:** C1 → Q16; C3 → Q17 (the Engineer's alone), Q21, Q23; C5 → Q22; C6 → Q19; C7 → Q18; C2 → Q20; C11 → Q13, amended.
- **Resolved in place:**
  - C1: D29 written, D17, D25 and the reference changed, fixtures 27 and 28 added.
  - C2: D20, D28, E7 part 3, and I10.
  - C3: D5, D25, D27.
  - C4: E9 removed; O1's run is D27, after E1 to E8; E6 waits on E5.
  - C5: D17, I8, E3 part 4.
  - C6: D13, D18, D24, fixtures 22 and 23 amended, fixture 29 added, and a test over every fixture and every random plan.
  - C8: Q9, D15 and the reference's docstring.
  - C9: D8 lists twelve properties for Rust, E1 part 5 points at them, and the generator makes seven kinds of plan.
  - C10: calls 6, 10, 14, 16, 17 and 19 to 23 added or widened, the instruction quoted, and D1 cites its source.
  - C11: D22's "What waits", and E4 part 4.
  - C12: D19 and M13 for the questions' way on; Q6's numbers from the replay's new rows; D26's union; the revision re-pinned to 7a0e213b.
- **Rejected, in part:**
  - C6's "kept waits on the interface ticket" alone does not hold O3. If the providing epic's own tasks never need the name, its interface never holds it. So the design adds the rule that an epic lifts every name other epics need (fixture 29).
  - C7 asked that the profile lose Bash. Profiles are shared by roles, and the agent file is where the challenger's tools are already narrowed, so the change goes there. WebFetch and WebSearch go too.
  - C10 asked to keep 0.6.0's own epics in the baseline if built the old way. They were built the old way, but O2 and S8 name the 0.5.x epics, so they stay out of both rows and are named. The Engineer may widen it (Q14).
  - C12: QA's 9 chain and 7.61 hours describe a design where the names needed across epics are never lifted. With Q19's rule, the replay of each epic alone gives chain 6 and 6.99 hours, the same as lifting across the release, so Q6 states both.
