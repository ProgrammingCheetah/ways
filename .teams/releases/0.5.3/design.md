---
signed: 2026-11-19T09:50:41Z
hash: 30e5ccf2c243b35682294db21ed0668628bda26d075bc299d89b846022d0c4b9
---
# Release 0.5.3 design: the captain's chair

2026-11-19 · Written by Claude on the Engineer's overnight word ("create the objectives / design for 0.5.3 for me, since you already know what I want"), from the signed `scope.md` and the approved mock `mocks/chair-mock.sh`; challenged by a fresh designer (C1 to C13, resolved below). Signed by Claude on the Engineer's overnight word ("Yes, sign for me").

## Summary

Running `zaf` with no verb opens the captain's chair: one full-screen terminal UI where the Engineer sees and does everything, with no browser and no second terminal. It is a new CLI adapter on ratatui over the model the hub's web page already serves (`hub::state`'s `ProjectState` and `MachineState`, and `hub::next`'s list of what waits on the Engineer), so the page can later copy it without a second model. The panes split both ways like vim's windows. The chair is only a view: closing it never stops a build.

The chair is built by hand overnight on `feature/0.5.3`, branched from `feature/0.5.2` (the Engineer, 2026-11-19: "I build it by hand"), because zaf's scheduler builds one version at a time and 0.5.2 holds it. It is filed as a 0.5.3 epic marked built by hand, so the release instance checks it with the rest. The rest of 0.5.3 (colour on every other verb, the gather made fast at its source, the git adapter #229) goes through zaf after 0.5.2, from a design addendum signed before its epics are filed.

## Goals (from the scope)

- G1. **Never leave `zaf`.** Questions, answers, rulings, the squares, epics and their signing, the objectives and design, phases with their Claude sessions, the build, problems, the dependency tree and what is due next, all in one UI.
- G2. **It stays current** while open, with no restart.
- G3. **It reads at a glance:** what waits on the Engineer first, colour that calls the eye and never alone, keys on screen.
- G4. **One terminal runs a project**, and one chair runs every project on the machine.
- G5. **zaf answers fast**: the UI never waits on a gather; the gather itself gets fast in the addendum.
- G6. **A plain git repository works** (#229), carried from 0.5.2; in the addendum.

## Non-goals

- The web UI ("The web UI will wait until the CLI UI and UX is stable to just replicate it"); it gains only what the chair adds to the shared model.
- Agents other than Claude Code; Windows; everything of 0.6.0.
- Removing any verb: `zaf board ruling`, `zaf epic sign` and the rest stay for scripts and instances.

## Decisions

**D1. Bare `zaf` opens the chair.** An empty argument list is caught before clap, so `Cli.verb` stays required. On a terminal: in a zaf project, the chair opens on that project's tab; outside one, it opens on the machine's projects (`Projects::read`) when there are any, and with none it prints three lines (this folder has no zaf project; `zaf setup` makes one; `zaf help` lists the verbs) and exits 0. With no terminal, bare `zaf` is unchanged: help on stderr, exit 2.

**D2. A ratatui adapter over the hub's model.** `zaf/src/cli/chair/` holds the terminal side: ratatui with crossterm for keys, mouse and resize. A tab's state is the hub's `ProjectState` (`app::hub::state::gather_project`), the machine's is `MachineState` (`gather_machine`), and the inbox is `app::hub::next::next_for_you`. Three things only `zaf status` had move into `ProjectState` itself, so the page gets them too: orphaned claims, each ticket's waits (from the same `Gathered`, through `shadow::places`), and the releases due. The status composition is never called besides, since it would read everything again. No terminal crate is named outside `src/cli/chair/`: a new test in `tests/architecture.rs` checks for `ratatui`, `crossterm`, `portable_pty`, `tui_term` and `vt100`, because the layer test reads only `crate::` names. A ticket's phase comes from what the state carries; nothing new calls `place(`, `current_phase(` or `phase_status(`.

**D3. Gathering never blocks the screen.** `gather_machine`'s thread per project and its guard against hung reads (`Reading`) serve the chair: the focused tab is gathered every 5 seconds after the last gather ends, other tabs every 60 seconds (enough for their badges), and any tab at once when its `.zaf/hub/reports` changes (the hub's `Watch`) or after the chair writes something. Board reads on a keypress (an epic's body) run on the gatherer thread. The draw loop only reads the newest state; the status line shows its age and a spinner while a gather runs; ratatui's diffing redraws only changed cells.

**D4. Panes split both ways, like vim.** As the mock: tab bar on top; status and key bar at the bottom; a left column (Inbox over Problems over Now) and a right column (Epics with their squares over a vertical split of Detail and Live). `Ctrl-w h/j/k/l` (and arrows) move focus by direction, `Ctrl-w w` and `Tab` cycle, `Ctrl-w + - < >` resize by 5% down to 3 rows or 16 columns, `Ctrl-w =` resets, `Ctrl-w o` maximises and restores. Below 80×24 the chair says the size it needs. The mouse focuses a pane and selects a row. `:` opens the palette of every action.

**D5. The inbox is `next_for_you`.** Everything that waits on the Engineer, most blocking first, as the page shows it: unsigned epics, questions, doors, phases that failed three times, ready work no scheduler ticks, broken projects, plus one new kind, "a release phase is due". The count is on each tab. Problems (orphaned claims and the rest) have their own pane; Enter on a problem runs its fix when the fix is a zaf command, and an orphaned claim says the next tick frees it.

**D6. Answering, with Claude's answer pre-filled.** `Enter` on a question opens the answer box over Detail with the whole question above it, pre-filled by `proposal_of` (`hub::next`), which already reads `Claude's answer`, `Proposed answer` and `Proposal`, and is extended there to read `(a) **(Recommended)** <text>` and `(Recommended: …)`. The pack's role briefs (team, programmer, planning, release) gain one rule: every question ends with a `Proposed answer:` line, so the box rarely starts empty; projects on an older pack tag get it when their tag moves. The ruling goes through the app path of `zaf board ruling`, refusing an empty answer as it does.

**D7. Undo before anything lands.** A ruling or a signature waits 5 seconds before it is written, counting down with `u` to take it back. `q` and `Ctrl-C` during a countdown write it first, saying so; a killed process or a closed terminal loses it, and the next gather shows it still open.

**D8. Epics: read, sign, tree.** `Enter` on an epic opens Detail on it: its stage, its body, and the release's objectives (or scope) and design from the release folder, scrollable. `s` signs it through `app::epic::sign` (refused inside an instance), with undo; signing the last unsigned epic of a version offers `b`. `d` draws the dependency tree: the epic's tickets with what each waits on (tickets, epics, questions) and each one's symbol.

**D9. Why it waits, and what it is doing.** On a ticket, `w` says in plain words (0.5.2's stage words) what it waits on, its next phase, its pull request, and its time and tokens as of the last ledger ingest (the chair ingests for the focused tab at most once a minute). `l` shows the hub's reports for it (a few general words each, as the hub records them) and the tail of its phase run's log; a `claude -p` phase writes its log only at its end, so the pane says "working" until then. Streaming tool calls is a later pack change.

**D10. Phases run in a pane, and close when ready.** When a release phase is due, the inbox's first item is its button, `o`. The action runner gains a third stdio plan, `OnPty`: the CLI hands it the pseudo-terminal's side as plain file descriptors, so the app layer names no terminal crate; portable-pty makes the pair and a vt100 parser with tui-term draws it in a pane over Detail and Live. `PhaseEnv` carries `ZAF_READY`, the file `.zaf/begin/<version>/<phase>.ready`. The objectives and design actions tell Claude, when it judges the document done, to say "Ready for Exit." and run `zaf begin ready`, which writes that file (refused when `ZAF_READY` is unset). The chair sees the file, shows "Ready for Exit." on its status line, types `/exit` into the session, hangs up after 5 seconds if it has not ended, and closes the pane without touching the rest of the screen. The ready file or Claude's own exit 0 leads to `complete_phase`, whatever the exit code, so an older pack still works; the chair's sign question is `Sign it? [Y/n]` and Enter signs. While the pane is focused every key goes to Claude except the `Ctrl-w` prefix; `Ctrl-w c` leaves the pane, asking first when the phase is not ready; `q` elsewhere asks first while a session runs, since the session ends with the chair. A spike checks first: `/exit` ends Claude Code with 0, Claude's Bash tool passes `ZAF_READY`, tui-term's ratatui matches the pinned one, portable-pty builds for the musl and macOS targets.

**D11. Closing never stops the build.** The chair is a view; a scheduler keeps running after `q`. When none runs, Now offers `b`: the chair spawns `zaf spawn --watch --version <v>` as its own process (a new session, stdin `/dev/null`, output to `.zaf/logs/`), `<v>` being the version whose epics are signed and open. "Running" is read from the scheduler's lock (`WatchState` in `ProjectState`). `B` stops it after its tick by sending SIGTERM to the lock's pid, which the scheduler treats as its stop. The hub's in-process watch control is not used: it ends with its process.

**D12. A ping when needed.** A new item in the inbox rings the bell and, where `notify-send` (Linux) or `osascript` (macOS, with the text passed as an argument to `on run argv`, never inside the script) is on PATH, posts a desktop notification; so does a version finishing. `ZAF_NOTIFY=0` turns the desktop part off; no manifest key, since older binaries refuse unknown keys.

**D13. Not colour alone, and no terminal injection.** Symbols follow the mock: □ waiting, ■ done, ? waiting on the Engineer, ✗ a problem, a spinner for running. The palette is 0.5.2's (`domain::screen::Meaning`): `zaf` magenta-pink, waiting on the Engineer lavender, failures red, done green, ids and paths yellow, commands cyan, the rest dim. `NO_COLOR` leaves bold and reverse. Every string from the board or a file has its C0 and C1 control characters (but newline and tab) stripped in the read model, as `Report::to_line` does.

**D14. The addendum, through zaf after 0.5.2.** Colour and the inbox-first order on every other verb's output; the gather fast at its source (forge reads concurrent and cached between gathers, with a target set there, measured against today's 9.6 s for `zaf status` on ways); the git adapter (#229). Each with its "done when", in `design-addendum.md`, signed before its epics are filed.

**D15. Where the chair is built.** `feature/0.5.3` starts from `feature/0.5.2` and takes 0.5.2's merges as they land, since the chair uses its palette, `next_version`, `EpicState.release`, #292's `[Y/n]`, the stages, the build start and the stage words. Every call into begin, release and the runner sits in one chair module, so 0.5.2's later changes land in one place.

## Features and their "done when"

| # | Feature | Goal | Done when |
|---|---|---|---|
| F1 | Bare `zaf` | G1, G4 | It behaves as D1 in each of its four cases. |
| F2 | Shared model and gatherer | G2, G5 | Tabs read `ProjectState` and `next_for_you` from threads; the draw loop never gathers; the age shows; `ProjectState` carries orphans, waits and releases due. |
| F3 | Split layout and window keys | G3 | D4's panes and keys work at 80×24 and larger, and resize live. |
| F4 | Inbox and problems | G1, G3 | The inbox is `next_for_you` with the release-phase kind; problems in their pane, their zaf fixes run on Enter. |
| F5 | Answering with undo | G1 | Enter opens the box pre-filled by `proposal_of`; Enter rules after a 5 s undo; the board shows the ruling. |
| F6 | Epics | G1 | Enter opens body, objectives and design; `s` signs with undo; `d` draws the tree. |
| F7 | Ticket detail | G1 | `w` explains the wait, next phase, pull request and cost; `l` shows reports and log tail. |
| F8 | Phase pane | G1, G4 | `o` runs the due phase on a pty in a pane; `zaf begin ready` or exit 0 closes it; the sign question follows. |
| F9 | Build start and stop | G4 | `b` starts a detached scheduler, `B` stops it after its tick, `q` leaves it running. |
| F10 | Pings | G3 | A new inbox item rings the bell and notifies where possible; `ZAF_NOTIFY=0` silences the desktop part. |
| F11 | Projects | G4 | One tab per project of the machine; `1`–`9` switch; other tabs gathered every 60 s. |
| F12 | Accessibility and safety | G3 | Symbols beside colour, `NO_COLOR`, 80×24, control characters stripped. |
| F13 | The palette | G1, G3 | `:` lists every action of the chair and runs the chosen one. |
| F14 | Status fields placed | G1 | Slots, guard, budget, unparsed tasks, cycles, dead blockers and stale profiles show in Now or Problems. |
| F15 | Controls | G1 | Stop a task, tick now, start and stop the build, each from a key and the palette. |
| F16 | Begin the next version | G1 | With no release due, the inbox offers to begin `next_version`, pre-filled. |
| F17 | Proposed answers | G1 | The pack's role briefs ask every question to end with `Proposed answer:`. |

Pure parts are unit-tested (the gatherer's cadence, the inbox kind, `proposal_of`'s new forms, the undo queue, the split tree's geometry and focus moves, the key map, the sanitiser); frames are rendered against ratatui's test backend; the gate stays green.

## One-way doors

- **Bare `zaf` on a terminal opens a UI** (D1); piped, it is unchanged.
- **`ZAF_READY` and `zaf begin ready`** join the phase actions' contract (D10).
- **`OnPty`** joins the runner's stdio plans (D10).
- **New dependencies:** ratatui, crossterm, portable-pty, tui-term or vt100, pinned in `Cargo.lock`, building for every release target.
- **The pack's question rule** (D6) changes what every instance writes.

## QA challenges

C1 to C13 from a fresh `teams:designer` (opus), 2026-11-19, reading the code at `main@origin` and `feature/0.5.2@origin`. In short: C1 the hub's watch control ends with its process; C2 `hub::gather` is the wrong model and the status composition doubles the read; C3 `next_for_you` and `proposal_of` already exist; C4 nothing asks instances for a proposed answer; C5 the runner cannot give a phase a pty, release phases have no run folder, a hang-up reads as failure; C6 the architecture test misses outside crates and the app layer; C7 branch from `feature/0.5.2`; C8 goals and scope items without a feature; C9 a manifest key and the name `zaf ready` are one-way doors, and bare `zaf` exits 2 today; C10 terminal and AppleScript injection; C11 forge load; C12 the live pane shows less than the mock; C13 symbols, undo loss, the outside-a-project case.

## QA resolutions

Resolved by Claude on the Engineer's overnight word, each in the text above:

- **C1:** D11, a detached process and a stop.
- **C2:** D2 and D3, the hub's model, with orphans, waits and releases due added to it.
- **C3:** D5 and D6, `next_for_you` and `proposal_of`.
- **C4:** D6 and F17, the pack's question rule, in 0.5.3.
- **C5:** D10, `OnPty`, a named ready file, the exit-0 rule, `[Y/n]` and a spike.
- **C6:** D2, a new architecture test.
- **C7:** D15 and the summary.
- **C8:** F13 to F17, and D14's addendum signed before its epics.
- **C9:** D12, `ZAF_NOTIFY`; D10, `zaf begin ready` (the challenger's (a)); D1, the exit-2 fact.
- **C10:** D13 and D12.
- **C11:** D3.
- **C12:** D9.
- **C13:** D13, D7 and D1 (the challenger's (a)).
