# ways

A Claude Code plugin marketplace of **ways of working**: one plugin per way, each usable alone. It is the single source of the method: every machine and project installs from here, and nothing is copied out of a project again.

## Packs

| Pack | Status | What it is |
|---|---|---|
| `teams` | built | The standing briefs of the instances that build a project, one per zaf role (team, programmer, keeper, planning, challenger, release) plus the design session's judgment lane (designer); a session-start hook that prints `zaf doctor`; and the routines the Engineer runs in the design session: release objectives, the queue, catch-up briefings, doc comments and retrospectives. The work runs through `zaf`, the project-blind binary of verbs built in `zaf/`, and the Engineer rules every domain fork. [README](plugins/teams/README.md) |
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

**Goal:** your work project has a `.zaf.toml` and Claude starts there with a clean `zaf doctor`.

1. **Check** Claude Code's version.
   `claude --version`
   Done when: it prints 2.1.296 or later.
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
6. **Install** `zaf`: open https://github.com/ProgrammingCheetah/ways/releases, download the latest `zaf-v*` archive for your platform (Linux x86_64 or macOS Apple silicon), unpack it, and put its `zaf` binary on your PATH.
   `zaf --version`
   Done when: it prints a version number.
7. **Run** `zaf init` at your work project's root folder and answer its four questions (project name, board repo, forge repo, gate command).
   `cd <your project root> && zaf init`
   Done when: the output ends with `wrote .zaf.toml`.
8. **Run** `zaf setup`, do the steps it prints, and run it again until nothing is left.
   `cd <your project root> && zaf setup`
   Done when: it exits 0.
9. **Start** Claude in a terminal at your work project's root folder.
   `claude`
   Done when: the `zaf doctor` table the teams hook prints at session start shows every row `ok`.

Tell me: "done", or the step that failed and its last line of output.

To get worksheets like this one from Claude in every project, also run `claude plugin install handoff@ways`.

**On the computer where you edit the method,** add your local clone instead (`claude plugin marketplace add <path to this repo>`): the plugins then load in place, and an edit applies at the next session or after `/reload-plugins`. Inside a session, `/plugin marketplace add …` adds the marketplace the same way; `/plugin install teams@ways` opens the plugin's details, where you choose to install it.

## Updating

`claude plugin marketplace update ways`, then `claude plugin update teams@ways` (and each other pack you use), then start a new session. Or turn on auto-update: `/plugin` → **Marketplaces** → `ways` → **Enable auto-update**. `main` is the release channel and stays shippable: every commit on it is a new version of the plugins. Only `teams` carries a version number in its `plugin.json` (2.1.0), which `zaf doctor` reports; the other packs carry none. The `zaf` binary is released separately, from tags `zaf-vX.Y.Z`.

## Layout

```
.claude-plugin/marketplace.json   the catalog: name "ways"
plugins/teams/                    the role briefs (team, programmer, keeper, planning, challenger, release, designer), the doctor hook, five design-session skills
plugins/handoff/                  /handoff:how-to
plugins/nodes/                    the nodes pack
zaf/                              the zaf crate: the binary the teams pack drives, released from tags zaf-vX.Y.Z
docs/                             zuri-ai-framework.md, the method's design draft, and the 2026-10-27 baseline
.zaf.toml                         zaf's manifest for this repo, which runs the framework on itself
.zaf/actions/                     this repo's own actions (slice steps, merge, release), the project tier
```
