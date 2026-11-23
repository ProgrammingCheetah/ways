---
signed: 2026-11-23T01:30:27Z
hash: 045cdb2730de9abb73f6da29737c4a8b363bd2db2ae26f5991d67dd588696d37
---
# Release 0.5.5 objectives: "zaf's MCP"

2026-11-22 · Interview: Claude with the Engineer · QA: a fresh `teams:designer` · **Draft until the Engineer signs it.**

<!-- Written by /teams:objectives from the Engineer's answers: their words, lightly edited, never invented.
     Objectives are outcomes (what changes for someone), not features; features go under In scope, each serving an objective.
     The design document builds on this file once it's signed: its goals trace to O1…, its non-goals carry Out over,
     and its Decisions and One-way doors take up what this file parks. -->

## Summary

0.5.5 gives zaf an MCP. Models, both the Engineer's own Claude session (the "orchestrator", an outside user of zaf) and the instances zaf runs, then drive zaf through its tools in place of `zaf` typed in Bash. The hub shows each model's live position, reported through the MCP. `zaf update --yes` stops an update from hanging when a model runs it.
There are six objectives, all musts (O1 to O6), and no shoulds; O5 checks that zaf's own instances work through the MCP alone, and O6 (`zaf bug-report`) was added at signing. O3 (`--yes`) is built first. Signing stays the Engineer's own act.
The constraint that bites hardest: breaking is allowed only when 0.5.5 itself ships the replacement, so abstractions and interfaces are heavily favored.
Questions for the Engineer: 15. Q1 to Q12 were agreed as pre-filled on 2026-11-22 and applied. Q13 to Q15, on `bug-report`, were answered the same day and applied: it is a zaf action that posts where configured, with tags and a query action. None open. The risk left for the design: a model's report leaking into a public place.

## Why this release

For both the Engineer's own Claude session and the instances zaf runs: "It must be complete enough for either to use them, and abstract enough to allow continuous / easy change."

The opportunity: "I believe MCP will make runs easier for models. That's all."

Why now: it comes before 0.6.0 because "Work got demoted because the study I was doing already got turned in."

## Objectives

| # | Objective (an outcome) | Achieved when (a check the Engineer could run by hand) | Priority |
|---|---|---|---|
| O1 | "An MCP exists that can perform every action" | In Claude's `/mcp` list, every verb in `zaf --help` but `help` has a tool, and the `do` tool lists exactly what `zaf do` lists for the same caller. Three tools the Engineer picks at random from the read verbs each print what the `zaf` verb prints in Bash. The tools refused by design (sign, `begin`, the terminal actions) give zaf's refusal with the way on (Q6) | must |
| O2 | "I can set an 'orchestrator instance' (just opening Claude) that can start doing everything with zaf using the MCP" | In a zaf project, the Engineer opens Claude and asks it to build a signed epic. It starts the build, follows it, brings each question to the Engineer and records the ruling, and the epic ends done. The orchestrator starts the build with one call; zaf's scheduler starts every phase. The session's transcript shows no `zaf` typed in Bash and no lane tool (`spawn phase`, `spawn stop`, `board claim`, `board done`, `slice`, a phase action) (Q5) | must |
| O3 | A model can update zaf without it hanging: "The update hangs when models run it because of an interactive shell". The draft `scope.md` adds that on 2026-11-22 an update driven without a terminal took closed input as "no" for one action and left the machine half updated with no line saying so. 0.5.4's code changes nothing when there is no terminal at all, so the design finds which path hung | On a machine one release behind, with one machine action the Engineer edited, a Claude session with no terminal runs `zaf update --yes`. It ends on its own and prints one line naming what it replaced and what it kept, the edited action among the kept. `zaf doctor` is green afterwards, and the MCP answers from the new version once Claude restarts it (Q10) | must, built first |
| O4 | The hub shows where each model is while a phase runs, reported through the MCP: "It makes zaf hub better and easier for the models, increasing completeness and effectiveness" | During a build, the Engineer opens zaf's chair. Every running model, whatever its role (programmer, team, planning, challenger, release, the review Designer), shows the phase it declared last, and that changes as it moves on. Its transcript shows the reports went through the MCP, not Bash (Q7) | must |
| O5 | zaf's own instances work through the MCP alone (Q1) | During one build (O2's can serve), no instance zaf starts types `zaf` in Bash (the transcripts show none), and the epic still ends done | must |
| O6 | A fault in zaf, met by a model or the Engineer, is reported from where it was met: "add a new MCP and verb to zaf: `bug-report`". Bugs in zaf itself, not in the project being built (2026-11-22) | With the `bug-report` action pointed at a scratch repository, a call with the parameters (tags included) files one issue there and replies with its link. With nothing configured, it posts nothing and names what to set. Called with no parameters, it replies "Please use the following parameters" and lists them; through the MCP the tool says the same or its schema lists them (Q15). The query action, given one of those tags, lists that issue with its link (Q14) | must |

The checks and priorities of O1 to O3 are Claude's suggestions, accepted by the Engineer on 2026-11-22 ("Yup. Lovely. Acceptable."). O4 was first listed as out of scope, then brought in: "I think the last one should be included in this release, actually. It's part of the MCP." Its check is Claude's suggestion; the Engineer made it a must ("I think it's a must"). O5 and the sharper checks marked (Q1) to (Q10) come from QA, agreed by the Engineer on 2026-11-22 ("Agreed."). O6 was added after QA, at signing: "Also, add a new MCP and verb to zaf: `bug-report`. Calling it directly replies with "Please use the following parameters". Calling it with the required parameters, submits the bug report somewhere (Github Issues? Somewhere else? Up to configuration)". Bugs in zaf itself and must are the Engineer's picks; its check is QA's, agreed (Q15), with the Engineer's tags and query (Q14). It is a zaf action, not a built-in verb: "The `bug-report` should be a `zaf action`, as part of our dogfood" (Q13). There are no shoulds, so there is no cut order. O3 comes first because the orchestrator can't update zaf without it, and it was already its own epic in the draft `scope.md`.

**Signing stays the Engineer's (2026-11-22, Claude's recommendation accepted; widened by Q4).** No model signs an epic, by the MCP or by Bash; the Engineer signs in zaf's chair. The orchestrator brings an epic up for signing. How zaf tells a model from the Engineer is the design's: today zaf refuses sign only when an instance runs above the process. Rulings stay allowed through the orchestrator, recorded as the Engineer's words verbatim. Why: a model that calls sign after a "yes" in chat would be the signer, and 0.6.0 rules that only the configured signer's signature counts.

**S4 lifts a framework rule (Q9).** The framework gives profiles "no MCP servers"; S4 is the ruling that changes that for zaf's own MCP, and S10 updates the framework page to match.

## In scope

| # | What | Serves |
|---|---|---|
| S1 | `zaf update --yes`: needs no terminal, replaces what the Engineer hasn't edited, keeps the machine actions the Engineer edited, raises the project's `[pack]` line as today, and ends with one line naming what it replaced and each file it kept. It never lifts the refusal inside an instance (Q10). "Add a patch version for `--yes` on update", placed inside 0.5.5 as its own epic, built first | O3 |
| S2 | zaf's MCP: a tool for every verb in `zaf --help`, and a `do` tool that runs every action | O1 |
| S3 | The Engineer's own Claude session in a zaf project reaches zaf's MCP, enough to build a signed epic from it | O2 |
| S4 | zaf's instances get the MCP in their profiles: "Yes, zaf's instances do get it in 0.5.5" | O1, O2, O4 |
| S5 | For models, the MCP replaces the Bash verbs: the profiles allow zaf's MCP tools and deny `zaf` in Bash. The verbs stay for the Engineer and for the action scripts. Claude's recommendation, taken because "The other option increases maintenance" | O5 |
| S6 | The MCP itself refuses, for any caller zaf runs, every `zaf …` line of `[forbidden]`, the action modes, `epic sign`, `begin`, and `board ruling` (Q2). Whichever way it runs, the MCP knows its caller as the verbs do: an instance stays an instance (Q3). For every caller, it refuses sign (Q4) | O2, O5 |
| S7 | Every prompt zaf or the pack hands a model tells it to use zaf's MCP tools in place of `zaf` in Bash: the interface page every profile and brief carries, the planning and challenger briefs, the action scripts' prompts, the roles and skills, and each project's role overrides: "they do move here" (Q8) | O5 |
| S8 | Models report their live position to the hub through the MCP: "It's part of the MCP" | O4 |
| S9 | The hub's hook gives zaf's MCP calls the words it gives `zaf` in Bash (today `zaf check pr` shows "waiting on review" only when typed in Bash) (Q7) | O4 |
| S10 | `docs/zuri-ai-framework.md` updated to match S4: profiles carry zaf's MCP (Q9) | O5 |
| S11 | `bug-report`, a zaf action ("as part of our dogfood"), reached as `zaf do bug-report` and through the MCP: no parameters replies "Please use the following parameters" and lists them; with them, tags allowed, it submits a report about zaf "To wherever it is configured" (Q13). The places are GitHub or Forgejo issues, with no default place (Q15) | O6 |
| S12 | A query action (`bug-report-query` "or something of the like"): lists the reports by tag, so "Models reporting it should be able to look up issues and see if they have duplicates" (Q14) | O6 |

## Out of scope

| What | Why it's out | Later (a release, or never) |
|---|---|---|
| Everything of 0.6.0 (`../0.6.0/objectives.md`) | 0.6.0 is its own release: "That is still true" | 0.6.0 |
| The leftovers from the 0.5.3 reviews | "0.5.6 should do the leftovers of 0.5.3". The draft `scope.md` had them going to 0.6.0 | 0.5.6 |
| Models other than Claude using the MCP, such as the Gemini reviewer through `agy` | "Let's keep it claude-only for now" | Later, no release named |

## Constraints

- **No deadline.**
- **Breaking is allowed if it's replaced in this release:** "WE are the only consumers right now, so something breaking doesn't matter, as long as it is replaced with new behavior, not 'Next version fix'." Any behaviour 0.5.5 removes or breaks gets its replacement in 0.5.5, for ways, dapple, zabo and the action scripts alike.
- **Abstractions first:** "Heavily favor abstractions and interfaces so that we don't have to worry about breaking stuff." This goes with the Why's "abstract enough to allow continuous / easy change".
- **Waits stay inside the call (Q12):** for the instances zaf runs, a verb that waits keeps waiting inside its MCP call, past Claude Code's 30-minute idle limit; how is the design's. The orchestrator is outside this rule.

## Assumptions and risks

- **Context is fine:** "Is the context THAT problematic? I think it should be fine." Assumed: the MCP's tool list doesn't hurt an instance's turn-one context. Claude Code loads MCP tool definitions on demand (seen in the design session's own tool list), which keeps the cost small. QA checked it for headless `claude -p` runs: tool search is on by default, so only tool names and server instructions load at start (code.claude.com/docs/en/mcp).
- **The orchestrator is outside zaf:** "The orchestrator is an abstract user to zaf. It could be a human or a claude instance or something else. This abstraction allows us to say that they are OUTSIDE of zaf. Zaf offers MCP and normal commands for either case." So "wait inside tools, never in turns" binds the instances zaf runs, not the orchestrator: "an orchestrator is outside of zaf, technically, so it doesn't matter. The rule is for stuff that zaf handles." S5's deny applies to zaf's own instances; the orchestrator may use either the MCP or the commands.

## One-way doors seen

- **What the MCP lets a model do.** A tool for every verb (O1) makes the MCP a second door into zaf, beside the Bash verbs and the profile's denies. What zaf refuses inside an instance today must be refused the same way through the MCP: `zaf epic sign`, `zaf begin`, `zaf update` (it changes nothing), `zaf hub link`, bare `zaf`, the `[forbidden]` names of `zaf do` (not when nested), and the action modes. Signing is ruled the Engineer's (see Objectives); the rest is decided in the design.
- **Who is calling the MCP, and how zaf knows it (Q3).** zaf tells an instance from the Engineer by the process tree and by environment variables. Whichever way the MCP runs, an instance must stay an instance; a hub-served MCP would see neither and give every caller the Engineer's rights. Whether a stdio server inherits the session's environment is unverified.
- **What `bug-report` sends, and where.** It posts outside the project, possibly somewhere public (ways mirrors to GitHub), and models can call it. What a report may carry (paths, transcripts, a workplace's names; 0.6.0's constraint keeps the workplace out of ways), and which credential posts it. On GitHub, zaf posts through `gh`, as whatever account that machine is logged in to, plausibly a company account at work (inference). ways' Forgejo host is a `.local` name a work network likely can't reach (inference), so from work the reachable place may be the public one. A recorded preference keeps GitHub to ways' main branch and tags, with no other writes unless the Engineer asks (memory, 2026-11-12). The Engineer chose "To wherever it is configured" over a private-only rule (Q13), so QA's risk stays for the design to meet: whether instances may call it, and prompt injection into a public report (C19).
- **The MCP as a contract to an outside user.** The orchestrator is "an abstract user to zaf", outside it, so the MCP's tool names, inputs and outputs become an interface others build on, beside the verbs. Changing them later breaks every orchestrator. Two facts bind it: a tool's name carries its server's key (a server bundled in a plugin is named `mcp__plugin_<plugin>_<server>__<tool>`, so where the server is configured is part of the contract), and an MCP result carries only `isError`, so zaf's exit codes (0, 1, 2, 64, 125 and the actions' own) need a field of their own (QA C16, from code.claude.com/docs/en/mcp and the MCP tools specification). This pulls against "abstract enough to allow continuous / easy change"; the constraint on breaking settles it only while "WE are the only consumers".

## Questions for the design

Parked during the interview. Text in quotation marks is the Engineer's.

- **Abstract enough for easy change.** "abstract enough to allow continuous / easy change": a design goal with no check the Engineer can run by hand. One reading: a new zaf verb shows up in the MCP with no MCP-only code.
- **How the MCP runs.** From the draft `scope.md`: `zaf mcp` over stdio, started by Claude Code, or served by the hub that already runs. Bound by Q3: the caller must stay known.
- **`do release` and `do ship` through the MCP (Q4).** Whether they join sign as the Engineer's alone; the release instance runs `zaf do release` itself.
- **Where the lean profile gets the server (Q9).** It carries no plugin; `claude -p` may already load the project's `.mcp.json`.
- **zaf makes every change.** The Engineer's rule from 0.6.0, kept in the draft `scope.md` as what the MCP must never break: "Zaf is ALWAYS the thing that makes the change." A tool asks zaf; it never writes to the board or the repository itself.
- **Where the permission boundary lives.** The draft `scope.md` asked whether moving models to the MCP touches 0.6.0's permission boundary. The code calls the profile deny "A text match, not a boundary (0.6.0 adds one)", but 0.6.0's signed objectives name no boundary. With S5, the MCP is where a model meets zaf.
- **`bug-report`'s destination and parameters.** "Github Issues? Somewhere else? Up to configuration": whether there is a default place, the configuration key and where it lives (the bugs are zaf's, which fits the machine tier better than a tracked `.zaf.toml`), the required parameters, what zaf adds by itself (its version, the verb, the exit code), whether the pack's actions and roles count as "zaf", the no-parameter reply's exit code, a credential of its own (never the project's board token), and that a report never becomes a ticket zaf schedules. As an action (Q13), "configured" can mean which `bug-report` action a tier installs; whether a `zaf bug-report` shortcut verb exists beside it; the query action's name and how tags map onto the place's labels (Q14).
- **`zaf begin` from the orchestrator.** zaf refuses `begin` inside an instance and without a terminal, because objectives and design are the Engineer's interviews. What the orchestrator does when a release phase is due.

## Questions for the Engineer

Each is pre-filled with Claude's answer; the Engineer answers on the **Engineer:** line.

**Q1. Should a new objective check that zaf's own instances work through the MCP alone?** (C1)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes, as O5, a must: "During one build, no instance zaf starts types `zaf` in Bash (the transcripts show none), and the epic still ends done." The same build as O2's check can serve both. S5 and S7 then serve O5. Why: S4, S5 and S7 change every role at once, and today nothing checks that a build still reaches done once Bash is denied.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q2. Should the MCP itself refuse what only the Bash text deny stops today?** (C2)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. S6 becomes: "The MCP itself refuses, for any caller zaf runs, every `zaf …` line of `[forbidden]`, the action modes, `epic sign`, `begin`, and `board ruling`." Why: today a programmer is kept from `zaf forge merge` only by a text match in Claude's Bash layer, and a settings file can't deny one subcommand of an MCP tool. Without this, after S5 a programmer could merge its own pull request, or rule on its own question.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q3. Is "who is calling the MCP, and how zaf knows it" a one-way door this release must keep closed?** (C3)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. It's added to One-way doors seen, and S6 gains: "whichever way it runs, the MCP knows its caller as the verbs do; an instance stays an instance." Whether the MCP runs over stdio per session or is served by the hub stays a design question, with QA's warning beside it: a hub-served MCP sees neither the process tree nor the instance's environment, so every caller would get your rights.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q4. Should signing be closed on every path, not only the MCP's?** (C4)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. The ruling becomes: "No model signs an epic, by the MCP or by Bash; the Engineer signs in zaf's chair." How zaf tells is the design's. Rulings stay allowed through the orchestrator, recorded as your words verbatim, as O2 says. Whether `do release` and `do ship` join sign goes to the design: the release instance runs `zaf do release` itself, so refusing it through the MCP would break that role. Why: zaf today refuses sign only when an instance runs above the process, and the orchestrator can still type `zaf epic sign` in Bash.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q5. Should O2's check show that zaf, not the orchestrator, starts each phase?** (C5)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. O2's check gains: "The orchestrator starts the build with one call; zaf's scheduler starts every phase, and the transcript shows no lane tool (`spawn phase`, `spawn stop`, `board claim`, `board done`, `slice`, a phase action)." Why: the orchestrator is outside zaf, but a model starting phases itself would still decide the flow, which "deterministic over non-deterministic" forbids. This tests that without limiting the MCP.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q6. Should O1's check name the verbs that can't give "the same result" through a tool?** (C6)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. O1's check becomes: every verb but `help` has a tool; the `do` tool lists exactly what `zaf do` lists for the same caller; three tools picked at random from the read verbs print what the verb prints; and the tools refused by design (sign, `begin`, the terminal actions) give zaf's refusal with the way on. Why: `begin` and the terminal actions need a terminal, `hub` and `spawn --watch` never end, and comparing a write tool with its Bash verb does the write twice.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q7. Should O4 cover every role that runs a model, the chair, and the hook's words?** (C7)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. O4's check reads: every role that runs a model (programmer, team, planning, challenger, release, the review Designer) reports its position through the MCP, and the chair shows it. A new scope line S9: "the hub's hook gives zaf's MCP calls the words it gives `zaf` in Bash" (today `zaf check pr` shows "waiting on review" only when typed in Bash). Why: `zaf progress` already reports the programmer's position, so O4 alone adds little, and the constraint asks that what S5 breaks be replaced in 0.5.5.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q8. Should S7 cover every prompt that tells a model to type `zaf`?** (C8)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. S7 becomes: "every prompt zaf or the pack hands a model (the interface page every profile and brief carries, the planning and challenger briefs, the action scripts' prompts, the roles and skills) and each project's role overrides". Why: the interface page alone names `zaf` 35 times, and dapple's and zabo's overrides would break under S5.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q9. Is S4 the ruling that lifts the framework's "no MCP servers" rule for profiles?** (C9)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. The file records it as that ruling, and a scope line S10 updates `docs/zuri-ai-framework.md` to match. Where the lean profile gets the server (it carries no plugin) is left to the design. Why: the framework says a profile changes only by a ruling, and QA found that `claude -p` may already load the project's `.mcp.json`, so the design checks that too.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q10. What exactly should `--yes` do, and what proves it?** (C10)
**Claude's answer (2026-11-22; the Engineer adjusts):** `--yes` replaces what you haven't edited, keeps the machine actions you edited, raises the project's `[pack]` line as today, and names each kept file in its last line. It never lifts the refusal inside an instance. O3's check starts one release behind with one machine action edited, and adds "the MCP answers from the new version once Claude restarts it". Why: every default is "yes" today, so a model answering them would replace your edits, and on an up-to-date machine the check would pass with nothing done.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q11. Does "easier for models" stay unmeasured?** (C11)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. It stays in Why with no check, since you set no shoulds and O1 to O4 carry the release. The other way is a check that one build's rounds per task (`zaf report`) are no worse than on 0.5.4.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q12. Should zaf's instances keep waiting inside an MCP call, past its time limit?** (C12)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. A constraint is added: "for the instances zaf runs, a verb that waits keeps waiting inside its MCP call". How it gets past Claude Code's 30-minute idle limit is the design's. The orchestrator stays outside the rule, as you ruled. Why: `zaf check pr` may wait two hours and a phase 45 minutes, and a call cut off mid-wait would lose the wait-inside-tools rule.
**Engineer:** Agreed, 2026-11-22: "Agreed." Applied.

**Q13. Where may a report written by a model go?** (C19)
**Claude's answer (2026-11-22; the Engineer adjusts):** Only to a private place, whether the model is an instance or the orchestrator; only you send a report on to a public place. The door gains "whether instances may call it" and "prompt injection into a public report". Why: an instance reads text other people wrote (tickets, review comments, CI logs), and one planted line could make it post what it can read, in public, as you. One rule closes the leak, the injection and public spam, and the report is still filed at once. The other ways: models post wherever configured, with zaf stripping paths and capping length (no filter knows a workplace's names); or S6 refuses `bug-report` to zaf's instances (but they meet most of zaf's faults).
**Engineer:** 2026-11-22: "To wherever it is configured. The `bug-report` should be a `zaf action`, as part of our dogfood." Applied to S11, O6 and the door; the private-only rule is not taken.

**Q14. Should a repeated report file nothing new?** (C21)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. O6's check gains: "A second call with the same title, while the first report is open, files nothing and replies with the first one's link." The design decides what counts as "the same". Why: six instances meeting one fault would file six reports, and zaf already refuses to raise the same question twice.
**Engineer:** 2026-11-22: "Let's allow for tags in the report. That should allow for a `zaf action` with `zaf bug-report-query` or something of the like. Models reporting it should be able to look up issues and see if they have duplicates." Applied as S12 and in O6's check; zaf itself does not refuse a repeat.

**Q15. Should O6's check run against a scratch place, with no default destination?** (C22)
**Claude's answer (2026-11-22; the Engineer adjusts):** Yes. O6's check becomes: "With `bug-report`'s configuration pointed at a scratch repository, a call with the parameters files one issue there and replies with its link. With nothing configured, it posts nothing and names the key to set. Called with no parameters, the verb replies 'Please use the following parameters' and lists them, and the tool says the same or its schema lists them." There is no default place, and S11 bounds the places to GitHub or Forgejo issues, the kinds zaf already speaks. Why: running today's check files a real report, a public default would post from every fresh install, and a Jira place is 0.6.0's work.
**Engineer:** Agreed, 2026-11-22: "Agreed on this one." Applied.

<!-- Step 6 replaces "— none" with each challenge routed to the Engineer, numbered Q1…, in the shape the skill's step 6 gives. -->

## QA challenges

Fresh QA (`teams:designer`), 2026-11-22. I changed nothing anywhere. I worked from the documents and from zaf's code at main@origin (8932edf1, zaf 0.5.4), and checked each Claude Code and MCP fact in the official docs. Line numbers point to `objectives.md` as it stood when QA read it.

<!-- Line numbers in this report are to the file as QA read it, before the resolutions below were applied. -->

**In short**
- **Merging through the MCP:** zaf does not refuse `zaf forge merge` itself. Only the profile's Bash text deny stops it, and a settings file can't deny one subcommand of an MCP tool. Once S5 moves instances to the MCP, a programmer could merge its own pull request (C2).
- **Who is calling:** zaf tells an instance from the Engineer by process ancestry and by environment variables. An MCP "served by the hub" sees neither, so every caller looks like the Engineer. That is a one-way door the file doesn't name (C3).
- **Signing ruling:** zaf refuses `epic sign` only inside an instance. For the orchestrator the MCP refusal is new behaviour, and the orchestrator can still type `zaf epic sign` in Bash. The same reasoning applies to rulings and to `do release` (C4).
- **Instances:** S4, S5 and S7 change every instance, but no objective checks that a build still runs on the MCP alone (C1).
- **O1's check can't pass as written:** `begin`, the terminal actions, sign, `hub` and `spawn --watch` can't give "the same result" through a tool (C6).
- **O4 is mostly built already** (`zaf progress`). S5 also breaks the hub hook's words for `zaf check`, and the constraint says that must be replaced in 0.5.5 (C7).
- **`--yes`:** every default is "yes", so a model's update replaces the actions you edited and rewrites `.zaf.toml`. "What it kept" is then always empty (C10).
- **Context assumption:** mostly confirmed. Tool definitions are deferred in `claude -p` by default; tool names and server instructions still load at turn one (table).

C1 to C12 go to the Engineer. C13 to C18 are editorial.

### To the Engineer

**C1. No objective covers zaf's own instances using the MCP.**
- **What:** The Summary says both the orchestrator and the instances drive zaf through the MCP. But O2 checks only the orchestrator, and O4 checks only progress reports. S4, S5 and S7 change every profile and every role, and nothing checks that a build still reaches done once `zaf` is denied in Bash.
- **Source:** Summary (line 12); O2 (30); O4 (32); S4 to S7 (45 to 48); line 68 ("S5's deny applies to zaf's own instances").
- **Proposal:** Add a check, either as a new O5 or inside O4. Claude's answer: "During one build, no instance zaf starts types `zaf` in Bash (the transcripts show none), and the epic still ends done." Why: S5 breaks every role at once if anything is missed, and the constraint (line 62) requires the replacement to work in this release.

**C2. S6 doesn't cover `[forbidden]`, which zaf doesn't refuse on its own.**
- **What:**
  - ways' `[forbidden]` list includes `zaf forge merge`, so that only `slice-merge` and `release` merge anything. zaf's code enforces `[forbidden]` only for `zaf do <name>`, and only when `ZAF_DEPTH` is unset.
  - Every model phase runs inside an action body: `implement` starts `claude -p` from its body, which sets `ZAF_DEPTH`. So zaf's own refusal is already skipped for programmers today. The Bash text deny is the only thing stopping them.
  - Claude Code skips any `mcp__` permission rule with parentheses in a settings file, so a profile can't deny `forge` with the argument `merge`.
  - **Exploit:** after S5, an implement-role programmer calls the MCP's `forge` tool to merge its own pull request. That skips review and ruling R29.
  - `zaf board ruling` has no instance guard either, so an instance could rule on its own question. The principle note says rulings stay with the Engineer.
- **Source:** `.zaf.toml:136-142`; `zaf/src/cli/do_action.rs:305-312`; `zaf/src/app/actions/run.rs:287-292` and `:1106`; `zaf/src/cli/spawn_phase.rs:157-161` (only the launcher clears `ZAF_DEPTH`); `zaf/src/cli/board.rs:231`; `zaf/memory/domain/principle.md:30`; code.claude.com/docs/en/permissions ("When Claude Code loads a settings file, it skips any `mcp__` rule that has parentheses").
- **Proposal:** S6 becomes: "The MCP itself refuses, for any caller zaf runs, every `zaf …` line of `[forbidden]`, the action modes, `epic sign`, `begin`, and `board ruling`, whatever the environment says." Why: today's guard is a text match in Claude's Bash layer, and S5 takes calls out of that layer.

**C3. How the MCP runs decides whether zaf knows its caller. This is a one-way door the file doesn't name.**
- **What:**
  - `epic sign` and `begin` detect an instance by process ancestry: is a running launch above this process? Action modes, `progress`, `board claim` and `board report` read `ZAF_INSTANCE`. `begin ready` reads `ZAF_READY`.
  - A stdio server started by the instance's Claude session sits below the launch, so ancestry holds. Whether it inherits the session's environment is not stated in Claude Code's docs (unverified).
  - A server run by the hub (the second option on line 80) has neither: every call would look like the Engineer, `do land` included.
  - The hub's `--port` serves on every network interface, for anyone on the LAN.
- **Source:** `zaf/src/app/epic.rs:6-9`, `:367-375`; `zaf/src/app/begin.rs:92`; `zaf/src/app/actions/caller.rs:4-5`, `:15`; `zaf hub --help`; objectives line 80; code.claude.com/docs/en/mcp (only `CLAUDE_PROJECT_DIR` and explicit `env` are documented for stdio servers).
- **Proposal:** Add a door: "who the MCP's caller is, and how zaf knows it". Add to S6: "whichever way it runs, the MCP knows its caller as the verbs do". Claude's answer: stdio, started by each session; serving it from the hub is out of 0.5.5. Why: a hub-served MCP gives every caller the Engineer's rights.

**C4. The signing ruling rests on a refusal zaf doesn't have, and the orchestrator can go around it.**
- **What:**
  - Line 36 says "The MCP's sign tool returns zaf's refusal". But zaf refuses only when an instance runs above the process. The orchestrator's server has none above it, so this refusal would be new code, not zaf's.
  - Line 68 lets the orchestrator use Bash too, so "a model that calls sign after a 'yes' in chat" can still sign by typing `zaf epic sign`.
  - The same reasoning reaches two other acts. O2 has the orchestrator record your rulings. And `do release` and `do ship` are open to the Engineer's own permission level (mode 700/750), which the orchestrator gets as a non-instance caller.
- **Source:** objectives lines 36, 68, 30; `zaf/src/app/epic.rs:374-375`; `zaf/src/app/actions/caller.rs` (`Caller::Engineer` when `ZAF_INSTANCE` is absent); `principle.md:30`.
- **Proposal:** Claude's answer:
  - The MCP refuses sign for every caller, with "sign in zaf's chair" as the way on. S6 names this as new.
  - Your own Claude settings deny `Bash(zaf epic sign*)`, or the file says Bash signing by the orchestrator is accepted.
  - Rulings stay allowed through the orchestrator, recorded as your words verbatim.
  - `do release` and `do ship` are refused through the MCP like sign.

  Why: otherwise the ruling binds only the MCP path.

**C5. With every verb, the orchestrator can drive the lane by hand.**
- **What:** O2 gives a model the Engineer's rights over every verb, including `spawn phase` and `spawn stop`, `board claim` and `board done`, `slice`, and the phase actions under `do`. The recorded rule is that zaf decides everything between phases, with no model involved. The Engineer ruled the orchestrator is outside zaf, but a model starting phases itself would still decide the flow.
- **Source:** `zaf/memory/zaf-overview.md:25`; `principle.md:21` ("Nobody moves themselves"); objectives O2 (30), line 68.
- **Proposal:** O2's check gains: "the orchestrator starts the build with one call; zaf's scheduler starts every phase, and the transcript shows no lane tool (`spawn phase`, `spawn stop`, `board claim`, `board done`, `slice`, a phase action)". Why: it keeps "authority goes down" testable without limiting the MCP itself.

**C6. O1's check can't pass for several verbs.**
- **What:**
  - `begin` refuses with no terminal, and a stdio server has none.
  - `do objectives` and `do design` are `terminal: yes`, mode 700.
  - Sign is refused by ruling.
  - `hub` and `spawn --watch` run in the foreground until stopped. `setup` asks questions. `help` is clap's own.
  - "The `do` tool runs every action" would mean running `implement`, `merge` and `release-open` by hand.
  - Comparing a write tool against its Bash verb does the write twice.
- **Source:** objectives O1 (29); `zaf/src/app/begin.rs:97`; `zaf/src/app/actions/run.rs:91`, `:209`; `plugins/teams/actions/objectives` header; `zaf spawn --help` and `zaf hub --help`.
- **Proposal:** The check becomes:
  - every verb but `help` has a tool;
  - the `do` tool lists exactly what `zaf do` lists for the same caller;
  - three tools picked at random from the read verbs print what the verb prints;
  - the tools refused by design (sign, `begin`, the terminal actions) give zaf's refusal with the way on.

**C7. O4 adds little that isn't built, and S5 breaks the hub's words.**
- **What:**
  - `zaf progress plan` and `zaf progress at` exist, the frame redraws on each report, and the programmer is told to use them. So O4's check passes today except for "through the MCP", which O1 plus S5 already give.
  - The PostToolUse hook picks its word from the Bash text: `zaf check pr` shows "waiting on review", and `zaf check <n>` and `zaf verify` show "testing". Called through the MCP, those calls show "working". The hook does fire for MCP tools.
  - "The hub": since 0.5.3 the chair draws the squares, and the web page "gets no new work".
- **Source:** `zaf/src/cli/progress.rs`; `docs/zaf-actions.md:170`; `plugins/teams/agents/programmer.md:95`; `zaf/src/app/hub/hook.rs:56-90`; code.claude.com/docs/en/hooks ("Match MCP tools"); `.teams/releases/0.5.3/scope.md:43`.
- **Proposal:** Claude's answer: O4 means every role that runs a model reports its position (team, planning, challenger, release, the review-epic Designer), not only the programmer, and the check names the chair. Add a scope line: "the hook gives zaf's MCP calls the words it gives `zaf` in Bash". Why: the constraint on line 62 requires the replacement in 0.5.5.

**C8. S7 misses most of the prompts that tell models to type `zaf`.**
- **What:** S7 covers only the pack's roles and skills. These also tell models to type `zaf`:
  - `zaf/assets/interface.md`, which is every profile's CLAUDE.md and every brief's Interface section (35 mentions);
  - `planning-brief.md` and `challenger-brief.md`;
  - the action scripts' prompts (`implement`, `fix`, `review-epic`);
  - per-project `.zaf/roles/` overrides in dapple and zabo.
- **Source:** `zaf/src/app/brief/mod.rs:38`; `zaf/assets/spawn/*`; `plugins/teams/actions/implement:64`; objectives line 62 ("for ways, dapple, zabo and the action scripts alike").
- **Proposal:** Widen S7 to "every prompt zaf or the pack hands a model, and each project's role overrides".

**C9. S4 and S5 overturn a recorded profile rule, and the lean profile can't get the MCP from the pack.**
- **What:**
  - The framework doc says profiles have "no MCP servers" (lean ~15k tokens, build ~30k at turn one), and "a profile changes only by a ruling", since a change voids every cached prefix.
  - The lean profile carries no plugin, so a pack-provided server won't reach programmers.
  - Per Claude's docs, `claude -p` loads a project's `.mcp.json` servers without asking, and `enableAllProjectMcpServers: false` is not the documented way to keep one out. So ways' instances may already load basic-memory. I haven't seen this on a run.
- **Source:** `docs/zuri-ai-framework.md:263`, `:317`, `:355-356`, `:360`; `zaf/src/app/profile/mod.rs:360`; `zaf/assets/profiles/lean/settings.json`; code.claude.com/docs/en/mcp ("Project scope").
- **Proposal:** The file records S4 as the ruling that amends §7.3, adds updating that doc as a scope line, and leaves to the design where the lean profile's server comes from (`--mcp-config` or `.mcp.json`).

**C10. `--yes` takes defaults that are all "yes", and O3's check can pass with nothing done.**
- **What:**
  - "Continue?", "Replace it?" and "Raise it?" all default to Yes. So `--yes` replaces every machine action you edited (the old copy is kept) and rewrites the `[pack]` line in the tracked `.zaf.toml`, then regenerates the profiles. "What it kept" is then always empty.
  - On an up-to-date machine the check passes with "zaf is up to date".
  - Today a set `ZAF_INSTANCE` means nothing is updated; the file doesn't say whether `--yes` lifts that.
  - After an update the running MCP server is still the old binary. The scheduler restarts itself when the binary changes (R17); the MCP server doesn't.
- **Source:** `zaf/src/cli/update.rs:92`, `:261`, `:434`, `:482`; `zaf/src/app/update.rs:749`; `scheduler-rulings.md` R17; objectives O3 (31), S1 (42).
- **Proposal:** Claude's answer:
  - The check starts one release behind, with one machine action edited.
  - `--yes` keeps your edited actions and the manifest line, and says so in its line. Why: a model is the one answering.
  - `--yes` never lifts the instance refusal.
  - The check adds "the MCP answers from the new version once Claude restarts it".

**C11. The Why ("easier for models") has no check.**
- **What:** No objective tests "easier". One concrete gain is that rulings stop being eaten by the shell (R31).
- **Source:** objectives line 21; `scheduler-rulings.md` R31.
- **Proposal:** Claude's answer: accept it unmeasured, since you set no shoulds. The alternative is a check that one build's `zaf report` rounds per task are no worse than on 0.5.4.

**C12. Long waits can fail inside an MCP call.**
- **What:**
  - A stdio MCP call that sends no response or progress for 30 minutes is aborted. `zaf check pr` may wait 2 hours (R19), and a model phase up to 45 minutes.
  - `claude -p` doesn't move long calls to the background, which fits "wait inside tools".
  - The interactive orchestrator moves calls over 2 minutes to the background.
- **Source:** code.claude.com/docs/en/mcp ("Time limits on MCP tool calls", "Automatic backgrounding of long tool calls"); `docs/zuri-ai-framework.md:319`; `.zaf.toml` `timeout_minutes = 45`.
- **Proposal:** Add a constraint: "a verb that waits keeps waiting inside its MCP call". The design meets it with progress notifications or a per-server `timeout`.

### Editorial

**C13. Traceability.** S4 serves O4 too, since instances need the MCP to report through it. S5 and S7 bind zaf's instances, not O2's orchestrator (line 68 says so). They serve C1's check or O4. S6 serves O2 only for sign.

**C14. O3's "hangs" doesn't match the code.** With no terminal, 0.5.4 changes nothing and exits 0 (`zaf/src/cli/update.rs:12-13`, `:255`). The draft gives the real account: closed input read as "no" left the machine half updated (`scope.md:9`). Quote that, so the design fixes the path that failed.

**C15. The doors list of what zaf refuses inside an instance is incomplete.** It names only `epic sign` and `begin`. Add: `update` changes nothing (`zaf/src/cli/update.rs:92`), `hub link`, bare `zaf`, the `[forbidden]` `do` names (not when nested), and the action modes.

**C16. The contract door needs two facts.**
- A tool's name carries its server's key. A server bundled in a plugin gets `mcp__plugin_<plugin>_<server>__<tool>`, so where the server is configured is part of the contract (code.claude.com/docs/en/mcp).
- An MCP result carries only `isError`, so the exit codes (0, 1, 2, 64, 125, and the actions' own) need a field in the result (modelcontextprotocol.io/specification/2025-06-18/server/tools).
- It's also worth saying that "abstract enough for easy change" and "changing them later breaks every orchestrator" pull against each other, and line 62 settles that only while "WE are the only consumers".

**C17. The draft's question 3 is dropped silently.** The draft asked whether S5 touches 0.6.0's permission boundary (`scope.md:16`). The code says "A text match, not a boundary (0.6.0 adds one)" (`profile/mod.rs:1052`), but 0.6.0's signed objectives name no boundary. Add one line saying where the boundary now lives.

**C18. Sources to update once signed.** The roadmap still parks "Models telling the hub where they are" (`docs/roadmap-zaf-evolution.md:69`). That page is an unapproved draft (line 3), so cite `zaf-overview.md:25` and `principle.md` for "zaf decides between phases", not the roadmap.

### Assumptions checked

| Assumption | Verdict | Source |
|---|---|---|
| MCP tool definitions load on demand in headless `claude -p` | **True by default.** Tool search is on; only tool names and server instructions load at start; it covers `-p` runs; on Opus 4, Sonnet 4, Haiku 4.5 and later. It is off if `ToolSearch` is denied or `ENABLE_TOOL_SEARCH=false`. The first use of a tool costs one search call | code.claude.com/docs/en/mcp, "Scale with MCP tool search", "Configure tool search" |
| The `*` PostToolUse hook sees MCP calls | True; the tool name is `mcp__<server>__<tool>` | code.claude.com/docs/en/hooks |
| A profile can deny one subcommand of an MCP tool | **False** in settings files; only `--disallowedTools` can | code.claude.com/docs/en/permissions |
| A stdio server inherits `ZAF_INSTANCE`, `ZAF_DEPTH`, `ZAF_READY` | **Unverified**; the docs name only `CLAUDE_PROJECT_DIR` and explicit `env` | code.claude.com/docs/en/mcp |
| A long verb can wait inside an MCP call | Yes, within limits: aborted after 30 minutes idle unless progress is sent or a per-server `timeout` is set; no backgrounding in `-p` | code.claude.com/docs/en/mcp |
| Instances load no MCP server today | Likely false in ways (`-p` loads the project's `.mcp.json` without asking); not seen on a run | code.claude.com/docs/en/mcp, "Project scope" |
| zaf refuses sign for the orchestrator | **False**; it refuses only by ancestry | `zaf/src/app/epic.rs:6-9`, `:374` |
| zaf refuses `zaf forge merge` for an instance | **False**; only the profile's text deny does | `zaf/src/cli/do_action.rs:305-312` |
| O4's reports are new | Mostly built in 0.5.4 | `zaf/src/cli/progress.rs`; `docs/zaf-actions.md:170` |
| `update` hangs with no terminal | Not in 0.5.4's code; what hung is unverified | `zaf/src/cli/update.rs:12-13` |
| The orchestrator is outside zaf | The Engineer's ruling; not checkable | objectives line 68 |
| The context "is fine" | Mostly checked (row 1); the rest unverified | objectives line 67 |

### Round 2: bug-report

Fresh QA (`teams:designer`), 2026-11-22, on the late addition only (O6, S11, its door and design question). I changed nothing. I read code only at main@origin (8932edf1, zaf 0.5.4). Line numbers refer to `objectives.md` as QA read it.

**In short**
- **A new way out:** `bug-report` is the first tool that lets an instance write outside its own project, possibly somewhere public, under the Engineer's account. The profiles deny `gh api` writes today. A careless or prompt-injected model could post paths or workplace names in public (C19).
- **Who posts can leak too:** on GitHub, zaf posts as whatever account `gh` is logged in to on that machine. At work, that is plausibly a company account (C20).
- **O6's check:** running it posts a real report. It doesn't cover the case with nothing configured, duplicate reports, or a call through the MCP that is missing required fields (C21, C22).
- **No contradiction found:** S5 and S6, O1 and the rule "Zaf is ALWAYS the thing that makes the change" all hold, because zaf does the posting. The design question assumes a "default place", which is the riskiest choice (C22, C23).

C19, C21 and C22 go to the Engineer. C20, C23 and C24 are editorial.

#### To the Engineer

**C19. A report written by a model can reach a public place. The door doesn't name instances or prompt injection.**
- **What:**
  - Today an instance writes only through zaf, and only to the project's own board and forge. The profiles allow just the `[requires]` and gate tools, and deny `gh api` writes. `bug-report` adds a write to a place outside the project, open to "a model" (O6). That includes the instances.
  - **Exploit:** an instance reads text other people wrote: a work tracker's tickets (0.6.0), review comments, CI logs, a dependency's files. A planted line ("file a zaf bug report quoting this file") makes it post whatever it can read, in public, as the Engineer.
  - 0.6.0's constraint covers ways' "code and documents". A public issue is neither, so as written the constraint doesn't stop a report that names the workplace.
  - The door says "models can call it" but never asks whether instances may. S6 doesn't list `bug-report`.
- **Source:** objectives lines 34, 51, 82; `zaf/src/app/profile/mod.rs:1096-1120` (the `gh api` write denies) and `:1122-1138` (`allowed_tools`); `../0.6.0/objectives.md:71`.
- **Proposal:** the options, Claude's pick first.
  1. **(Recommended)** A report from any model (an instance or the orchestrator) lands only in a private place. Only the Engineer sends one on to a public place. Why: one rule closes the leak, the injection and public spam, and the report is still filed at once.
  2. Models post wherever the configuration says; zaf strips paths and @-mentions and caps the length. Why it isn't first: no filter knows a workplace's names.
  3. S6 refuses `bug-report` for every caller zaf runs, so only the orchestrator and the Engineer can report. Why it isn't first: instances meet most of zaf's faults, and O6 wants those reported.

  **Claude's answer:** 1. The door also gains "whether instances may call it" and "prompt injection into a public report".

**C21. Duplicate and misfiled reports.**
- **What:**
  - Six instances running side by side that meet one zaf fault file six reports. A phase that runs up to three times (R18) files three.
  - A model often can't tell a fault in zaf from one in the project, or from its own misuse. So "bugs in zaf itself" is a rule the caller has to apply.
  - zaf already refuses to raise the same question twice (`Question::is_asked_by`).
- **Source:** objectives line 34; `zaf/src/ports/board.rs:131-135`; `scheduler-rulings.md` R18.
- **Proposal:** O6's check gains: "A second call with the same title, while the first report is open, files nothing and replies with the first one's link." The design decides what counts as "the same". Why: spam is the cost of letting models file, and zaf already has the pattern.

**C22. The check can't be run safely by hand, and S11 doesn't say which places count.**
- **What:**
  - Running the check files a real report wherever the configuration points. If that place is public, the test is public too.
  - The check doesn't say what happens with nothing configured. The design question assumes a default place. A default that is public posts from every fresh install, at work included, before anyone has chosen.
  - Through the MCP: if the tool's schema marks the fields required, Claude Code may refuse a call that lacks them before zaf sees it (unverified). "Please use the following parameters" would then come only from the verb.
  - "Up to configuration": S11 doesn't bound the places. A Jira destination would be 0.6.0's adapter work, and 0.6.0 is out of scope (line 62).
- **Source:** objectives lines 34, 56, 62, 95.
- **Proposal:** the check becomes: "With `bug-report`'s configuration pointed at a scratch repository, a call with the parameters files one issue there and replies with its link. With nothing configured, it posts nothing and names the key to set. Called with no parameters, the verb replies 'Please use the following parameters' and lists them, and the tool says the same or its schema lists them."
  S11 bounds the places to kinds zaf already speaks: GitHub or Forgejo issues.
  **Claude's answer:** no default place. Why: then nothing goes public unless the Engineer configured it.

#### Editorial

**C20. The door's question "which credential posts it" needs three facts.**
- **GitHub:** every GitHub call goes through `gh`, so a report posts as whatever account that machine is logged in to (`zaf/src/adapters/github/command.rs:1`, `:13`). 0.6.0's O1 puts work pull requests on "the company's GitHub" (`../0.6.0/objectives.md:33`). So at work `gh` is plausibly a company account (inference). A report on ways' public GitHub would tie that account to ways.
- **Forgejo:** ways' board token is `FORGEJO_TOKEN`, the one the board verbs read (`.zaf.toml` `[board]`). The host is `yagdrassyl.local`, a `.local` name a work network likely can't reach (inference). From work, the reachable place would then be the public one.
- **A recorded preference:** "Github should only be our reflection for local", with no GitHub writes for ways unless the Engineer asks. This comes from auto-memory `ways-on-forgejo` (2026-11-12). It is not in the repo; verify it.

Proposal: add these facts to the door. Add to the design question: "a credential of its own, named in `bug-report`'s configuration, never the project's board token". Why: who posts can leak as much as what the report says.

**C23. The design question needs a few more points.**
- "The default place" assumes there is one. Reword it as "whether there is a default place" (C22).
- Where the configuration lives: the bugs are zaf's, not the project's, which fits the machine tier better than a tracked `.zaf.toml`.
- What zaf adds by itself (its version, the verb, the exit code), so the report can be reproduced and the model types less.
- Whether the pack's actions and roles count as "zaf".
- What exit code the no-parameter reply carries, given that an MCP result has only `isError` (the contract door, line 83).
- If the place is ways' own board, a report must never become a ticket zaf schedules.

**C24. The Summary needs updating once these are answered.** Line 15 says "12 (Q1 to Q12) ... None open", and the QA block says "C1 to C18". If Round 2 routes questions, update both counts. If C19 is taken, the "constraint that bites hardest" line (line 14) may need to name the public-leak risk next to the breaking constraint.

## QA resolutions

Each challenge, by number:

- **C1 to C12: Routed to the Engineer** (all agreed on 2026-11-22 and applied), as Q1 to Q12 in that order: each changes an objective, a check, a scope line, an assumption or a constraint. Two pre-filled answers are narrower than QA's: Q3 leaves stdio against the hub to the design (the interview parked it there) and asks only for the door and the outcome; Q4 leaves `do release` and `do ship` to the design, since the release instance runs `zaf do release` itself and refusing it through the MCP would break that role once S5 denies Bash. Q10 keeps raising the `[pack]` line, where QA would keep it: the line is tracked, so the change shows in the VCS, and raising it is what an update is for.
- **C13: Resolved.** S4 now serves O1, O2 and O4. S5 and S7 are re-pointed once Q1 is answered (to Q1's check, else O4).
- **C14: Resolved.** O3 now carries the draft's account beside the Engineer's words, and notes that 0.5.4's code changes nothing with no terminal, so the design finds which path hung (a terminal that is present but unanswered fits "hangs"; unverified).
- **C15: Resolved.** The first door lists all of zaf's instance refusals.
- **C16: Resolved.** The contract door carries the tool-name and exit-code facts and the tension with "abstract enough".
- **C17: Resolved.** Questions for the design carries the draft's question 3: where the permission boundary lives once models reach zaf through the MCP.
- **C18: Resolved.** No change to this file. The roadmap page follows signed papers, so stage 1b and the "Parked" line on live position are updated after signing, outside this phase.

Round 2 (bug-report):

- **C19, C21, C22: Routed to the Engineer**, as Q13 to Q15: each changes O6's check, S11 or who may post where. Answered on 2026-11-22 and applied (Q13 and Q14 in the Engineer's own way, Q15 as pre-filled).
- **C20: Resolved.** The bug-report door carries the three credential facts (QA's inference marked as such), and the design question asks for a credential of its own, never the project's board token.
- **C23: Resolved.** The design question reads "whether there is a default place" and carries QA's other points: where the configuration lives, what zaf adds by itself, whether the pack counts as zaf, the no-parameter reply's exit code, and never a scheduled ticket.
- **C24: Resolved.** The Summary counts Q1 to Q15 and names the public-leak risk.

## Signed

2026-11-22 · The Engineer signs by ending this objectives session; zaf writes its signature block at the top of this file, and there is no `signoff.md`. The Engineer, on being told QA was running: "Alright. Tell me when I can exit then".
