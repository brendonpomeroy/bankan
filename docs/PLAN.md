# BanKan: plan

BanKan is a local, lightweight ticket tracker with Kanban boards, docs and proposals for all your projects. It runs as one background process with a React web UI and an MCP server for AI agents. Nothing lives in your code repos except an optional one-line link file.

## Goals

1. **One long-running process** that starts automatically at login and holds the data for every project.
2. **A React web UI** with project boards (drag and drop), a list view, a ticket editor, docs and proposals.
3. **MCP support**, so Claude Code and other agents can read and write tickets, docs and proposals.
4. **No repo conflicts.** Data lives in `~/.bankan`. A repo only holds the `.bankan` link file, which never changes.
5. **Optional git backup** of all data to a repo, run periodically.
6. **Lightweight:** a single Rust binary with the UI built in, using about 10–20MB of RAM.

Out of scope: multi-user use, authentication, teams, time tracking, cycles/sprints, hosted deployment.

## Architecture

```
┌──────────────────────────── bankan serve (launchd, 127.0.0.1:7777) ───────────────────────────┐
│  axum HTTP                                                                                     │
│   ├── /            embedded React app (rust-embed)                                             │
│   ├── /api/*       REST JSON API ──┐                                                           │
│   └── /api/events  SSE "changed" ──┤                                                           │
│                                    ▼                                                           │
│                       Store (in-memory, RwLock) ── writes Markdown+YAML ──► ~/.bankan/data/    │
│                                                                                 ▲              │
│  backup task (every N min): git add/commit [+ push] ────────────────────────────┘              │
└────────────────────────────────────────────────────────────────────────────────────────────────┘
          ▲ HTTP                                                    ▲ HTTP
   Browser / Dock web app                         bankan mcp (stdio, started by Claude Code per repo)
                                                    reads ./.bankan → default project
```

- **`bankan serve`** is the daemon. It owns the data, so nothing else writes to the files.
- **`bankan mcp`** is a thin stdio MCP server. Claude Code starts it with the repo as its working directory, so it can find `.bankan` there. It forwards tool calls to the daemon over HTTP. This keeps a single writer and means the MCP side never needs repo paths stored on the server.
- **The UI** refreshes live through Server-Sent Events, so you can watch an agent move cards.

## Data layout

```
~/.bankan/
  config.toml                 # port, backup settings
  data/                       # ← the git-backed folder
    projects/
      RES/
        project.yaml          # name, key, statuses, types, labels, milestones, custom fields, counters
        tickets/RES-12.md     # YAML front matter + Markdown description
        proposals/RES-P3.md
        docs/guides/setup.md
```

The full format is in [DATA-FORMAT.md](DATA-FORMAT.md). The reasoning for the fields is in [RESEARCH.md](RESEARCH.md).

## Components

| Component | Tech | Notes |
|---|---|---|
| CLI | `clap`, `cliclack`, `console` | `setup` (interactive wizard), `serve`, `mcp`, `install`, `uninstall`, `link`, `unlink`, `open`, `status`, `backup` |
| HTTP | `axum` 0.8, `tokio` | binds to 127.0.0.1 only; checks Host and Origin to block DNS-rebinding and cross-site requests |
| Store | `serde`, `serde_yaml`, `chrono` | atomic writes (temp file + rename), per-project counters |
| Live updates | `tokio::sync::broadcast` + SSE | event `{kind, project, id}` |
| Backup | shells out to `git` | uses your existing git and SSH credentials, so there's no auth code |
| MCP | hand-written JSON-RPC over stdio | protocol versions 2024-11-05 to 2025-06-18; tools and prompts |
| Autostart | launchd plist on macOS, systemd user unit on Linux | `bankan install` |
| Web | React 18, Vite, TypeScript, TanStack Query, dnd-kit, react-markdown | built into `web/dist`, then embedded |

## REST API (summary)

```
GET    /api/status
GET    /api/projects                     POST /api/projects
GET    /api/projects/:key                PATCH /api/projects/:key   DELETE (deletes the folder; archive with PATCH {archived: true})
GET    /api/projects/:key/tickets        POST /api/projects/:key/tickets
GET    /api/tickets/:id                  PATCH /api/tickets/:id     DELETE /api/tickets/:id
POST   /api/tickets/:id/comments
POST   /api/tickets/:id/move             {status, index?} → reorder (index within the target column)
GET    /api/projects/:key/docs           GET/PUT/DELETE /api/projects/:key/docs/*path
GET    /api/projects/:key/proposals      POST /api/projects/:key/proposals
GET    /api/proposals/:id                PATCH /api/proposals/:id   DELETE /api/proposals/:id
GET    /api/search?q=&project=
POST   /api/backup
GET    /api/events                       (SSE)
```

## MCP tools

`context`, `list_projects`, `create_project`, `list_tickets`, `get_ticket`, `create_ticket`, `update_ticket`, `add_comment`, `list_docs`, `get_doc`, `write_doc`, `list_proposals`, `get_proposal`, `create_proposal`, `update_proposal`, `search`. Prompts: `next`, `wrap`, `plan`.

The tools exclude deletes on purpose. Destructive actions stay in the UI. The first draft had `current_project` and `get_project`; following [AGENT-INTERFACE.md](AGENT-INTERFACE.md), they were merged into one `context` tool. The full reference is in [MCP.md](MCP.md).

How the tools should format results, page through long lists and help agents find them is covered in [AGENT-INTERFACE.md](AGENT-INTERFACE.md).

## UI

- **Home:** every project with open/in-progress/done counts, plus recently updated tickets across projects.
- **Board:** one column per status, cards you drag within and between columns, and filters (search, type, priority, label, assignee). You can collapse closed columns and add a card inline.
- **List:** a sortable table of the same tickets.
- **Ticket panel:** edit the title and Markdown description; set fields (status, type, priority, labels, assignee, estimate, due, milestone, parent); manage relations, links, branch, custom fields and comments.
- **Docs:** a folder tree, a Markdown view and editor.
- **Proposals:** grouped by status, with an editor, linked tickets and "create ticket from proposal".
- **Settings:** project name and description; edit statuses, types, labels, milestones and custom fields; the link instructions.
- **Global:** `⌘K` search, light/dark theme.

## Build milestones

1. **M1 Core:** repo scaffold, store and file format, REST API, SSE, Host/Origin guard.
2. **M2 MCP:** the stdio MCP server, `.bankan` resolution, and `link`.
3. **M3 Web:** React app with all the views above, embedded in the binary.
4. **M4 Ops:** `install`/`uninstall` (launchd/systemd), git backup, `status`/`open`/`backup`.
5. **M5 Docs and verification:** README, data format, MCP guide, end-to-end checks (API, MCP over stdio, UI build).

## Decisions made

| Question | Decision |
|---|---|
| Name / location | BanKan, `~/Documents/projects/bankan` |
| Web stack | React (your call) |
| Information model | see [RESEARCH.md](RESEARCH.md) |
| Repo ↔ project link | the repo holds `.bankan` (your call); the server stores no repo paths |
| Storage | Markdown + YAML front matter (for git diffs) |
| MCP transport | stdio shim → daemon HTTP (so the working directory is known and there's one writer) |
| Port | 7777 by default, configurable |

## Build status (2026-10-22)

All five milestones are built.

| Area | Status | How it was checked |
|---|---|---|
| Store, file format, REST API | Done | Unit tests (`cargo test`); curl against a running server |
| SSE live updates | Done | The UI refreshes on events. Watched in the browser, not tested automatically |
| Host/Origin guard | Done | curl with a foreign `Origin` and with a foreign `Host` both get 403 |
| MCP server (16 tools, 3 prompts) | Done | Driven over stdio: every tool's output shape, errors, `section` reads, prompts |
| Web UI | Done | Type-checks cleanly; every page screenshotted in light and dark with Playwright; no console errors |
| Drag and drop | Done | Playwright drags between columns and within a column; the order survives a reload |
| Git backup | Done | Commits and pushes to a local bare remote; final backup on shutdown |
| `setup` wizard | Done | Driven with `expect`, declining autostart and Claude. The autostart and `claude mcp add` steps themselves weren't run on this machine |
| launchd / systemd install | Written | Not run, to avoid installing a service during development |
| Agent evals | Not run | See [EVALS.md](EVALS.md) |
| Review fixes: write-then-commit store, link sync, strict inputs, URL checks, stale-settings 409, backup timeouts | Done | `cargo test` (20 tests); curl against a scratch server for 400/409 responses |
| Review fixes in the UI: optimistic edits, conflict notices, drag order, a11y | Done | Type-checks and builds; Playwright screenshots in light and dark show no console errors; drag between and within columns survives a reload; MCP smoke test passes against the strict API |

### Known gaps

- Files edited by hand aren't picked up until the server restarts. There's no file watcher yet.
- There are no MCP resources (`@bankan:ticket://…` mentions) yet.
- The CLI has no ticket subcommands for agents without MCP. They're low priority (see AGENT-INTERFACE.md section 4).
