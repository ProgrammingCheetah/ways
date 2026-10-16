# ways

A Claude Code plugin marketplace of **ways of working**: one plugin per way, each usable alone. It is the single source of the method: every machine and project installs from here, and nothing is copied out of a project again.

## Packs

| Pack | Status | What it is |
|---|---|---|
| `teams` | built | The Orchestrator and its Spawns: agent teams (a standing lead, short-lived programmers; QA and an architect at the top) run through a release lifecycle (design document, team roadmaps and claims, a Groundwork plan, read-only slice plans, build, milestone QA, release reviews, one PR into main) while the Engineer decides every domain fork. Its routines also work alone: the Engineer's queue, catch-up briefings, workspace cleanup, critique, docs, PR preparation, review triage and retrospectives. [README](plugins/teams/README.md) |
| `handoff` | built | How Claude hands the Engineer something to do: a short worksheet of numbered steps. [README](plugins/handoff/README.md) |
| `nodes` | built | The NODE.inf.json node tree: chart a repository so agents load a path's context from the root down. Needs the `nodes` binary. `teams` offers it at setup. [README](plugins/nodes/README.md) |
| `decide` | roadmap | A domain decision from question to record: check what's already decided, state the objectives, research, weigh the options, record the ruling and keep the records in sync. |
| `research` | roadmap | Deep investigation with cited sources, adversarially checked, so no consequential choice rests on memory. |
| agent board | separate plugin | Mail, channels and locks for parallel sessions. Not in this marketplace; `teams` uses it when its tools are present. |

**The rules for every pack:** one plugin per way of working, and **no plugin requires another**. A skill may describe a job ("the repo's review step") but never require another plugin's skill. An optional integration is detected by tool presence or offered at setup with an opt-out, never required, and lives in one clearly marked section. The method names roles only: **the Engineer** (the human who decides) and **the Orchestrator** (the main Claude session).

**Three layers:**

| Layer | Lives in | Holds |
|---|---|---|
| Method | these plugins | how the work is done, in role names |
| Person | `~/.claude/CLAUDE.md` | the Engineer's own preferences |
| Project | the project, in a file each pack seeds on its first run and owns from then on | `teams` uses `.claude/TEAMS.md` (or `.yaml`, `.toml`); `nodes` uses the root `NODE.toml`. A pack with no settings has no file. |

In a repo the Engineer doesn't own (the push remote's owner is neither their forge login nor an organization in `WAYS_OWN_ORGS`; a fork counts by its parent's owner, never its own; admin rights don't count), a pack's project files go in the repo's exclude file (`git rev-parse --git-path info/exclude`), written before the files themselves.

## Install on a new computer

**Goal:** Claude in your work project runs `/teams:orchestrate` and has written its teams file.

1. **Check** Claude Code's version.
   `claude --version`
   Done when: it prints 2.1.292 or later.
2. **Check** that git is installed.
   `git --version`
   Done when: it prints a version number.
3. **Add** the marketplace.
   `claude plugin marketplace add https://github.com/ProgrammingCheetah/ways.git`
   Done when: the output ends with `Successfully added marketplace: ways (declared in user settings)`.
4. **Install** the teams pack.
   `claude plugin install teams@ways`
   Done when: the output ends with `Successfully installed plugin: teams@ways (scope: user)`.
5. **Optional: name** your organizations, only if some of your own repos live under an organization. Add this line to your shell profile (`~/.zshrc` or `~/.bashrc`; on Windows run `setx WAYS_OWN_ORGS example-org`), with several organizations comma-separated (spaces around the commas are ignored), then open a new terminal. Skip it where no organization is yours, as on a work computer.
   `export WAYS_OWN_ORGS=example-org`
   Done when: `printenv WAYS_OWN_ORGS` (PowerShell: `$env:WAYS_OWN_ORGS`) prints your organization(s).
6. **Start** Claude in a terminal at your work project's root folder.
   `claude`
   Done when: the Claude prompt is open.
7. **Type** `/teams:orchestrate`.
   Done when: Claude offers to create `.claude/TEAMS.md`, with its answers pre-filled.
8. **Answer** its one round of questions, changing any pre-filled answer you disagree with, and approve the file writes it asks for.
   Done when: Claude shows the teams file it wrote and hands you a short worksheet for the project's permissions.

Tell me: "done", or the step that failed and its last line of output.

To get worksheets like this one from Claude in every project, also run `claude plugin install handoff@ways`.

**On the computer where you edit the method,** add your local clone instead (`claude plugin marketplace add <path to this repo>`): the plugins then load in place, and an edit applies at the next session or after `/reload-plugins`. Inside a session, `/plugin marketplace add …` adds the marketplace the same way; `/plugin install teams@ways` opens the plugin's details, where you choose to install it.

## Updating

`claude plugin marketplace update ways`, then `claude plugin update teams@ways` (and each other pack you use), then start a new session. Or turn on auto-update: `/plugin` → **Marketplaces** → `ways` → **Enable auto-update**. The plugins carry no version number, so every commit on `main` is a new version: `main` is the release channel and stays shippable.

## Layout

```
.claude-plugin/marketplace.json   the catalog: name "ways"
plugins/teams/                    /teams:orchestrate and its templates, eight more skills, the five agent lanes
plugins/handoff/                  /handoff:how-to
plugins/nodes/                    the nodes pack
```
