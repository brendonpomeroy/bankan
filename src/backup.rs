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

/// A leftover `index.lock` older than this is from a git that was killed, not one still running:
/// our own git commands are stopped after `GIT_TIMEOUT`.
const STALE_LOCK: Duration = Duration::from_secs(10 * 60);

/// Removes `index.lock` when a killed git left it behind. Without this, one interrupted backup
/// (a crash, a restart) makes every later backup fail until someone deletes the file by hand.
fn clear_stale_lock(dir: &Path) {
    let lock = dir.join(".git").join("index.lock");
    let age = std::fs::metadata(&lock).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if age.is_some_and(|age| age >= STALE_LOCK) && std::fs::remove_file(&lock).is_ok() {
        eprintln!("bankan: removed a stale git lock from the data folder");
    }
}

/// Brings in commits the remote has and this folder doesn't (another machine backing up to the
/// same remote), so the push that follows is accepted. Returns how many commits were merged.
fn merge_remote(dir: &Path, cfg: &BackupConfig) -> Result<usize, String> {
    git(dir, &["fetch", "-q", "origin"])?;
    let upstream = format!("origin/{}", cfg.branch);
    // A new, empty remote has no branch yet: nothing to merge.
    if git(dir, &["rev-parse", "--verify", "-q", &format!("refs/remotes/{upstream}")]).is_err() {
        return Ok(0);
    }
    let behind: usize = git(dir, &["rev-list", "--count", &format!("HEAD..{upstream}")])?.parse().unwrap_or(0);
    if behind == 0 {
        return Ok(0);
    }
    // Unrelated histories are expected: a new machine's folder starts its own history before it
    // first meets the remote.
    if let Err(e) = git(dir, &["merge", "-q", "--no-edit", "--no-verify", "--allow-unrelated-histories", &upstream]) {
        // Leave the folder as it was; the files the server is using must not hold conflict markers.
        let _ = git(dir, &["merge", "--abort"]);
        return Err(format!("the remote has {behind} commit(s) that could not be merged automatically, so nothing was pushed. Merge {upstream} by hand in the data folder. ({e})"));
    }
    Ok(behind)
}

/// Commits any changes, merges what the remote has, and pushes, if a remote is configured. Blocking.
pub fn run(dir: &Path, cfg: &BackupConfig, status: &mut BackupStatus) {
    let now = Utc::now();
    status.last_run = Some(now);
    let result = (|| -> Result<(), String> {
        ensure_repo(dir, cfg)?;
        clear_stale_lock(dir);
        let changes = git(dir, &["status", "--porcelain"])?;
        if !changes.is_empty() {
            git(dir, &["add", "-A"])?;
            let msg = format!("bankan backup {}", now.format("%Y-%m-%d %H:%M UTC"));
            git(dir, &["commit", "-q", "--no-verify", "-m", &msg])?;
            status.last_commit = Some(now);
        }
        if cfg.remote.is_some() {
            let merged = merge_remote(dir, cfg)?;
            if merged > 0 {
                eprintln!("bankan: merged {merged} commit(s) from the backup remote. Restart the server to load any projects they added.");
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) -> String {
        git(dir, args).unwrap_or_else(|e| panic!("{e}"))
    }

    /// A bare remote and two data folders backing up to it, as two machines would.
    fn two_machines() -> (tempfile::TempDir, BackupConfig) {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        sh(&remote, &["init", "-q", "--bare", "-b", "main"]);
        let cfg = BackupConfig { enabled: true, interval_minutes: 15, remote: Some(remote.to_string_lossy().into_owned()), branch: "main".into() };
        for name in ["a", "b"] {
            std::fs::create_dir_all(tmp.path().join(name)).unwrap();
        }
        (tmp, cfg)
    }

    fn backup(dir: &Path, cfg: &BackupConfig) -> BackupStatus {
        let mut status = BackupStatus::default();
        run(dir, cfg, &mut status);
        status
    }

    #[test]
    fn merges_what_another_machine_pushed_then_pushes() {
        let (tmp, cfg) = two_machines();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::write(a.join("one.md"), "from a\n").unwrap();
        assert_eq!(backup(&a, &cfg).last_error, None);
        // b starts from the remote, then both change different files.
        assert_eq!(backup(&b, &cfg).last_error, None);
        assert!(b.join("one.md").exists());
        std::fs::write(a.join("two.md"), "from a\n").unwrap();
        assert_eq!(backup(&a, &cfg).last_error, None);
        std::fs::write(b.join("three.md"), "from b\n").unwrap();
        let status = backup(&b, &cfg);
        assert_eq!(status.last_error, None);
        assert!(status.last_push.is_some());
        assert!(b.join("two.md").exists());
        assert_eq!(sh(&b, &["rev-list", "--count", "HEAD..origin/main"]), "0");
        assert_eq!(sh(&b, &["rev-list", "--count", "origin/main..HEAD"]), "0");
    }

    #[test]
    fn a_conflict_leaves_the_folder_untouched_and_reports() {
        let (tmp, cfg) = two_machines();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::write(a.join("one.md"), "start\n").unwrap();
        backup(&a, &cfg);
        backup(&b, &cfg);
        std::fs::write(a.join("one.md"), "from a\n").unwrap();
        backup(&a, &cfg);
        std::fs::write(b.join("one.md"), "from b\n").unwrap();
        let status = backup(&b, &cfg);
        assert!(status.last_error.unwrap().contains("could not be merged"));
        assert_eq!(std::fs::read_to_string(b.join("one.md")).unwrap(), "from b\n");
        assert_eq!(sh(&b, &["status", "--porcelain"]), "");
    }

    #[test]
    fn removes_only_a_stale_lock() {
        let (tmp, cfg) = two_machines();
        let a = tmp.path().join("a");
        ensure_repo(&a, &cfg).unwrap();
        let lock = a.join(".git").join("index.lock");
        std::fs::write(&lock, "").unwrap();
        clear_stale_lock(&a);
        assert!(lock.exists(), "a fresh lock may belong to a running git");
        let old = std::time::SystemTime::now() - STALE_LOCK - Duration::from_secs(1);
        std::fs::File::options().write(true).open(&lock).unwrap().set_modified(old).unwrap();
        clear_stale_lock(&a);
        assert!(!lock.exists());
    }
}
