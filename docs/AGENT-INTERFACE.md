# Research: how agents should use BanKan

This note covers how AI agents find and call BanKan's tools, and what the tools should return so agents use few tokens and make few mistakes. It checks common claims ("YAML is cheaper than JSON", "use TOON", "CLIs beat MCP") against published studies and against measurements on BanKan's own ticket shape. It ends with concrete changes to the MCP design in [PLAN.md](PLAN.md).

Token counts below were measured with [token-bench.py](token-bench.py) using the `o200k_base` tokenizer as a stand-in. Claude's tokenizer gives different absolute numbers, but the ratios between formats are what matter here, and those carry over roughly.

## 1. Claim: "YAML is more token-efficient than JSON, so agents should get YAML"

**Mostly refuted.** YAML only beats *pretty-printed* JSON. Compact JSON beats YAML.

Measured on 25 BanKan tickets (a `list_tickets` response):

| Format | Tokens | vs pretty JSON |
|---|---|---|
| JSON, pretty (`serde_json::to_string_pretty`) | 3,210 | 100% |
| YAML, all fields | 2,602 | 81% |
| JSON, compact | 2,209 | 69% |
| YAML, summary fields only | 1,010 | 31% |
| JSON compact, summary fields only | 831 | 26% |
| One line per ticket (plain text) | 522 | 16% |
| TOON-style table, summary fields | 489 | 15% |

A single full ticket: pretty JSON 341, YAML 266, compact JSON 250, Markdown file with front matter 258.

**What the studies say**
- The YAML-wins result most people cite is [improvingagents.com's nested-data test](https://www.improvingagents.com/blog/best-nested-data-format). There, YAML scored 62.1% against 50.3% for JSON on GPT-5 Nano, and 51.9% against 43.1% on Gemini 2.5 Flash Lite. Three caveats apply:
  - The JSON was pretty-printed.
  - The data was 6–7 levels deep and sized on purpose so models failed about half the time.
  - Llama 3.2 3B showed no YAML advantage at all.
- [Structured Context Engineering (arXiv 2602.05447)](https://arxiv.org/abs/2602.05447) ran 9,649 trials across 11 models. It found that format **does not significantly affect aggregate accuracy** (χ²=2.45, p=0.484). The model tier mattered far more: there was a 21-point gap between frontier and open-source models.
- [Notation Matters (arXiv 2605.29676)](https://arxiv.org/abs/2605.29676) puts YAML's saving over JSON at about 1–6%, too small to matter.

**Conclusion:** YAML-versus-JSON is a small effect, and it reverses once JSON is compact. Ticket data is shallow, only one or two levels deep, so YAML's accuracy edge on deep nesting doesn't apply. **Which fields you return matters 3–4× more than the syntax.** Cutting to summary fields saved 74%. Switching syntax saved at most 31%.

## 2. Claim: "Use a compact format like TOON"

**Partly supported.** Tables do save tokens for flat lists, but novel formats carry an accuracy cost in agent loops.

- TOON's own benchmark claims about 40% fewer tokens than pretty JSON and slightly better accuracy: 76.4% against 75.0% on 209 retrieval questions. That benchmark is [vendor-run](https://www.mintlify.com/toon-format/toon/introduction), and the project itself says TOON is "not ideal for deeply nested" data.
- Independent agentic benchmarks ([arXiv 2605.29676](https://arxiv.org/abs/2605.29676): BFCL, MCPToolBench++, MCP-Universe, StableToolBench) found TOON cut tokens by up to 18% **at a cost of about 9 accuracy points**. It also "cascades on multi-turn parsing failures" and broke parallel tool calls for most models.
- [arXiv 2602.05447](https://arxiv.org/abs/2602.05447) found that compact or unfamiliar formats can *raise* total tokens through "pattern unfamiliarity": models spend more turns working them out.

**Conclusion:** plain text with one line per ticket got 16% of the pretty-JSON size, almost identical to TOON's 15%, in a shape every model already reads. Use that. Don't adopt TOON.

## 3. Finding: Markdown must never be escaped inside JSON or YAML

**Strongly supported by measurement.** The docs and proposals are mostly Markdown, and ticket descriptions are Markdown too. Here is [PLAN.md](PLAN.md) returned as a doc:

| Format | Tokens |
|---|---|
| YAML (`body:` as a YAML string) | 2,794 (+60%) |
| JSON compact (`"body": "...\n..."`) | 2,495 (+43%) |
| **Front matter + raw Markdown** | **1,771 (+2%)** |
| Raw body alone | 1,742 |

JSON turns every newline into `\n` and every quote into `\"`. YAML adds quoting and folding to long strings. Both also make the text harder for the model to read and quote back accurately.

**Conclusion:** return a single item exactly as it sits on disk: YAML front matter, then the raw Markdown body. This is BanKan's existing file format, so it costs nothing to build.

## 4. Claim: "MCP wastes context on tool definitions; CLIs are cheaper"

**True in 2025, mostly fixed now.**

- The CLI-versus-MCP benchmarks ([Smithery via PubNub](https://www.pubnub.com/blog/mcp-vs-cli-context-window-cost-blocks-ai), [Firecrawl](https://firecrawl.dev/blog/mcp-vs-cli)) put almost all of the gap down to **schema overhead**, meaning every tool definition being sent on every request. Several of the sources sell CLI products.
- Claude Code now **defers MCP tools by default** ([docs](https://code.claude.com/docs/en/mcp#scale-with-mcp-tool-search)). Only tool names and the server's `instructions` load at the start, and a tool's full schema loads when the agent searches for it. [Anthropic reports](https://www.anthropic.com/engineering/advanced-tool-use) this cuts definition context by 85% and raised tool-selection accuracy from 49% to 74% (Opus 4) and from 79.5% to 88.1% (Opus 4.5).
- [Checkly](https://www.checklyhq.com/blog/mcp-vs-cli-token-efficiency/) reaches the same conclusion: the gap has largely closed.

**Conclusion:** keep MCP as the main interface. It works in every MCP client, the `.bankan` lookup works through it, and it keeps a single writer. With tool search on, 17 tools cost almost nothing up front. What matters now is the server `instructions` text, because that is what tells the agent BanKan's tools exist. A CLI that mirrors the tools is optional and can wait (see section 7).

## 5. Claude Code limits that shape the design

From the [Claude Code MCP docs](https://code.claude.com/docs/en/mcp):

| Limit | Value | What it means for BanKan |
|---|---|---|
| Output warning | 10,000 tokens | Lists need a default limit well below this |
| Output cap | 25,000 tokens (`MAX_MCP_OUTPUT_TOKENS`) | Bigger results are written to a file and the agent has to read it back, which costs an extra step |
| Text result size | 50,000 characters goes to a file | Large docs need `_meta["anthropic/maxResultSizeChars"]` or a range parameter |
| Server instructions | cut at 4,096 characters | Put keywords and the most important rules first |
| Description of a tool loaded at start | cut at 4,096 characters | Keep short |
| Description of a tool loaded by search | cut at 16,384 characters, but search only matches on the first 4,096 | Put search words near the start |
| Error text | over about 11,000 characters keeps only the first and last 5,000 | Keep errors short |
| `_meta["anthropic/alwaysLoad"]` | per tool | Load the one or two tools used every session at start |

The MCP spec ([2025-06-18](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)) says a tool that returns `structuredContent` **SHOULD also return the same JSON as text**. If a client shows the model both, the cost doubles: 6,420 tokens for the 25-ticket list. Agents gain nothing from `outputSchema` here, so BanKan should skip it and return one text block.

## 6. Tool design guidance from Anthropic

From [Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents) and [Advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use):

- **Fewer tools that each match a workflow** beat thin wrappers over every API endpoint. Combine steps that always happen together.
- **A `response_format` setting (`concise`/`detailed`):** their example went from 206 to 72 tokens, about ⅓.
- **Pagination, filters and truncation with sensible defaults.** When a result is cut, say how to narrow the request.
- **Human-readable IDs** over UUIDs. Resolving IDs to names reduced hallucinated IDs. BanKan's `RES-12` keys already do this.
- **Errors that say what to do,** with an example of valid input, not error codes.
- **Write descriptions as if for a new hire,** and spell out conventions. 1–5 examples raised accuracy on complex parameters from 72% to 90%.
- **Mark tools with annotations** (read-only, destructive, idempotent).
- **The format choice is "no one-size-fits-all":** pick whatever your evals show works. That matches sections 1–3.

## 7. Recommendations for BanKan

### Response formats

| Tool kind | Return |
|---|---|
| List (`list_tickets`, `search`, `list_docs`, `list_proposals`) | Plain text, one line per item, then a footer:<br>`RES-12 \| In Progress \| Bug \| high \| @claude \| #api \| Fix SSE reconnect`<br>`25 of 112 shown. More: cursor="c25". Narrow with status=, label=, assignee=, q=.` |
| Single item (`get_ticket`, `get_doc`, `get_proposal`) | The on-disk file: YAML front matter, then raw Markdown. Comments go last as `### me · 2026-10-20` blocks. |
| Write (`create_*`, `update_*`, `add_comment`, `write_doc`) | One confirmation line naming what changed: `Updated RES-12: status Todo → In Progress, assignee → claude.` Don't echo the whole ticket back. |
| Error | One or two lines, valid values included: `Unknown status "Doing" for RES. Valid: Backlog, Todo, In Progress, In Review, Done, Canceled.` |

- Lists default to **25 items** and **open tickets only** (category not `completed` or `canceled`). Add an optional `detail: "full"` argument.
- Agent-facing output drops `order`, cuts timestamps to dates, and leaves out empty fields. Full timestamps cost about 18 tokens each, which is 36 per ticket for `created` and `updated`.
- `get_ticket` shows the last 10 comments by default and says how many were left out.
- Set `_meta["anthropic/maxResultSizeChars"]` on `get_doc` and `get_proposal`, or give them a `section` argument, so a long doc doesn't get written to a file.
- Don't declare `outputSchema` or return `structuredContent`.

### Discovery: helping agents find and use the tools

1. **Fill in the server `instructions`** in the `initialize` response, under 4,096 characters, keywords first. Cover:
   - what BanKan holds (tickets, docs, proposals)
   - that the current repo maps to a project through `.bankan`
   - ticket keys look like `RES-12`
   - call `context` first
   - statuses are per project, but categories are fixed
2. **Replace `current_project` with a `context` tool** marked `anthropic/alwaysLoad`. In one call it returns:
   - the project key and name
   - statuses with their categories
   - types, labels and milestones
   - custom field definitions
   - the tickets currently in progress and assigned to the agent

   This removes the usual opening of `current_project`, then `get_project`, then `list_tickets`, and the agent learns valid values before writing.
3. **Merge calls that always happen together.** Give `update_ticket` an optional `comment` argument, so "move to In Review and note the PR" is one call. Let `create_ticket` take `parent`, `blocks` and `labels` directly.
4. **Write descriptions with examples** for the tools with tricky arguments:
   - `update_ticket`: relations, custom `fields`, clearing a field
   - `create_ticket`
5. **Add annotations.** Mark every `get_`, `list_` and `search` tool `readOnlyHint: true`, `write_doc` `idempotentHint: true`, and every tool `destructiveHint: false`, since there are no deletes.
6. **Match statuses, types and labels loosely.** `Project::status` already ignores case. Extend that to types and labels, and suggest the nearest valid value in the error.

### Optional, later

- **MCP prompts** show up as slash commands in Claude Code: `/bankan:next` (pick the next ticket and start it) and `/bankan:wrap` (comment and move to review).
- **MCP resources** let people @-mention items: `@bankan:ticket://RES-12` attaches a ticket to a prompt.
- **CLI subcommands that mirror the tools** (`bankan ticket list --status open`) for agents without MCP. They're cheap to add because the CLI and MCP shim both call the same HTTP API. They're low priority now that tool search has removed the schema cost.

### Check it with evals, not guesses

Anthropic's main advice is to measure. Once M2 works, write about 10 realistic tasks, for example:
- "start the highest-priority bug"
- "turn proposal P3 into tickets"
- "what's blocking RES-20?"

Run them through Claude Code and record calls, tokens and errors per task. Re-run them when changing formats or descriptions.

## Sources

- Anthropic, [Writing effective tools for agents](https://www.anthropic.com/engineering/writing-tools-for-agents)
- Anthropic, [Introducing advanced tool use](https://www.anthropic.com/engineering/advanced-tool-use)
- Claude Code, [MCP docs](https://code.claude.com/docs/en/mcp): output limits, tool search, `_meta` keys
- MCP spec 2025-06-18, [Tools](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)
- [Structured Context Engineering for File-Native Agentic Systems](https://arxiv.org/abs/2602.05447) (arXiv 2602.05447)
- [Notation Matters: token-optimized formats in agentic AI systems](https://arxiv.org/abs/2605.29676) (arXiv 2605.29676)
- improvingagents.com, [Which nested data format do LLMs understand best?](https://www.improvingagents.com/blog/best-nested-data-format)
- [TOON format docs](https://www.mintlify.com/toon-format/toon/introduction) (vendor benchmark)
- [CLIs are more token-efficient than MCP. Or are they?](https://www.checklyhq.com/blog/mcp-vs-cli-token-efficiency/) (Checkly); [MCP vs CLI context cost](https://www.pubnub.com/blog/mcp-vs-cli-context-window-cost-blocks-ai) (PubNub); [MCP vs CLI](https://firecrawl.dev/blog/mcp-vs-cli) (Firecrawl)
