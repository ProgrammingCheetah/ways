---
title: Open decisions
type: note
tags:
- roadmap
- open
permalink: zaf/roadmap/open-decisions
---

# Open decisions

Not ruled as of 2026-11-13. The scheduler direction is built on `feature/0.4.0` (see [[The scheduler]]); these are what it left open.

| Question | Leaning |
|---|---|
| Are planning and the challenge phases in the same queue, an epic being a ticket whose phases are plan and challenge? | Yes: one scheduler, not two launchers |
| Do adjacent model phases still run as one model (today's rule), or does each go to the back of the lane? | Not ruled; the QA review flagged it as dropped silently |
| On Forgejo "closed as not planned" is only a label. May zaf read that one label to tell a done dependency from a dead one? | Yes, as the one exception: it is the forge's missing close reason, not a state zaf wrote |
| How does a phase that comes back from waiting get the answer (the ruling's text, the check's result)? | Less pressing now the current phase is derived; a model phase still needs the ruling in its briefing |
| How long is a wait's deadline, per kind of wait? | Built as 2 hours for a check wait (10 minutes for a head with no check at all, 0.5.1) and 24 hours for an unchallenged epic; waits on a question or an unsigned epic have none. Not ruled by the Engineer |
| One scheduler per machine or per project? | Built per project, because `--version` is per project |
| Should the scheduler hand its lock across a self-restart, closing a window of a few milliseconds? | Left open; needs more unsafe code |
| How do several projects share the machine's slots fairly, beyond each project's own count? | Not discussed |
| When are the code, labels and verbs renamed from "task" to "ticket"? | Not scheduled |

Settled since the first version of this note: a running ticket has its own place (R16), `is_completed` is gone in favour of each phase's end fact (R12), an epic is something a ticket waits on (R15).

## Observations

- [open] Planning and challenge as phases in the same queue #open
- [open] Adjacent model phases as one model #open
- [open] The Forgejo "not planned" label as the one label read #open
- [open] Deadlines per kind of wait #open
- [status] The scheduler is built on feature/0.4.0; these questions remain #status

## Relations

- continues [[Scheduler rulings]]
- part_of [[zaf overview]]
