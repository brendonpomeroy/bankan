//! In-memory store backed by Markdown files with YAML front matter.
//!
//! The daemon is the only writer. Everything is loaded at startup, and every
//! mutation is written straight back to disk (atomically) before it returns.

use crate::model::*;
use chrono::{DateTime, NaiveDate, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Error {
    NotFound(String),
    Invalid(String),
    Conflict(String),
    Internal(anyhow::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NotFound(m) | Error::Invalid(m) | Error::Conflict(m) => f.write_str(m),
            Error::Internal(e) => write!(f, "{e:#}"),
        }
    }
}

impl<E: Into<anyhow::Error>> From<E> for Error {
    fn from(e: E) -> Self {
        Error::Internal(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(msg.into()))
}

/// Distinguishes a missing field (`None`) from an explicit `null` (`Some(None)`) in patches.
fn double_option<'de, T: Deserialize<'de>, D: Deserializer<'de>>(d: D) -> std::result::Result<Option<Option<T>>, D::Error> {
    Ok(Some(Option::deserialize(d)?))
}

// ---------------------------------------------------------------------------
// Inputs

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewProject {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Renames {
    #[serde(default)]
    pub statuses: HashMap<String, String>,
    #[serde(default)]
    pub types: HashMap<String, String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub milestones: HashMap<String, String>,
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectPatch {
    pub name: Option<String>,
    pub description: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub color: Option<Option<String>>,
    pub archived: Option<bool>,
    pub statuses: Option<Vec<Status>>,
    pub types: Option<Vec<Named>>,
    pub labels: Option<Vec<Named>>,
    pub milestones: Option<Vec<Milestone>>,
    pub fields: Option<Vec<FieldDef>>,
    /// Old name → new name, applied to tickets when statuses/types/labels/milestones are renamed.
    #[serde(default)]
    pub renames: Renames,
    /// The project's `updated` time when the caller loaded it. If set and the
    /// project has changed since, the patch is refused, so a stale settings
    /// form can't overwrite newer statuses, labels or fields.
    pub expected_updated: Option<DateTime<Utc>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub status: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub priority: Option<Priority>,
    pub labels: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub assignee: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub estimate: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub due: Option<Option<NaiveDate>>,
    #[serde(default, deserialize_with = "double_option")]
    pub milestone: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub parent: Option<Option<String>>,
    pub blocks: Option<Vec<String>>,
    pub related: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub duplicate_of: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub proposal: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub branch: Option<Option<String>>,
    pub links: Option<Vec<Link>>,
    /// Merged into the ticket's custom fields; a `null` value removes the field.
    pub fields: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MoveInput {
    pub status: String,
    /// Position within the target column (0 = top). Defaults to the bottom.
    pub index: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalInput {
    pub title: Option<String>,
    pub status: Option<ProposalStatus>,
    pub body: Option<String>,
    pub authors: Option<Vec<String>>,
    pub labels: Option<Vec<String>>,
    pub tickets: Option<Vec<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub supersedes: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub superseded_by: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub decided: Option<Option<NaiveDate>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocInput {
    pub title: Option<String>,
    pub body: Option<String>,
    pub tags: Option<Vec<String>>,
    /// Move the doc to a new path.
    pub new_path: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct TicketFilter {
    pub status: Option<String>,
    pub category: Option<Category>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub priority: Option<Priority>,
    pub label: Option<String>,
    pub assignee: Option<String>,
    pub milestone: Option<String>,
    pub parent: Option<String>,
    pub q: Option<String>,
    /// Include completed/canceled tickets (default true for the API, false for MCP).
    pub include_closed: Option<bool>,
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// Outputs

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    #[serde(flatten)]
    pub project: Project,
    pub counts: Counts,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Counts {
    pub backlog: usize,
    pub unstarted: usize,
    pub started: usize,
    pub completed: usize,
    pub canceled: usize,
    pub docs: usize,
    pub proposals: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub kind: &'static str,
    pub project: String,
    pub id: String,
    pub title: String,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

// ---------------------------------------------------------------------------

struct ProjectData {
    project: Project,
    tickets: BTreeMap<u32, Ticket>,
    proposals: BTreeMap<u32, Proposal>,
    docs: BTreeMap<String, Doc>,
}

pub struct Store {
    root: PathBuf,
    projects: BTreeMap<String, ProjectData>,
}

fn now() -> chrono::DateTime<Utc> {
    // Second precision keeps the files tidy.
    let n = Utc::now();
    n - chrono::Duration::nanoseconds(n.timestamp_subsec_nanos() as i64)
}

/// A project's new `updated` time: now, but always at least a second past the old one.
/// Timestamps are stored to the second, and `expected_updated` relies on every change
/// producing a different value, even two changes within the same second.
fn next_revision(prev: DateTime<Utc>) -> DateTime<Utc> {
    now().max(prev + chrono::Duration::seconds(1))
}

fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn dedupe(list: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for v in list.into_iter().map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
        if !out.iter().any(|o| o.eq_ignore_ascii_case(&v)) {
            out.push(v);
        }
    }
    out
}

pub fn valid_key(key: &str) -> bool {
    (2..=10).contains(&key.len())
        && key.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && key.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// `RES-12` → (`RES`, 12)
pub fn parse_ticket_id(id: &str) -> Option<(String, u32)> {
    let (key, n) = id.trim().rsplit_once('-')?;
    let key = key.to_ascii_uppercase();
    let n: u32 = n.parse().ok()?;
    valid_key(&key).then_some((key, n))
}

/// `RES-12` or `RES-P3` → `RES` (for ids already known to be valid).
fn project_of_id(id: &str) -> String {
    id.rsplit_once('-').map(|(k, _)| k.to_string()).unwrap_or_default()
}

/// `RES-P3` → (`RES`, 3)
pub fn parse_proposal_id(id: &str) -> Option<(String, u32)> {
    let (key, n) = id.trim().rsplit_once('-')?;
    let n = n.strip_prefix('P').or_else(|| n.strip_prefix('p'))?;
    let key = key.to_ascii_uppercase();
    let n: u32 = n.parse().ok()?;
    valid_key(&key).then_some((key, n))
}

fn clean_doc_path(path: &str) -> Result<String> {
    let p = path.trim().trim_matches('/').trim_end_matches(".md");
    if p.is_empty() {
        return invalid("doc path is empty");
    }
    let mut parts = Vec::new();
    for part in p.split('/') {
        let part = part.trim();
        if part.is_empty() || part == "." || part == ".." || part.starts_with('.') {
            return invalid(format!("invalid doc path '{path}'"));
        }
        if part.chars().any(|c| matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control()) {
            return invalid(format!("invalid character in doc path '{path}'"));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

/// Turns a title into a path segment: "Getting Started!" → "getting-started".
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    for c in title.trim().chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() { "untitled".into() } else { out }
}

// --- file format ------------------------------------------------------------

fn split_front_matter(text: &str) -> (&str, &str) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if let Some(rest) = text.strip_prefix("---\n").or_else(|| text.strip_prefix("---\r\n")) {
        for marker in ["\n---\n", "\n---\r\n"] {
            if let Some(end) = rest.find(marker) {
                let body = &rest[end + marker.len()..];
                return (&rest[..end + 1], body.strip_prefix('\n').unwrap_or(body));
            }
        }
        if let Some(fm) = rest.strip_suffix("\n---") {
            return (fm, "");
        }
    }
    ("", text)
}

fn read_md<T: DeserializeOwned>(path: &Path, body_key: &str) -> anyhow::Result<T> {
    let text = std::fs::read_to_string(path)?;
    let (fm, body) = split_front_matter(&text);
    let mut value: Value = if fm.trim().is_empty() { Value::Object(Default::default()) } else { serde_yaml::from_str(fm)? };
    let obj = value.as_object_mut().ok_or_else(|| anyhow::anyhow!("front matter is not a map"))?;
    obj.insert(body_key.into(), Value::String(body.trim_end().to_string()));
    Ok(serde_json::from_value(value)?)
}

fn md_text<T: Serialize>(item: &T, body_key: &str, skip: &[&str]) -> anyhow::Result<String> {
    let mut value = serde_json::to_value(item)?;
    let obj = value.as_object_mut().expect("struct serializes to a map");
    let body = match obj.remove(body_key) {
        Some(Value::String(s)) => s,
        _ => String::new(),
    };
    for k in skip {
        obj.remove(*k);
    }
    let yaml = serde_yaml::to_string(&value)?;
    let mut out = format!("---\n{yaml}---\n");
    if !body.trim().is_empty() {
        out.push('\n');
        out.push_str(body.trim_end());
        out.push('\n');
    }
    Ok(out)
}

fn write_atomic(path: &Path, content: &str) -> anyhow::Result<()> {
    let dir = path.parent().expect("file has a parent");
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".tmp-{}", path.file_name().unwrap().to_string_lossy()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn remove_file(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

/// Removes empty folders between `path`'s parent and `stop`.
fn prune_empty_dirs(path: &Path, stop: &Path) {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == stop || !d.starts_with(stop) || std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

fn walk_md(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            walk_md(&path, out);
        } else if path.extension().is_some_and(|e| e == "md") {
            out.push(path);
        }
    }
}

// --- store ------------------------------------------------------------------

impl Store {
    pub fn open(root: &Path) -> anyhow::Result<Store> {
        let projects_dir = root.join("projects");
        std::fs::create_dir_all(&projects_dir)?;
        let mut store = Store { root: root.to_path_buf(), projects: BTreeMap::new() };
        for entry in std::fs::read_dir(&projects_dir)?.flatten() {
            let dir = entry.path();
            if !dir.join("project.yaml").exists() {
                continue;
            }
            match Self::load_project(&dir) {
                // Files are written to projects/<key>/, so a folder whose name doesn't match its key
                // (e.g. a copy) would have its edits land in the other project's folder.
                Ok(data) if entry.file_name().to_str() != Some(data.project.key.as_str()) => {
                    eprintln!("bankan: skipping {}: its project.yaml says key {}, which doesn't match the folder name", dir.display(), data.project.key)
                }
                Ok(data) if store.projects.contains_key(&data.project.key) => eprintln!("bankan: skipping duplicate project {}", dir.display()),
                Ok(data) => {
                    store.projects.insert(data.project.key.clone(), data);
                }
                Err(e) => eprintln!("bankan: skipping project {}: {e:#}", dir.display()),
            }
        }
        Ok(store)
    }

    fn load_project(dir: &Path) -> anyhow::Result<ProjectData> {
        let project: Project = serde_yaml::from_str(&std::fs::read_to_string(dir.join("project.yaml"))?)?;
        let mut data = ProjectData { project, tickets: BTreeMap::new(), proposals: BTreeMap::new(), docs: BTreeMap::new() };

        let mut files = Vec::new();
        walk_md(&dir.join("tickets"), &mut files);
        for f in files.drain(..) {
            match read_md::<Ticket>(&f, "description") {
                Ok(t) => match parse_ticket_id(&t.id) {
                    Some((k, n)) if k == data.project.key => {
                        data.project.next_ticket = data.project.next_ticket.max(n + 1);
                        data.tickets.insert(n, t);
                    }
                    _ => eprintln!("bankan: bad ticket id in {}", f.display()),
                },
                Err(e) => eprintln!("bankan: skipping {}: {e:#}", f.display()),
            }
        }
        walk_md(&dir.join("proposals"), &mut files);
        for f in files.drain(..) {
            match read_md::<Proposal>(&f, "body") {
                Ok(p) => match parse_proposal_id(&p.id) {
                    Some((k, n)) if k == data.project.key => {
                        data.project.next_proposal = data.project.next_proposal.max(n + 1);
                        data.proposals.insert(n, p);
                    }
                    _ => eprintln!("bankan: bad proposal id in {}", f.display()),
                },
                Err(e) => eprintln!("bankan: skipping {}: {e:#}", f.display()),
            }
        }
        let docs_dir = dir.join("docs");
        walk_md(&docs_dir, &mut files);
        for f in files.drain(..) {
            match read_md::<Doc>(&f, "body") {
                Ok(mut d) => {
                    let rel = f.strip_prefix(&docs_dir).unwrap().with_extension("");
                    d.path = rel.to_string_lossy().replace('\\', "/");
                    data.docs.insert(d.path.clone(), d);
                }
                Err(e) => eprintln!("bankan: skipping {}: {e:#}", f.display()),
            }
        }
        Ok(data)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn project_dir(&self, key: &str) -> PathBuf {
        self.root.join("projects").join(key)
    }

    fn data(&self, key: &str) -> Result<&ProjectData> {
        self.projects.get(&key.to_ascii_uppercase()).ok_or_else(|| Error::NotFound(format!("project '{key}' not found")))
    }

    fn data_mut(&mut self, key: &str) -> Result<&mut ProjectData> {
        self.projects.get_mut(&key.to_ascii_uppercase()).ok_or_else(|| Error::NotFound(format!("project '{key}' not found")))
    }

    // Every change is written to disk first and only then applied in memory,
    // so a failed write never leaves the daemon showing data that isn't saved.

    /// Writes `project.yaml` and then replaces the in-memory project.
    fn put_project(&mut self, p: Project) -> Result<()> {
        write_atomic(&self.project_dir(&p.key).join("project.yaml"), &serde_yaml::to_string(&p)?)?;
        let key = p.key.clone();
        self.data_mut(&key)?.project = p;
        Ok(())
    }

    fn put_ticket(&mut self, t: Ticket) -> Result<()> {
        self.save_ticket(&t)?;
        let (key, n) = parse_ticket_id(&t.id).expect("stored ticket ids are valid");
        self.data_mut(&key)?.tickets.insert(n, t);
        Ok(())
    }

    fn put_proposal(&mut self, p: Proposal) -> Result<()> {
        self.save_proposal(&p)?;
        let (key, n) = parse_proposal_id(&p.id).expect("stored proposal ids are valid");
        self.data_mut(&key)?.proposals.insert(n, p);
        Ok(())
    }

    fn save_ticket(&self, t: &Ticket) -> Result<()> {
        let (key, _) = parse_ticket_id(&t.id).expect("stored ticket ids are valid");
        let path = self.project_dir(&key).join("tickets").join(format!("{}.md", t.id));
        write_atomic(&path, &md_text(t, "description", &[])?)?;
        Ok(())
    }

    fn save_proposal(&self, p: &Proposal) -> Result<()> {
        let (key, _) = parse_proposal_id(&p.id).expect("stored proposal ids are valid");
        let path = self.project_dir(&key).join("proposals").join(format!("{}.md", p.id));
        write_atomic(&path, &md_text(p, "body", &[])?)?;
        Ok(())
    }

    fn doc_file(&self, key: &str, path: &str) -> PathBuf {
        self.project_dir(key).join("docs").join(format!("{path}.md"))
    }

    fn save_doc(&self, key: &str, d: &Doc) -> Result<()> {
        write_atomic(&self.doc_file(key, &d.path), &md_text(d, "body", &["path"])?)?;
        Ok(())
    }

    // --- projects -----------------------------------------------------------

    fn counts(data: &ProjectData) -> Counts {
        let mut c = Counts { docs: data.docs.len(), proposals: data.proposals.len(), ..Default::default() };
        for t in data.tickets.values() {
            match data.project.status(&t.status).map(|s| s.category).unwrap_or(Category::Backlog) {
                Category::Backlog => c.backlog += 1,
                Category::Unstarted => c.unstarted += 1,
                Category::Started => c.started += 1,
                Category::Completed => c.completed += 1,
                Category::Canceled => c.canceled += 1,
            }
        }
        c
    }

    pub fn list_projects(&self, include_archived: bool) -> Vec<ProjectSummary> {
        self.projects
            .values()
            .filter(|d| include_archived || !d.project.archived)
            .map(|d| ProjectSummary { project: d.project.clone(), counts: Self::counts(d) })
            .collect()
    }

    pub fn get_project(&self, key: &str) -> Result<ProjectSummary> {
        let d = self.data(key)?;
        Ok(ProjectSummary { project: d.project.clone(), counts: Self::counts(d) })
    }

    pub fn create_project(&mut self, input: NewProject) -> Result<Project> {
        let key = input.key.trim().to_ascii_uppercase();
        if !valid_key(&key) {
            return invalid("project key must be 2–10 characters: uppercase letters and digits, starting with a letter (e.g. RES)");
        }
        if self.projects.contains_key(&key) {
            return Err(Error::Conflict(format!("project '{key}' already exists")));
        }
        let name = input.name.trim();
        if name.is_empty() {
            return invalid("project name is required");
        }
        let t = now();
        let project = Project {
            key: key.clone(),
            name: name.into(),
            description: input.description.trim().into(),
            color: clean(input.color),
            archived: false,
            statuses: Project::default_statuses(),
            types: Project::default_types(),
            labels: vec![],
            milestones: vec![],
            fields: vec![],
            next_ticket: 1,
            next_proposal: 1,
            created: t,
            updated: t,
        };
        write_atomic(&self.project_dir(&key).join("project.yaml"), &serde_yaml::to_string(&project)?)?;
        self.projects.insert(key, ProjectData { project: project.clone(), tickets: BTreeMap::new(), proposals: BTreeMap::new(), docs: BTreeMap::new() });
        Ok(project)
    }

    pub fn update_project(&mut self, key: &str, patch: ProjectPatch) -> Result<Project> {
        let key = self.data(key)?.project.key.clone();
        let mut p = self.data(&key)?.project.clone();
        if patch.expected_updated.is_some_and(|t| t != p.updated) {
            return Err(Error::Conflict(format!("project {key} was changed elsewhere since you loaded it (for example, a new label). Reload and try again.")));
        }

        if let Some(name) = patch.name {
            if name.trim().is_empty() {
                return invalid("project name is required");
            }
            p.name = name.trim().into();
        }
        if let Some(d) = patch.description {
            p.description = d.trim().into();
        }
        if let Some(c) = patch.color {
            p.color = clean(c);
        }
        if let Some(a) = patch.archived {
            p.archived = a;
        }
        if let Some(statuses) = patch.statuses {
            if statuses.is_empty() {
                return invalid("a project needs at least one status");
            }
            check_unique(statuses.iter().map(|s| &s.name), "status")?;
            p.statuses = statuses.into_iter().map(|s| Status { name: s.name.trim().into(), ..s }).collect();
        }
        if let Some(types) = patch.types {
            if types.is_empty() {
                return invalid("a project needs at least one ticket type");
            }
            check_unique(types.iter().map(|s| &s.name), "type")?;
            p.types = types.into_iter().map(|s| Named { name: s.name.trim().into(), ..s }).collect();
        }
        if let Some(labels) = patch.labels {
            check_unique(labels.iter().map(|s| &s.name), "label")?;
            p.labels = labels.into_iter().map(|s| Named { name: s.name.trim().into(), ..s }).collect();
        }
        if let Some(ms) = patch.milestones {
            check_unique(ms.iter().map(|s| &s.name), "milestone")?;
            p.milestones = ms.into_iter().map(|m| Milestone { name: m.name.trim().into(), ..m }).collect();
        }
        if let Some(fields) = patch.fields {
            check_unique(fields.iter().map(|s| &s.name), "field")?;
            p.fields = fields.into_iter().map(|f| FieldDef { name: f.name.trim().into(), ..f }).collect();
        }
        p.updated = next_revision(p.updated);

        // Bring tickets in line with renamed or removed statuses, types, labels and milestones.
        let r = &patch.renames;
        let rename = |map: &HashMap<String, String>, v: &str| map.get(v).cloned().unwrap_or_else(|| v.to_string());
        let old_project = self.data(&key)?.project.clone();
        let mut changed = Vec::new();
        for t in self.data(&key)?.tickets.values() {
            let mut t2 = t.clone();
            let status = rename(&r.statuses, &t.status);
            t2.status = match p.status(&status) {
                Some(s) => s.name.clone(),
                None => {
                    // Status removed: fall back to the first status in the same category.
                    let cat = old_project.status(&t.status).map(|s| s.category);
                    p.statuses.iter().find(|s| Some(s.category) == cat).unwrap_or(&p.statuses[0]).name.clone()
                }
            };
            let kind = rename(&r.types, &t.kind);
            t2.kind = p.types.iter().find(|x| x.name.eq_ignore_ascii_case(&kind)).unwrap_or(&p.types[0]).name.clone();
            t2.labels = dedupe(
                t.labels
                    .iter()
                    .map(|l| rename(&r.labels, l))
                    .filter_map(|l| p.labels.iter().find(|x| x.name.eq_ignore_ascii_case(&l)).map(|x| x.name.clone()))
                    .collect(),
            );
            t2.milestone = t
                .milestone
                .as_ref()
                .map(|m| rename(&r.milestones, m))
                .and_then(|m| p.milestones.iter().find(|x| x.name.eq_ignore_ascii_case(&m)).map(|x| x.name.clone()));
            let defs = &p.fields;
            t2.fields = t.fields.iter().map(|(k, v)| (rename(&r.fields, k), v.clone())).filter(|(k, _)| defs.iter().any(|f| &f.name == k)).collect();
            if t2.status != t.status || t2.kind != t.kind || t2.labels != t.labels || t2.milestone != t.milestone || t2.fields != t.fields {
                changed.push(t2);
            }
        }

        self.put_project(p.clone())?;
        for t in changed {
            self.put_ticket(t)?;
        }
        Ok(p)
    }

    pub fn delete_project(&mut self, key: &str) -> Result<()> {
        let key = self.data(key)?.project.key.clone();
        let dir = self.project_dir(&key);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        self.projects.remove(&key);
        Ok(())
    }

    // --- tickets ------------------------------------------------------------

    pub fn list_tickets(&self, key: &str, f: &TicketFilter) -> Result<Vec<Ticket>> {
        let data = self.data(key)?;
        let p = &data.project;
        let q = f.q.as_deref().map(str::to_lowercase).filter(|q| !q.is_empty());
        let include_closed = f.include_closed.unwrap_or(true);
        let eq = |a: &Option<String>, b: &str| a.as_deref().is_none_or(|a| a.eq_ignore_ascii_case(b));
        let mut out: Vec<Ticket> = data
            .tickets
            .values()
            .filter(|t| {
                let cat = p.status(&t.status).map(|s| s.category).unwrap_or(Category::Backlog);
                (include_closed || !cat.is_closed() || f.status.is_some() || f.category.is_some_and(|c| c.is_closed()))
                    && f.category.is_none_or(|c| c == cat)
                    && eq(&f.status, &t.status)
                    && eq(&f.kind, &t.kind)
                    && f.priority.is_none_or(|pr| pr == t.priority)
                    && f.label.as_deref().is_none_or(|l| t.labels.iter().any(|x| x.eq_ignore_ascii_case(l)))
                    && f.assignee.as_deref().is_none_or(|a| t.assignee.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(a)))
                    && f.milestone.as_deref().is_none_or(|m| t.milestone.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(m)))
                    && f.parent.as_deref().is_none_or(|m| t.parent.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(m)))
                    && q.as_ref().is_none_or(|q| {
                        t.id.to_lowercase() == *q || t.title.to_lowercase().contains(q) || t.description.to_lowercase().contains(q)
                    })
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| a.order.total_cmp(&b.order));
        if let Some(limit) = f.limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    pub fn get_ticket(&self, id: &str) -> Result<Ticket> {
        let (key, n) = parse_ticket_id(id).ok_or_else(|| Error::Invalid(format!("'{id}' is not a ticket id (expected e.g. RES-12)")))?;
        self.data(&key)?.tickets.get(&n).cloned().ok_or_else(|| Error::NotFound(format!("ticket {id} not found")))
    }

    fn proposal_exists(&self, id: &str) -> bool {
        self.get_proposal(id).is_ok()
    }

    fn check_ticket_refs(&self, own: &str, ids: &[String]) -> Result<Vec<String>> {
        let mut out = Vec::new();
        for id in dedupe(ids.to_vec()) {
            let canon = self.get_ticket(&id).map_err(|_| Error::Invalid(format!("ticket {id} does not exist")))?.id;
            if canon == own {
                return invalid(format!("{own} cannot reference itself"));
            }
            out.push(canon);
        }
        Ok(out)
    }

    fn bottom_order(data: &ProjectData, status: &str) -> f64 {
        data.tickets.values().filter(|t| t.status == status).map(|t| t.order).fold(0.0, f64::max) + 1000.0
    }

    /// Validates and applies a ticket input to `t`. Unknown labels are added to
    /// `p`, a working copy of the project the caller saves along with the ticket,
    /// so nothing is written if any part of the input is invalid.
    fn apply_ticket_input(&self, p: &mut Project, t: &mut Ticket, input: TicketInput) -> Result<()> {
        if let Some(title) = input.title {
            if title.trim().is_empty() {
                return invalid("title is required");
            }
            t.title = title.trim().into();
        }
        if let Some(d) = input.description {
            t.description = d.trim_end().into();
        }
        if let Some(s) = input.status {
            let st = p.status(&s).ok_or_else(|| {
                Error::Invalid(format!("unknown status '{s}'. Valid: {}", p.statuses.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))
            })?;
            if st.name != t.status {
                t.status = st.name.clone();
                t.order = Self::bottom_order(self.data(&p.key)?, &t.status);
            }
        }
        if let Some(k) = input.kind {
            t.kind = p.types.iter().find(|x| x.name.eq_ignore_ascii_case(&k)).map(|x| x.name.clone()).ok_or_else(|| {
                Error::Invalid(format!("unknown type '{k}'. Valid: {}", p.types.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")))
            })?;
        }
        if let Some(pr) = input.priority {
            t.priority = pr;
        }
        if let Some(labels) = input.labels {
            let labels = dedupe(labels);
            let mut new_labels = Vec::new();
            let mut canon = Vec::new();
            for l in labels {
                match p.labels.iter().find(|x| x.name.eq_ignore_ascii_case(&l)) {
                    Some(x) => canon.push(x.name.clone()),
                    None => {
                        new_labels.push(Named { name: l.clone(), color: None });
                        canon.push(l);
                    }
                }
            }
            if !new_labels.is_empty() {
                p.labels.extend(new_labels);
                p.updated = next_revision(p.updated);
            }
            t.labels = canon;
        }
        if let Some(a) = input.assignee {
            t.assignee = clean(a);
        }
        if let Some(e) = input.estimate {
            t.estimate = e;
        }
        if let Some(d) = input.due {
            t.due = d;
        }
        if let Some(m) = input.milestone {
            t.milestone = match clean(m) {
                None => None,
                Some(m) => Some(p.milestones.iter().find(|x| x.name.eq_ignore_ascii_case(&m)).map(|x| x.name.clone()).ok_or_else(|| {
                    Error::Invalid(format!("unknown milestone '{m}'. Valid: {}", valid_names(p.milestones.iter().map(|s| s.name.as_str()))))
                })?),
            };
        }
        if let Some(parent) = input.parent {
            t.parent = match clean(parent) {
                None => None,
                Some(id) => Some(self.check_ticket_refs(&t.id, &[id])?.remove(0)),
            };
        }
        if let Some(b) = input.blocks {
            t.blocks = self.check_ticket_refs(&t.id, &b)?;
        }
        if let Some(r) = input.related {
            t.related = self.check_ticket_refs(&t.id, &r)?;
        }
        if let Some(d) = input.duplicate_of {
            t.duplicate_of = match clean(d) {
                None => None,
                Some(id) => Some(self.check_ticket_refs(&t.id, &[id])?.remove(0)),
            };
        }
        if let Some(pr) = input.proposal {
            t.proposal = match clean(pr) {
                None => None,
                Some(id) => {
                    if !self.proposal_exists(&id) {
                        return invalid(format!("proposal {id} does not exist"));
                    }
                    Some(self.get_proposal(&id)?.id)
                }
            };
        }
        if let Some(b) = input.branch {
            t.branch = clean(b);
        }
        if let Some(links) = input.links {
            let mut out = Vec::new();
            for l in links {
                let url = l.url.trim().to_string();
                if url.is_empty() {
                    continue;
                }
                check_url(&url)?;
                out.push(Link { url, title: clean(l.title) });
            }
            t.links = out;
        }
        if let Some(fields) = input.fields {
            for (k, v) in fields {
                if v.is_null() || v.as_str() == Some("") {
                    t.fields.remove(&k);
                    continue;
                }
                let def = p.fields.iter().find(|f| f.name == k).ok_or_else(|| Error::Invalid(format!("unknown custom field '{k}'")))?;
                if def.kind == FieldKind::Select {
                    let s = v.as_str().unwrap_or_default();
                    if !def.options.iter().any(|o| o == s) {
                        return invalid(format!("'{s}' is not an option for {k}. Valid: {}", def.options.join(", ")));
                    }
                }
                match def.kind {
                    FieldKind::Number if !v.is_number() => return invalid(format!("{k} must be a number")),
                    FieldKind::Text | FieldKind::Select | FieldKind::Date | FieldKind::Url if !v.is_string() => return invalid(format!("{k} must be a string")),
                    FieldKind::Date if v.as_str().unwrap().parse::<NaiveDate>().is_err() => return invalid(format!("{k} must be a date like 2026-10-22")),
                    FieldKind::Url => check_url(v.as_str().unwrap())?,
                    _ => {}
                }
                t.fields.insert(k, v);
            }
        }
        let closed = p.status(&t.status).is_some_and(|s| s.category.is_closed());
        match (closed, t.closed) {
            (true, None) => t.closed = Some(now()),
            (false, Some(_)) => t.closed = None,
            _ => {}
        }
        Ok(())
    }

    pub fn create_ticket(&mut self, key: &str, input: TicketInput) -> Result<Ticket> {
        let data = self.data(key)?;
        let key = data.project.key.clone();
        if input.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
            return invalid("title is required");
        }
        let n = data.project.next_ticket;
        let first_status = data
            .project
            .statuses
            .iter()
            .find(|s| s.category == Category::Unstarted)
            .unwrap_or(&data.project.statuses[0])
            .name
            .clone();
        let t0 = now();
        let mut t = Ticket {
            id: format!("{key}-{n}"),
            title: String::new(),
            status: first_status.clone(),
            kind: data.project.types[0].name.clone(),
            priority: Priority::None,
            labels: vec![],
            assignee: None,
            estimate: None,
            due: None,
            milestone: None,
            parent: None,
            blocks: vec![],
            related: vec![],
            duplicate_of: None,
            proposal: None,
            branch: None,
            links: vec![],
            fields: BTreeMap::new(),
            order: Self::bottom_order(data, &first_status),
            created: t0,
            updated: t0,
            closed: None,
            comments: vec![],
            description: String::new(),
        };
        let mut p = self.data(&key)?.project.clone();
        self.apply_ticket_input(&mut p, &mut t, input)?;
        // The counter is saved first so a failed ticket write never reuses a key.
        p.next_ticket = n + 1;
        self.put_project(p)?;
        self.put_ticket(t.clone())?;
        self.sync_ticket_proposal(None, &t)?;
        Ok(t)
    }

    /// Keeps proposals' `tickets` lists in step with a ticket's `proposal` field:
    /// removes the ticket from the proposal it left and adds it to the new one.
    fn sync_ticket_proposal(&mut self, old: Option<&str>, t: &Ticket) -> Result<()> {
        if let Some(old) = old.filter(|o| Some(*o) != t.proposal.as_deref()) {
            if let Ok(mut p) = self.get_proposal(old) {
                if p.tickets.contains(&t.id) {
                    p.tickets.retain(|x| x != &t.id);
                    p.updated = now();
                    self.put_proposal(p)?;
                }
            }
        }
        if let Some(pid) = &t.proposal {
            let mut p = self.get_proposal(pid)?;
            if !p.tickets.contains(&t.id) {
                p.tickets.push(t.id.clone());
                p.updated = now();
                self.put_proposal(p)?;
            }
        }
        Ok(())
    }

    pub fn update_ticket(&mut self, id: &str, input: TicketInput) -> Result<Ticket> {
        let mut t = self.get_ticket(id)?;
        let old_proposal = t.proposal.clone();
        let mut p = self.data(&project_of_id(&t.id))?.project.clone();
        let labels_before = p.labels.len();
        self.apply_ticket_input(&mut p, &mut t, input)?;
        t.updated = now();
        if p.labels.len() != labels_before {
            self.put_project(p)?;
        }
        self.put_ticket(t.clone())?;
        self.sync_ticket_proposal(old_proposal.as_deref(), &t)?;
        Ok(t)
    }

    pub fn move_ticket(&mut self, id: &str, input: MoveInput) -> Result<Ticket> {
        let mut t = self.get_ticket(id)?;
        let key = project_of_id(&t.id);
        let p = &self.data(&key)?.project;
        let status = p.status(&input.status).ok_or_else(|| Error::Invalid(format!("unknown status '{}'", input.status)))?.clone();

        let data = self.data(&key)?;
        let mut column: Vec<&Ticket> = data.tickets.values().filter(|x| x.status == status.name && x.id != t.id).collect();
        column.sort_by(|a, b| a.order.total_cmp(&b.order));
        let idx = input.index.unwrap_or(column.len()).min(column.len());
        let before = idx.checked_sub(1).map(|i| column[i].order);
        let after = column.get(idx).map(|x| x.order);
        let order = match (before, after) {
            (None, None) => 1000.0,
            (Some(b), None) => b + 1000.0,
            (None, Some(a)) => a - 1000.0,
            (Some(b), Some(a)) => (a + b) / 2.0,
        };
        // Renumber the column if midpoints have become too close to distinguish.
        let needs_renumber = matches!((before, after), (Some(b), Some(a)) if (a - b).abs() < 1e-6);

        t.status = status.name.clone();
        t.order = order;
        t.updated = now();
        match (status.category.is_closed(), t.closed) {
            (true, None) => t.closed = Some(now()),
            (false, Some(_)) => t.closed = None,
            _ => {}
        }
        if needs_renumber {
            let mut column: Vec<Ticket> = column.into_iter().cloned().collect();
            column.insert(idx, t.clone());
            for (i, mut x) in column.into_iter().enumerate() {
                let order = (i as f64 + 1.0) * 1000.0;
                if x.id == t.id || x.order != order {
                    x.order = order;
                    self.put_ticket(x)?;
                }
            }
            return self.get_ticket(&t.id);
        }
        self.put_ticket(t.clone())?;
        Ok(t)
    }

    pub fn add_comment(&mut self, id: &str, author: &str, body: &str) -> Result<Ticket> {
        if body.trim().is_empty() {
            return invalid("comment body is empty");
        }
        let mut t = self.get_ticket(id)?;
        let cid = t.comments.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        let author = if author.trim().is_empty() { "me" } else { author.trim() };
        t.comments.push(Comment { id: cid, author: author.into(), at: now(), body: body.trim_end().into() });
        t.updated = now();
        self.put_ticket(t.clone())?;
        Ok(t)
    }

    pub fn delete_comment(&mut self, id: &str, comment: u32) -> Result<Ticket> {
        let mut t = self.get_ticket(id)?;
        let before = t.comments.len();
        t.comments.retain(|c| c.id != comment);
        if t.comments.len() == before {
            return Err(Error::NotFound(format!("comment {comment} not found on {id}")));
        }
        t.updated = now();
        self.put_ticket(t.clone())?;
        Ok(t)
    }

    pub fn delete_ticket(&mut self, id: &str) -> Result<()> {
        let t = self.get_ticket(id)?;
        let (key, n) = parse_ticket_id(&t.id).unwrap();
        remove_file(&self.project_dir(&key).join("tickets").join(format!("{}.md", t.id)))?;
        self.data_mut(&key)?.tickets.remove(&n);

        // Drop references to the deleted ticket from every other ticket and proposal.
        let mut changed_tickets = Vec::new();
        let mut changed_proposals = Vec::new();
        for data in self.projects.values() {
            for x in data.tickets.values() {
                let mut x2 = x.clone();
                x2.blocks.retain(|b| b != &t.id);
                x2.related.retain(|b| b != &t.id);
                if x2.parent.as_deref() == Some(&t.id) {
                    x2.parent = None;
                }
                if x2.duplicate_of.as_deref() == Some(&t.id) {
                    x2.duplicate_of = None;
                }
                if (&x2.blocks, &x2.related, &x2.parent, &x2.duplicate_of) != (&x.blocks, &x.related, &x.parent, &x.duplicate_of) {
                    changed_tickets.push(x2);
                }
            }
            for p in data.proposals.values().filter(|p| p.tickets.contains(&t.id)) {
                let mut p = p.clone();
                p.tickets.retain(|b| b != &t.id);
                changed_proposals.push(p);
            }
        }
        for x in changed_tickets {
            self.put_ticket(x)?;
        }
        for p in changed_proposals {
            self.put_proposal(p)?;
        }
        Ok(())
    }

    /// Tickets that block `id` (the inverse of `blocks`) and its sub-tickets.
    pub fn ticket_backlinks(&self, id: &str) -> (Vec<String>, Vec<String>) {
        let mut blocked_by = Vec::new();
        let mut children = Vec::new();
        for data in self.projects.values() {
            for t in data.tickets.values() {
                if t.blocks.iter().any(|b| b == id) {
                    blocked_by.push(t.id.clone());
                }
                if t.parent.as_deref() == Some(id) {
                    children.push(t.id.clone());
                }
            }
        }
        (blocked_by, children)
    }

    // --- proposals ----------------------------------------------------------

    pub fn list_proposals(&self, key: &str) -> Result<Vec<Proposal>> {
        let mut out: Vec<Proposal> = self.data(key)?.proposals.values().cloned().collect();
        out.sort_by_key(|p| std::cmp::Reverse(p.updated));
        Ok(out)
    }

    pub fn get_proposal(&self, id: &str) -> Result<Proposal> {
        let (key, n) = parse_proposal_id(id).ok_or_else(|| Error::Invalid(format!("'{id}' is not a proposal id (expected e.g. RES-P3)")))?;
        self.data(&key)?.proposals.get(&n).cloned().ok_or_else(|| Error::NotFound(format!("proposal {id} not found")))
    }

    fn apply_proposal_input(&self, p: &mut Proposal, input: ProposalInput) -> Result<()> {
        if let Some(title) = input.title {
            if title.trim().is_empty() {
                return invalid("title is required");
            }
            p.title = title.trim().into();
        }
        if let Some(body) = input.body {
            p.body = body.trim_end().into();
        }
        if let Some(a) = input.authors {
            p.authors = dedupe(a);
        }
        if let Some(l) = input.labels {
            p.labels = dedupe(l);
        }
        if let Some(t) = input.tickets {
            p.tickets = self.check_ticket_refs("", &t)?;
        }
        for (field, value) in [(&mut p.supersedes, input.supersedes), (&mut p.superseded_by, input.superseded_by)] {
            if let Some(v) = value {
                *field = match clean(v) {
                    None => None,
                    Some(id) => {
                        let other = self.get_proposal(&id).map_err(|_| Error::Invalid(format!("proposal {id} does not exist")))?.id;
                        if other == p.id {
                            return invalid(format!("{other} cannot supersede itself"));
                        }
                        Some(other)
                    }
                };
            }
        }
        if let Some(d) = input.decided {
            p.decided = d;
        }
        if let Some(s) = input.status {
            let decided = matches!(s, ProposalStatus::Accepted | ProposalStatus::Rejected);
            if decided && p.status != s && p.decided.is_none() {
                p.decided = Some(Utc::now().date_naive());
            }
            p.status = s;
        }
        Ok(())
    }

    pub fn create_proposal(&mut self, key: &str, input: ProposalInput) -> Result<Proposal> {
        let data = self.data(key)?;
        let key = data.project.key.clone();
        if input.title.as_deref().is_none_or(|t| t.trim().is_empty()) {
            return invalid("title is required");
        }
        let n = data.project.next_proposal;
        let t0 = now();
        let mut p = Proposal {
            id: format!("{key}-P{n}"),
            title: String::new(),
            status: ProposalStatus::Draft,
            authors: vec![],
            labels: vec![],
            tickets: vec![],
            supersedes: None,
            superseded_by: None,
            decided: None,
            created: t0,
            updated: t0,
            body: PROPOSAL_TEMPLATE.into(),
        };
        self.apply_proposal_input(&mut p, input)?;
        let mut project = self.data(&key)?.project.clone();
        project.next_proposal = n + 1;
        self.put_project(project)?;
        self.put_proposal(p.clone())?;
        self.sync_proposal_links(&[], None, &p)?;
        Ok(p)
    }

    pub fn update_proposal(&mut self, id: &str, input: ProposalInput) -> Result<Proposal> {
        let mut p = self.get_proposal(id)?;
        let before = p.clone();
        self.apply_proposal_input(&mut p, input)?;
        p.updated = now();
        self.put_proposal(p.clone())?;
        self.sync_proposal_links(&before.tickets, before.supersedes.as_deref(), &p)?;
        Ok(p)
    }

    /// Keeps the other side of a proposal's links in step after it changes:
    /// - tickets added to `tickets` get `proposal` set (unless they already point at another
    ///   proposal); tickets removed from it that pointed here get it cleared;
    /// - when P2 supersedes P1, P1 gets `superseded_by: P2` (and the superseded status once P2
    ///   is accepted); if P2 stops superseding P1, that back-link is cleared.
    fn sync_proposal_links(&mut self, old_tickets: &[String], old_supersedes: Option<&str>, p: &Proposal) -> Result<()> {
        for id in old_tickets.iter().filter(|id| !p.tickets.contains(id)) {
            if let Ok(mut t) = self.get_ticket(id) {
                if t.proposal.as_deref() == Some(&p.id) {
                    t.proposal = None;
                    t.updated = now();
                    self.put_ticket(t)?;
                }
            }
        }
        for id in p.tickets.iter().filter(|id| !old_tickets.contains(id)) {
            let mut t = self.get_ticket(id)?;
            if t.proposal.is_none() {
                t.proposal = Some(p.id.clone());
                t.updated = now();
                self.put_ticket(t)?;
            }
        }
        if let Some(old_id) = old_supersedes.filter(|o| Some(*o) != p.supersedes.as_deref()) {
            if let Ok(mut old) = self.get_proposal(old_id) {
                if old.superseded_by.as_deref() == Some(&p.id) {
                    old.superseded_by = None;
                    old.updated = now();
                    self.put_proposal(old)?;
                }
            }
        }
        if let Some(old_id) = &p.supersedes {
            let mut old = self.get_proposal(old_id)?;
            let accepted = matches!(p.status, ProposalStatus::Accepted | ProposalStatus::Implemented);
            if old.superseded_by.as_deref() != Some(&p.id) || (accepted && old.status != ProposalStatus::Superseded) {
                old.superseded_by = Some(p.id.clone());
                if accepted {
                    old.status = ProposalStatus::Superseded;
                }
                old.updated = now();
                self.put_proposal(old)?;
            }
        }
        Ok(())
    }

    pub fn delete_proposal(&mut self, id: &str) -> Result<()> {
        let p = self.get_proposal(id)?;
        let (key, n) = parse_proposal_id(&p.id).unwrap();
        remove_file(&self.project_dir(&key).join("proposals").join(format!("{}.md", p.id)))?;
        self.data_mut(&key)?.proposals.remove(&n);
        let data = self.data(&key)?;
        let tickets: Vec<Ticket> = data.tickets.values().filter(|t| t.proposal.as_deref() == Some(&p.id)).cloned().collect();
        let proposals: Vec<Proposal> =
            data.proposals.values().filter(|x| x.supersedes.as_deref() == Some(&p.id) || x.superseded_by.as_deref() == Some(&p.id)).cloned().collect();
        for mut t in tickets {
            t.proposal = None;
            self.put_ticket(t)?;
        }
        for mut x in proposals {
            x.supersedes = x.supersedes.filter(|s| s != &p.id);
            x.superseded_by = x.superseded_by.filter(|s| s != &p.id);
            self.put_proposal(x)?;
        }
        Ok(())
    }

    // --- docs ---------------------------------------------------------------

    pub fn list_docs(&self, key: &str) -> Result<Vec<Doc>> {
        Ok(self.data(key)?.docs.values().map(|d| Doc { body: String::new(), ..d.clone() }).collect())
    }

    pub fn get_doc(&self, key: &str, path: &str) -> Result<Doc> {
        let path = clean_doc_path(path)?;
        self.data(key)?.docs.get(&path).cloned().ok_or_else(|| Error::NotFound(format!("doc '{path}' not found")))
    }

    /// Creates the doc if it doesn't exist, otherwise updates it.
    pub fn put_doc(&mut self, key: &str, path: &str, input: DocInput) -> Result<Doc> {
        let key = self.data(key)?.project.key.clone();
        let path = clean_doc_path(path)?;
        let existing = self.data(&key)?.docs.get(&path).cloned();
        if existing.is_none() {
            self.check_doc_path_free(&key, &path, None)?;
        }
        let t0 = now();
        let mut d = existing.clone().unwrap_or_else(|| Doc {
            path: path.clone(),
            title: path.rsplit('/').next().unwrap_or(&path).replace('-', " "),
            tags: vec![],
            created: t0,
            updated: t0,
            body: String::new(),
        });
        if let Some(title) = input.title {
            if title.trim().is_empty() {
                return invalid("title is required");
            }
            d.title = title.trim().into();
        }
        if let Some(body) = input.body {
            d.body = body.trim_end().into();
        }
        if let Some(tags) = input.tags {
            d.tags = dedupe(tags);
        }
        d.updated = t0;
        let mut old_file = None;
        if let Some(new_path) = input.new_path {
            let new_path = clean_doc_path(&new_path)?;
            if new_path != path {
                self.check_doc_path_free(&key, &new_path, Some(&path))?;
                if existing.is_some() {
                    let (from, to) = (self.doc_file(&key, &path), self.doc_file(&key, &new_path));
                    if new_path.eq_ignore_ascii_case(&path) {
                        // A case-only rename: on a case-insensitive disk (the macOS default) both names are
                        // the same file, so rename it in place rather than writing one and deleting "the other".
                        std::fs::rename(&from, &to)?;
                    } else {
                        old_file = Some(from);
                    }
                }
                d.path = new_path;
            }
        }
        // Write the new file before touching the old one, so a failed write loses nothing.
        self.save_doc(&key, &d)?;
        let docs = &mut self.data_mut(&key)?.docs;
        docs.remove(&path);
        docs.insert(d.path.clone(), d.clone());
        if let Some(old) = old_file {
            remove_file(&old)?;
            prune_empty_dirs(&old, &self.project_dir(&key).join("docs"));
        }
        Ok(d)
    }

    /// Doc paths must be unique ignoring case: on a case-insensitive disk `Guides/Setup`
    /// and `guides/setup` are the same file, so one would silently overwrite the other.
    fn check_doc_path_free(&self, key: &str, path: &str, except: Option<&str>) -> Result<()> {
        match self.data(key)?.docs.keys().find(|k| Some(k.as_str()) != except && k.eq_ignore_ascii_case(path)) {
            Some(k) => Err(Error::Conflict(format!("doc '{k}' already exists"))),
            None => Ok(()),
        }
    }

    pub fn delete_doc(&mut self, key: &str, path: &str) -> Result<()> {
        let key = self.data(key)?.project.key.clone();
        let path = clean_doc_path(path)?;
        if !self.data(&key)?.docs.contains_key(&path) {
            return Err(Error::NotFound(format!("doc '{path}' not found")));
        }
        let file = self.doc_file(&key, &path);
        remove_file(&file)?;
        prune_empty_dirs(&file, &self.project_dir(&key).join("docs"));
        self.data_mut(&key)?.docs.remove(&path);
        Ok(())
    }

    // --- search -------------------------------------------------------------

    pub fn search(&self, q: &str, project: Option<&str>, limit: usize) -> Result<Vec<SearchHit>> {
        let q = q.trim().to_lowercase();
        if q.is_empty() {
            return Ok(vec![]);
        }
        let projects: Vec<&ProjectData> = match project {
            Some(k) => vec![self.data(k)?],
            None => self.projects.values().filter(|d| !d.project.archived).collect(),
        };
        // (rank, hit): exact id match first, then title matches, then body matches.
        let mut hits: Vec<(u8, SearchHit)> = Vec::new();
        let mut consider = |kind, project: &str, id: &str, title: &str, body: &str, status: Option<String>| {
            let rank = if id.to_lowercase() == q {
                0
            } else if title.to_lowercase().contains(&q) {
                1
            } else if body.to_lowercase().contains(&q) {
                2
            } else {
                return;
            };
            hits.push((rank, SearchHit { kind, project: project.into(), id: id.into(), title: title.into(), snippet: snippet(body, &q), status }));
        };
        for d in projects {
            let key = &d.project.key;
            for t in d.tickets.values() {
                consider("ticket", key, &t.id, &t.title, &t.description, Some(t.status.clone()));
            }
            for p in d.proposals.values() {
                consider("proposal", key, &p.id, &p.title, &p.body, Some(format!("{:?}", p.status).to_lowercase()));
            }
            for doc in d.docs.values() {
                consider("doc", key, &doc.path, &doc.title, &doc.body, None);
            }
        }
        hits.sort_by_key(|(r, _)| *r);
        Ok(hits.into_iter().take(limit).map(|(_, h)| h).collect())
    }
}

/// Links are rendered as clickable hrefs in the UI, so only web and mail links are allowed
/// (a `javascript:` URL would run code on the API's own origin).
fn check_url(url: &str) -> Result<()> {
    let lower = url.trim().to_ascii_lowercase();
    if ["http://", "https://", "mailto:"].iter().any(|p| lower.starts_with(p)) {
        Ok(())
    } else {
        invalid(format!("'{url}' is not an http(s) or mailto link"))
    }
}

/// "a, b, c", or a hint when the list is empty.
fn valid_names<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let list: Vec<&str> = names.collect();
    if list.is_empty() { "none defined yet (add them in the project's settings)".into() } else { list.join(", ") }
}

fn check_unique<'a>(names: impl Iterator<Item = &'a String>, what: &str) -> Result<()> {
    let mut seen: Vec<String> = Vec::new();
    for n in names {
        let n = n.trim();
        if n.is_empty() {
            return invalid(format!("{what} names cannot be empty"));
        }
        if seen.iter().any(|s| s.eq_ignore_ascii_case(n)) {
            return invalid(format!("duplicate {what} '{n}'"));
        }
        seen.push(n.into());
    }
    Ok(())
}

fn snippet(body: &str, q: &str) -> String {
    let lower = body.to_lowercase();
    let Some(pos) = lower.find(q) else {
        return body.chars().take(140).collect::<String>().replace('\n', " ");
    };
    // Work in chars so we never split a UTF-8 sequence.
    let char_pos = lower[..pos].chars().count();
    let start = char_pos.saturating_sub(50);
    let text: String = body.chars().skip(start).take(160).collect();
    let prefix = if start > 0 { "…" } else { "" };
    format!("{prefix}{}", text.replace('\n', " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(dir.path()).unwrap();
        (dir, s)
    }

    fn ticket(title: &str) -> TicketInput {
        TicketInput { title: Some(title.into()), ..Default::default() }
    }

    #[test]
    fn ids() {
        assert_eq!(parse_ticket_id("RES-12"), Some(("RES".into(), 12)));
        assert_eq!(parse_ticket_id("res-3"), Some(("RES".into(), 3)));
        assert_eq!(parse_proposal_id("RES-P3"), Some(("RES".into(), 3)));
        assert_eq!(parse_ticket_id("RES-P3"), None);
        assert_eq!(parse_ticket_id("R-1"), None);
        assert_eq!(slugify("Getting Started!"), "getting-started");
    }

    #[test]
    fn roundtrip_through_disk() {
        let (dir, mut s) = store();
        s.create_project(NewProject { key: "res".into(), name: "Resure".into(), ..Default::default() }).unwrap();
        let t = s
            .create_ticket(
                "RES",
                TicketInput {
                    description: Some("Body with ---\nlines".into()),
                    labels: Some(vec!["auth".into(), "Auth".into()]),
                    priority: Some(Priority::High),
                    ..ticket("Login breaks")
                },
            )
            .unwrap();
        assert_eq!(t.id, "RES-1");
        assert_eq!(t.status, "Todo");
        assert_eq!(t.labels, vec!["auth"]);
        s.add_comment("RES-1", "claude", "Looking into it").unwrap();
        s.put_doc("RES", "guides/setup", DocInput { title: Some("Setup".into()), body: Some("# Hi".into()), ..Default::default() }).unwrap();
        s.create_proposal("RES", ProposalInput { title: Some("Rewrite auth".into()), ..Default::default() }).unwrap();

        let s2 = Store::open(dir.path()).unwrap();
        let t2 = s2.get_ticket("RES-1").unwrap();
        assert_eq!(t2.description, "Body with ---\nlines");
        assert_eq!(t2.comments.len(), 1);
        assert_eq!(t2.priority, Priority::High);
        assert_eq!(s2.get_doc("RES", "guides/setup").unwrap().title, "Setup");
        assert!(s2.get_proposal("RES-P1").unwrap().body.contains("## Motivation"));
        assert_eq!(s2.get_project("RES").unwrap().project.labels.len(), 1);
    }

    #[test]
    fn move_and_close() {
        let (_d, mut s) = store();
        s.create_project(NewProject { key: "AB".into(), name: "AB".into(), ..Default::default() }).unwrap();
        for i in 0..3 {
            s.create_ticket("AB", ticket(&format!("t{i}"))).unwrap();
        }
        let t = s.move_ticket("AB-3", MoveInput { status: "todo".into(), index: Some(0) }).unwrap();
        let col = s.list_tickets("AB", &TicketFilter { status: Some("Todo".into()), ..Default::default() }).unwrap();
        assert_eq!(col[0].id, "AB-3");
        assert!(t.closed.is_none());
        let t = s.move_ticket("AB-1", MoveInput { status: "Done".into(), index: None }).unwrap();
        assert!(t.closed.is_some());
        let open = s.list_tickets("AB", &TicketFilter { include_closed: Some(false), ..Default::default() }).unwrap();
        assert_eq!(open.len(), 2);
    }

    #[test]
    fn renames_and_removals_update_tickets() {
        let (_d, mut s) = store();
        s.create_project(NewProject { key: "AB".into(), name: "AB".into(), ..Default::default() }).unwrap();
        s.create_ticket("AB", TicketInput { status: Some("In Review".into()), ..ticket("x") }).unwrap();
        let mut statuses = Project::default_statuses();
        statuses.retain(|s| s.name != "In Review");
        statuses[0].name = "Icebox".into();
        s.update_project("AB", ProjectPatch { statuses: Some(statuses), ..Default::default() }).unwrap();
        // "In Review" was removed → falls back to the first "started" status.
        assert_eq!(s.get_ticket("AB-1").unwrap().status, "In Progress");
    }

    #[test]
    fn relations_and_delete() {
        let (_d, mut s) = store();
        s.create_project(NewProject { key: "AB".into(), name: "AB".into(), ..Default::default() }).unwrap();
        s.create_ticket("AB", ticket("a")).unwrap();
        s.create_ticket("AB", TicketInput { blocks: Some(vec!["ab-1".into()]), ..ticket("b") }).unwrap();
        assert_eq!(s.ticket_backlinks("AB-1").0, vec!["AB-2"]);
        assert!(s.update_ticket("AB-1", TicketInput { blocks: Some(vec!["AB-1".into()]), ..Default::default() }).is_err());
        s.delete_ticket("AB-1").unwrap();
        assert!(s.get_ticket("AB-2").unwrap().blocks.is_empty());
    }

    #[test]
    fn doc_paths_are_safe() {
        assert!(clean_doc_path("../etc/passwd").is_err());
        assert!(clean_doc_path(".git/config").is_err());
        assert_eq!(clean_doc_path("/guides/setup.md").unwrap(), "guides/setup");
    }

    fn project(s: &mut Store) {
        s.create_project(NewProject { key: "AB".into(), name: "AB".into(), ..Default::default() }).unwrap();
    }

    #[test]
    fn repeated_midpoint_moves_renumber_the_column() {
        let (dir, mut s) = store();
        project(&mut s);
        for i in 0..3 {
            s.create_ticket("AB", ticket(&format!("t{i}"))).unwrap();
        }
        // Moving the bottom card between the top two over and over halves the gap until it renumbers.
        for _ in 0..60 {
            let bottom = s.list_tickets("AB", &TicketFilter::default()).unwrap().pop().unwrap().id;
            s.move_ticket(&bottom, MoveInput { status: "Todo".into(), index: Some(1) }).unwrap();
        }
        let col = s.list_tickets("AB", &TicketFilter::default()).unwrap();
        // Without renumbering, 60 halvings would fall below f64 precision and orders would collide.
        assert!(col.windows(2).all(|w| w[1].order > w[0].order));
        let reopened = Store::open(dir.path()).unwrap().list_tickets("AB", &TicketFilter::default()).unwrap();
        assert_eq!(reopened.iter().map(|t| &t.id).collect::<Vec<_>>(), col.iter().map(|t| &t.id).collect::<Vec<_>>());
    }

    #[test]
    fn rejected_input_changes_nothing() {
        let (_d, mut s) = store();
        project(&mut s);
        s.create_ticket("AB", ticket("a")).unwrap();
        let bad = TicketInput { labels: Some(vec!["new".into()]), milestone: Some(Some("nope".into())), ..Default::default() };
        assert!(s.update_ticket("AB-1", bad).is_err());
        assert!(s.get_project("AB").unwrap().project.labels.is_empty());
        assert!(serde_json::from_str::<TicketInput>(r#"{"assigne": "claude"}"#).is_err());
    }

    #[test]
    fn failed_write_leaves_memory_untouched() {
        let (dir, mut s) = store();
        project(&mut s);
        // A folder where the ticket file should go makes the write fail.
        std::fs::create_dir_all(dir.path().join("projects/AB/tickets/AB-1.md")).unwrap();
        assert!(s.create_ticket("AB", ticket("a")).is_err());
        assert!(s.get_ticket("AB-1").is_err());
        assert_eq!(s.create_ticket("AB", ticket("b")).unwrap().id, "AB-2");
    }

    #[test]
    fn only_web_links() {
        let (_d, mut s) = store();
        project(&mut s);
        let link = |url: &str| TicketInput { links: Some(vec![Link { url: url.into(), title: None }]), ..ticket("a") };
        assert!(s.create_ticket("AB", link("javascript:alert(1)")).is_err());
        assert!(s.create_ticket("AB", link(" JavaScript:alert(1)")).is_err());
        assert!(s.create_ticket("AB", link("https://github.com/o/r/pull/4")).is_ok());
        let fields = vec![FieldDef { name: "Spec".into(), kind: FieldKind::Url, options: vec![] }];
        s.update_project("AB", ProjectPatch { fields: Some(fields), ..Default::default() }).unwrap();
        let input = TicketInput { fields: Some([("Spec".to_string(), Value::from("javascript:x"))].into()), ..Default::default() };
        assert!(s.update_ticket("AB-1", input).is_err());
    }

    #[test]
    fn field_renames_keep_values_and_stale_patches_conflict() {
        let (_d, mut s) = store();
        project(&mut s);
        let def = |name: &str| vec![FieldDef { name: name.into(), kind: FieldKind::Text, options: vec![] }];
        s.update_project("AB", ProjectPatch { fields: Some(def("Severity")), ..Default::default() }).unwrap();
        s.create_ticket("AB", TicketInput { fields: Some([("Severity".to_string(), Value::from("S2"))].into()), ..ticket("a") }).unwrap();
        let loaded = s.get_project("AB").unwrap().project.updated;
        // A label added by a ticket in the same second still counts as a change.
        s.create_ticket("AB", TicketInput { labels: Some(vec!["fresh".into()]), ..ticket("b") }).unwrap();
        assert!(matches!(
            s.update_project("AB", ProjectPatch { name: Some("x".into()), expected_updated: Some(loaded), ..Default::default() }),
            Err(Error::Conflict(_))
        ));
        let loaded = s.get_project("AB").unwrap().project.updated;
        let renames = Renames { fields: [("Severity".to_string(), "Sev".to_string())].into(), ..Default::default() };
        s.update_project("AB", ProjectPatch { fields: Some(def("Sev")), renames, expected_updated: Some(loaded), ..Default::default() }).unwrap();
        assert_eq!(s.get_ticket("AB-1").unwrap().fields["Sev"], "S2");
        // A second save from the same (now stale) form is refused.
        let stale = ProjectPatch { name: Some("x".into()), expected_updated: Some(loaded - chrono::Duration::seconds(1)), ..Default::default() };
        assert!(matches!(s.update_project("AB", stale), Err(Error::Conflict(_))));
    }

    #[test]
    fn proposal_links_stay_in_sync() {
        let (_d, mut s) = store();
        project(&mut s);
        for t in ["p1", "p2"] {
            s.create_proposal("AB", ProposalInput { title: Some(t.into()), ..Default::default() }).unwrap();
        }
        s.create_ticket("AB", TicketInput { proposal: Some(Some("AB-P1".into())), ..ticket("a") }).unwrap();
        s.update_ticket("AB-1", TicketInput { proposal: Some(Some("AB-P2".into())), ..Default::default() }).unwrap();
        assert!(s.get_proposal("AB-P1").unwrap().tickets.is_empty());
        assert_eq!(s.get_proposal("AB-P2").unwrap().tickets, vec!["AB-1"]);
        // Removing it from the proposal's side clears the ticket's field; adding it sets it.
        s.update_proposal("AB-P2", ProposalInput { tickets: Some(vec![]), ..Default::default() }).unwrap();
        assert!(s.get_ticket("AB-1").unwrap().proposal.is_none());
        s.update_proposal("AB-P1", ProposalInput { tickets: Some(vec!["AB-1".into()]), ..Default::default() }).unwrap();
        assert_eq!(s.get_ticket("AB-1").unwrap().proposal.as_deref(), Some("AB-P1"));

        // supersedes: no self-references, and the back-link follows changes and deletes.
        assert!(s.update_proposal("AB-P2", ProposalInput { supersedes: Some(Some("AB-P2".into())), ..Default::default() }).is_err());
        s.create_proposal("AB", ProposalInput { title: Some("p3".into()), supersedes: Some(Some("AB-P1".into())), ..Default::default() }).unwrap();
        assert_eq!(s.get_proposal("AB-P1").unwrap().superseded_by.as_deref(), Some("AB-P3"));
        s.update_proposal("AB-P3", ProposalInput { supersedes: Some(Some("AB-P2".into())), ..Default::default() }).unwrap();
        assert!(s.get_proposal("AB-P1").unwrap().superseded_by.is_none());
        s.delete_proposal("AB-P3").unwrap();
        assert!(s.get_proposal("AB-P2").unwrap().superseded_by.is_none());
    }

    #[test]
    fn doc_paths_ignore_case_and_renames_keep_content() {
        let (dir, mut s) = store();
        project(&mut s);
        let body = |b: &str| DocInput { body: Some(b.into()), ..Default::default() };
        s.put_doc("AB", "guides/setup", body("one")).unwrap();
        assert!(matches!(s.put_doc("AB", "Guides/Setup", body("two")), Err(Error::Conflict(_))));
        s.put_doc("AB", "guides/setup", DocInput { new_path: Some("guides/Setup".into()), ..Default::default() }).unwrap();
        s.put_doc("AB", "guides/Setup", DocInput { new_path: Some("intro".into()), ..Default::default() }).unwrap();
        let s2 = Store::open(dir.path()).unwrap();
        assert_eq!(s2.list_docs("AB").unwrap().len(), 1);
        assert_eq!(s2.get_doc("AB", "intro").unwrap().body, "one");
    }
}
