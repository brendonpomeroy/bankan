# Research: what a tracker needs to hold

This note covers what BanKan should store and how flexible it should be. It looks at how established trackers (Linear, Plane, Jira, GitHub Issues) model tickets, and how RFC/ADR processes model proposals. It then picks the subset that's worth having in a local, single-user, AI-assisted tool.

## 1. Workflow states

**What others do**
- **Linear:** statuses are custom per team, but every status belongs to a *fixed category*: `triage`, `backlog`, `unstarted`, `started`, `completed`, `canceled` (plus a reserved `duplicate`). The default flow is Backlog → Todo → In Progress → Done → Canceled. Each category must keep at least one status. Categories have a fixed order, and statuses can be reordered within one. ([Linear: configuring workflows](https://linear.app/docs/configuring-workflows))
- **Plane:** the same model with five groups: Backlog, Unstarted, Started, Completed, Cancelled. ([Plane: states](https://docs.plane.so/core-concepts/issues/states))

**Why categories matter:** a status's name is free text ("In Review", "Ready to merge", "Won't fix"), but the tool still needs to know whether a ticket is open, active or closed. With categories you can filter "open tickets", hide closed columns, count progress and let an AI ask "what's in progress?" without knowing the project's custom names.

**BanKan decision**
- Statuses are **custom per project**: name, category and colour, in board order.
- The categories are fixed: `backlog`, `unstarted`, `started`, `completed`, `canceled`.
- The default set is Backlog (backlog), Todo (unstarted), In Progress (started), In Review (started), Done (completed) and Canceled (canceled).
- Triage is left out. With one user, "Backlog" covers it.

## 2. Ticket fields

| Field | Linear | Plane | Jira | GH Issues | BanKan |
|---|---|---|---|---|---|
| Title, description (Markdown) | ✓ | ✓ | ✓ | ✓ | ✓ |
| Status (category-backed) | ✓ | ✓ | ✓ | open/closed | ✓ |
| Type (bug/feature/task…) | labels | work item types | issue types | issue types | ✓ custom per project |
| Priority | none/urgent/high/medium/low | same | 5 levels | — | ✓ Linear's 5 |
| Labels | ✓ | ✓ | ✓ | ✓ | ✓ custom, coloured |
| Assignee | ✓ | multiple | ✓ | multiple | ✓ free text (e.g. `me`, `claude`) |
| Estimate | points | points/time | points/time | — | ✓ number |
| Due date | ✓ | ✓ | ✓ | — | ✓ |
| Parent / sub-issues | ✓ | ✓ | epics/subtasks | sub-issues | ✓ `parent` |
| Relations | blocks, related, duplicate | blocking, relates, duplicate | links | — | ✓ `blocks`, `related`, `duplicate_of` (`blocked_by` derived) |
| Milestone / cycle | projects, cycles | cycles, modules | sprints, versions | milestones | ✓ milestones (no time-boxed cycles) |
| Comments | ✓ | ✓ | ✓ | ✓ | ✓ with author (you or the agent) |
| External links (PRs, commits) | ✓ | ✓ | ✓ | auto | ✓ `links` list |
| Git branch | ✓ generated | — | dev panel | linked | ✓ `branch` |
| Custom fields | limited | ✓ per type | ✓ | ✓ (projects) | ✓ per project: text, number, select, date, url |

Sources: [Linear issue properties](https://linear.app/docs/issue-property), [Plane properties](https://docs.plane.so/core-concepts/issues/properties), [Plane work item types](https://docs.plane.so/core-concepts/issues/issue-types).

**Deliberately left out:** time tracking, multiple assignees, time-boxed cycles/sprints with burndown, permissions and teams. These are team-scale features. A single user with agents doesn't need them, and they make the data model more complex. Custom fields cover the edge cases.

## 3. Ticket identity

Every tracker gives tickets a short, human-friendly key (`ENG-123`). This matters even more with AI agents: commits, branch names, chat and MCP tool calls all refer to a ticket by key.

**BanKan decision:** each project has a short uppercase key (`RES`). Tickets are `RES-1`, `RES-2`… from a per-project counter, and numbers are never reused. Proposals use their own counter: `RES-P1`, `RES-P2`…

## 4. Proposals (RFC/ADR)

**What others do:** ADR and RFC processes share one lifecycle: **draft → proposed (in review) → accepted / rejected**, and later **superseded** (by a newer one) or **abandoned/withdrawn**. Rejected proposals are kept, not deleted, so you have a record of what was decided against. Templates usually include Summary/Context, Decision/Proposal, Alternatives, Consequences, and metadata (authors, dates, `superseded_by`). ([Cosmos ADR process](https://docs.cosmos.network/v0.46/architecture/PROCESS.html), [cheqd ADR template](https://docs.cheqd.io/node/architecture/adr-list/adr-template))

**BanKan decision**
- Proposal statuses: `draft`, `review`, `accepted`, `rejected`, `withdrawn`, `superseded`, `implemented`. They're a fixed set, so an AI and the UI can both work with them.
- Fields: title, status, authors, labels, `supersedes`/`superseded_by`, linked `tickets`, `decided` date, and the Markdown body.
- New proposals start from a template: Summary, Motivation, Proposal, Alternatives considered, Risks & drawbacks, Open questions, Decision.
- "Create tickets from proposal" links the new tickets back to the proposal. That's the main way an accepted proposal turns into work.

## 5. Docs

Docs are free-form Markdown pages, stored per project in nested folders (`guides/setup`). Metadata: title, tags, created/updated dates. They aren't versioned on their own, because git backup provides history.

## 6. Linking code repos to projects

**Your requirement:** the *repo* points at the BanKan project. The server never stores repo paths.

**BanKan decision:** a one-line `.bankan` file in a repo (`project = "RES"`) links it to a project. `bankan mcp` (the agent's MCP entry point) walks up from its working directory to find that file and uses it as the default project. Every MCP tool still accepts an explicit `project` argument. If you don't want the file committed, add it to your global gitignore. It never changes once written, so it can't cause merge conflicts.

## 7. Storage format

The options were SQLite, JSON, or Markdown files with YAML front matter.

**BanKan decision: Markdown files with YAML front matter.** The reasons:
- git backup produces meaningful diffs and history
- you can read and edit files by hand, grep them, and open them in Obsidian
- they're easy to recover from

The daemon keeps everything in memory (a few thousand files take milliseconds to load) and writes each change straight back to disk. The data stays tiny: even 10k tickets is a few MB.
