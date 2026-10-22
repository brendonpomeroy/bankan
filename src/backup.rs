//! Periodic git backup of the data folder. Shells out to `git` so your
//! existing credentials (SSH agent, credential helper) just work.

use crate::config::BackupConfig;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::Path;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Default, Serialize)]
pub struct BackupStatus {
    pub last_run: Option<DateTime<Utc>>,
    pub last_commit: Option<DateTime<Utc>>,
    pub last_push: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Longest a single git command may run (a push over a bad network, say) before it's killed.
const GIT_TIMEOUT: Duration = Duration::from_secs(120);

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // Never wait on an SSH password or host-key prompt nobody can answer.
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let mut child = cmd.spawn().map_err(|e| format!("could not run git: {e}"))?;
    let drain = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut r) = r {
                let _ = r.read_to_string(&mut s);
            }
            s
        })
    };
    let stdout = drain(child.stdout.take().map(|r| Box::new(r) as _));
    let stderr = drain(child.stderr.take().map(|r| Box::new(r) as _));
    let deadline = Instant::now() + GIT_TIMEOUT;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("git {}: timed out after {}s", args.join(" "), GIT_TIMEOUT.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let (out, err) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    if status.success() {
        Ok(out.trim().to_string())
    } else {
        Err(format!("git {}: {}", args.join(" "), err.trim()))
    }
}

/// Makes sure the data folder is a git repo with an ignore file and the configured remote.
pub fn ensure_repo(dir: &Path, cfg: &BackupConfig) -> Result<(), String> {
    if !dir.join(".git").exists() {
        git(dir, &["init", "-q", "-b", &cfg.branch])?;
    }
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(&ignore, ".tmp-*\n.DS_Store\n").map_err(|e| e.to_string())?;
    }
    // Commits need an identity; set a repo-local fallback if the user has none.
    if git(dir, &["config", "user.email"]).is_err() {
        git(dir, &["config", "user.email", "bankan@localhost"])?;
        git(dir, &["config", "user.name", "BanKan"])?;
    }
    let current = git(dir, &["remote", "get-url", "origin"]).ok();
    match (&cfg.remote, current) {
        (Some(want), Some(have)) if *want != have => {
            git(dir, &["remote", "set-url", "origin", want])?;
        }
        (Some(want), None) => {
            git(dir, &["remote", "add", "origin", want])?;
        }
        (None, Some(_)) => {
            git(dir, &["remote", "remove", "origin"])?;
        }
        _ => {}
    }
    Ok(())
}

/// Commits any changes and pushes if a remote is configured. Blocking.
pub fn run(dir: &Path, cfg: &BackupConfig, status: &mut BackupStatus) {
    let now = Utc::now();
    status.last_run = Some(now);
    let result = (|| -> Result<(), String> {
        ensure_repo(dir, cfg)?;
        let changes = git(dir, &["status", "--porcelain"])?;
        if !changes.is_empty() {
            git(dir, &["add", "-A"])?;
            let msg = format!("bankan backup {}", now.format("%Y-%m-%d %H:%M UTC"));
            git(dir, &["commit", "-q", "--no-verify", "-m", &msg])?;
            status.last_commit = Some(now);
        }
        if cfg.remote.is_some() {
            // Push when there's anything the remote doesn't have yet (including commits from earlier failed pushes).
            let ahead = git(dir, &["rev-list", "--count", &format!("origin/{}..HEAD", cfg.branch)]).unwrap_or_else(|_| "1".into());
            if ahead != "0" {
                git(dir, &["push", "-q", "-u", "origin", &format!("HEAD:{}", cfg.branch)])?;
                status.last_push = Some(now);
            }
        }
        Ok(())
    })();
    status.last_error = result.err();
    if let Some(e) = &status.last_error {
        eprintln!("bankan: backup failed: {e}");
    }
}
