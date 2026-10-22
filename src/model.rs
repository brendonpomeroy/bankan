//! Data types shared by the store, the REST API and the MCP server.
//! See docs/DATA-FORMAT.md for how these map onto files.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Backlog,
    Unstarted,
    Started,
    Completed,
    Canceled,
}

impl Category {
    pub fn is_closed(self) -> bool {
        matches!(self, Category::Completed | Category::Canceled)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub name: String,
    pub category: Category,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Named {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Milestone {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    #[serde(default)]
    pub done: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    Text,
    Number,
    Select,
    Date,
    Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub kind: FieldKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub archived: bool,
    pub statuses: Vec<Status>,
    pub types: Vec<Named>,
    #[serde(default)]
    pub labels: Vec<Named>,
    #[serde(default)]
    pub milestones: Vec<Milestone>,
    #[serde(default)]
    pub fields: Vec<FieldDef>,
    pub next_ticket: u32,
    pub next_proposal: u32,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
}

impl Project {
    pub fn status(&self, name: &str) -> Option<&Status> {
        self.statuses.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    pub fn default_statuses() -> Vec<Status> {
        let s = |name: &str, category, color: &str| Status { name: name.into(), category, color: Some(color.into()) };
        vec![
            s("Backlog", Category::Backlog, "#8b8d98"),
            s("Todo", Category::Unstarted, "#6e8efb"),
            s("In Progress", Category::Started, "#f2a93b"),
            s("In Review", Category::Started, "#a46ef2"),
            s("Done", Category::Completed, "#3fb67d"),
            s("Canceled", Category::Canceled, "#b4b6bf"),
        ]
    }

    pub fn default_types() -> Vec<Named> {
        let t = |name: &str, color: &str| Named { name: name.into(), color: Some(color.into()) };
        vec![t("Feature", "#6e8efb"), t("Bug", "#e5534b"), t("Task", "#8b8d98"), t("Chore", "#b08d57")]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    #[default]
    None,
    Urgent,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: u32,
    pub author: String,
    pub at: DateTime<Utc>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub title: String,
    pub status: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<NaiveDate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, Value>,
    #[serde(default)]
    pub order: f64,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub comments: Vec<Comment>,
    /// Markdown body. Stored below the front matter, not inside it.
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalStatus {
    Draft,
    Review,
    Accepted,
    Rejected,
    Withdrawn,
    Superseded,
    Implemented,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub title: String,
    pub status: ProposalStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tickets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided: Option<NaiveDate>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    #[serde(default)]
    pub body: String,
}

pub const PROPOSAL_TEMPLATE: &str = "## Summary\n\nOne paragraph: what is being proposed.\n\n## Motivation\n\nWhy is this needed? What problem does it solve?\n\n## Proposal\n\nThe detailed design.\n\n## Alternatives considered\n\n## Risks & drawbacks\n\n## Open questions\n\n## Decision\n\n_Filled in when accepted or rejected._\n";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Doc {
    /// Path inside the project's docs folder, without `.md` (e.g. `guides/setup`).
    #[serde(default)]
    pub path: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    #[serde(default)]
    pub body: String,
}

/// A change notification sent to the UI over SSE.
#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub kind: &'static str,
    pub action: &'static str,
    pub project: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}
