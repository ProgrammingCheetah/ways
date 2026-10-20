# Teams: <project>

<!-- The teams plugin's whole configuration for this project: settings, teams, opt-outs.
     Section titles and keys are fixed; the Orchestrator reads them by name. Edit values freely.
     "—" means unset. Written once by /teams:orchestrate; never regenerated over your edits.
     A file written before QA moved to the top has a QA column: drop it and name the top-level QA in the qa row. -->

## Repository

| Key | Value | Notes |
|---|---|---|
| owner | engineer | `engineer` (the Engineer's own repo: no remote, or its push remote's owner is the Engineer's forge login or an org in `WAYS_OWN_ORGS`, a fork counted by its parent's owner, never its own, and never judged by permission; this file and the docs are committed) or `others` (both kept out of git) |
| docs | `.teams` | queue, releases/, retros/ live here (outside `.claude/`, whose writes always prompt) |
| style | — | the project's style rules, read by every agent, e.g. `docs/style.md` |
| decisions | — | where the project records decisions, e.g. `docs/adr/`; — = the rulings files only |
| config dir | `.` | root of this pack's configuration files, relative to the project root; each team's Config (**Teams**) resolves against it |
| vcs | git | `git` or `jj` |
| main | main | the default branch; never pushed to directly |
| workspaces | `../{repo}-workspaces` | one workspace per branch, named after it (`/` as `-`); its reviewers and fix rounds reuse it |
| teardown | — | optional: commands run inside a workspace before `/teams:cleanup` removes it, e.g. `docker compose -p {name} down -v` (`{name}`: the workspace folder's name; `{path}`: its absolute path; both substituted single-quoted, so don't quote them). Several: each in its own backticks, separated by `;` |
| gate | — | exactly what CI runs, all of it |
| light gate | — | optional, for base merges and fix rounds; — = the gate |

## Branches

| Key | Value | Notes |
|---|---|---|
| strategy | integration | `integration` (one branch per release, fed by slice PRs; one PR into main at release end) or `trunk` (a PR per slice into main, the Engineer's; milestone QA reviews what merged, and its fixes are new PRs) |
| integration | `release/{release}` | cut and pushed at main's tip before the release's first slice; main is merged in after each off-cycle merge. `{release}`: the version as `Release › version format` writes it |
| slice | `{team}/{release}-{n}-{part}` | its PR targets the integration branch. `{team}`: the team's name, `/` as `-`. An off-cycle branch matches none of these patterns |
| fix | `{team}/{release}-m{n}-fix` | a milestone fix round's branch; its PR targets the integration branch |
| engineer reviews | main | PRs into these branches are the Engineer's to review and merge |
| merge slice | merge | `merge`, `squash` or `rebase`, for PRs the Orchestrator merges |
| merge main | squash | for PRs into the Engineer's branches; a squashed release PR is one commit on main, `merge` keeps its slices |

## Tracker

| Key | Value | Notes |
|---|---|---|
| kind | none | `none`, `github`, `jira`, `linear` or another name |
| project | — | e.g. an issue project key |
| commit format | — | e.g. `[{ticket}][{n}] {title}`; — = the repo's own convention |

## Release

| Key | Value | Notes |
|---|---|---|
| version format | `v{version}` | e.g. `{stage}-v{version}` |
| stages | — | e.g. pre-alpha, alpha, beta, release |
| documents | design | one design document; or e.g. pdd, tdd, plan |
| format | markdown | `markdown` (with Mermaid) or `typst` (needs Typst installed) |

## Lanes

| Lane | Agent | Model | Effort |
|---|---|---|---|
| plan | `teams:planner` | — | — |
| judgment | `teams:designer` | — | — |
| build | `teams:builder` | — | — |
| subsystem | `teams:myth` | — | — |
| glue | `teams:machinery` | — | — |

<!-- A Model or Effort set here is passed as the Agent call's model or effort; — = the agent file's.
     Only programmers may run on Sonnet: a build or subsystem Model pins every programmer to it
     (e.g. opus only); — = the build lead picks sonnet or opus per task. Security-nature work and
     every other role are always Opus, whatever is set here (their Effort may still be set). -->

## Limits

| Key | Value | Notes |
|---|---|---|
| max lanes | 2 | lanes running at once |
| small lanes | 4 | allowed when every lane is small: one or two agents on a short slice |
| internal after | 3 | new retrospectives that wake the internal team's lead |

<!-- The Orchestrator gives each build lead a programmer budget (how many at once) that fits these. -->

## Reviews

| Key | Value | Notes |
|---|---|---|
| release reviews | on | compliance, architecture and security at the end of each release, before its last QA |
| security triggers | authentication; authorization and permissions; sessions, tokens, secrets and keys; cryptography; untrusted input; trust boundaries; network egress; personal data storage | a slice that touches one gets a security review of its plan and its code; first run fits these to the project |
| compliance triggers | personal data; payments; accessibility; a new dependency's license | a slice that touches one calls compliance in early; first run fits these to the project |
| bot | — | a review bot requested on PRs into the Engineer's branches, e.g. copilot |
| comment prefix | `Claude:` | starts every comment posted through the Engineer's accounts |

## Teams

The Orchestrator is the tech lead; the Engineer is Product Owner, manager and stakeholder. A team is a named lead, spawned only when there's work. The qa row is the top-level QA and the architects' lead is the architect: both stand beside the Orchestrator, at every milestone and the release. Programmers aren't listed: each is short-lived, spawned and named by its build lead for one task, and ends when it's done. Review teams list their standing reviewers. Config is the team's config file (its Roles and the special instructions each must follow, shaped by the pack's `team-config.md`), relative to `Repository › config dir`, in this file's format; — = none.

| Team | Mission | Owns (paths) | Lead | Reviewers | Config |
|---|---|---|---|---|---|
| <team> | <one line> | `<path/**>` | <Name> | — | — |
| qa | Top-level QA: documents for the Engineer, every milestone, every PR into main | — | <Name> | — | — |
| internal | Finds the most effective way for the teams to work and spend their time | — | <Name> | — | — |
| compliance | Release review: what the project is legally bound to follow | — | <Name> | <Name> | — |
| architects | Reviews every milestone, before QA; release review: correct architecture | — | <Name> | <Name> | — |
| security | Every security-triggered slice (its plan, then its code), and the release review | — | <Name> | <Name> | — |

**Shared ground** (no team owns it; one Spawn at a time, assigned by the Orchestrator): <lockfiles, migrations, generated files, root config, CI>.

## Opt-outs

<!-- One line each: what, the date, the Engineer's words. Recognised: internal team, nodes, release reviews, main's protection. -->

- —
