---
title: Scheduler rulings
type: note
tags:
- decisions
- scheduler
permalink: zaf/decisions/scheduler-rulings
---

# Scheduler rulings

All ruled by the Engineer on 2026-11-12, after the first real runs of two projects found several scheduler bugs in one day.

| # | Ruling | Why |
|---|---|---|
| R1 | The unit is a **Ticket** and its steps are **Phases** | `Ticket` is already the code's type; one word per thing |
| R2 | **Tickets are queued, not phases** | Phases always run one after the other, so pulling one back pulls back the ones after it |
| R3 | **State is structural, not a label** | A label nothing clears leaves a ticket stuck; a place cannot be forgotten |
| R4 | **The lane is an ordered queue; waiting is an unordered map** keyed by what is waited on | A ticket that waits has no turn, so it must not take one or hold the queue |
| R5 | **Tickets are placed from facts; state labels are output only** | Reading a label back makes the label the input again |
| R6 | **Weight is age plus an optional priority** the Engineer sets | Age alone cannot express urgency |
| R7 | **A phase returns one thing when its turn ends**, never calls functions to move itself. Amended by R13: what it returns is a report of facts | A returned value is always exactly one; three callable functions could be called twice or not at all |
| R8 | **No kinds of phase.** A phase with no model returns "wait on the check" at once | The outcome is all the lane needs |
| R9 | **The queue lives in memory and zaf stays up**; facts win on every tick and a restart rebuilds from them | It lets zaf react to events, share slots across projects and be one process with the hub |
| R10 | **A verb still works with no scheduler running** | The scheduler picks the change up from the facts when it next starts |
| R11 | **The principle:** authority goes down, facts come up | One rule to test any design or code question against |

## After the QA review (same day)

A fresh QA instance tested the design against the day's eight real failures and found it ruled out only one in full. The Engineer agreed with all twelve findings.

| # | Ruling | Why |
|---|---|---|
| R12 | **Memory is a cache:** everything the scheduler holds names the fact it is rebuilt from, the current phase included | A restart must lose nothing, and a phase can end while no scheduler runs |
| R13 | **A phase reports facts; the lane works out the outcome** | "Done" from the phase is a verdict, which the principle forbids; an exit 0 after a question was read as done |
| R14 | **A failed check runs its `on_fail` model phase**, and the current phase is the first whose end fact fails at the head | A straight list cannot say "red, fix, check again", and a push after review must be reviewed |
| R15 | **A ticket waits on its epic** while the epic is unsigned or unchallenged | Otherwise planned tickets start before the challenge |
| R16 | **Running comes first in the placement and has its own place** | A ticket both running and questioned would lose its slot and be started twice |
| R17 | **One scheduler under a lock, restarting itself when the binary changes** | A process for the machine's life would otherwise run old code or run twice |
| R18 | **Getting nowhere is what counts**, three times at the same head; machinery failures never count | Done, failed, done, failed never reaches three failures in a row |
| R19 | **Every wait has a deadline; a dead dependency raises a question** | A check that never registers or a ticket closed as not planned would wait forever |
| R20 | **The budget stays as built; a priority is worth a fixed amount of age** | Sizes, the machine budget and per-project slots were dropped without a word; priority must not starve the rest |
| R21 | **The lane enqueues; before-run and after-run actions leave a record, are safe to run twice, and wait on a question when they fail** | A ticket must never place itself or be left with no place |
| R22 | **A ticket sits in exactly one place; events only ask for a tick; a timer asks too** | Two movers could queue a ticket twice; an answer typed into the forge announces nothing |
| R23 | **The tick hooks stay** (`pre-tick`, `post-tick`, `post-phase`) | "They cost nothing and may reveal more complex patterns" |
| R24 | **Verbose: a trace of where everything was.** Every tick appends where every ticket sits and why to `.zaf/ticks.jsonl`, kept 14 days; the tick prints each move; `zaf spawn trace` reads it back. Output only, never read back to decide | "It'll allow us to check where everything was" |
| R25 | **The lowest priority number is the most urgent.** Built as the labels `priority/1` to `priority/3`, 1 the most urgent (2026-11-13) | "It doesn't matter to me, as long as lowest = urgent" |

## Observations

- [decision] Ticket and Phase are the words #R1
- [decision] Tickets are queued, never phases #R2
- [decision] State is where a ticket sits, never a label #R3
- [decision] Labels on the board are output only #R5
- [decision] zaf stays up and keeps the queue in memory #R9
- [decision] Memory is a cache; the current phase is derived from facts #R12
- [decision] A phase reports facts and the lane works out the outcome #R13
- [decision] Running comes first and has its own place #R16
- [decision] The tick hooks stay #R23
- [decision] Every tick leaves a trace of where each ticket sits; it is output only #R24
- [context] The QA review of 2026-11-12 found the first version ruled out one of eight real failures in full #qa
- [context] The bugs that led here: a blocked label nothing cleared, a script's exit code read as "done", a gate present in one scheduler and missing in the other #why

## Relations

- decides [[The scheduler]]
- part_of [[zaf overview]]
- continues_in [[Open decisions]]
