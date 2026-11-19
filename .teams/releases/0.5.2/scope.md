---
signed: 2026-11-19T07:31:55Z
hash: b8089395450a000510c94c9f5ccb1777b9d190c519649b9e37c44fa84f099d89
---
# Release 0.5.2 scope: a stranger installs zaf and knows what to type

2026-11-18 · a small release on the Engineer's word (2026-11-18: "Let's make it into a patch"), planned by questions first ("let's plan out what it should do and the relationship of the user with the program"). **Draft until the Engineer signs it.**

**Why.** For a complete stranger: "assume that it is someone with NO knowledge of zaf at all." And first for the Engineer's work computer: "installing it at work needs the script", so it comes before 0.6.0. Today the install is nine hand steps in the README, and knowing what to type next means asking Claude. The stranger should "know what to do and how to call `zaf` without Claude telling them how". zaf still runs Claude instances; the person never needs Claude to call zaf for them.

**Scope.** Each screen was played to the Engineer as a mock and approved; the mocks are in `mocks/` (`install`, `setup`, `begin`, `spawn`, `update`), and they are the reference for how each screen reads and moves.

- **One setup, top to bottom.** "Have the setup basically setup everything needed from top to bottom. It would install zaf and the Claude Plugin."
- **The script says what it will do and why.** "The script should tell exactly what it will do and for what reason." Its output stays quiet: "Do not flood the CLI." Progress may be drawn on one line that updates in place.
- **The install folder is the newcomer's choice, and it goes on PATH.** "The installation on #2 must be configurable and added to PATH."
- **Colour calls the eye.** "I don't want a wall of text, but rather, I want color to call me to things I want to pay attention to. Verbs, nouns, etc." `zaf` has its own colour: "Make `zaf` have a different color." On the Engineer's pick (2026-11-18): `zaf` is a bright magenta-pink, and anything waiting on the Engineer is lavender, the purple #201 gives the hub, so the CLI and the hub mean the same thing by it.
- **The first line says what zaf stands for: Zuri's Amazing Framework.** "It should also say what `zaf` stands for as the first line." The docs' "Zuri's AI Framework" may stay: "Either works for our purposes."
- **zaf is introduced as driving any AI agent.** "By driving your AI Agents for you"; the screens may name Claude Code, since today it is the only agent.
- **It teaches the first steps.** "It would tell me how the plugin works and the main actions I will need for it": the commands "that lets them start working on whatever they want".
- **`zaf setup` speaks to a stranger.** Played and approved (2026-11-18: "Looks good to me!"): it lists what it will do and why before asking, explains each question in plain words (board, forge, check command), shows each step on one updating line, lists only what failed with the command that fixes it and why, and on a green run names the next command, `zaf begin --version 0.1.0`.
- **`zaf begin` guides the release.** Played and approved (2026-11-18: "I love that guided type of thing"): it shows the release's four stages (objectives, design, epics, build) with where you are, says before each Claude Code session what the session is for and how to leave it, asks to sign each document, and ends with the epics to read and sign and the command that starts the build.
- **`zaf spawn --watch` shows the build as one live view.** Played and approved (2026-11-18: "Love it"): one view redrawn in place, never a stream of lines; each epic with a bar of tickets merged, each ticket with its stage in plain words (Claude writes the code, your check command runs, Claude reviews) or what it waits on; Ctrl-C keeps the work. A question for the Engineer shows in the view and can be answered right there, or from any terminal with `zaf board ruling`. At the end it names the release pull request to review and the next `zaf begin`.
- **`zaf update` updates everything.** "The update should update everything": the binary, the teams plugin and the actions `zaf setup` installed. "If overwriting anything, it should show the diff and give a small changelog if possible." Played and approved (2026-11-18: "Approved"): the plan first, a short "What is new" with a link to the release notes, then each action it would overwrite with why, its diff and a question, keeping the old one.

**Out.**

- **Agents other than Claude Code**, for a later version: "In a future version of `zaf`, I want it to be able to use ANY agent. For now, mentions of Claude Code are fine."
- **Windows**: "windows is currently left out." The script covers Linux x86_64 and macOS Apple silicon, the platforms zaf is released for today.
- **Renaming the docs' "Zuri's AI Framework"**: "Either works for our purposes."
- **Everything of 0.6.0** (`../0.6.0/objectives.md`): the git adapter, the Jira and zabo boards, signing and design where the project says.

**Also in 0.5.2** (kept on the Engineer's word, 2026-11-18), the issues slated here before the rescope:

- **#167:** zaf makes the integration branch, asking before it builds.
- **#196:** the papers `zaf begin` signs get committed; today nothing commits them.
- **#201:** the hub draws a task that waits on the Engineer in purple, not yellow.
- **#223:** a phase run's pid keeps the action's process. Already on `main` (d119b0db); it ships with the 0.5.2 tag.

**Done when.** One pasted command on a clean Linux or macOS machine installs zaf, puts it on PATH and adds the teams plugin, and `zaf setup` in a project then ends green, with no Claude session opened along the way (the Engineer, 2026-11-18).
