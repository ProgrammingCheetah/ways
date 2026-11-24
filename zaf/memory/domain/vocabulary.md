---
title: Vocabulary
type: note
tags:
- domain
- vocabulary
permalink: zaf/domain/vocabulary
---

# Vocabulary

The words of the scheduler direction, each with one meaning.

| Word | Meaning |
|---|---|
| **Ticket** | A scope-defined unit of work, created from an issue on the project's board. It is what gets queued. It is never expanded: more work is a new ticket |
| **Phase** | One step of a ticket. A ticket's phases always run one after the other; a phase is never queued by itself |
| **Current phase** | The phase a ticket is on. The ticket knows it; the queue does not |
| **Lane** | The ordered queue of tickets that can run: the event loop |
| **Slot** | The right to run one phase now. The lane gives each free slot to the ticket in front |
| **Waiting** | The unordered map of tickets that cannot run, keyed by what each waits on |
| **WaitingOn** | What a ticket waits on: its epic, a question, another ticket, or the checks at its head |
| **Report** | What a phase run leaves behind: facts only (exit code, any question raised, the head), written as its run record |
| **Outcome** | What the lane works out from a report and the facts: done, wait on something, or failed. Never handed up by the phase |
| **End fact** | The fact that says a phase is over, read at the pull request's head: a change is published, the checks are green, the review is posted, the pull request is merged |
| **Running** | The place of a ticket whose phase process is alive; one entry per slot in use |
| **Weight** | What decides a ticket's turn: how long it has been ready, plus its priority when one is set. Computed, never stored |
| **Priority** | Optional, set by the Engineer on a ticket. zaf never changes it |
| **Tick** | Not a length of time: the moment the next snapshot of the program exists |
| **Fact** | Something zaf reads and does not decide: a dependency is done, a question is open, a check has ended, a process is alive |
| **Engineer** | The person. Signs epics and rules questions; nothing else may |
| **Plan** | The epic's tasks as the planner writes them, one section per task, before zaf files anything: the input of `plan check` and `plan file` (D2) |
| **Node** | One task of the graph: the planner's key (`A`), `A+B` for a merge, `A.a` for a part, or `I@<epic>.<k>` for an interface task. It is a ticket once filed (D3) |
| **Behaviour** | One thing a task does. A task is one behaviour; a merged ticket holds one section per behaviour (D2, D3) |
| **Name** | What a task provides or needs, in lowercase letters, digits, `.`, `-`, `/` and `_`. It has a signature or none, and is given by an epic or by existing code (D5) |
| **Area** | A part of the system in words, never a file. A task writes areas, and two tasks that write one area must be ordered or merged (D5, D12) |
| **Interface task** | The one task per plan of an epic that gives the signatures of the names others need: stand-ins that build, with no behaviour. Its id is `I@<epic>.<k>` (D11) |
| **Turn** | The order zaf gives two tasks that write one area and do not merge: the one with more tasks waiting on it goes first, written `Turn: after <node> on <area>` (D12) |
| **Kept wait** | A wait on another epic's name or on a filed task named `#<n>`, never optimised or lifted, written `Waits on:` (D13) |
| **Chain** | The number of layers of the graph, by longest path: how many tasks run one after another at the least (D15) |
| **Width** | The size of a layer: how many tasks can run side by side. Layers narrower than the lanes are the narrow ones (D15) |
| **Critical path** | The heaviest chain of waits, weighed by size, named node by node with its length; every tie goes to the lower id (D15) |
| **Plan record** | The comment zaf posts on the epic before it files: the whole plan as written, its number k, the dropped tickets and the design's hash (D17) |
| **Filing mark** | The comment zaf posts after it files and reads the epic back: the record's number and the graph's hash. A record with no mark is a half-filed plan (D17) |
| **Re-plan** | Another `plan file` on a planned epic. It only adds: a filed ticket is fixed, never split, merged, lifted or rewritten, and a dropped one is closed as not planned (D29) |

## Relationship words

How the nodes of a plan relate. These are zaf's and fixed, not per release (D5).

| Word | Meaning |
|---|---|
| **needs** | An order: a node needs a name, and waits for the node that provides it. The planner writes `Needs`, never an edge |
| **implements** | A node builds the body of a name whose signature an interface task gave. For the graph it reads as a need of that name |
| **turn** | An order between two nodes that write one area, kept when they do not merge (see **Turn**) |
| **waits on** | A kept wait: the ticket waits on another epic's name or on a filed task `#<n>` (see **Kept wait**) |

## Observations

- [definition] A plan is read as a graph: nodes are tasks, and needs, implements, turn and waits on are how they relate #plan #node
- [definition] A ticket is the unit of work and a phase is one step of it #ticket #phase
- [definition] A tick is the next snapshot of the program, not an amount of time #tick
- [naming] "Ticket and Phase" was ruled on 2026-11-12; the code still says "task" for a ticket and "phase" for a step #naming
- [definition] A check is anything that runs outside zaf's slots, such as CI or a review #check

## Relations

- part_of [[zaf overview]]
- used_by [[The scheduler]]
