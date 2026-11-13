---
title: The scheduler
type: note
tags:
- domain
- scheduler
- queue
permalink: zaf/domain/scheduler
---

# The scheduler

How tickets move. This is the direction ruled on 2026-11-12, after a QA review the same day; see `zaf/zuri.domain.md` for the Engineer's own text and code sketches. Built by 2026-11-13 on `feature/0.4.0`: the pure rules are `zaf/src/domain/schedule/`, the tick and the scheduler process `zaf/src/app/schedule/`. As built there is one scheduler per project (not per machine), a check wait is overdue after 2 hours and an unchallenged epic after 24, and a priority is the label `priority/1` to `priority/3`, 1 the most urgent, each level worth 60 minutes of age.

## One scheduler, always current

zaf is a process that stays up. It holds a lock for its whole life, so a second one says where the first is and exits. Each tick it compares itself with the installed binary and starts again as the new one when nothing is being launched. Across machines, the claim on the board is still the guard. A verb run while no scheduler is up still works by itself.

## Memory is a cache

The queue lives in memory and nowhere else, but everything in it names the fact it is rebuilt from:

| What the scheduler holds | The fact behind it |
|---|---|
| Where a ticket sits | The placement facts below |
| A ticket's current phase | The first of its phases whose end fact does not hold at the pull request's head |
| How a run ended | Its run record: exit code, any question it raised, the head it left |
| How often a phase got nowhere | The run records |
| The order of the lane | Each ticket's ready-since time and priority |

A restart, an upgrade or a crash loses nothing.

## Where a ticket sits is its state

A ticket sits in exactly one place: the first of these that holds.

1. **Running:** a phase process of it is alive. It leaves only when its end is recorded.
2. **Waiting on its epic:** the epic is not signed, or not challenged yet.
3. **Waiting on a question:** one is open on it or on its epic.
4. **Waiting on a ticket:** a dependency is not done (done means closed as completed).
5. **Waiting on a check:** the checks required at its head have not all ended; no check yet counts as not ended.
6. **In the lane:** none of the above.

The lane is an ordered queue. Waiting is an unordered map from what is waited on to tickets. Running is its own place, one entry per slot in use. A completed ticket is in none.

State labels on the board are output only. What the Engineer sets on a ticket (priority, kind) is input and is read.

## The tick

A tick is the next snapshot, not an amount of time. Events (a phase ending, a ruling, a check finishing) only ask for a tick; the tick alone moves tickets. A timer asks too, for what no event announces. A tick that cannot read the facts keeps the last placement and starts nothing. The `pre-tick`, `post-tick` and `post-phase` hooks stay.

## A turn

1. The lane gives a free slot to the ticket in front, by weight and within the budget.
2. The ticket runs its current phase.
3. The phase leaves a **report** of facts: exit code, any question raised, the head. It judges nothing.
4. The lane works out the **outcome** from the report and the facts, in this order:

| If | Outcome |
|---|---|
| A question this run raised is still open | Wait on it, whatever the exit code |
| The phase's end fact holds at the current head | Done |
| The phase started something outside that has not ended | Wait on the check |
| Anything else | Failed |

## Failing

- A failed check runs its `on_fail` phase (a model that fixes), then the check applies again at the new head.
- What counts is getting nowhere: the same phase ending at the same head with the same failure. Three times, and the ticket waits on a question.
- A failure of the machinery (an action that never ran, an unreadable forge) never counts.

## Waits that cannot end

Every wait has a deadline; past it a question is raised on the waiting ticket. A dependency that can never complete (closed as not planned, a cycle, not a ticket) raises a question at once.

## Budget and weight

The budget stays as built: a ticket's size costs units, the machine has a budget, each project has its own slots, and a big ticket passed over for too long holds the line. Weight is age plus an optional priority; a priority is worth a fixed amount of age, so age wins in the end.

## The trace

Every tick appends one line to `.zaf/ticks.jsonl`: where every ticket sits and why, what the tick did and skipped, and which tickets moved. The tick prints each move, and `zaf spawn trace` reads the history back. It is kept for 14 days. Like the labels it is output only: zaf never reads it back to decide.

## Observations

- [rule] Every tick leaves a trace of where each ticket sits and why; output only #trace

- [rule] A ticket's state is where it sits: running, waiting under a reason, or in the lane #structural
- [rule] Tickets are placed from facts, the first that holds; running comes first #facts
- [rule] Memory is a cache: every field names the fact it is rebuilt from #memory
- [rule] A phase leaves a report of facts and the lane works out the outcome #report
- [rule] The current phase is derived at the pull request's head, so a push after review opens the review again #phase
- [rule] Events only ask for a tick; the tick alone moves tickets #tick
- [rule] Three runs that get nowhere at the same head raise a question; machinery failures never count #failures
- [rule] Every wait has a deadline and a dead dependency raises a question #waiting
- [rule] One scheduler per machine, under a lock, restarting itself when the binary changes #process
- [status] Built by 2026-11-13 on feature/0.4.0, in seven slices with a trace; not released #status

## Relations

- follows [[The principle]]
- uses [[Vocabulary]]
- decided_in [[Scheduler rulings]]
- part_of [[zaf overview]]
