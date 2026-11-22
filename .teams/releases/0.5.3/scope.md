---
signed: 2026-11-19T09:33:54Z
hash: 091bd59b4e897c1d9dfd68f516280d84fee29738456cc43db0673ea5a4affcf4
---
# Release 0.5.3 scope: the Unified UI patch

2026-11-19 · a small release on the Engineer's word. Signed by Claude on the Engineer's overnight word (2026-11-19: "You are allowed to make rulings on my side from right now and create the objectives / design for 0.5.3 for me"; signing: "Yes, sign for me"). The chair mock was approved in advance ("Approve in advance").

**Goal.** "Let's call this next patch the “Unified UI” patch. Our goal for it is to unify the UI in a way that I do not have to leave the CLI to check on anything at all. Not even to check a browser." Everything the hub's web page shows today, and everything `zaf status` gathers, is in the terminal zaf runs in.

**The captain's chair.** "What I want for zaf is for zaf to be a unified UI that basically works as the captain's chair. Everything I need can be found in there. Answers, rulings, the squares and everything as well. EVERYTHING." The test for every item below: nothing the Engineer needs, to see or to do, sits outside `zaf`'s UI: the questions and their answers, the rulings, the squares the hub draws, the epics and their signing, the phases and their Claude sessions, the build, problems, and what is due next. And it stays current: "It also updates consistently, without needing to close it and open it again." What changes on the board, in a run or in a pull request shows on its own while the UI is open. "Signing epics becomes buttons inside the UI. Answering questions and seeing the dependency tree between tasks and epics become part of the UI as well." "If I click on an epic, I am able to read the objectives and the general design in the UI. I am to be able to be informed about everything I need inside the UI."

**Claude's suggestions, all ten taken** (the Engineer, 2026-11-19):

- **Inbox by impact.** One list of everything waiting on the Engineer, sorted by how much work each item unblocks; its count always on screen.
- **Claude's answer pre-filled.** Each question shows Claude's recommended answer first; Enter accepts it, or the Engineer types their own.
- **Ping when needed.** A terminal bell or desktop notification when a question arrives or a version is built.
- **Undo a ruling.** A few seconds to take back a ruling or a signature before it unblocks anything.
- **The UI is only a view.** Closing `zaf` never stops the build; the scheduler runs behind it, and reopening shows where things stand.
- **Keys shown on screen.** A bottom bar with the focused pane's keys, a palette of every action, and the mouse working too.
- **Why is it waiting.** Selecting a ticket says in plain words what it waits on, what it does next, and its time and tokens so far.
- **Not colour alone.** Every state also has a symbol; red and green are never the only difference; it works at 80×24 and on light terminals.
- **Live output of an instance.** Selecting a running ticket shows what its Claude is doing right now (the Engineer, 2026-11-19: "I like both as well").
- **One chair for every project.** All the machine's zaf projects in one UI, a tab each.

**Scope.** "The next patch version should really focus on making the UX way better."

- **Open questions are easy to see.** "Right now, it is so surprisingly hard to see what questions are open." Today `zaf status --more` lists them last, under 45 waiting lines.
- **Colour calls the eye on every screen.** "Our terminal stuff also needs more color. Make color call for a specific place rather than just everything white." Every zaf output, not only 0.5.2's five screens (`zaf status`, `zaf epic`, `zaf board`, `zaf doctor` and the rest), with the palette 0.5.2's screen kit brings: `zaf` magenta-pink, what waits on the Engineer lavender, failures red, done green, ids and paths yellow, commands cyan, the rest dim. Mock: `mocks/status-mock.sh`.
- **zaf answers fast.** "I want a full optimization of our architecture. `zaf status` takes a hot minute to gather everything." Measured 2026-11-19 in ways: 9.6 s wall clock for `zaf status` at 18% CPU, so most of the time is waiting, most likely on the forge's API.
- **One terminal runs a project.** "I am using three different terminals for the exact same project. One has the watch, one has the hub and one is being used for rulings. This is very bad UX." 0.5.2 already lets a question be answered in the `spawn --watch` view (#251); the hub still needs a terminal of its own.
- **The git adapter and the jj check**, moved here from 0.5.2 (the Engineer, 2026-11-19: "We are moving git adapter stuff to 0.5.3"; #229): a plain git repository works, not jj only. It stays in the Unified UI patch (the Engineer, 2026-11-19).
- **`zaf` alone opens the UI.** "Let's have it so that I can just run `zaf` in a project with zaf and it'll open some UI. If no design has been run (or objectives), it has a button. It opens a mini terminal window with a claude instance and allows me to answer it. When Claude thinks everything is done, it says "Ready for Exit." on both instances. The UI doesn't flicker or change, but rather just closes Claude and it may continue." Claude's reading: the button starts the phase that is due; the Claude pane opens inside the UI; "Ready for Exit." shows in the Claude pane and in zaf's own part of the screen; closing the pane leaves the rest of the screen as it was, and zaf goes on (the sign question, then the next stage).

**Layout.** Panes split both ways, like vim's windows: "I want not only columns, but also rows. Unequal-size rows and columns are fine. VIM uses this layout, for example." Mock: `mocks/chair-mock.sh`.

**For the design.** The terminal UI is built on **ratatui** (the Engineer, 2026-11-19: "Ratatui sounds good to me"), with crossterm for input, as one more adapter. A Claude session opens in a pane inside that UI (tui-term and portable-pty are the likely pieces), never taking the terminal over. Open for the design: how zaf learns that Claude is ready (a signal Claude sends through zaf, rather than reading its screen), and what a bare `zaf` does outside a zaf project.

**Also.** 0.6.1's "better UI for zaf" moves here whole; 0.6.1 no longer carries UI work (the Engineer, 2026-11-19).

**Out.**

- **The web UI.** "The web UI will wait until the CLI UI and UX is stable to just replicate it. For now, let's just focus on the CLI." The hub's page gets no new work; later it copies the CLI.
- Everything of 0.6.0 (`../0.6.0/objectives.md`).
