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
| R19 | **Every wait has a deadline; a dead dependency raises a question.** A head with no check at all has a deadline of its own, 10 minutes (0.5.1, #144), and its question ends `No check: <head>.`; a ruling on it ends `ci` at that commit only, so the task goes on with no check, and a new head starts the clock again. Pending checks and outside waits keep their 2 hours and `Check deadline: <head>.` | A check that never registers or a ticket closed as not planned would wait forever; and a ruling that changed nothing left a ruled task waiting in silence for good |
| R20 | **The budget stays as built; a priority is worth a fixed amount of age** | Sizes, the machine budget and per-project slots were dropped without a word; priority must not starve the rest |
| R21 | **The lane enqueues; before-run and after-run actions leave a record, are safe to run twice, and wait on a question when they fail** | A ticket must never place itself or be left with no place |
| R22 | **A ticket sits in exactly one place; events only ask for a tick; a timer asks too** | Two movers could queue a ticket twice; an answer typed into the forge announces nothing |
| R23 | **The tick hooks stay** (`pre-tick`, `post-tick`, `post-phase`) | "They cost nothing and may reveal more complex patterns" |
| R24 | **Verbose: a trace of where everything was.** Every tick appends where every ticket sits and why to `.zaf/ticks.jsonl`, kept 14 days; the tick prints each move; `zaf spawn trace` reads it back. Output only, never read back to decide | "It'll allow us to check where everything was" |
| R25 | **The lowest priority number is the most urgent.** Built as the labels `priority/1` to `priority/3`, 1 the most urgent (2026-11-13) | "It doesn't matter to me, as long as lowest = urgent" |
| R26 | **A run that raised a question does not end its phase, whatever its end fact** (2026-11-15). `pr-open` used to hold as soon as the pull request changed a file, so a stand-in pushed before the question counted as done once the question was ruled. The phase runs again with the ruling | Ticket #19: its `implement` asked #71, and after the ruling it went on to `review` and `merge` with a stand-in page |
| R27 | **A ticket's test name is one shell word, and a name no runner knows fails** (2026-11-15). `zaf check` quotes `{name}`; ways' `[gate] tests` is `tools/run-test {name}`, which runs the crate's test of that name, else the page's, and fails when none ran. Named runners in the manifest are for 0.5.0 | A frontend test's name read as a pass having run nothing, and a name of several words broke the line |
| R28 | **Merge when clean** (2026-11-16). A slice whose own checks passed, and into which the base merges with no conflict, is merged with no new round, though the base moved on: `slice-merge` only probes the merge (`merge-base --probe`), pushes nothing, and the forge merges the pull request at the head CI and the review saw. A base that conflicts still starts `fix`. `workspace-open` cuts a new slice from a fetched base. From dapple's first whole run (2026-11-16): 11 of its 16 repeated merge tries were a clean base brought in, pushed and checked again, and one slice opened on a stale base raised a question. Cost, accepted: two tickets each green can break together, seen one step later on the integration branch (R29). |
| R29 | **One merge at a time, and the next waits for the integration branch's checks** (2026-11-16). A ticket about to merge waits (`waiting(base <sha>)`) while the integration branch has a check pending at its head; no check there, every check ended, or a branch that could not be read holds nothing. A tick runs at most one merge that goes through. The wait has the check wait's deadline (R19): a ticket held at its merge for 2 hours raises a question on itself naming the branch, the commit and the checks still pending, and once ruled those checks no longer hold that ticket at that commit. A red branch never holds a merge, since the fix could not land: the scheduler raises one question for each red head on the epic, whose tickets wait on it, and once ruled work goes on though the branch is still red there | On one integration branch every merge moved the base under the other pull requests, and the next merge went in before the last one was checked |
| R30 | **A ticket out of `fix` merges before any other** (2026-11-16). A ticket is fresh from a fix while the head of its open pull request is the one a run of an `on-fail` action left (exit 0, no question), its current phase has not failed there, and it waits on neither the Engineer nor a ticket. Every other ticket about to merge waits on it (`waiting(merge of #12)`); of two fresh tickets the one whose fix ended first goes first, the lower ticket on a tie. The hold ends when it merges, when its phase fails at that head, when a question opens on it, or when its head moves by anything but a fix | A ticket whose conflict `fix` had just resolved was overtaken while its CI ran and conflicted again: three tickets needed up to five merge tries |
| R31 | **A ruling with a gap is refused, and shared files get a skeleton task** (2026-11-16). `zaf board ruling` refuses an empty answer and one with two or more spaces in a row inside a line, writes nothing, and names the column: a shell eats text in backticks and after `$` inside double quotes (ruling #30 on dapple lost both of its names so). `--as-written` sends it as it is; the hub's answer is never checked. The planning instance files one early skeleton for a file three or more tasks would each add lines to: a node whose `Provides` holds the stand-ins its fillers `Need` (0.6.0, S11), so tickets built side by side do not conflict there. |

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
- [decision] A run that raised a question does not end its phase, whatever its end fact #R26
- [decision] A test name is one shell word, and a name no runner knows fails #R27
- [decision] Merge when clean: a base that merges into the slice with no conflict asks for no new round, and a slice opens on a fetched base #R28
- [decision] One merge at a time: the next waits for the integration branch's checks, and a red branch asks on the epic #R29
- [decision] A ticket out of fix merges before any other #R30
- [decision] A ruling with a gap is refused; a skeleton is a node whose `Provides` holds the stand-ins its fillers need #R31
- [decision] Every tick leaves a trace of where each ticket sits; it is output only #R24
- [context] The QA review of 2026-11-12 found the first version ruled out one of eight real failures in full #qa
- [context] The bugs that led here: a blocked label nothing cleared, a script's exit code read as "done", a gate present in one scheduler and missing in the other #why

## Relations

- decides [[The scheduler]]
- part_of [[zaf overview]]
- continues_in [[Open decisions]]
