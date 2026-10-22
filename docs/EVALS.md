# Agent evals

[AGENT-INTERFACE.md](AGENT-INTERFACE.md) recommends measuring how agents use BanKan instead of guessing. This file lists the tasks to run.

**Status: not run yet.** The MCP server has been checked by driving it directly over stdio. These tasks need a real Claude Code session against a seeded project.

## Setup

1. Create a project `EV` with about 30 tickets in mixed statuses, priorities and labels.
2. Add a few `blocks` relations.
3. Add one proposal in `review` and two docs.
4. Link a scratch repo with `bankan link EV`.

## Tasks

| # | Prompt | A good run looks like |
|---|---|---|
| 1 | "What's in progress?" | One `context` call, or `context` then `list_tickets category=started` |
| 2 | "Start the highest-priority bug." | `list_tickets type=Bug`, `get_ticket`, then `update_ticket` with status and assignee |
| 3 | "What's blocking EV-20?" | `get_ticket EV-20` and reading `blocked_by` |
| 4 | "File a bug: login fails for emails with a plus sign, high priority, label auth." | One `create_ticket` with type, priority and labels |
| 5 | "I opened PR #12 for EV-7, move it to review." | One `update_ticket` with status, links and comment |
| 6 | "Turn proposal EV-P1 into tickets." | `get_proposal`, then several `create_ticket` calls with `proposal` set |
| 7 | "Summarise the architecture doc's storage section." | `list_docs`, then `get_doc` with `section` |
| 8 | "Write up how deploys work as a doc." | `write_doc` with a sensible path |
| 9 | "Find anything about rate limiting." | `search` |
| 10 | "Move EV-3 to 'Doing'." (a status that doesn't exist) | Recovers from the error, which lists the valid statuses |

For each run, record:
- the tool calls made, and how many were wasted
- the tokens in tool results
- errors, and whether the agent recovered
- whether the final state is correct

Re-run after changing tool descriptions, output formats or server instructions.
