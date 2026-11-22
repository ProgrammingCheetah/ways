---
title: zaf overview
type: note
tags:
- overview
- start-here
permalink: zaf/zaf-overview
---

# zaf overview

Start here. `zaf` is a Rust CLI that runs a project's work deterministically: models design, implement and review, and `zaf` decides everything between those steps. It began as an action runner (short, fixed verbs so models spend few tokens talking) and became a scheduler.

Two things are true at once, and a reader must keep them apart:

- **What is built** is described in `docs/zaf-actions.md`, the design document of the code as it stands (verbs, actions, the tick, the board and forge backends).
- **Where it is going** is described in `zaf/zuri.domain.md`, the Engineer's own vision, and in the notes linked below. It was ruled on 2026-11-12 and its scheduler was built on 2026-11-12 and 13, in seven slices, on the branch `feature/0.4.0` (not released yet). `docs/zaf-actions.md` §5.1 and §5.2 describe it as built.

## The direction in one paragraph

zaf is a process that stays up and schedules tickets. A ticket is a small state machine: its phases run one after the other. Tickets that can run sit in an ordered lane; tickets that wait sit in an unordered map, under the thing they wait on. A ticket's state is where it sits, never a label. zaf places tickets from facts and writes the board's labels only for people to read.

## Observations

- [purpose] Models do the work inside a phase; zaf decides everything between phases, with no model involved #determinism
- [status] The scheduler direction was ruled on 2026-11-12 and built by 2026-11-13 on feature/0.4.0; not released #status
- [status] Not built from the direction: before-run and after-run actions, one scheduler per machine, planning and challenge as phases #status
- [status] zaf's code, labels and verbs still say "task" for what the direction calls a ticket #naming
- [source] zaf/zuri.domain.md is the Engineer's own document; change it only when they ask #source
- [reason] Every scheduler bug of 2026-11-12 was a state change that no single part owned #why

## Relations

- guided_by [[The principle]]
- explained_by [[Vocabulary]]
- explained_by [[The scheduler]]
- decided_in [[Scheduler rulings]]
- continues_in [[Open decisions]]
- shown_in [[The captain's chair]]
