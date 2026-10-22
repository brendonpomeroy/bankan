//! `bankan mcp`: a stdio MCP server for AI agents.
//!
//! The agent's client starts it inside a code repo, so the working directory
//! tells us which project to use (via the repo's `.bankan` file). Every tool
//! call is forwarded to the daemon's REST API; the daemon stays the only writer.
//!
//! Output follows docs/AGENT-INTERFACE.md: lists are one line per item with a
//! paging footer, single items are front matter + raw Markdown (never escaped
//! inside JSON), writes return a one-line confirmation, errors are short and
//! name the valid values.

use crate::config::Config;
use crate::link::{self, Link};
use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};

const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
const PAGE: usize = 25;
const RECENT_COMMENTS: usize = 10;
/// Lets long docs come back inline instead of being spilled to a file by Claude Code.
const MAX_RESULT_CHARS: u64 = 200_000;

// Kept under 4,096 characters (Claude Code truncates beyond that); keywords first.
const INSTRUCTIONS: &str = "BanKan: the user's local tracker for tickets (Kanban board), docs and proposals (RFC/ADR). Use it instead of writing TODOs, plans or bug lists into repo files.\n\
\n\
- Start with `context`: it shows the project linked to this repo (via its .bankan file), the valid statuses, types, labels and milestones, and what's in progress.\n\
- Ticket keys look like RES-12; proposals like RES-P3. Use keys in commit messages and branch names.\n\
- Statuses are custom per project but each has a fixed category: backlog, unstarted, started, completed, canceled.\n\
- Working a ticket: update_ticket status → a started status (e.g. In Progress) with assignee set to you; add a `comment` for notable progress or decisions; when finished move it to In Review (if the user should check) or Done.\n\
- File bugs and follow-ups you discover as new tickets instead of fixing everything at once.\n\
- Designs that need the user's sign-off go in proposals (draft → review); only the user accepts or rejects. Lasting reference material goes in docs.\n\
- Tools default to the linked project; pass `project` to work on another one.";

struct Api {
    base: String,
}

fn encode_path(p: &str) -> String {
    let mut out = String::new();
    for b in p.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl Api {
    fn call(&self, method: &str, path: &str, query: &[(&str, String)], body: Option<Value>) -> Result<Value, String> {
        let url = format!("{}/api{}", self.base, path);
        let mut req = ureq::request(method, &url).timeout(std::time::Duration::from_secs(30));
        for (k, v) in query {
            req = req.query(k, v);
        }
        let res = match body {
            Some(b) => req.send_json(b),
            None => req.call(),
        };
        match res {
            Ok(r) => r.into_json::<Value>().map_err(|e| format!("bad response from BanKan: {e}")),
            Err(ureq::Error::Status(code, r)) => {
                let v: Value = r.into_json().unwrap_or(Value::Null);
                Err(v["error"].as_str().map(str::to_string).unwrap_or_else(|| format!("BanKan returned HTTP {code}")))
            }
            Err(_) => Err(format!(
                "BanKan isn't running at {}. Ask the user to run `bankan install` (autostart) or `bankan serve`.",
                self.base
            )),
        }
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        self.call("GET", path, &[], None)
    }
}

// --- formatting ---------------------------------------------------------------

fn str_of<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(Value::as_str).filter(|s| !s.is_empty())
}

fn list_of(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// `2026-10-20T14:03:11Z` → `2026-10-20`
fn date(v: &Value, k: &str) -> Option<String> {
    str_of(v, k).map(|s| s.chars().take(10).collect())
}

/// `RES-12 | In Progress | Bug | high | @claude | #api,#ui | Fix SSE reconnect`
fn ticket_line(t: &Value, full: bool) -> String {
    let mut bits = vec![
        str_of(t, "id").unwrap_or("?").to_string(),
        str_of(t, "status").unwrap_or("?").to_string(),
        str_of(t, "type").unwrap_or("?").to_string(),
        str_of(t, "priority").unwrap_or("none").to_string(),
    ];
    if let Some(a) = str_of(t, "assignee") {
        bits.push(format!("@{a}"));
    }
    let labels = list_of(t, "labels");
    if !labels.is_empty() {
        bits.push(labels.iter().map(|l| format!("#{l}")).collect::<Vec<_>>().join(","));
    }
    bits.push(str_of(t, "title").unwrap_or("").to_string());
    let mut line = bits.join(" | ");
    if full {
        let mut extra = Vec::new();
        for k in ["milestone", "parent", "duplicate_of", "proposal", "branch"] {
            if let Some(v) = str_of(t, k) {
                extra.push(format!("{k}={v}"));
            }
        }
        if let Some(d) = str_of(t, "due") {
            extra.push(format!("due={d}"));
        }
        if let Some(e) = t.get("estimate").filter(|e| e.is_number()) {
            extra.push(format!("estimate={e}"));
        }
        for k in ["blocks", "related"] {
            let l = list_of(t, k);
            if !l.is_empty() {
                extra.push(format!("{k}={}", l.join(",")));
            }
        }
        if let Some(u) = date(t, "updated") {
            extra.push(format!("updated={u}"));
        }
        if !extra.is_empty() {
            line.push_str(" | ");
            line.push_str(&extra.join(" "));
        }
    }
    line
}

/// Front matter (YAML) + raw Markdown body, the same shape as the file on disk.
fn front_matter(meta: Map<String, Value>, body: &str) -> String {
    let yaml = serde_yaml::to_string(&Value::Object(meta)).unwrap_or_default();
    let mut out = format!("---\n{yaml}---\n");
    if !body.trim().is_empty() {
        out.push('\n');
        out.push_str(body.trim_end());
        out.push('\n');
    }
    out
}

/// Copies non-empty fields, trimming timestamps to dates.
fn meta_of(v: &Value, keys: &[&str], dates: &[&str]) -> Map<String, Value> {
    let mut m = Map::new();
    for k in keys {
        match v.get(*k) {
            None | Some(Value::Null) => {}
            Some(Value::Array(a)) if a.is_empty() => {}
            Some(Value::Object(o)) if o.is_empty() => {}
            Some(Value::String(s)) if s.is_empty() || (*k == "priority" && s == "none") => {}
            Some(x) => {
                m.insert((*k).into(), x.clone());
            }
        }
    }
    for k in dates {
        if let Some(d) = date(v, k) {
            m.insert((*k).into(), Value::String(d));
        }
    }
    m
}

fn render_ticket(t: &Value) -> String {
    let meta = meta_of(
        t,
        &[
            "id", "title", "status", "type", "priority", "labels", "assignee", "estimate", "due", "milestone", "parent", "children", "blocks", "blocked_by",
            "related", "duplicate_of", "proposal", "branch", "links", "fields",
        ],
        &["created", "updated", "closed"],
    );
    let mut out = front_matter(meta, str_of(t, "description").unwrap_or(""));
    let comments = t.get("comments").and_then(Value::as_array).cloned().unwrap_or_default();
    if !comments.is_empty() {
        let skip = comments.len().saturating_sub(RECENT_COMMENTS);
        out.push_str("\n## Comments");
        if skip > 0 {
            out.push_str(&format!(" (last {RECENT_COMMENTS} of {}; older ones omitted)", comments.len()));
        }
        out.push('\n');
        for c in &comments[skip..] {
            out.push_str(&format!(
                "\n### {} · {}\n\n{}\n",
                str_of(c, "author").unwrap_or("?"),
                date(c, "at").unwrap_or_default(),
                str_of(c, "body").unwrap_or("").trim_end()
            ));
        }
    }
    out
}

/// Returns only the Markdown section whose heading contains `name` (case-insensitive),
/// up to the next heading of the same or higher level.
fn extract_section(body: &str, name: &str) -> Option<String> {
    let want = name.trim().trim_start_matches('#').trim().to_lowercase();
    let lines: Vec<&str> = body.lines().collect();
    let levels = heading_levels(&lines);
    let start = (0..lines.len()).find(|&i| levels[i].is_some() && lines[i].trim_start_matches('#').trim().to_lowercase().contains(&want))?;
    let lvl = levels[start].unwrap();
    let end = (start + 1..lines.len()).find(|&i| levels[i].is_some_and(|n| n <= lvl)).unwrap_or(lines.len());
    Some(lines[start..end].join("\n"))
}

/// The ATX heading level of each line, skipping fenced code blocks (where `#` is often a shell comment).
fn heading_levels(lines: &[&str]) -> Vec<Option<usize>> {
    let mut fence: Option<&str> = None;
    lines
        .iter()
        .map(|l| {
            let t = l.trim_start();
            let marker = ["```", "~~~"].into_iter().find(|m| t.starts_with(m));
            match (fence, marker) {
                (None, Some(m)) => fence = Some(m),
                (Some(f), Some(m)) if f == m => fence = None,
                _ => {}
            }
            (fence.is_none() && marker.is_none() && l.starts_with('#')).then(|| l.chars().take_while(|c| *c == '#').count())
        })
        .collect()
}

fn headings(body: &str) -> Vec<String> {
    let lines: Vec<&str> = body.lines().collect();
    heading_levels(&lines).iter().zip(&lines).filter(|(lvl, _)| lvl.is_some()).map(|(_, l)| l.trim().to_string()).collect()
}

/// "status Todo → In Progress, assignee → claude"
fn describe_changes(before: &Value, after: &Value, keys: &[&str]) -> String {
    let show = |v: Option<&Value>| match v {
        None | Some(Value::Null) => "∅".to_string(),
        Some(Value::String(s)) if s.is_empty() => "∅".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) if a.is_empty() => "∅".to_string(),
        Some(Value::Array(a)) => a.iter().map(|x| x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string())).collect::<Vec<_>>().join(", "),
        Some(x) => x.to_string(),
    };
    let mut changes = Vec::new();
    for k in keys {
        let (b, a) = (before.get(*k), after.get(*k));
        if show(b) == show(a) {
            continue;
        }
        match *k {
            "description" | "body" => changes.push(format!("{k} updated")),
            _ if show(b) == "∅" => changes.push(format!("{k} → {}", show(a))),
            _ => changes.push(format!("{k} {} → {}", show(b), show(a))),
        }
    }
    if changes.is_empty() { "no changes".into() } else { changes.join(", ") }
}

const TICKET_KEYS: &[&str] = &[
    "title", "status", "type", "priority", "labels", "assignee", "estimate", "due", "milestone", "parent", "blocks", "related", "duplicate_of", "proposal",
    "branch", "links", "fields", "description",
];
const PROPOSAL_KEYS: &[&str] = &["title", "status", "authors", "labels", "tickets", "supersedes", "superseded_by", "decided", "body"];

fn page_footer(shown: usize, offset: usize, total: usize, narrow: &str) -> String {
    if total == 0 {
        return format!("No matches. {narrow}");
    }
    if offset + shown >= total {
        return format!("{total} total.");
    }
    format!("{}–{} of {total} shown. More: cursor=\"{}\". {narrow}", offset + 1, offset + shown, offset + shown)
}

// --- tool definitions -----------------------------------------------------------

fn s(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}
fn arr(desc: &str) -> Value {
    json!({ "type": "array", "items": { "type": "string" }, "description": desc })
}
fn nullable(desc: &str, ty: &str) -> Value {
    json!({ "type": [ty, "null"], "description": desc })
}

fn ticket_props(create: bool) -> Map<String, Value> {
    let mut p = Map::new();
    if create {
        p.insert("project".into(), s("Project key, e.g. RES. Defaults to the repo's linked project."));
        p.insert("title".into(), s("Short summary, e.g. \"Login fails when email has a plus sign\"."));
    } else {
        p.insert("id".into(), s("Ticket key, e.g. RES-12."));
        p.insert("title".into(), s("New title."));
    }
    p.insert("description".into(), s("Markdown body. Replaces the whole description."));
    p.insert("type".into(), s("Ticket type from `context`, e.g. Feature, Bug, Task, Chore. Case-insensitive."));
    p.insert("status".into(), s("Status from `context`, e.g. Todo, In Progress, Done. Case-insensitive. Changing it moves the card."));
    p.insert("priority".into(), json!({ "type": "string", "enum": ["none", "urgent", "high", "medium", "low"] }));
    p.insert("labels".into(), arr("Replaces the label list, e.g. [\"api\", \"auth\"]. Unknown labels are created."));
    p.insert("assignee".into(), nullable("Who is working on it, e.g. \"claude\" or \"me\". null clears.", "string"));
    p.insert("estimate".into(), nullable("Estimate in points. null clears.", "number"));
    p.insert("due".into(), nullable("Due date YYYY-MM-DD. null clears.", "string"));
    p.insert("milestone".into(), nullable("Milestone name from `context`. null clears.", "string"));
    p.insert("parent".into(), nullable("Parent ticket key, making this a sub-ticket, e.g. \"RES-3\". null clears.", "string"));
    p.insert("blocks".into(), arr("Replaces the keys this ticket blocks, e.g. [\"RES-14\"]. The other side shows it as blocked_by."));
    p.insert("related".into(), arr("Replaces the related ticket keys."));
    p.insert("proposal".into(), nullable("Proposal this ticket implements, e.g. \"RES-P3\" (also adds the ticket to the proposal).", "string"));
    p.insert("branch".into(), nullable("Git branch for the work, e.g. \"res-12-login-plus\".", "string"));
    p.insert(
        "links".into(),
        json!({ "type": "array", "description": "Replaces external links, e.g. [{\"url\": \"https://github.com/o/r/pull/4\", \"title\": \"PR #4\"}].", "items": { "type": "object", "properties": { "url": { "type": "string" }, "title": { "type": "string" } }, "required": ["url"] } }),
    );
    p.insert("fields".into(), json!({ "type": "object", "description": "Custom field values by name (see `context`), merged in; null removes one. E.g. {\"Severity\": \"S2\", \"Customer\": null}." }));
    p.insert("comment".into(), s("Optional comment to add in the same call, e.g. \"Opened PR #4\"."));
    if !create {
        p.insert("duplicate_of".into(), nullable("Mark as a duplicate of another ticket key. null clears.", "string"));
    }
    p
}

fn tools() -> Value {
    let project = s("Project key, e.g. RES. Defaults to the repo's linked project.");
    let cursor = s("Paging cursor from a previous result's footer.");
    let detail = json!({ "type": "string", "enum": ["concise", "full"], "description": "full adds milestone, parent, relations, due, estimate, branch and updated date to each line." });
    let proposal_status = json!({ "type": "string", "enum": ["draft", "review", "accepted", "rejected", "withdrawn", "superseded", "implemented"] });
    let read = json!({ "readOnlyHint": true, "destructiveHint": false, "openWorldHint": false });
    let write = json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false });
    let idempotent = json!({ "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false });
    let tool = |name: &str, title: &str, desc: &str, props: Value, required: &[&str], ann: &Value| {
        json!({ "name": name, "title": title, "description": desc, "inputSchema": { "type": "object", "properties": props, "required": required }, "annotations": ann })
    };
    let mut list = vec![
        tool(
            "context",
            "Project context",
            "BanKan project context for this repo: linked project, valid statuses (with categories), types, labels, milestones, custom fields, counts, and the tickets in progress. Call first in a session, before creating or updating tickets.",
            json!({ "project": project }),
            &[],
            &read,
        ),
        tool("list_projects", "List projects", "List all BanKan projects with open/in-progress/done counts.", json!({ "include_archived": { "type": "boolean" } }), &[], &read),
        tool(
            "create_project",
            "Create project",
            "Create a BanKan project. The key is a short uppercase prefix used in ticket keys (RES → RES-1). Then the user can link a repo with `bankan link RES`.",
            json!({ "key": s("2–10 uppercase letters/digits, e.g. RES"), "name": s("Display name, e.g. Resure"), "description": s("Optional description") }),
            &["key", "name"],
            &write,
        ),
        tool(
            "list_tickets",
            "List tickets",
            "List/find tickets, one line each: KEY | status | type | priority | @assignee | #labels | title. Open tickets only unless include_closed is true or a closed status/category is requested. 25 per page.",
            json!({
                "project": project,
                "status": s("Status name, e.g. \"In Progress\"."),
                "category": { "type": "string", "enum": ["backlog", "unstarted", "started", "completed", "canceled"], "description": "Status category, e.g. started = everything in progress." },
                "type": s("Type, e.g. Bug."),
                "priority": { "type": "string", "enum": ["none", "urgent", "high", "medium", "low"] },
                "label": s("Label."),
                "assignee": s("Assignee, e.g. claude."),
                "milestone": s("Milestone name."),
                "parent": s("Only sub-tickets of this key."),
                "query": s("Text in key, title or description."),
                "include_closed": { "type": "boolean" },
                "detail": detail,
                "cursor": cursor,
            }),
            &[],
            &read,
        ),
        tool(
            "get_ticket",
            "Get ticket",
            "Read a ticket: YAML front matter (fields, relations incl. blocked_by and children) then the Markdown description and the last 10 comments.",
            json!({ "id": s("Ticket key, e.g. RES-12") }),
            &["id"],
            &read,
        ),
        tool(
            "create_ticket",
            "Create ticket",
            "Create a ticket (bug, feature, task…). Defaults: first unstarted status (e.g. Todo), first type, priority none. Example: {\"title\": \"Login fails for plus-sign emails\", \"type\": \"Bug\", \"priority\": \"high\", \"labels\": [\"auth\"]}.",
            Value::Object(ticket_props(true)),
            &["title"],
            &write,
        ),
        tool(
            "update_ticket",
            "Update ticket",
            "Change a ticket; only passed fields change. Start work: {\"id\": \"RES-12\", \"status\": \"In Progress\", \"assignee\": \"claude\"}. Finish: {\"id\": \"RES-12\", \"status\": \"In Review\", \"comment\": \"PR #4 ready\", \"links\": [{\"url\": \"…/pull/4\"}]}. Clear a field with null; list fields (labels, blocks, related, links) are replaced.",
            Value::Object(ticket_props(false)),
            &["id"],
            &write,
        ),
        tool(
            "add_comment",
            "Comment on ticket",
            "Add a Markdown comment to a ticket: progress notes, findings, decisions. (update_ticket also takes `comment`.)",
            json!({ "id": s("Ticket key, e.g. RES-12"), "body": s("Markdown comment"), "author": s("Defaults to your agent name") }),
            &["id", "body"],
            &write,
        ),
        tool("list_docs", "List docs", "List a project's docs: path | title | #tags | updated.", json!({ "project": project }), &[], &read),
        tool(
            "get_doc",
            "Read doc",
            "Read a doc as front matter + Markdown. Pass `section` (a heading) to read just that part of a long doc.",
            json!({ "project": project, "path": s("Doc path, e.g. guides/setup"), "section": s("Optional heading text, e.g. \"Storage\"") }),
            &["path"],
            &read,
        ),
        tool(
            "write_doc",
            "Write doc",
            "Create or replace a doc (body replaces the whole content). Organise with folders in the path, e.g. architecture/storage.",
            json!({ "project": project, "path": s("Doc path, e.g. guides/setup"), "title": s("Title (defaults from the path)"), "body": s("Markdown content"), "tags": arr("Tags") }),
            &["path"],
            &idempotent,
        ),
        tool(
            "list_proposals",
            "List proposals",
            "List a project's proposals (RFC/ADR design docs): KEY | status | title | tickets | updated.",
            json!({ "project": project, "status": proposal_status }),
            &[],
            &read,
        ),
        tool(
            "get_proposal",
            "Read proposal",
            "Read a proposal as front matter + Markdown. Pass `section` (a heading) to read just that part.",
            json!({ "id": s("Proposal key, e.g. RES-P3"), "section": s("Optional heading text, e.g. \"Alternatives\"") }),
            &["id"],
            &read,
        ),
        tool(
            "create_proposal",
            "Create proposal",
            "Write a design proposal (RFC/ADR) for the user to review. Starts as draft from a template (Summary, Motivation, Proposal, Alternatives considered, Risks & drawbacks, Open questions, Decision) unless you pass `body`. Set status \"review\" when ready.",
            json!({ "project": project, "title": s("Title, e.g. \"Move auth to OAuth device flow\""), "body": s("Markdown body"), "status": proposal_status, "authors": arr("Authors"), "labels": arr("Labels"), "tickets": arr("Related ticket keys"), "supersedes": s("Key of a proposal this replaces") }),
            &["title"],
            &write,
        ),
        tool(
            "update_proposal",
            "Update proposal",
            "Change a proposal; only passed fields change. Only the user accepts or rejects proposals unless they ask you to. Link implementing tickets with `tickets` (or set `proposal` on the ticket).",
            json!({ "id": s("Proposal key, e.g. RES-P3"), "title": s("Title"), "status": proposal_status, "body": s("Markdown body (replaces it)"), "authors": arr("Authors"), "labels": arr("Labels"), "tickets": arr("Ticket keys implementing it"), "supersedes": s("Proposal key"), "superseded_by": s("Proposal key") }),
            &["id"],
            &write,
        ),
        tool(
            "search",
            "Search",
            "Full-text search across tickets, docs and proposals: KEY | kind | status | title — snippet. All projects unless `project` is given.",
            json!({ "query": s("Text to find"), "project": s("Limit to a project key"), "cursor": cursor }),
            &["query"],
            &read,
        ),
    ];
    for t in &mut list {
        match t["name"].as_str() {
            Some("context") => t["_meta"] = json!({ "anthropic/alwaysLoad": true }),
            Some("get_doc" | "get_proposal") => t["_meta"] = json!({ "anthropic/maxResultSizeChars": MAX_RESULT_CHARS }),
            _ => {}
        }
    }
    Value::Array(list)
}

fn prompts() -> Value {
    json!([
        { "name": "next", "title": "Start the next ticket", "description": "Pick the most important open ticket in this repo's BanKan project and start it.", "arguments": [] },
        { "name": "wrap", "title": "Wrap up a ticket", "description": "Summarise the work on the current ticket, comment, link it and move it to review.", "arguments": [{ "name": "ticket", "description": "Ticket key, e.g. RES-12 (defaults to the one in progress)", "required": false }] },
        { "name": "plan", "title": "Proposal to tickets", "description": "Break an accepted proposal into tickets linked to it.", "arguments": [{ "name": "proposal", "description": "Proposal key, e.g. RES-P3", "required": true }] }
    ])
}

fn prompt_text(name: &str, args: &Map<String, Value>) -> Option<String> {
    let arg = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Some(match name {
        "next" => "Use the BanKan tools. Call `context`, then `list_tickets` for open tickets. Pick the most important one that isn't blocked (urgent/high first, then anything already in progress and assigned to you, then Todo over Backlog). Read it with `get_ticket`, tell me which one you picked and why, then move it to the project's in-progress status with yourself as assignee and start the work.".into(),
        "wrap" => {
            let t = arg("ticket");
            let which = if t.is_empty() { "the ticket you're working on (check `list_tickets` with category=started if unsure)".to_string() } else { t };
            format!("Use the BanKan tools to wrap up {which}: write a short comment summarising what changed and anything left open, add links to the PR/commits if any, record the branch, and move it to In Review (or Done if no review is needed). File any follow-ups you found as new tickets linked with `related`.")
        }
        "plan" => format!(
            "Use the BanKan tools. Read proposal {} with `get_proposal`. Break it into small, independently shippable tickets (use a parent ticket for the overall effort if there are more than three). Create each with `create_ticket`, setting `proposal` to the proposal key, sensible types, priorities and `blocks` where order matters. Then list what you created.",
            arg("proposal")
        ),
        _ => return None,
    })
}

// --- server -------------------------------------------------------------------

struct Server {
    api: Api,
    link: Option<Link>,
    agent: String,
}

impl Server {
    fn project(&self, args: &Map<String, Value>) -> Result<String, String> {
        if let Some(p) = args.get("project").and_then(Value::as_str).filter(|p| !p.trim().is_empty()) {
            return Ok(p.trim().to_ascii_uppercase());
        }
        self.link.as_ref().map(|l| l.project.clone()).ok_or_else(|| {
            "This repo isn't linked to a BanKan project. Pass `project` (see list_projects), or ask the user to run `bankan link <KEY>` in the repo.".to_string()
        })
    }

    /// Request body: every argument except the routing/presentation ones.
    fn body(args: &Map<String, Value>, skip: &[&str]) -> Value {
        Value::Object(args.iter().filter(|(k, _)| !skip.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    fn context(&self, args: &Map<String, Value>) -> Result<String, String> {
        let key = match self.project(args) {
            Ok(k) => k,
            Err(_) => {
                let cwd = std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default();
                let projects = self.api.get("/projects")?;
                let lines: Vec<String> =
                    projects.as_array().into_iter().flatten().map(|p| format!("{} | {}", str_of(p, "key").unwrap_or("?"), str_of(p, "name").unwrap_or(""))).collect();
                return Ok(format!(
                    "No BanKan project is linked to {cwd} (no .bankan file here or above). Pass `project` to tools, or ask the user to run `bankan link <KEY>`.\n\nProjects:\n{}",
                    if lines.is_empty() { "(none yet; create one with create_project)".into() } else { lines.join("\n") }
                ));
            }
        };
        let p = self.api.get(&format!("/projects/{}", encode_path(&key)))?;
        let mut out = format!("Project {} · {}", key, str_of(&p, "name").unwrap_or(""));
        if let Some(l) = self.link.as_ref().filter(|l| l.project == key) {
            out.push_str(&format!(" (linked via {})", l.file.display()));
        }
        out.push('\n');
        if let Some(d) = str_of(&p, "description") {
            out.push_str(&format!("{d}\n"));
        }
        let names = |k: &str| -> Vec<String> { p[k].as_array().into_iter().flatten().filter_map(|x| str_of(x, "name").map(str::to_string)).collect() };
        let statuses: Vec<String> =
            p["statuses"].as_array().into_iter().flatten().map(|s| format!("{} [{}]", str_of(s, "name").unwrap_or("?"), str_of(s, "category").unwrap_or("?"))).collect();
        out.push_str(&format!("Statuses (board order): {}\n", statuses.join(", ")));
        out.push_str(&format!("Types: {}\n", names("types").join(", ")));
        out.push_str("Priorities: urgent, high, medium, low, none\n");
        let labels = names("labels");
        out.push_str(&format!("Labels: {}\n", if labels.is_empty() { "(none yet; new ones are created on use)".into() } else { labels.join(", ") }));
        let ms: Vec<String> = p["milestones"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|m| {
                let mut s = str_of(m, "name").unwrap_or("?").to_string();
                if let Some(d) = str_of(m, "due") {
                    s.push_str(&format!(" (due {d})"));
                }
                if m["done"].as_bool() == Some(true) {
                    s.push_str(" [done]");
                }
                s
            })
            .collect();
        if !ms.is_empty() {
            out.push_str(&format!("Milestones: {}\n", ms.join(", ")));
        }
        let fields: Vec<String> = p["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|f| {
                let opts = list_of(f, "options");
                let kind = str_of(f, "kind").unwrap_or("text");
                if opts.is_empty() { format!("{} ({kind})", str_of(f, "name").unwrap_or("?")) } else { format!("{} ({kind}: {})", str_of(f, "name").unwrap_or("?"), opts.join(", ")) }
            })
            .collect();
        if !fields.is_empty() {
            out.push_str(&format!("Custom fields: {}\n", fields.join(", ")));
        }
        let c = &p["counts"];
        out.push_str(&format!(
            "Counts: {} backlog, {} unstarted, {} started, {} completed, {} canceled · {} docs · {} proposals\n",
            c["backlog"], c["unstarted"], c["started"], c["completed"], c["canceled"], c["docs"], c["proposals"]
        ));
        out.push_str(&format!("You are \"{}\" (use as assignee and comment author).\n", self.agent));

        let started = self.api.call("GET", &format!("/projects/{}/tickets", encode_path(&key)), &[("category", "started".into())], None)?;
        let started = started.as_array().cloned().unwrap_or_default();
        out.push_str("\nIn progress:\n");
        if started.is_empty() {
            out.push_str("(nothing)\n");
        }
        for t in started.iter().take(PAGE) {
            out.push_str(&ticket_line(t, false));
            out.push('\n');
        }
        if started.len() > PAGE {
            out.push_str(&format!("…and {} more (list_tickets category=started)\n", started.len() - PAGE));
        }
        let proposals = self.api.get(&format!("/projects/{}/proposals", encode_path(&key)))?;
        let review: Vec<String> = proposals
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["status"] == "review")
            .map(|p| format!("{} | {}", str_of(p, "id").unwrap_or("?"), str_of(p, "title").unwrap_or("")))
            .collect();
        if !review.is_empty() {
            out.push_str(&format!("\nProposals awaiting review:\n{}\n", review.join("\n")));
        }
        Ok(out)
    }

    fn call_tool(&self, name: &str, args: &Map<String, Value>) -> Result<String, String> {
        let str_arg = |k: &str| -> Result<String, String> {
            args.get(k).and_then(Value::as_str).map(str::to_string).filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("`{k}` is required"))
        };
        let id_path = |k: &str| -> Result<String, String> { Ok(encode_path(&str_arg(k)?.trim().to_ascii_uppercase())) };
        let cursor = args.get("cursor").and_then(|c| c.as_str().map(str::to_string).or_else(|| c.as_u64().map(|n| n.to_string())));
        let offset: usize = cursor.and_then(|c| c.trim_start_matches('c').parse().ok()).unwrap_or(0);
        let api = &self.api;
        match name {
            "context" => self.context(args),
            "list_projects" => {
                let archived = args.get("include_archived").and_then(Value::as_bool).unwrap_or(false);
                let list = api.call("GET", "/projects", &[("archived", archived.to_string())], None)?;
                let lines: Vec<String> = list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|p| {
                        let c = &p["counts"];
                        let open = c["backlog"].as_u64().unwrap_or(0) + c["unstarted"].as_u64().unwrap_or(0);
                        let mut line = format!(
                            "{} | {} | {open} open, {} in progress, {} done | {} docs, {} proposals",
                            str_of(p, "key").unwrap_or("?"),
                            str_of(p, "name").unwrap_or(""),
                            c["started"],
                            c["completed"],
                            c["docs"],
                            c["proposals"]
                        );
                        if p["archived"].as_bool() == Some(true) {
                            line.push_str(" | archived");
                        }
                        line
                    })
                    .collect();
                Ok(if lines.is_empty() { "No projects yet. Create one with create_project.".into() } else { lines.join("\n") })
            }
            "create_project" => {
                let p = api.call("POST", "/projects", &[], Some(Self::body(args, &[])))?;
                Ok(format!(
                    "Created project {} ({}). Link a repo by running `bankan link {}` in it.",
                    str_of(&p, "key").unwrap_or("?"),
                    str_of(&p, "name").unwrap_or(""),
                    str_of(&p, "key").unwrap_or("?")
                ))
            }
            "list_tickets" => {
                let mut q: Vec<(&str, String)> = vec![("include_closed", "false".into())];
                for (arg, param) in [
                    ("status", "status"),
                    ("category", "category"),
                    ("type", "type"),
                    ("priority", "priority"),
                    ("label", "label"),
                    ("assignee", "assignee"),
                    ("milestone", "milestone"),
                    ("parent", "parent"),
                    ("query", "q"),
                ] {
                    if let Some(v) = args.get(arg).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                        q.push((param, v.to_string()));
                    }
                }
                if let Some(b) = args.get("include_closed").and_then(Value::as_bool) {
                    q[0].1 = b.to_string();
                }
                let full = args.get("detail").and_then(Value::as_str) == Some("full");
                let list = api.call("GET", &format!("/projects/{}/tickets", encode_path(&self.project(args)?)), &q, None)?;
                let all = list.as_array().cloned().unwrap_or_default();
                let page: Vec<String> = all.iter().skip(offset).take(PAGE).map(|t| ticket_line(t, full)).collect();
                let mut out = page.join("\n");
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&page_footer(page.len(), offset, all.len(), "Narrow with status=, category=, label=, assignee=, query=."));
                Ok(out)
            }
            "get_ticket" => Ok(render_ticket(&api.get(&format!("/tickets/{}", id_path("id")?))?)),
            "create_ticket" => {
                let project = self.project(args)?;
                let t = api.call("POST", &format!("/projects/{}/tickets", encode_path(&project)), &[], Some(Self::body(args, &["project", "comment"])))?;
                let id = str_of(&t, "id").unwrap_or("?").to_string();
                let mut out = format!("Created {id} in {}: {}", str_of(&t, "status").unwrap_or("?"), str_of(&t, "title").unwrap_or(""));
                if let Some(c) = args.get("comment").and_then(Value::as_str).filter(|c| !c.trim().is_empty()) {
                    api.call("POST", &format!("/tickets/{}/comments", encode_path(&id)), &[], Some(json!({ "author": self.agent, "body": c })))?;
                    out.push_str(" (comment added)");
                }
                Ok(out)
            }
            "update_ticket" => {
                let path = format!("/tickets/{}", id_path("id")?);
                let before = api.get(&path)?;
                let body = Self::body(args, &["id", "comment"]);
                let has_changes = body.as_object().is_some_and(|o| !o.is_empty());
                let after = if has_changes { api.call("PATCH", &path, &[], Some(body))? } else { before.clone() };
                let mut out = format!("Updated {}: {}", str_of(&after, "id").unwrap_or("?"), describe_changes(&before, &after, TICKET_KEYS));
                if let Some(c) = args.get("comment").and_then(Value::as_str).filter(|c| !c.trim().is_empty()) {
                    api.call("POST", &format!("{path}/comments"), &[], Some(json!({ "author": self.agent, "body": c })))?;
                    out.push_str(if has_changes { "; comment added" } else { " (comment added)" });
                }
                Ok(out)
            }
            "add_comment" => {
                let author = args.get("author").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| self.agent.clone());
                let t = api.call("POST", &format!("/tickets/{}/comments", id_path("id")?), &[], Some(json!({ "author": author, "body": str_arg("body")? })))?;
                let n = t["comments"].as_array().map(|c| c.len()).unwrap_or(0);
                Ok(format!("Added comment #{n} to {}.", str_of(&t, "id").unwrap_or("?")))
            }
            "list_docs" => {
                let list = api.get(&format!("/projects/{}/docs", encode_path(&self.project(args)?)))?;
                let lines: Vec<String> = list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|d| {
                        let tags = list_of(d, "tags");
                        let mut bits = vec![str_of(d, "path").unwrap_or("?").to_string(), str_of(d, "title").unwrap_or("").to_string()];
                        if !tags.is_empty() {
                            bits.push(tags.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(","));
                        }
                        bits.push(date(d, "updated").unwrap_or_default());
                        bits.join(" | ")
                    })
                    .collect();
                Ok(if lines.is_empty() { "No docs yet. Create one with write_doc.".into() } else { lines.join("\n") })
            }
            "get_doc" => {
                let project = self.project(args)?;
                let d = api.get(&format!("/projects/{}/docs/{}", encode_path(&project), encode_path(&str_arg("path")?)))?;
                let body = str_of(&d, "body").unwrap_or("");
                let body = match args.get("section").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                    Some(sec) => extract_section(body, sec).ok_or_else(|| format!("No heading matching \"{sec}\". Headings: {}", headings(body).join(" · ")))?,
                    None => body.to_string(),
                };
                Ok(front_matter(meta_of(&d, &["path", "title", "tags"], &["updated"]), &body))
            }
            "write_doc" => {
                let project = self.project(args)?;
                let path = str_arg("path")?;
                let url = format!("/projects/{}/docs/{}", encode_path(&project), encode_path(&path));
                let existed = api.get(&url).is_ok();
                let d = api.call("PUT", &url, &[], Some(Self::body(args, &["project", "path"])))?;
                Ok(format!("{} doc {} (\"{}\").", if existed { "Updated" } else { "Created" }, str_of(&d, "path").unwrap_or("?"), str_of(&d, "title").unwrap_or("")))
            }
            "list_proposals" => {
                let list = api.get(&format!("/projects/{}/proposals", encode_path(&self.project(args)?)))?;
                let status = args.get("status").and_then(Value::as_str);
                let lines: Vec<String> = list
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|p| status.is_none_or(|s| p["status"] == s))
                    .map(|p| {
                        let tickets = list_of(p, "tickets");
                        let mut bits = vec![str_of(p, "id").unwrap_or("?").to_string(), str_of(p, "status").unwrap_or("?").to_string(), str_of(p, "title").unwrap_or("").to_string()];
                        if !tickets.is_empty() {
                            bits.push(format!("tickets: {}", tickets.join(",")));
                        }
                        bits.push(date(p, "updated").unwrap_or_default());
                        bits.join(" | ")
                    })
                    .collect();
                Ok(if lines.is_empty() { "No proposals.".into() } else { lines.join("\n") })
            }
            "get_proposal" => {
                let p = api.get(&format!("/proposals/{}", id_path("id")?))?;
                let body = str_of(&p, "body").unwrap_or("");
                let body = match args.get("section").and_then(Value::as_str).filter(|s| !s.trim().is_empty()) {
                    Some(sec) => extract_section(body, sec).ok_or_else(|| format!("No heading matching \"{sec}\". Headings: {}", headings(body).join(" · ")))?,
                    None => body.to_string(),
                };
                Ok(front_matter(meta_of(&p, &["id", "title", "status", "authors", "labels", "tickets", "supersedes", "superseded_by", "decided"], &["created", "updated"]), &body))
            }
            "create_proposal" => {
                let project = self.project(args)?;
                let mut body = Self::body(args, &["project"]);
                if body.get("authors").is_none() {
                    body["authors"] = json!([self.agent]);
                }
                let p = api.call("POST", &format!("/projects/{}/proposals", encode_path(&project)), &[], Some(body))?;
                Ok(format!("Created {} ({}): {}", str_of(&p, "id").unwrap_or("?"), str_of(&p, "status").unwrap_or("?"), str_of(&p, "title").unwrap_or("")))
            }
            "update_proposal" => {
                let path = format!("/proposals/{}", id_path("id")?);
                let before = api.get(&path)?;
                let after = api.call("PATCH", &path, &[], Some(Self::body(args, &["id"])))?;
                Ok(format!("Updated {}: {}", str_of(&after, "id").unwrap_or("?"), describe_changes(&before, &after, PROPOSAL_KEYS)))
            }
            "search" => {
                let mut q = vec![("q", str_arg("query")?), ("limit", (offset + PAGE + 1).to_string())];
                if let Some(p) = args.get("project").and_then(Value::as_str).filter(|p| !p.is_empty()) {
                    q.push(("project", p.to_string()));
                }
                let hits = api.call("GET", "/search", &q, None)?;
                let hits = hits.as_array().cloned().unwrap_or_default();
                let lines: Vec<String> = hits
                    .iter()
                    .skip(offset)
                    .take(PAGE)
                    .map(|h| {
                        let kind = str_of(h, "kind").unwrap_or("?");
                        let id = if kind == "doc" { format!("{}:{}", str_of(h, "project").unwrap_or("?"), str_of(h, "id").unwrap_or("?")) } else { str_of(h, "id").unwrap_or("?").to_string() };
                        let mut line = format!("{id} | {kind}");
                        if let Some(st) = str_of(h, "status") {
                            line.push_str(&format!(" | {st}"));
                        }
                        line.push_str(&format!(" | {}", str_of(h, "title").unwrap_or("")));
                        if let Some(sn) = str_of(h, "snippet") {
                            let sn: String = sn.chars().take(120).collect();
                            line.push_str(&format!(" — {sn}"));
                        }
                        line
                    })
                    .collect();
                let more = hits.len() > offset + PAGE;
                let mut out = lines.join("\n");
                if lines.is_empty() {
                    out = "No matches.".into();
                } else if more {
                    out.push_str(&format!("\nMore results: cursor=\"{}\". Narrow with project= or a more specific query.", offset + PAGE));
                }
                Ok(out)
            }
            _ => Err(format!("Unknown tool '{name}'.")),
        }
    }

    fn handle(&mut self, msg: &Value) -> Option<Value> {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        // Notifications (no id) and responses get no reply.
        let id = msg.get("id").cloned()?;
        if method.is_empty() {
            return None;
        }
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => {
                let requested = params["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSIONS[0]);
                let version = if PROTOCOL_VERSIONS.contains(&requested) { requested } else { PROTOCOL_VERSIONS[0] };
                if std::env::var("BANKAN_AGENT").is_err() {
                    if let Some(name) = params["clientInfo"]["name"].as_str() {
                        self.agent = agent_name(name);
                    }
                }
                Ok(json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": { "listChanged": false }, "prompts": { "listChanged": false } },
                    "serverInfo": { "name": "bankan", "title": "BanKan", "version": env!("CARGO_PKG_VERSION") },
                    "instructions": INSTRUCTIONS,
                }))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tools() })),
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params["arguments"].as_object().cloned().unwrap_or_default();
                let (text, is_error) = match self.call_tool(name, &args) {
                    Ok(t) => (t, false),
                    Err(e) => (e, true),
                };
                Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
            }
            "prompts/list" => Ok(json!({ "prompts": prompts() })),
            "prompts/get" => {
                let name = params["name"].as_str().unwrap_or("");
                let args = params["arguments"].as_object().cloned().unwrap_or_default();
                match prompt_text(name, &args) {
                    Some(text) => Ok(json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] })),
                    None => Err((-32602, format!("unknown prompt '{name}'"))),
                }
            }
            "resources/list" => Ok(json!({ "resources": [] })),
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        Some(match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
        })
    }
}

/// "claude-code" → "claude"; other clients keep their name.
fn agent_name(client: &str) -> String {
    let lower = client.to_ascii_lowercase();
    if lower.contains("claude") { "claude".into() } else { lower }
}

pub fn run(cfg: &Config) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let link = link::find(&cwd).unwrap_or_else(|e| {
        eprintln!("bankan mcp: {e:#}");
        None
    });
    let link_url = link.as_ref().and_then(|l| l.url.clone()).filter(|u| {
        let ok = link::is_local_url(u);
        if !ok {
            eprintln!("bankan mcp: ignoring url \"{u}\" in .bankan: only http://localhost or 127.0.0.1 addresses are allowed");
        }
        ok
    });
    let base = std::env::var("BANKAN_URL")
        .ok()
        .or(link_url)
        .unwrap_or_else(|| cfg.base_url())
        .trim_end_matches('/')
        .to_string();
    let agent = std::env::var("BANKAN_AGENT").unwrap_or_else(|_| "agent".into());
    let mut server = Server { api: Api { base }, link, agent };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let replies: Vec<Value> = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => batch.iter().filter_map(|m| server.handle(m)).collect(),
            Ok(msg) => server.handle(&msg).into_iter().collect(),
            Err(e) => vec![json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("parse error: {e}") } })],
        };
        for r in replies {
            serde_json::to_writer(&mut stdout, &r)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_line_format() {
        let t = json!({ "id": "RES-12", "status": "In Progress", "type": "Bug", "priority": "high", "assignee": "claude", "labels": ["api"], "title": "Fix SSE reconnect", "order": 1000.0 });
        assert_eq!(ticket_line(&t, false), "RES-12 | In Progress | Bug | high | @claude | #api | Fix SSE reconnect");
    }

    #[test]
    fn sections() {
        let body = "# A\nintro\n## Storage\nfiles\n### Detail\nmore\n## Other\nx";
        assert_eq!(extract_section(body, "storage").unwrap(), "## Storage\nfiles\n### Detail\nmore");
        assert!(extract_section(body, "nope").is_none());
        let fenced = "## Setup\n```sh\n# install deps\nnpm i\n```\nthen run\n## Next\nx";
        assert_eq!(extract_section(fenced, "setup").unwrap(), "## Setup\n```sh\n# install deps\nnpm i\n```\nthen run");
        assert!(extract_section(fenced, "install").is_none());
    }

    #[test]
    fn change_summary() {
        let b = json!({ "status": "Todo", "assignee": null, "labels": [] });
        let a = json!({ "status": "In Progress", "assignee": "claude", "labels": ["api"] });
        assert_eq!(describe_changes(&b, &a, &["status", "assignee", "labels"]), "status Todo → In Progress, assignee → claude, labels → api");
    }

    #[test]
    fn instructions_fit_claude_code_limit() {
        assert!(INSTRUCTIONS.len() < 4096);
    }

    #[test]
    fn rendered_ticket_keeps_markdown_raw() {
        let t = json!({ "id": "RES-1", "title": "x", "status": "Todo", "type": "Bug", "priority": "none", "order": 5.0,
            "created": "2026-10-20T14:03:11Z", "updated": "2026-10-21T01:00:00Z", "description": "## Problem\n\n\"quoted\"\n",
            "comments": [{ "id": 1, "author": "me", "at": "2026-10-21T01:00:00Z", "body": "hi" }] });
        let out = render_ticket(&t);
        assert!(out.contains("## Problem\n\n\"quoted\""));
        assert!(out.contains("updated: 2026-10-21\n"));
        assert!(!out.contains("order"));
        assert!(!out.contains("priority"));
        assert!(out.contains("### me · 2026-10-21\n\nhi"));
    }
}
