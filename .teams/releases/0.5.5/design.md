---
signed: 2026-11-23T02:48:55Z
hash: a2569cbae869bc38a79bd62417a1e359734d98865dfe5b73c9b545da1159727d
---
# Release 0.5.5 design: zaf's MCP

2026-11-22 · Written by Claude (the Designer) from the signed `objectives.md` and zaf's code at main 8932edf1 (zaf 0.5.4). Every Claude Code fact is either seen on this machine (Claude Code 2.1.330, a probe server, 2026-11-22) or quoted from code.claude.com; each one says which in "Facts checked". **Draft until the Engineer signs it**; zaf takes the signature when this session ends.

## Summary

zaf gets one new hidden verb, `zaf mcp`. Claude Code starts it over stdio, one per session, for both the Engineer's own Claude (the orchestrator) and every instance zaf runs. It offers one tool for each verb in `zaf --help`. A tool call takes the same words you would type after `zaf <verb>`, runs the real `zaf` binary as a child process and returns exactly what the verb prints, with the exit code on a last line when it isn't 0. zaf still makes every change, and every guard zaf has today still runs.

Before it runs anything, the server works out who is calling, from the same environment and process tree the verbs use; the probe showed a stdio server inherits both. It then refuses what that caller may not do:

- **Everyone:** signing. Signing is also closed in Bash, for any process Claude Code starts.
- **Instances only:** the `[forbidden]` merges, rulings, and the verbs that move tickets or change the machine.

Instances lose `zaf` in Bash and get the MCP in their profiles. The pack's prompts change to match.

The rest of the release:

- **`zaf update --yes`, built first:** answers every question itself, keeps the actions you edited, and ends on one line.
- **Live position:** every role reports it through the `progress` tool, and the chair shows it.
- **The orchestrator's build:** `zaf spawn start` starts the scheduler with one call, and `zaf epic wait` returns when something needs you.
- **Bug reports:** `bug-report` and `bug-report-query` are pack actions that post to a GitHub or Forgejo repository you configure, with a token of their own. There is no default place.

**One finding that's live today:** zaf's lean and build profiles load your claude.ai connectors (Gmail, Google Drive, Calendar, Sheets, Notion, Atlassian, Claude Docs). A programmer instance listed all seven. Calls to an MCP tool with no allow rule were refused in `-p` on the probe, so it's a leak surface, not a seen leak. Q4 closes it in 0.5.5.

Eight epics are filed on the board, #1002 to #1009, unsigned. E1 (`--yes`) goes first, and E3 (refusals) must land before E4 moves instances onto the MCP. The Engineer answered the nine questions on 2026-11-22: Q1 takes `rmcp`, the official Rust SDK, in place of zaf's own protocol code; the other eight stand as pre-filled. The epics match those answers.

## Questions for the Engineer

Each is pre-filled with Claude's answer; the Engineer answers on the **Engineer:** line. The Engineer, 2026-11-22: "I answered the questions. An empty question means I agreed with you."

**Q1. Should the MCP protocol be zaf's own code, or the official Rust SDK?**

- (a) zaf's own JSON-RPC over stdin and stdout, behind a transport port (D22). It's about six messages, needs no new crate, and the probe server that worked with Claude Code was 60 lines.
- (b) `rmcp` 3.11, the official SDK. It brings tokio, and zaf runs on threads today.

**Claude's answer:** (a). Why: no async runtime enters zaf. If the protocol later outgrows it, only the adapter changes.
**Engineer:** I think `rmcp` is more future-proof. That, and one less thing to maintain, other than our consumption of it.
**Applied (Claude, 2026-11-22):** D5, D22, M1, M4, M5, I2, the risks and epic #1003 now use `rmcp`.

**Q2. What does a tool take: the verb's own words, or typed fields?**

- (a) Words. `zaf board claim 12` becomes the tool `board` with `args: ["claim", "12"]`, and every tool also takes `stdin`, `json` and `more`. The description is the verb's own `--help`.
- (b) Typed fields generated from clap: a `command` list for subcommands, and one field per flag and argument.

**Claude's answer:** (a). Why:

- every page's `zaf …` line keeps working as written, through one rule;
- a new verb or flag reaches the MCP with no MCP code ("abstract enough to allow continuous / easy change");
- nothing goes through a shell, so a ruling is never cut by quotes again (R31).

(b) catches a wrong field before the call, but merging the subcommands' fields gives clashes and a second way of writing every command.
**Engineer:** Agree on (a)

**Q3. Where does your own Claude session get zaf's MCP?**

- (a) `zaf setup` registers it at user scope in your Claude (`claude mcp add-json --scope user zaf …`), on a `[Y/n]` line. Its tools are then `mcp__zaf__<verb>` in every session, the same names the instances get.
- (b) The teams plugin bundles it. Its tools would be `mcp__plugin_teams_zaf__<verb>`, a different name from the instances' (the lean profile carries no plugin), so every prompt would need two names.
- (c) Each project's `.mcp.json`. That's a tracked file, and at work it would be a change to the workplace's repository.

**Claude's answer:** (a). Why: one name everywhere. The name is part of the contract (see One-way doors).
**Engineer:**

**Q4. Should instances load zaf's MCP and nothing else?** On 2026-11-22 a lean-profile `claude -p` listed tools from `claude_ai_Gmail`, `claude_ai_Google_Drive`, `claude_ai_Google_Calendar`, `claude_ai_Google_Sheets`, `claude_ai_Notion`, `claude_ai_Atlassian_Rovo` and `claude_ai_Claude_Docs`. `claude mcp list` shows the same for the build profile. Under the lean profile, a tool with no allow rule was refused in `-p` (seen on the probe). Whether a connector tool that is marked read-only is also refused is not stated in the docs. ways' `.mcp.json` server (basic-memory) also loads in `-p` runs without asking (docs).
**Claude's answer:** Yes, in E4. Every profile gets `"disableClaudeAiConnectors": true` and lists the project's `.mcp.json` servers in `disabledMcpjsonServers`, so the framework's "no MCP servers" rule holds for everything but zaf's own (S4, S10). Why: a prompt-injected instance shouldn't be one allow rule away from your mail. The connector names also cost every instance context at turn one. If you want it sooner, adding that one key to `.zaf/profiles/lean/settings.json` and `build/settings.json` by hand closes it today, until the next `zaf profile init` rewrites them.
**Engineer:**

**Q5. May the orchestrator run `zaf do release` and `zaf do ship` through the MCP?** The release instance runs `zaf do release` itself, and the orchestrator has your rights. Bash stays open to it either way.
**Claude's answer:** Yes. Both stay under the actions' modes and `[forbidden]` (ways forbids `do ship` to instances), with no extra MCP refusal. Why: refusing them through the MCP alone would bind nothing while Bash is open, and it would break the release instance. Releasing "on your word" stays a working rule, written into the `do` tool's description.
**Engineer:**

**Q6. Should the MCP also refuse the lane verbs to instances: `spawn` (but `spawn trace`), `board claim`, `board done` and `slice`?** This goes past S6's list. Today an instance may type them in Bash. No role zaf runs needs them: zaf claims, opens and lands, and the team role that used them is retired. They show up in the briefs only through the shared interface page.
**Claude's answer:** Yes. Why: "Nobody moves themselves" (`principle.md`). Moving instances to the MCP is the moment to make it code, not text.
**Engineer:**

**Q7. How does the orchestrator follow a build?**

- (a) A new verb, `zaf epic wait <epic>`. It returns when a question waits on you, a problem shows, or the epic is done, and prints what changed. An interactive session moves a call longer than two minutes to the background and tells Claude when it ends (docs), so the orchestrator waits for free.
- (b) It polls `zaf status` turn by turn.

**Claude's answer:** (a). Why: O2 says "follows it" and "brings each question", and polling spends a turn's tokens for every look.
**Engineer:** (a), and I like the idea of basically long-polling. Good call.

**Q8. Where does `bug-report` read its place and token from?**

- (a) A table any action can have: `[actions.bug-report]` in the machine config (`~/.zaf/config.toml`), with `kind` (`github-issues` or `forgejo-issues`), `url`, `repo` and `token_env`. zaf hands each key to that action as `ZAF_SETTING_<KEY>`. A project's manifest may override it, property by property, as it does today.
- (b) A file of the action's own, which a shell script would have to parse.

**Claude's answer:** (a), set on the machine. Why: the bugs are zaf's, not the project's. It's general, so no zaf code is specific to bug reports ("dogfood"). And nothing lands in a tracked file at work.
**Engineer:**

**Q9. May instances call `bug-report`?** You ruled "To wherever it is configured", and the door left "whether instances may call it" to the design.
**Claude's answer:** Yes, with five guards in the action itself:

- no default place;
- its own token, never the project's board token or `gh`'s login (`GH_TOKEN` is set from `token_env`);
- your home folder written as `~`;
- a body capped at 8,000 characters;
- a `Reported by:` line naming the caller (a role, or "outside zaf").

Why: instances meet most of zaf's faults (O6). If you'd rather keep them out, one row in the policy table refuses `do bug-report` to instances.
**Engineer:**

<details>
<summary><b>Picture: one call, from a model to zaf</b></summary>

```
  your Claude            zaf instance (claude -p)
  (orchestrator)         profile: zaf MCP only
        \                    /
         v                  v
     zaf mcp  (stdio, one per session)
       1. words -> zaf command line
       2. who calls? env + process tree
       3. policy: run or refuse
                 |
                 v
     zaf <verb> ...  (child, same binary)
        |        |          |
        v        v          v
      board    forge    hub report -> chair
```

The server never prints a verb's output itself and never writes to the board or the forge: the child `zaf` does, through the code it runs today.

</details>

## Goals and non-goals

| Objective                                     | Met by                                    |
| --------------------------------------------- | ----------------------------------------- |
| O1 a tool for every verb, `do` lists the same | D2, D3, D4 (E2)                           |
| O2 the orchestrator builds a signed epic      | D14, D19, D24 (E4, E7)                    |
| O3 `update --yes`                             | D18 (E1)                                  |
| O4 live position for every role               | D16, D17 (E6)                             |
| O5 instances work through the MCP alone       | D7 to D9, D13, D15, D23, D24 (E3, E4, E5) |
| O6 `bug-report` and its query                 | D20 (E8)                                  |

Not in 0.5.5, carried over from the objectives: everything of 0.6.0, the 0.5.3 review leftovers (0.5.6), and models other than Claude. Also out: an MCP served by the hub (D1), and a real permission boundary (D21, still 0.6.0's).

<details>
<summary><b>Decisions D1 to D25</b> (the design itself)</summary>

### The server

**D1. stdio only, one server per Claude session, started as `zaf mcp`.** No MCP is served by the hub. Why: a stdio server is a child of the session that started it, so it inherits that session's environment and sits under its process tree (seen: `ZAF_INSTANCE` and `ZAF_DEPTH` set on `claude -p` reached the server, and its parents were `claude` then the launch). A hub-served MCP would see neither, and every caller would get your rights (objectives Q3). `zaf mcp` is hidden from `zaf --help`, like `spawn phase-run`, so O1's "every verb in `zaf --help`" doesn't ask for a tool for it.

**D2. One tool per visible top-level verb, generated from clap.** The catalog walks `Cli::command()` and takes every subcommand that isn't hidden, but `help`. A tool's name is the verb. Its description is the verb's long help with each subcommand's usage line, rendered by clap. Its input is Q2's words: `args` (array of strings, the words after the verb), `stdin` (text for the verbs that read it: `epic file`, `board file`), and `json` and `more` (booleans). A test asserts that the tool names equal clap's visible verbs. A new verb therefore reaches the MCP with no MCP code, which is the check for "abstract enough for easy change".

**D3. A call runs the binary on disk as a child process.** The server builds `zaf [--json] [--more] <verb> <args…>`. It first parses that line with clap in-process: a usage error comes back as clap's message with exit 2, and nothing runs. Then it starts the child with the input's `stdin` and captures stdout, stderr and the exit code. Why a child and not an in-process call: the verbs print to stdout, which is the MCP channel, and a panicking verb must not take the server down. Every guard zaf has today runs in the child as it does from Bash: the action modes read `ZAF_INSTANCE`, sign checks the process tree, and the hub report is left by `dispatch` (`cli.rs:694-700`).

**D4. What a result carries.** A single text block: stdout, then stderr. When the exit isn't 0, a last line `exit <n>` follows and `isError` is true. The result's `_meta` holds `"zaf/exit": <n>` for programs that drive zaf (the task checks the spec's `_meta` naming rules). There is no `structuredContent`. Why: on Claude Code 2.1.330, a result carrying `structuredContent` showed the model only that JSON and dropped the text (seen). An error result shows its text inside `<error>` (seen). Exit codes keep their meanings (`exit.rs`: 0, 1, 2, 64, and for `do` 125 and the action's own).

**D5. Waits stay inside the call.** While the child runs, the server sends `notifications/progress` every 60 seconds with the call's `progressToken`, through `rmcp`'s peer. Claude Code sends that token on every `tools/call` (seen). Each notification counts as activity against the 30-minute idle limit for stdio servers (docs). The `message` is the child's last stderr line, or `<verb> waiting, <m> min`. No per-server `timeout` is set, because the docs call it a hard wall-clock limit that progress doesn't extend. The global default is about 28 hours, above zaf's longest wait (two hours, R19). When `rmcp` reports the call cancelled (`notifications/cancelled`), the server kills the child's process group. Each call runs on its own blocking task, so a two-hour `check` doesn't block other calls.

**D6. Where a call runs.** The child's directory is `ZAF_ROOT` when set (instances have it), else `CLAUDE_PROJECT_DIR` (Claude Code sets it for stdio servers, docs), else the server's own. Why: a programmer's session runs in its slice workspace, which holds a checkout with its own `.zaf.toml`. Discovery from there would find the workspace, not the project.

**D12. When zaf is updated under a running server.** The server runs the binary at its resolved path for every call (`binary::without_deleted`, as `update` and the scheduler already do). After an update, calls therefore run the new zaf at once. When the child's version differs from the server's own, each result gets one more line: `zaf was updated to <v>; restart zaf's MCP (/mcp) for the new tool list`. That meets O3's "answers from the new version once Claude restarts it", and the calls in between already run new code.

**D22. The protocol, by Q1: `rmcp`.** The server is built on `rmcp`, the official Rust SDK (3.11.0 on crates.io, 2026-11-17; features `server` and `transport-io`), pinned in `Cargo.lock`. `rmcp` owns JSON-RPC, the handshake, protocol versions and the stdio transport. zaf implements its server trait by hand, listing the tools from the catalog (D2) and answering a call through the app layer (D3), because the tools come from clap at run time and not from `rmcp`'s `#[tool]` macros. tokio enters zaf only here:
- the runtime is made inside `zaf mcp`, and every other verb stays on threads as today;
- the domain and the app layer import neither `rmcp` nor tokio, and `tests/architecture.rs` enforces it;
- a call's child process runs on a blocking task, so the `VerbRunner` port stays synchronous.

Claude Code 2.1.330 first sends `server/discover` (protocol 2026-07-28) and falls back to `initialize` with `2025-11-25` when that isn't answered (seen). Whichever versions `rmcp` speaks, the live test in E2 shows Claude Code reaching a tool. The `instructions` field (at most 4,096 characters, shown to the model at start, docs) carries D15's rule.

### Who calls, and what is refused

**D7. Who is calling.** Read once, when the server starts, from its own environment and process tree:

- `ZAF_INSTANCE` is set → an instance of that role. One that doesn't parse → every call is refused, so the server fails closed, as `caller.rs:35` does.
- No `ZAF_INSTANCE`, but a running launch sits above the server (`instance_above`, `app/epic.rs:270`) → every call is refused, because an instance has lost its name.
- Neither → an outside caller: the orchestrator, your own Claude, or any program. zaf's guards inside the child then treat it as you, as they do your Bash today.

**D8. A call through the MCP is never nested.** The child's environment is the server's minus `ZAF_DEPTH` and `ZAF_ACTION_CHAIN`. Why: every model runs inside an action body, which sets `ZAF_DEPTH` (`run.rs:1106`), and zaf skips its own `[forbidden]` refusal for a nested call (`do_action.rs:305-312`). That is why only the Bash text deny stops a programmer today (objectives Q2). With the markers dropped, `zaf do pr-merge` from a model is refused by zaf itself. Action bodies still nest as before, because they call the binary, not the MCP.

**D9. The policy table.** The server refuses before it starts a child, using the parsed command's path (`Verb::path()`, `cli.rs:548`, plus the action name for `do`), never its text. A refusal is exit 1 with one line: what was refused and the way on.

| Command                                                        | Instance | Outside caller | The way on                                                                                     |
| -------------------------------------------------------------- | -------- | -------------- | ---------------------------------------------------------------------------------------------- |
| `epic sign`                                                    | refused  | refused        | "Sign in zaf's chair."                                                                         |
| `begin` (not `begin ready`)                                    | refused  | refused        | "Objectives and design are the Engineer's interviews: open zaf's chair in a terminal (`zaf`)." |
| `hub` serving, `hub report`, `spawn --watch`                   | refused  | refused        | they never end, or are the hook's own; "`spawn start --version <v>` starts the scheduler"      |
| each `zaf …` line of `[forbidden]`                             | refused  | runs           | "Forbidden by the manifest; only `slice-merge` and `release` merge."                           |
| `board ruling`                                                 | refused  | runs           | "Rulings are the Engineer's: raise a question."                                                |
| `update`, `setup`, `profile`, `hub link`, `hub service`        | refused  | runs           | "It changes the machine; the Engineer runs it."                                                |
| `spawn` but `trace`, `board claim`, `board done`, `slice` (Q6) | refused  | runs           | "zaf moves tickets; nobody moves themselves."                                                  |

The action modes need no row: the child reads the inherited `ZAF_INSTANCE` as it does today. The terminal actions (`do objectives`, `do design`) are refused by zaf itself, since the child has no terminal. A `[forbidden]` line is matched by its words with flags dropped, as `forbidden_action` already reads `zaf do --tier machine land` (`run.rs:256-274`), but widened to every verb, so `zaf forge merge` matches too.

**D10. Signing is closed on every path.** The MCP refuses `epic sign` for every caller (D9). The verb `zaf epic sign` also refuses when `CLAUDECODE` is set or stdin isn't a terminal, on top of today's process-tree check. Claude Code sets `CLAUDECODE=1` in every process it starts, its Bash tool and stdio MCP servers included (docs; seen in the probe's environment). The chair signs through `epic::sign` directly (`cli/chair/source.rs:342`), not through the verb, so the check lives in the verb and the chair is unchanged. You can still sign with `zaf epic sign` from a terminal of your own. These are guards against a mistake, not against a process that clears its own variables (D21).

**D11. `begin` from the orchestrator.** It is refused, with D9's way on. `begin ready` stays open to every caller, because the phase session inside the chair calls it and it only writes the file `ZAF_READY` names.

**D21. Where the permission boundary is.** The MCP's policy is code over the parsed command, which is stronger than today's text deny. It is still not a boundary: a model with Bash can run the zaf binary by its path, and the Bash deny on `zaf` is a text match (`profile/mod.rs:1052` already says so). A real boundary, a hook or a sandbox, stays on 0.6.0's deferred list.

**D23. Lane verbs refused to instances, by Q6.** These are the last row of D9.

### Profiles, setup and prompts

**D13. Profiles carry zaf's MCP.** `zaf profile init` registers the server in each profile with Claude's own command, run through the Shell port with `CLAUDE_CONFIG_DIR` set to the profile, as it already installs the pack:

`claude mcp add-json --scope user zaf '{"type":"stdio","command":"zaf","args":["mcp"]}'`

Seen: with `CLAUDE_CONFIG_DIR` set, that writes `mcpServers.zaf` into the profile's own `.claude.json`. `command` is a bare `zaf`, as the profiles' hook already is (`zaf hub report`). The profile's settings change:

- `mcp__zaf__*` is added to `allow`, and `Bash(zaf *)` (`profile/mod.rs:740`) is removed.
- `Bash(zaf)` and `Bash(zaf *)` are added to `deny`.
- `disableClaudeAiConnectors: true` is set, and the project's `.mcp.json` server names go into `disabledMcpjsonServers` (Q4).
- `GENERATION` goes from 5 to 6, so every existing profile reads as stale and is regenerated.

A setting can't deny one subcommand of an MCP tool: Claude Code skips an `mcp__` rule with parentheses (docs). That is why D9's refusals live in the server.

**D14. Your own Claude session, by Q3.** `zaf setup` shows the line "register zaf's MCP in your Claude (user scope)" and runs the same `claude mcp add-json --scope user zaf …` on `[Y/n]`, unless `claude mcp get zaf` already shows it. `zaf doctor` gets a row: `mcp: ok (user scope)`, or what to run. Your session keeps Bash `zaf` (objectives: the orchestrator may use either).

**D15. One rule in every prompt.** The interface page (`zaf/assets/interface.md`, which every profile's CLAUDE.md, every brief and every role carries) and the server's `instructions` both say: "zaf's verbs are MCP tools. Wherever a page says `zaf <verb> <words>`, call the tool `<verb>` (`mcp__zaf__<verb>`) with `args` set to those words. Never type `zaf` in Bash; it is denied." The pack's own pages are rewritten in the tool form:

- the interface page's 35 mentions;
- the planning and challenger briefs;
- the role files;
- the skills;
- the action scripts' prompts (`implement`, `fix`, `review-epic`).

A project's role overrides keep working through the rule. None exists on this machine today: `.zaf/roles/` is empty in ways, dapple and zabo (seen 2026-11-22). `zaf doctor` notes any later one that tells a model to type `zaf` in Bash.

**D24. O2 and O5 are checked by a script, not by reading.** A ways tool, `tools/mcp-only-check <since>`, reads the transcripts of the instances that ran since `<since>` (each profile's `projects/`) and an orchestrator transcript given by path. It fails on any Bash tool call whose command runs `zaf`, and, in the orchestrator's transcript, on any lane tool (`spawn phase`, `spawn stop`, `board claim`, `board done`, `slice`, a phase action under `do`). It prints only the failures (memory: "automate, do not read").

### Live position

**D16. The hook gives MCP calls the same words.** `ToolCall` (`app/hub/hook.rs`) reads `mcp__zaf__<verb>` with `tool_input.args`, builds the same command words, and puts them through the word rules Bash commands already use (`hook.rs:56-90`). So `check pr` is "waiting on review", and `check <n>` and `verify` are "testing". Docs say the `*` PostToolUse hook fires for MCP tools. They don't state the shape of `tool_input` for an MCP call, so the task checks it on a real run.

**D17. Every role reports its position.** `zaf progress plan` and `progress at` take the task as optional. Without it, the ticket comes from `ZAF_INSTANCE` (`<role> instance <n> on #<id>`), which every role zaf runs has, the release instance included when it is launched by hand (`agents/release.md:36`). The chair draws the last declared phase of every running model on its ticket's row: a task's square, or for planning, challenger, release and the review Designer, the epic's line. Each role's brief says to declare its phases at the start and report each change (E5).

### Update

**D18. `zaf update --yes`.**

- It never reads stdin and needs no terminal.
- "Continue?" is yes.
- A machine action is replaced when the machine's copy matches the pack copy installed before the update (it's unedited) or when the action is new. It is kept when it differs from both the old pack copy and the new one, because that is your edit. The plan reads the old copy before the plugin step replaces it, so nothing new is stored ("derive state on read").
- The project's `[pack]` line is raised, as today.
- It ends on one line, for example: `zaf 0.5.4 → 0.5.5; replaced: plugin teams 2.3.0 → 2.4.0, actions ci, merge; kept: action fix (yours); pack line raised`. It exits 2 when a step failed and names the step.
- Inside an instance it changes nothing and says so, as today (`cli/update.rs:92`).

Which path hung in 0.5.4 isn't proven. 0.5.4 changes nothing when there is no terminal. The likely path is a pty that no one types into: every question waits forever in `ask_yes` (`cli/screen.rs:256`). The memory of 2026-11-22 records an update run on a pty that "hung on a later step". The first part of E1 reproduces the hang on 0.5.4 before anything is changed.

### The orchestrator's build

**D19. Starting and following a build.**

- **`zaf spawn start --version <v>`** starts the project's scheduler detached and returns its pid. If one is already running, it is handed back. It is the chair's `b` (`cli/chair/build.rs`), moved to the app layer so the chair and the verb share it. It keeps the chair's environment scrub (`build/machine.rs:48`) and also drops every `CLAUDE*` variable, so the scheduler doesn't carry the orchestrator's session.
- **`zaf epic wait <epic> [--minutes <n>]`** (Q7) returns when a question on the epic or its tasks waits on you, a problem shows, or the epic is done. It prints `epic show` and the open questions. With nothing by `--minutes` (default 60) it prints "nothing yet" and exits 0.

**O2's run:** you sign in the chair, the orchestrator calls `spawn start`, then loops on `epic wait`, `status` and `board ruling` with your words as you typed them. `do release` and `do ship` follow Q5.

### Bug reports

**D20. `bug-report` and `bug-report-query`, pack actions.**

- **Shipped and installed:** they ship in `plugins/teams/actions/`. Setup and update install them on the machine tier, like the other standard actions. Both are mode 755, so instances and the orchestrator can run them (Q9).
- **Settings:** they read `ZAF_SETTING_KIND`, `_URL`, `_REPO` and `_TOKEN_ENV`, from `[actions.bug-report]` (Q8). `bug-report-query` reads the same table, so one place serves both.
- **No settings:** each prints the missing keys and the table to add, files nothing, and exits 1.
- **`bug-report`:** called as `zaf do bug-report --title <t> --body <b> [--tags a,b] [--verb <verb or tool>] [--exit <n>]`. With no parameters, it prints "Please use the following parameters" and the list, files nothing, and exits 1.
  - zaf adds itself: zaf's version, the OS and architecture, the caller (`ZAF_INSTANCE`'s role, or "outside zaf"), and `Tags: a, b` as the body's first line.
  - Before posting, it writes your home folder as `~` and caps the body at 8,000 characters (Q9).
  - On Forgejo it posts with `curl` and the token. On GitHub it posts with `gh issue create -R <repo>`, with `GH_TOKEN` set from `token_env`, so it never posts as the machine's own `gh` login.
  - A tag becomes a label when the place has a label of that name. It never sets zaf's `type/` or `state/` labels, so a report on a project's own board is never scheduled.
  - It prints the issue's link and exits 0.
- **`bug-report-query`:** called as `zaf do bug-report-query --tag <t> [--all]`. It lists the open reports that carry the tag, as a label or on the `Tags:` line, one per line with its link; `--all` adds closed ones.
- **Tests:** both are tested against the fake forge and fake `gh` the pack tests already use. O6's own check runs against a scratch repository.

### Records

**D25. Docs and memory.** In the same changes:

- `docs/zaf-actions.md` gets a section on the MCP: the contract, the policy table, and how waits work;
- `docs/zuri-ai-framework.md` §7.3 says profiles carry zaf's MCP and nothing else (S10);
- a memory note "The MCP" is added and linked from `zaf-overview.md`, with the facts in "Facts checked".
</details>

<details>
<summary><b>Architecture: modules, interfaces, data shapes</b></summary>

zaf stays ports and adapters: the domain imports no outer layer (`tests/architecture.rs`).

### Modules

| Id  | Where                                                                                             | What it owns                                                                                                               | New or changed |
| --- | ------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | -------------- |
| M1 | `cli/mcp.rs` | the hidden verb `zaf mcp`: makes the tokio runtime, composes the server from the adapters and runs it | new |
| M2  | `cli/mcp/catalog.rs`                                                                              | the tool catalog from `Cli::command()`, and a call's words turned into a command line (I6)                                 | new            |
| M3  | `domain/mcp.rs`                                                                                   | `ToolSpec`, `ToolCall`, `McpCaller`, `CommandPath`, the policy rows and `Verdict`, `ToolResult`: pure types and rules      | new            |
| M4 | `app/mcp/` | the use case `ToolService`: a call from request to result, the policy applied, results shaped (D3 to D5, D9, D12) | new |
| M5 | `adapters/mcp_rmcp.rs` | the `rmcp` server over stdio: the handshake, tools listed from the catalog, a call handed to M4 on a blocking task, progress and cancel (I2, D22); the only module that imports `rmcp` and tokio | new |
| M6  | `adapters/verb_process.rs`                                                                        | runs the zaf binary on disk: the environment never nested, the directory, stdin, the 60-second tick, a kill on cancel (I3) | new            |
| M7  | `app/mcp/caller.rs`                                                                               | who calls: environment plus process tree, built on `Caller` and `instance_above` (I4, D7)                                  | new, reuses    |
| M8  | `cli/update.rs`, `app/update.rs`                                                                  | `--yes`, the answers port, edited actions, the last line (I7, D18)                                                         | changed        |
| M9  | `app/profile/`                                                                                    | server registration, allow and deny, connectors off, `GENERATION` 6 (D13)                                                  | changed        |
| M10 | `app/setup.rs`, `app/doctor.rs`                                                                   | user-scope registration for the Engineer, the doctor's `mcp` row and override note (D14, D15)                              | changed        |
| M11 | `app/hub/hook.rs`                                                                                 | words for MCP calls (I8, D16)                                                                                              | changed        |
| M12 | `cli/progress.rs`, `app/hub/phases.rs`, chair rows                                                | the task made optional, positions drawn on the epic's line (I12, D17)                                                      | changed        |
| M13 | `app/spawn/` (from `cli/chair/build.rs`), `cli/epic.rs`                                           | `spawn start`, `epic wait` (I11, D19)                                                                                      | new, moved     |
| M14 | `zaf/assets/interface.md`, `zaf/assets/spawn/*`, `plugins/teams/agents/*`, skills, action prompts | the prompts in tool form (D15)                                                                                             | changed        |
| M15 | `plugins/teams/actions/bug-report*`, `manifest` `[actions.<name>]`, `app/actions/run.rs`          | the two actions and settings handed to actions as environment (I9, I10, D20)                                               | new, changed   |
| M16 | `tools/mcp-only-check` (ways repo)                                                                | O2 and O5's transcript check (D24)                                                                                         | new            |
| M17 | `docs/`, `zaf/memory/`                                                                            | D25                                                                                                                        | changed        |

### Interfaces

| Id  | Interface                                           | Shape                                                                                                                                                                                                                                       |
| --- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| I1  | **The tool contract** (external; see One-way doors) | name `mcp__zaf__<verb>`; input `{args?: string[], stdin?: string, json?: bool, more?: bool}`; result one text block (stdout, stderr, `exit <n>` when not 0), `isError` when not 0, `_meta["zaf/exit"]`                                      |
| I2 | `ToolService` (app layer, driven by M5) | `list() -> Vec<ToolSpec>`; `call(ToolCall, progress: &dyn Fn(Tick), cancel: &Cancel) -> ToolResult`. M5 turns these into `rmcp`'s types, so a later SDK change stays in M5 |
| I3  | `VerbRunner` port                                   | `run(VerbRun { argv, stdin, cwd }, tick: &dyn Fn(Tick), cancel: &Cancel) -> VerbOutcome { stdout, stderr, exit }`. The adapter drops `ZAF_DEPTH` and `ZAF_ACTION_CHAIN`, and `Tick` carries the elapsed time and the last stderr line       |
| I4  | `CallerProbe` port                                  | `caller() -> Result<McpCaller, Refusal>`, where `McpCaller` is `Outside` or `Instance(RoleName)`                                                                                                                                            |
| I5  | `Policy::judge`                                     | `(caller, &CommandPath, &ForbiddenSection) -> Verdict { Run, Refuse { what, way_on } }`; the rows are data in M3                                                                                                                            |
| I6  | Catalog                                             | `Catalog::from_command(&clap::Command) -> Vec<ToolSpec>`; `ToolSpec::command_line(&ToolCall) -> Vec<OsString>`; the line is then parsed by clap to get `CommandPath`                                                                        |
| I7  | `UpdateAnswers`                                     | `proceed() -> bool`, `replace(&ActionDiff) -> bool`, `raise_pack(&PackChange) -> bool`; two adapters: `Asked` (today's questions) and `Assumed` (`--yes`, D18)                                                                              |
| I8  | Hook words                                          | `ToolCall::word()` takes `mcp__zaf__<verb>` plus `tool_input.args`, through the word rules Bash uses                                                                                                                                        |
| I9  | Action settings                                     | `[actions.<name>]` keys become `ZAF_SETTING_<KEY>` (upper case, `-` as `_`) in that action's run only                                                                                                                                       |
| I10 | `bug-report`, `bug-report-query`                    | D20's command lines and exit codes, declared in each action's `# zaf` metadata                                                                                                                                                              |
| I11 | New verbs                                           | `zaf spawn start --version <v>`; `zaf epic wait <epic> [--minutes <n>]`                                                                                                                                                                     |
| I12 | `progress`                                          | `zaf progress plan [<task>] <phase>…` and `progress at [<task>] <phase>`; the ticket comes from `ZAF_INSTANCE` when the task isn't given                                                                                                    |

### Data shapes

- **Profile `settings.json`:** `allow` gains `mcp__zaf__*` and loses `Bash(zaf *)`; `deny` gains `Bash(zaf)` and `Bash(zaf *)`; it adds `disableClaudeAiConnectors: true` and `disabledMcpjsonServers: [<the project's .mcp.json names>]`. `GENERATION` goes from 5 to 6.
- **Profile `.claude.json`, and the Engineer's `~/.claude.json`:** `mcpServers.zaf = {"type": "stdio", "command": "zaf", "args": ["mcp"]}`, written by `claude mcp add-json --scope user`, never by hand.
- **Machine config:** `[actions.bug-report]` with `kind`, `url`, `repo` and `token_env`. An older zaf that doesn't know the key refuses its verbs (seen before with `[release]`), so the machine is updated before the table is added.
- **No new store.** The MCP keeps nothing between calls, and `--yes` derives "edited" on read.
</details>

## One-way doors

- **The tool contract (I1).** The tool names, the input words and the result's text and `_meta` become what orchestrators build on. The server key `zaf` is part of every name (`mcp__zaf__<verb>`), which is why Q3 keeps it at user scope everywhere. Q2's words make the contract the verbs themselves, so it changes only when a verb does. Breaking it stays allowed while "WE are the only consumers", if 0.5.5 ships the replacement.
- **Who is calling (D1, D7).** stdio only; the caller comes from the same environment and process tree as the verbs; unknown or nameless instances fail closed. An MCP served over the network would have to carry identity some other way, and that's a new design, not a setting.
- **What a model may do (D9, D10, D23).** The policy table is where a model meets zaf now. A row removed later widens every instance at once.
- **What `bug-report` sends, and where (D20, Q8, Q9).** It posts outside the project with its own token. There is no default place, and only you configure one.
- **Profiles carry an MCP (D13, Q4).** S4 lifts the framework's rule for zaf's MCP only, and Q4 makes "only" true.

<details>
<summary><b>Facts checked</b> (seen here, read in the docs, or read in the code)</summary>

**Seen on this machine, 2026-11-22, Claude Code 2.1.330.** The probe was a 60-line stdio server run with `claude -p --mcp-config … --strict-mcp-config --model haiku`.

| Fact                                                         | Result                                                                                                                                                  |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A stdio server inherits the session's environment            | Yes: 119 variables, `ZAF_INSTANCE` and `ZAF_DEPTH` set on `claude -p` among them, plus `CLAUDECODE=1`, `CLAUDE_PROJECT_DIR` and the config's `env`      |
| The server sits under the session's process tree             | Yes: `python3 ← claude ← … ← zaf` (the launch)                                                                                                          |
| Claude Code sends a `progressToken`                          | Yes, in `_meta` of every `tools/call`, along with `claudecode/toolUseId`                                                                                |
| `structuredContent` alongside text                           | The model saw only `{"exit":0,"marker":"STRUCT-MARKER"}`; the text was dropped                                                                          |
| `isError: true`                                              | The model saw `<error>FAIL-TEXT: refused, exit 1</error>`                                                                                               |
| Protocol                                                     | `server/discover` (2026-07-28) first, then `initialize` with `2025-11-25` after a -32601                                                                |
| Lean profile, MCP tool with no allow rule, `-p`              | Refused: "Claude requested permissions to use mcp**probe**whoami, but you haven't granted it yet."                                                      |
| `CLAUDE_CONFIG_DIR=<dir> claude mcp add-json --scope user …` | Writes `mcpServers` into `<dir>/.claude.json`; `claude mcp get` shows "User config"                                                                     |
| What a lean-profile instance sees                            | Tools from seven claude.ai connectors (Q4). `claude mcp list` shows them Connected in the lean and build profiles, with basic-memory "Pending approval" |

**Read in the docs** (code.claude.com/docs/en/…, quoted by a research agent on 2026-11-22):

- `mcp`: "The idle window defaults to … 30 minutes for stdio servers"; a call that "sends no response and no progress notification for the idle window aborts with an error"; "The per-server `timeout` is a hard wall-clock limit per tool call, and progress notifications from the server don't extend it"; `MCP_TOOL_TIMEOUT` defaults to "about 28 hours".
- `mcp`: "An MCP tool call in the main conversation that is still running after two minutes moves to a background task". In `-p` that is off unless `CLAUDE_AUTO_BACKGROUND_TASKS` is set.
- `mcp`: the maximum output is "25,000 tokens" (`MAX_MCP_OUTPUT_TOKENS`), and a result over 50,000 characters is saved to a file.
- `mcp`: "Tool search is enabled by default … Only tool names and server instructions load at session start". Server instructions are capped at 4,096 characters.
- `mcp`: in `claude -p`, Claude Code "loads project-scoped servers without asking". `disabledMcpjsonServers` "blocks it in every permission mode". `disableClaudeAiConnectors` can be set "in any settings scope".
- `mcp`: a plugin's tools are named `mcp__plugin_<plugin-name>_<server-name>__<tool-name>`.
- `permissions`: "To match every tool from an MCP server, write `mcp__<server>__*`", and "it skips any `mcp__` rule that has parentheses".
- `hooks`: MCP tools "appear as regular tools in tool events (`PreToolUse`, `PostToolUse`, …)". The shape of `tool_input` for an MCP call is not stated.
- `env-vars`: `CLAUDECODE` is "Set to `1` in subprocesses Claude Code spawns … stdio MCP server subprocesses". Settings `env` values are copied into Claude Code's environment.
- modelcontextprotocol.io 2025-06-18: progress needs the client's `progressToken`, and "The `progress` value **MUST** increase with each notification". A tool error is a result with `isError: true`. The latest spec is 2026-07-28.
- github.com/modelcontextprotocol/rust-sdk and crates.io: `rmcp` 3.11.0, released 2026-11-17, needs Rust 1.88. tokio is a required dependency; `transport-io` adds `tokio/io-std`, and `server` adds `schemars` and `uuid`.

**Not verified, and checked by the task that needs it:**

- whether a connector tool marked read-only is refused in `-p` (Q4 removes the question);
- the shape of `tool_input` for MCP calls (D16);
- the spec's rules for a `_meta` key (D4);
- whether Claude Code's Bash tool gives a terminal on stdin (D10 doesn't depend on it, because `CLAUDECODE` is checked first);
- which path hung in 0.5.4 (D18).

**Read in the code** at main 8932edf1. `cli.rs:575-700` is dispatch and the hub report; `cli/exit.rs` holds the exit codes. `app/actions/caller.rs`, `app/epic.rs:268-309` and `app/actions/run.rs:256-292` and `:1106` cover the caller, sign's guard, `[forbidden]` and nesting. The others: `cli/do_action.rs:305-312`, `app/profile/mod.rs:740`, `:879`, `assets/profiles/*/settings.json`, `app/hub/hook.rs:56-90`, `cli/progress.rs`, `cli/update.rs:92`, `:420-500`, `cli/screen.rs:256`, `cli/chair/build.rs`, and `plugins/teams/actions/implement:64`, `fix:52` and `review-epic:38`, where each model is started with `CLAUDE_CONFIG_DIR` set to its profile.

</details>

## Epics

| Id       | Epic                                               | Serves | Waits on                   |
| -------- | -------------------------------------------------- | ------ | -------------------------- |
| E1 #1002 | `zaf update --yes`                                 | O3     | nothing, first in the lane |
| E2 #1003 | The MCP server: one tool per verb, the same output | O1     | E1                         |
| E3 #1004 | Who calls the MCP, and what it refuses             | O2, O5 | E2                         |
| E4 #1006 | Profiles and setup carry zaf's MCP                 | O2, O5 | E3                         |
| E5 #1007 | Every prompt uses zaf's MCP tools                  | O5, O4 | E4, E6                     |
| E6 #1005 | Every model's live position, through the MCP       | O4     | E2                         |
| E7 #1008 | The orchestrator builds a signed epic              | O2     | E3                         |
| E8 #1009 | `bug-report` and `bug-report-query`                | O6     | E1                         |

E3 must land before E4. If E4 landed first, instances would lose Bash `zaf` and gain an MCP that doesn't yet refuse `forge merge`. The build itself runs on the installed 0.5.4, so the order guards the release, not the run. Each epic's body on the board carries its parts with a "Done when" each, and points back here by D and Q number.

<details>
<summary><b>Risks</b></summary>

- **A young dependency.** `rmcp` 3.11.0 is five days old at this writing, and it brings tokio, `schemars` and `uuid`. zaf's release builds Linux (musl) and macOS, and macOS is compiled only at release time (memory, 0.5.3). E2 adds a local macOS compile check of the crate with `rmcp` in it, and the live `claude -p` test fails loudly if Claude Code can't reach a tool.
- **A big result.** A result over 50,000 characters goes to a file Claude must read (docs). Bash behaves the same, so nothing gets worse.
- **Mixed versions after an update (D12).** The new binary runs under the old tool list until a restart. An argument that changed between versions gives clap's usage error (exit 2), with D12's line saying to restart.
- **A model that clears its own variables** gets past D10 and D7's instance check. That's no worse than today; the boundary is 0.6.0's (D21).
</details>
