---
title: The principle
type: note
tags:
- domain
- principle
- design-rules
permalink: zaf/domain/principle
---

# The principle

**Authority goes down. Facts come up.**

- **Down goes permission to act:** a turn, a slot, a briefing. The level above decides who may do what, and when.
- **Up comes what happened:** a report, stated as facts. Never a verdict on itself.
- **Each level changes only what it owns.** The lane owns positions, a ticket owns its phases, a phase owns its work.

## Three tests for any design or code question

1. **Nobody moves themselves.** A phase does not move its ticket, a ticket does not place itself in the lane, and a model does not manage its own lifecycle.
2. **A report is a fact, not a verdict.** "I raised a question" is a fact. "I'm done" is a verdict. What a fact means is decided one level above where it happened.
3. **What no level can decide keeps going up.** It ends at the Engineer: questions come up, rulings and signatures go down.

## Observations

- [rule] Authority goes down and facts come up #principle
- [rule] Each level changes only what it owns #ownership
- [rule] A phase reports facts (exit code, question raised, head) and the lane works out the outcome; a phase never says "done" #report
- [rule] Signing an epic and ruling a question stay with the Engineer, at the top of the same tree #engineer
- [rule] Lane verbs (`spawn` but `trace`, `board claim`, `board done`, `slice`) are refused to instances in code, by the MCP's policy table: zaf moves tickets, nobody moves themselves #lane
- [example] A script that reported "done" after its programmer had only asked a question broke test 2 (2026-11-12) #bug
- [analogy] React passes props down and bubbles events up; Rust passes ownership down and returns results up #analogy

## Relations

- guides [[The scheduler]]
- part_of [[zaf overview]]
