"""Token cost of BanKan payloads in different formats (o200k_base as a proxy tokenizer).

Run: pip install tiktoken pyyaml && python docs/token-bench.py
See docs/AGENT-INTERFACE.md for the results.
"""
import json, random, yaml, tiktoken

enc = tiktoken.get_encoding("o200k_base")
tok = lambda s: len(enc.encode(s))
random.seed(1)

statuses = ["Backlog", "Todo", "In Progress", "In Review", "Done"]
types = ["Feature", "Bug", "Task", "Chore"]
prios = ["urgent", "high", "medium", "low", "none"]
labels = ["api", "ui", "mcp", "perf", "docs", "infra"]
titles = [
    "Add pagination to list_tickets", "Fix SSE reconnect after daemon restart", "Board column collapse state not saved",
    "Search should match ticket keys", "Write MCP guide", "Atomic write fails on network drive", "Add due-date filter",
    "Proposal template missing Decision section", "Drag card between columns loses order", "Support custom select fields",
]

def ticket(i, full=False):
    t = {
        "id": f"RES-{i}", "title": random.choice(titles), "status": random.choice(statuses),
        "type": random.choice(types), "priority": random.choice(prios),
        "labels": random.sample(labels, random.randint(0, 2)),
        "assignee": random.choice([None, "me", "claude"]),
        "estimate": random.choice([None, 1, 2, 3, 5]),
        "milestone": random.choice([None, "v0.1"]),
        "parent": random.choice([None, None, "RES-3"]),
        "blocks": random.choice([[], [], [f"RES-{i+1}"]]),
        "order": round(random.random() * 1000, 3),
        "created": "2026-10-01T09:12:44.123456Z", "updated": "2026-10-20T14:03:11.987654Z",
    }
    t = {k: v for k, v in t.items() if v not in (None, [])}
    if full:
        t["branch"] = "res-12-pagination"
        t["links"] = [{"url": "https://github.com/me/bankan/pull/4", "title": "PR #4"}]
        t["comments"] = [
            {"id": 1, "author": "me", "at": "2026-10-19T10:00:00Z", "body": "Default page size should be 25."},
            {"id": 2, "author": "claude", "at": "2026-10-20T14:03:11Z", "body": "Implemented cursor pagination; see PR #4."},
        ]
        t["description"] = (
            "## Problem\n\nlist_tickets returns every ticket, which blows the context window on big projects.\n\n"
            "## Acceptance\n\n- default limit 25\n- `cursor` param\n- response says how many remain\n"
        )
    return t

def md_frontmatter(t):
    t = dict(t); body = t.pop("description", "")
    return "---\n" + yaml.safe_dump(t, sort_keys=False, allow_unicode=True) + "---\n" + body

def oneline(t):
    bits = [t["id"], t["status"], t["type"], t["priority"]]
    if "assignee" in t: bits.append("@" + t["assignee"])
    if t.get("labels"): bits.append("#" + ",#".join(t["labels"]))
    return " | ".join(bits) + " | " + t["title"]

def toon(ts, cols):
    rows = [",".join(str(t.get(c, "")) for c in cols) for t in ts]
    return f"tickets[{len(ts)}]{{{','.join(cols)}}}:\n  " + "\n  ".join(rows)

def report(name, variants):
    print(f"\n## {name}")
    base = tok(variants["JSON pretty (serde to_string_pretty)"])
    for k, s in variants.items():
        n = tok(s)
        print(f"{k:45s} {n:6d} tok  {n/base*100:5.0f}%")

# 1. A list of 25 tickets, as list_tickets would return them
ts = [ticket(i) for i in range(1, 26)]
slim_keys = ["id", "title", "status", "type", "priority", "assignee", "labels"]
slim = [{k: t[k] for k in slim_keys if k in t} for t in ts]
report("list_tickets, 25 tickets", {
    "JSON pretty (serde to_string_pretty)": json.dumps(ts, indent=2),
    "JSON compact": json.dumps(ts, separators=(",", ":")),
    "YAML (all fields)": yaml.safe_dump(ts, sort_keys=False),
    "JSON compact, summary fields only": json.dumps(slim, separators=(",", ":")),
    "YAML, summary fields only": yaml.safe_dump(slim, sort_keys=False),
    "TOON-style table, summary fields": toon(slim, ["id", "status", "type", "priority", "assignee", "title"]),
    "One line per ticket (text)": "\n".join(oneline(t) for t in ts),
    "MCP: pretty JSON text + structuredContent": json.dumps(ts, indent=2) * 2,
})

# 2. A single full ticket, as get_ticket would return it
full = ticket(12, full=True)
report("get_ticket, 1 full ticket", {
    "JSON pretty (serde to_string_pretty)": json.dumps(full, indent=2),
    "JSON compact": json.dumps(full, separators=(",", ":")),
    "YAML": yaml.safe_dump(full, sort_keys=False),
    "Markdown + YAML front matter (on-disk file)": md_frontmatter(full),
})

# 3. Noise fields worth dropping from agent-facing output
print("\n## per-ticket cost of noisy fields (compact JSON)")
for f in ["order", "created", "updated"]:
    print(f"{f:10s} {tok(json.dumps({f: ts[0][f]}, separators=(',', ':'))) - 2:3d} tok/ticket")
print("timestamp trimmed to date:", tok('"updated":"2026-10-20"'), "vs", tok('"updated":"2026-10-20T14:03:11.987654Z"'))

# 4. A long Markdown doc (docs/PLAN.md)
body = open(__import__("pathlib").Path(__file__).with_name("PLAN.md")).read()
doc = {"path": "plan", "title": "BanKan: plan", "tags": ["design"], "updated": "2026-10-20", "body": body}
meta = {k: v for k, v in doc.items() if k != "body"}
print("doc (PLAN.md as a doc)")
print("JSON compact         ", tok(json.dumps(doc, separators=(",", ":"))))
print("YAML                 ", tok(yaml.safe_dump(doc, sort_keys=False)))
print("front matter + raw md", tok("---\n" + yaml.safe_dump(meta, sort_keys=False) + "---\n" + body))
print("raw body alone       ", tok(body))
