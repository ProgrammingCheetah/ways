---
signed: 2026-11-18T08:08:20Z
hash: c4f8a16be4ec7bc1f73431e85810c0e397392b80793be8cf1b9ccc282298f4ae
---
# Release 0.5.1 scope: the fixes left from 0.5.0, and the hub's grid

2026-11-18 · a small release on the Engineer's word (2026-11-18: "Small release", chosen over a full objectives interview).

**Scope.** Four things the Engineer sent to 0.5.1:

- **A task whose pull request gets no check is raised, not left waiting two hours (#144).** "Add that as an issue to fix in a bugfix version then." The issue's three open points are for the design: how soon a head with no check is raised, what the Engineer's ruling can then do, and whether `zaf doctor` or `zaf check` warns when a task's paths fall outside CI's path filter.
- **`zaf setup` asks before it overwrites an installed action, and shows the difference (#149).** The ruling on #107: "Since it is setup, it should ask if an action should be overwritten and show a diff between both (we may use a git tool or something for this)."
- **The hub's squares in a uniform grid**, moved here from 0.6.0: "I am fine with them going into 0.5.1 if I can sign the epic for that as well. All the UI changes may go there." Its check is the one 0.6.0's objectives give (Out of scope, first row): an N by M grid, wider than tall, uniform spacing like GitHub's contribution grid, squares running top to bottom down a column, colours, hover and click as today. Its open design point comes with it: what a square stands for once tasks are no longer grouped, and where a task's squares start.
- **The repeated `zaf profile init` messages left from 0.4.3**, moved here from 0.6.0 with the hub's UI.

**Out.** Everything of release 0.6.0 (`../0.6.0/objectives.md`): the git adapter, the Jira and zabo boards, and signing and design where the project says. Also out: a model reporting where it is to the hub ("for a later date").
