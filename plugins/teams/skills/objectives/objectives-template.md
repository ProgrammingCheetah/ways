# Release <version> objectives: <one line, in the Engineer's words>

<date> · Interview: <the Orchestrator, or "Claude"> with the Engineer · QA: <qa name> (the top-level QA, fresh) · **Draft until the Engineer signs it.**

<!-- Written by /teams:objectives from the Engineer's answers: their words, lightly edited, never invented.
     Objectives are outcomes (what changes for someone), not features; features go under In scope, each serving an objective.
     The design document builds on this file once it's signed: its goals trace to O1…, its non-goals carry Out over. -->

## Summary

<Three to five lines: what this release is for, how many objectives (musts and shoulds), the constraint that bites hardest, and the open questions (count, all pre-filled).>

## Why this release

<Who it's for, the problem or opportunity, why now.>

## Objectives

| # | Objective (an outcome) | Achieved when (a check the Engineer could run by hand) | Priority |
|---|---|---|---|
| O1 | | | must |

<Priority is `must` (the release doesn't ship without it) or `should` (cut first when time runs short). Order is the order to cut shoulds in, last first.>

## In scope

| # | What | Serves |
|---|---|---|
| S1 | | O1 |

## Out of scope

| What | Why it's out | Later (a release, or never) |
|---|---|---|

## Constraints

<Each as the Engineer stated it: a deadline or timebox, what must not break, technology, compatibility, budget, anything the project is bound by. — none stated.>

## Assumptions and risks

<What this release takes as true, and what could sink it. QA checks the assumptions it can.>

## One-way doors seen

<A security boundary, the shape of stored data, a permission, a published contract this release will touch: named now, decided in the design. — none seen.>

## Questions for the design

<"How" answers and forks the interview raised that belong to the design document's Decisions, parked here, not decided.>

## Questions for the Engineer

**Q1. <question in plain words>**

**Claude's answer (<date>; the Engineer adjusts):** <answer and why>.

**Engineer:**

## QA challenges

## QA resolutions

## Signed

<date · the Engineer's words. Written before the file's id is taken; the id lives in `signoff.md`, never here.>
