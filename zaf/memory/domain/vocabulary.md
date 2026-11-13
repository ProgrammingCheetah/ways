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

## Observations

- [definition] A ticket is the unit of work and a phase is one step of it #ticket #phase
- [definition] A tick is the next snapshot of the program, not an amount of time #tick
- [naming] "Ticket and Phase" was ruled on 2026-11-12; the code still says "task" for a ticket and "phase" for a step #naming
- [definition] A check is anything that runs outside zaf's slots, such as CI or a review #check

## Relations

- part_of [[zaf overview]]
- used_by [[The scheduler]]
