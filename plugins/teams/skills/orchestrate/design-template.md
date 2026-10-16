# Release <version>: <one-line what it delivers>

<date> · <author> · QA: <qa name> · **Draft until the Engineer signs it.**

<!-- One document by default. If [teams.release].documents splits it (for example product, technical
     and delivery documents), each section below goes to the document that owns it. -->

## Summary

<One screen, plain words: what this release delivers, what the Engineer can click or check at the end, the decisions it needs (count, all pre-filled), the biggest risk.>

## Goals and non-goals

## Features

<Per feature: who it's for, what they can do, and "done when" as a check the Engineer could run by hand.>

## Architecture

<Diagrams for approval (Mermaid, about 12 boxes at most, labels of 5 words or fewer): components and seams, one sequence of the main request, state diagrams only for real lifecycles. Pending decisions drawn as pending. Contracts, data shapes and security notes in collapsed detail.>

## Implementation plan

| Package | Team | What | Size (S/M/L) | Depends on |
|---|---|---|---|---|

**Critical path:** <the chain of packages that sets the release's length>

## Timeline: phases and milestones

| Phase | Ends in (a visible milestone, demoed) | Packages |
|---|---|---|

<The first milestone is the smallest increment a user can actually use.>

## Review gates and merge plan

<Which reviews each package gets (QA, security, compliance), and how it reaches the main branch.>

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
