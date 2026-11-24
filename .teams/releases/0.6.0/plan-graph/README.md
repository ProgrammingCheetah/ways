# The plan as a graph of tasks: reference, fixtures and a first simulation

2026-11-22 · a draft by Claude on the Engineer's word ("try to make some tests so we can have an initial version implemented in 0.6"). Nothing here is signed. The design talk and its open points are on the page <https://claude.ai/artifact/KPaxz8to7W8rG9A8a2RY8x>.

## What is here

| Path | What it is |
|---|---|
| `fixtures/*.json` | 29 small plans (18 drafted, 11 added by the design), each with its options and its expected result (the rewritten plan and its measures, or the refusal). **These are the tests the Rust implementation must pass**: it loads the same files. |
| `reference/optimize.py` | The algorithm in about 300 lines of plain Python, no dependency. It is the oracle that wrote the fixtures' expected results; each was read by hand before it was kept. |
| `reference/test_reference.py` | Runs the fixtures, then checks the invariants on 400 random plans of seven kinds in four option sets. |
| `reference/simulate.py` | A clock-driven model of a build: lanes, run minutes, a wait for CI and merge. |
| `reference/replay.py` | Runs a real plan through the optimiser and the simulator. |
| `real/zaf-0.5.5.json` | 0.5.5's 59 tasks as its planners wrote them, read from the board. |

Run the tests: `cd reference && python3 -m unittest test_reference`. Run the replay: `python3 replay.py ../real/zaf-0.5.5.json`.

## The input

A plan is a list of nodes. A node is a task; it holds behaviours; a behaviour has a text, a size (1 small, 2 medium), and three lists of names: what it **writes**, what it **provides**, what it **needs**. Nobody writes an edge: the provider of a name comes before every node that needs it. A plan also has a `limit` (the largest a node may be), its `lanes`, the names with `no_signature` (a prompt, a shell action, a doc) and the names `existing` code already provides.

This differs from the first pseudocode, where a planner wrote order edges that carried a "needs". Once every behaviour lists its needs, a written edge says the same thing twice, and two statements of one fact can disagree. So edges are derived.

## The passes

1. **Refuse** a plan with a duplicate id, a node with no behaviour, a behaviour larger than the limit, a name with two providers, a need nobody provides, a behaviour that needs what it provides, or a cycle. Nothing is repaired.
2. **Split** a node into one node per behaviour.
3. **Lift** each needed name that has a signature into an interface node. The provider now needs the name too (it implements the signature). With `interfaces: one` every interface of an epic is one node.
4. **Settle** each pair of nodes that write the same thing and have no order between them: merge them if they are both bodies and fit the limit; otherwise put them in order, the one more nodes wait on first, the lower id on a tie.
5. **Thin**: drop a wait another path implies.
6. **Measure**: layers by longest path, the chain, the width of each layer, the work, the critical path.

## What always holds (tested on random plans)

- The meaning is kept: the behaviours of the body nodes are the same before and after.
- No cycle; every edge has both ends; ids are unique.
- No body node passes the limit.
- The same plan gives the same result however its nodes are listed.
- No two nodes that write the same thing are left unordered.
- No wait is left that another path implies.
- After lifting, a body waits on a body only where the name has no signature or the two took turns on something they both write.
- With no conflicts and every name liftable, the chain is 2.
- A rerun changes nothing, ids included: the optimiser run on its own output gives the same plan (added by the design, D9).
- The critical path is a path of waits whose sizes add up to its length (added by the design, D15).
- A kept wait on a name with a signature comes from an interface (added by the design, D13).
- A re-plan never changes a filed ticket (added by the design, D29).

## Amended by the 0.6.0 design (2026-11-23, unsigned)

`../design.md` changes some answers, as S1 allows; its section "Amendments to the fixtures" is the record. In short:

- **Every fixture's measures** gain `critical_path`: the path's nodes, first to last. Nothing else in fixtures 01 to 18 changed.
- **A merged node splits back into its parts** (`A+B` into `A` and `B`), so a rerun keeps its ids. An id with `+` must hold one behaviour per part (`bad-id`).
- **The vocabulary**: a plan may carry `vocabulary` (areas, names with a signature, names with none, names the code provides) and `new_names` (areas, signature, no_signature). Then the lists come from it, an unknown name is refused (`unknown-name`), and a plan that writes the lists itself, or adds a name the code provides, is refused (`planner-list`).
- **Another epic's names**: `outside` maps a name to the task of another epic that provides it. A need on it makes no edge inside the plan; the output's `kept` lists those waits, never optimised. Providing such a name is `two-providers`.
- **Plans filed with `Blocked by`**: a plan marked `"filed_with": "blocked_by"` reads each line as a need of that task's name `#<id>`, and a line naming an epic (listed in `epics`) as a need of each of its tasks.
- **New fixtures**: 19 (a rerun keeps ids), 20 (the critical path's nodes and its ties), 21 (`Blocked by` read as needs), 22 (a need another epic provides), 23 (providing another epic's name), 24 (an unknown name), 25 (the vocabulary gives the lists), 26 (a plan that adds a name the code provides).

After the design's QA (same day):

- **Another epic's signature is in its interface ticket.** Fixtures 22 and 23 now map `mcp.catalog` to `I@1004.1`, so a kept wait on a name with a signature goes through an interface, as O3 says. A plan's `shared` names (names other epics need) are lifted even when nothing in the plan needs them (fixture 29).
- **Re-plans.** A plan may carry the epic's filed tickets: `"filed": "open"` ones stay fixed (never split, merged or lifted; their `turns` stay waits), `"filed": "done"` ones leave the plan and what they provide counts as existing. A new task and a filed one that write the same area take turns, the filed one first. A new task may not wait on an open filed task for a name with a signature no interface holds (`fixed-provider`). The interface node of the plan's `record` is `I@<epic>.<record>`, so a re-plan's signatures never land in a ticket already filed (fixtures 27, 28).
- **The tests** make random plans of seven kinds (plain, vocabulary, other epics' names, one epic, shared names, `Blocked by`, re-plans) and check two more things on them: a kept wait on a name with a signature comes from an interface, and a filed ticket is never changed. 20 tests.
- **The replay** gains two rows, each epic optimised alone as zaf does it. With kept waits on the providing epic's interface (the design): 67 nodes, chain 6, 6.99 hours at 4 lanes, the same as the whole-release row. Had the 17 names needed across epics never been lifted, the chain would be 9 and the build 7.61 hours at 4 lanes.

## The first simulation: 0.5.5 as it was planned

The simulator's numbers are tonight's: a task held a lane for about 12 minutes, then waited about 38 minutes for CI and its merge, in no lane. Calibration: the simulated build lands its 40th merge at 252 minutes; the real one did at 269.

| Plan | Nodes | Chain | Hours at 4 lanes | Hours at 8 lanes | Lanes busy (of 4) |
|---|---|---|---|---|---|
| As planned | 59 | 13 | 10.9 | 10.9 | 1.1 |
| Code edges lifted, one interface run per epic | 67 | 6 | 7.0 | 5.7 | 1.9 |
| Code edges lifted, an interface node each | 95 | 6 | 6.9 | 5.7 | 2.2 |
| Every edge lifted (upper bound) | 67 | 2 | 4.4 | 3.0 | 3.1 |

What it says:

- **The chain decides the time, not the lanes.** As planned, 13 layers at about 50 minutes each is the whole build; going from 4 lanes to 8 saves nothing.
- **The wait after a run is three times the run.** Every layer pays the 38 minutes once. Shortening the chain and shortening that wait are the two levers; more lanes is not one until the chain is short.
- **Lifting the code edges takes a third off** (10.9 to about 7 hours). What keeps the chain at 6 is the work with no signature: the `bug-report` shell actions and the docs that quote them.

What it does not say:

- **Which edges can really be lifted is assumed.** "Code edges" means every edge whose provider is not in a docs, prompts, roles, shell-action or install group. A model pass over the tasks would say it truly.
- **Conflicts are not modelled**, because the board does not hold what each task writes. Real conflicts would lengthen the lifted plans.
- **Interface runs have never happened.** Their minutes (6 for one signature; 8 plus 1.5 a signature for a batch) are guesses, and a wrong signature's cost is not modelled at all.
- **A blocker on a whole epic** is taken as a need of every task in it.

## Open, the Engineer's

The page lists them. The simulation adds one: since the wait after a run dominates, is the cheaper win to shorten that wait (CI by tree, merges in groups, both already on 0.7.0's list as integrity inputs) before or beside the optimiser?
