# Teams: <project>

<!-- The teams plugin's whole configuration for this project: settings, roster, opt-outs.
     Section titles and keys are fixed; the Orchestrator reads them by name. Edit values freely.
     "—" means unset. Written once by /teams:orchestrate; never regenerated over your edits. -->

## Repository

| Key | Value | Notes |
|---|---|---|
| owner | engineer | `engineer` (the Engineer's own repo: this file and the docs are committed) or `others` (both kept out of git) |
| docs | `.teams` | queue, releases/, retros/ live here (outside `.claude/`, whose writes always prompt) |
| style | — | the project's style rules, read by every agent, e.g. `docs/style.md` |
| decisions | — | where the project records decisions, e.g. `docs/adr/`; — = the rulings files only |
| vcs | git | `git` or `jj` |
| main | main | the default branch; never pushed to directly |
| workspaces | `../{repo}-workspaces` | one workspace per programmer, named after them |
| gate | — | exactly what CI runs, all of it |
| light gate | — | optional, for restacks and fix rounds; — = the gate |

## Branches

| Key | Value | Notes |
|---|---|---|
| strategy | integration | `integration` (a feature branch fed by slice PRs) or `trunk` (one PR per slice into main) |
| integration | `feature/{slug}` | cut at main's tip |
| slice | `feature/{slug}-{n}-{part}` | its PR targets the integration branch |
| engineer reviews | main | PRs into these branches are the Engineer's to review and merge |
| merge slice | merge | `merge`, `squash` or `rebase`, for PRs the Orchestrator merges |
| merge main | squash | for PRs into the Engineer's branches |

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

<!-- A Model or Effort set here is passed as the Agent call's model or effort; — = the agent file's. -->

## Limits

| Key | Value | Notes |
|---|---|---|
| max lanes | 2 | lanes running at once |
| small lanes | 4 | allowed when every lane is small: one or two agents on a short slice |
| internal after | 3 | new retrospectives that wake the internal team's lead |

## Reviews

| Key | Value | Notes |
|---|---|---|
| release reviews | on | compliance, architecture and security at the end of each release |
| security triggers | authentication; authorization and permissions; sessions, tokens, secrets and keys; cryptography; untrusted input; trust boundaries; network egress; personal data storage | a slice that touches one gets a security review; first run fits these to the project |
| compliance triggers | personal data; payments; accessibility; a new dependency's license | a slice that touches one calls compliance in early; first run fits these to the project |
| bot | — | a review bot requested on PRs into the Engineer's branches, e.g. copilot |
| comment prefix | `Claude:` | starts every comment posted through the Engineer's accounts |

## Teams

The Orchestrator is the tech lead; the Engineer is Product Owner, manager and stakeholder. A team is one lead, 2–3 programmers and one QA; members keep their names and are spawned only when there's work.

| Team | Mission | Owns (paths) | Lead | Programmers | QA |
|---|---|---|---|---|---|
| <team> | <one line> | `<path/**>` | <Name> | <Name>, <Name> | <Name> |
| internal | Finds the most effective way for the teams to work and spend their time | — | <Name> | <Name>, <Name> | <Name> |
| compliance | Release review: what the project is legally bound to follow | — | <Name> | <Name> (reviewer) | <Name> |
| architects | Release review: correct architecture | — | <Name> | <Name> (reviewer) | <Name> |
| security | Every security-triggered slice, big merges, and the release review | — | <Name> | <Name> (reviewer) | <Name> |

**Shared ground** (no team owns it; one Spawn at a time, assigned by the Orchestrator): <lockfiles, migrations, generated files, root config, CI>.

## Opt-outs

<!-- One line each: what, the date, the Engineer's words. Recognised: internal team, nodes, release reviews. -->

- —
