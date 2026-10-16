# <Slice name>: <one-line what>

<date> · <lead name> (<team> lead) · Release <version>, slice <n>, milestone <m> · **A read-only plan: nothing was changed to write it.**

<!-- Claude Code's plan-mode shape: Context first, one recommended approach, the critical files,
     what to reuse, and how to verify. The teams sections follow it. Keep the plan scannable. -->

## Context

<One screen, plain words: why this slice, what prompted it, the intended outcome, and which increment it is (the first usable one, or what it grows). What the Engineer could click or check when it's done. How many questions this plan builds on pre-filled answers, and anything surprising.>

## Approach

<The one recommended approach. Alternatives appear only inside a question below.>

<Diagrams, when the slice crosses a seam: Mermaid, about 12 boxes at most, labels of 5 words or fewer, pending decisions drawn as pending.>

## Critical files

| Path | Change |
|---|---|

<A change that repeats a pattern across many files: describe the pattern once and list a few representative paths.>

**Reuse:** <existing functions and utilities to build on, each with its path.>

## Steps

| # | Owner | What | Files | Needs |
|---|---|---|---|---|

## Verification

<How to test it end to end: the gate, the test that proves each rule (each must fail when its rule breaks), and what to run or click. Where milestone QA should plant bugs.>

## Questions for this slice

**Q1. <question in plain words>**
- A. <option>
- B. <option>

**Claude's answer (<date>; the Engineer adjusts):** <answer and why, with evidence>. The plan above builds on it.

**Engineer:**

## One-way doors

- <thing that gets costlier the longer it waits, and why. Says whether it waits for the Engineer's explicit review before it's built.>

## Parked (later slices)

- <question> → <the slice that will ask it>

## Implementation calls (overturn any in review)

| # | Call | Why |
|---|---|---|

## Challenges

<The Orchestrator's check against the Groundwork; the security reviewer's, when the slice hits a trigger.>

## Resolutions
