# MCP reference

`bankan mcp` is a stdio MCP server, protocol versions `2024-11-05` to `2025-06-18`. Your AI client starts it with the repo as its working directory. It looks for the repo's `.bankan` file and forwards every call to the BanKan server over HTTP, so the server stays the only process that writes data.

Why the outputs look the way they do is covered in [AGENT-INTERFACE.md](AGENT-INTERFACE.md). In short:
- Lists come back one line per item, with a paging footer.
- Single items come back as front matter plus raw Markdown, never Markdown escaped inside JSON.
- Writes return a one-line summary of what changed.
- Errors are short and name the valid values.

## Setup

```bash
claude mcp add bankan --scope user -- bankan mcp
```

Other clients:

```json
{ "mcpServers": { "bankan": { "command": "bankan", "args": ["mcp"] } } }
```

`bankan mcp` finds the server through these, in order:
1. `$BANKAN_URL`
2. the `url` key in `.bankan`, which must be a `http://localhost` or `http://127.0.0.1` address. A `.bankan` file comes with the repo, so a cloned repo could otherwise send your agent's tool calls to someone else's server. Other addresses are ignored with a warning on stderr; use `$BANKAN_URL` if you really need one.
3. the port in `~/.bankan/config.toml`

## Project resolution

Every tool that works on a project takes an optional `project` key. Without one, it uses the project from the nearest `.bankan` file, searching from the working directory upwards. If neither exists, the tool returns an error that explains how to link the repo.

A `.bankan` file may also name the repo's `component` (`bankan link HH --component apps`) when one project spans several repos. Then `create_ticket` sets that component unless told otherwise, and `context` says which component the repo is and lists its in-progress tickets first. Nothing is filtered implicitly: `list_tickets` shows every component unless you pass `component`.

## Tools

Arguments are checked strictly. A misspelt argument such as `assigne` is an error that lists the valid names, rather than being silently ignored. Ticket `links` and `url` custom fields must be `http://`, `https://` or `mailto:` links.

All tools are annotated `destructiveHint: false`. Read tools are also `readOnlyHint: true`, and `write_doc` is `idempotentHint: true`.

| Tool | Purpose | Returns |
|---|---|---|
| `context` | The linked project's statuses (with categories), types, labels, milestones, custom fields, counts, in-progress tickets, proposals awaiting review, locked decisions (accepted proposals labelled `locked`, up to 30) and docs to read first (tagged `start-here`). Loads at session start (`anthropic/alwaysLoad`). | text summary |
| `list_projects` | All projects with counts. | one line each |
| `create_project` | `key`, `name`, `description?` | confirmation |
| `list_tickets` | Filters: `status`, `category`, `type`, `priority`, `label`, `assignee`, `milestone`, `parent`, `component`, `query`, `include_closed`. Plus `detail: concise\|full` and `cursor`. Shows open tickets only by default, 25 per page. | `KEY \| status \| type \| priority \| @assignee \| #labels \| title` |
| `get_ticket` | `id` | front matter + description + last 10 comments |
| `create_ticket` | `title` plus any ticket field, and an optional `comment`. `component` defaults to the repo's (see below). | `Created RES-13 in Todo: …` |
| `update_ticket` | `id` plus the fields to change. Pass `null` to clear a field. List fields are replaced. Takes an optional `comment`. | `Updated RES-12: status Todo → In Progress, assignee → claude` |
| `add_comment` | `id`, `body`, `author?` | `Added comment #3 to RES-12.` |
| `list_docs` | | `path \| title \| #tags \| updated` |
| `get_doc` | `path`, `section?` (a heading or section number such as `§15` or `3.1`, to read only that part) | front matter + Markdown |
| `write_doc` | `path`, `title?`, `body?`, `tags?`. Creates or replaces the doc. | confirmation |
| `list_proposals` | `status?` | `KEY (aliases) \| status \| title \| tickets \| updated` |
| `get_proposal` | `id` (a key, or an alias such as `D17` resolved in `project`), `section?` | front matter + Markdown |
| `create_proposal` | `title`, `body?` (defaults to the template), `status?`, `aliases?`, `authors?`, `labels?`, `tickets?`, `supersedes?` | confirmation |
| `update_proposal` | `id` (key or alias) plus the fields to change | change summary |
| `search` | `query`, `project?`, `cursor?` | `KEY \| kind \| status \| title — snippet` |

The tools leave out deletes on purpose.

## Prompts (slash commands)

| Prompt | Arguments | What it asks the agent to do |
|---|---|---|
| `next` | | Pick the most important unblocked open ticket, explain the choice, move it to in-progress and start. |
| `wrap` | `ticket?` | Comment with a summary, add links and the branch, move to In Review or Done, and file follow-ups. |
| `plan` | `proposal` | Break a proposal into linked tickets, using `blocks` where order matters. |

In Claude Code these appear as `/bankan:next`, `/bankan:wrap` and `/bankan:plan`.

## Agent identity

The agent's name, used as comment author and suggested assignee, comes from the client's `clientInfo.name`. Any client whose name contains "claude" becomes `claude`. Set `BANKAN_AGENT` to override it.

## Trying it by hand

```bash
printf '%s\n' \
 '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}' \
 '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"context","arguments":{}}}' \
 | bankan mcp
```
