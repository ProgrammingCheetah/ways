# Orchestration kit

A portable copy of the Orchestrator-and-Spawns way of working: one Claude session is the tech lead, teams of background agents study, build and review, and the human decides.

| Path | What it is | Installs to |
|---|---|---|
| `skills/orchestrate/` | The workflow (`/orchestrate`), plus templates for a study, a Spawn brief and the teams roster | `~/.claude/skills/orchestrate/` |
| `skills/how-to/` | How Claude hands the human a task: a short worksheet | `~/.claude/skills/how-to/` |
| `agents/` | The three lanes: `builder` (code), `designer` (leads, QA, security), `machinery` (glue) | `~/.claude/agents/` |
| `CLAUDE-snippet.md` | The always-on roles section (names Isaac and Zuri) | Zuri picks at install: the project's `CLAUDE.local.md` (recommended), `~/.claude/CLAUDE.md`, or the project's `CLAUDE.md` |
| `INSTALL.md` | Steps Claude follows to install the kit; no programs, no scripts | — |

No message board or other tool is needed. The Orchestrator is the hub: it briefs Spawns, resumes them with answers, and teams coordinate through shared documents. The teams roster lives with the roles: a `## Teams` section in the same file, or `TEAMS.md` beside a shared `CLAUDE.md`.

## Install

**Goal:** Isaac on the work computer runs `/orchestrate` with teams.

1. **Clone** the kit on the work computer.
   `git clone https://github.com/ProgrammingCheetah/orchestration-kit.git ~/code/orchestration-kit`
   Done when: `ls ~/code/orchestration-kit` lists `skills`, `agents` and `INSTALL.md`.
2. **Open** Claude in the kit's folder.
   `cd ~/code/orchestration-kit && claude`
   Done when: the Claude prompt is open.
3. **Type** `Install the kit: follow INSTALL.md`.
   Done when: Claude asks which project Isaac will orchestrate.
4. **Answer** Claude's questions: the project, then where the roles go (it recommends one).
   Done when: Claude shows its install table, ending with a start command.
5. **Run** that start command in a new terminal.
   Done when: a Claude prompt opens in the work project.
6. **Type** `/orchestrate`.
   Done when: Isaac proposes teams for you to approve.

Tell me (back in the home session): "done", or the step that failed and what Claude said.

## Updating

Run `git pull` in the kit's folder, then steps 2–4 again. Claude shows every file that differs from yours and asks before replacing it; it never deletes anything. To change a lane's model or effort, edit the `model:` and `effort:` lines in `~/.claude/agents/<lane>.md`.
