# Release <version>: <one-line what it delivers>

<date> · <author> · QA: <qa name> (the top-level QA, fresh, not the author) · **Draft until the Engineer signs it.**

<!-- One document by default. If the teams file's `Release › documents` splits it (for example product, technical
     and delivery documents), each section below goes to the document that owns it. -->

## Summary

<One screen, plain words: what this release delivers, what the Engineer can click or check at the end, the decisions it needs (count, all pre-filled), the biggest risk.>

## Goals and non-goals

<Each goal names the objective it serves (O1… in the signed `objectives.md`); every objective has one. Non-goals carry its Out of scope over, plus any the design adds.>

## Features

<Per feature: who it's for, what they can do, and "done when" as a check the Engineer could run by hand.>

## Architecture

<Diagrams for approval (Mermaid, about 12 boxes at most, labels of 5 words or fewer): components and seams, one sequence of the main request, state diagrams only for real lifecycles. Pending decisions drawn as pending. Contracts, data shapes and security notes in collapsed detail.>

## Implementation plan

| Package | Team | What | Size (S/M/L) | Depends on |
|---|---|---|---|---|

**Critical path:** <the chain of packages that sets the release's length>

## Timeline: phases and milestones

| Phase | Ends in (a milestone: reviewed, then demoed) | Packages |
|---|---|---|

<The first milestone is the smallest increment a user can actually use. Keep each timeline phase small enough for one QA to read its new diff whole beside these documents.>

## Review gates and merge plan

<Each slice: the gate and its build lead's critique, plus a security review when it hits a trigger (name those packages) and compliance called early when it hits a compliance trigger. Each milestone: the architect, then QA, on the cumulative diff. Release end: the release reviews, then QA, then one PR into main.>

## One-way doors

## Risks, and what to cut first

## Decisions

**D1. <decision in plain words>**
- A. <option>
- B. <option>

**Claude's answer (<date>; the Engineer adjusts):** <answer and why>.

**Engineer:**

## Parked (later releases)

## QA challenges

## QA resolutions
