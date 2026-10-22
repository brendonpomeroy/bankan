//! The `.bankan` file a code repo uses to point at its BanKan project.
//! The server never stores repo paths; the link lives with the repo.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = ".bankan";

#[derive(Debug, Clone, Deserialize)]
pub struct Link {
    pub project: String,
    /// Optional daemon URL override (defaults to the local config's port).
    pub url: Option<String>,
    /// For a project spread over several repos: which part this repo is (e.g. `apps`).
    /// New tickets made from here get it, and `context` puts its tickets first.
    pub component: Option<String>,
    #[serde(skip)]
    pub file: PathBuf,
}

/// Walks up from `start` looking for a `.bankan` file.
pub fn find(start: &Path) -> Result<Option<Link>> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        let file = d.join(FILE_NAME);
        if file.is_file() {
            let text = std::fs::read_to_string(&file)?;
            let mut link: Link = toml::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;
            link.project = link.project.trim().to_ascii_uppercase();
            link.component = link.component.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
            link.file = file;
            return Ok(Some(link));
        }
        dir = d.parent();
    }
    Ok(None)
}

/// True for `http://127.0.0.1[:port]`, `http://localhost[:port]` and `http://[::1][:port]`.
/// A `.bankan` file comes from a repo, which may not be trusted, so its `url`
/// override must not be able to send the agent's tool calls off this machine.
pub fn is_local_url(url: &str) -> bool {
    let Some(rest) = url.trim().strip_prefix("http://") else { return false };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.split(':').next().unwrap_or(""),
    };
    matches!(host.to_ascii_lowercase().as_str(), "127.0.0.1" | "localhost" | "::1")
}

pub fn write(dir: &Path, project: &str, component: Option<&str>) -> Result<PathBuf> {
    let file = dir.join(FILE_NAME);
    let mut text = format!("# Links this repo to a BanKan project (tickets, docs and proposals live outside the repo).\nproject = \"{}\"\n", project.to_ascii_uppercase());
    if let Some(c) = component {
        text.push_str(&format!("# This repo's part of the project: new tickets made from here are tagged with it.\ncomponent = {}\n", toml::Value::String(c.into())));
    }
    std::fs::write(&file, text)?;
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::is_local_url;

    #[test]
    fn only_local_urls() {
        assert!(is_local_url("http://127.0.0.1:7777"));
        assert!(is_local_url("http://localhost:8000/"));
        assert!(is_local_url("http://[::1]:7777"));
        assert!(!is_local_url("https://evil.example"));
        assert!(!is_local_url("http://localhost.evil.example"));
        assert!(!is_local_url("http://localhost@evil.example"));
        assert!(!is_local_url("http://127.0.0.1.nip.io"));
    }
}
