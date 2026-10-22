use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const DEFAULT_PORT: u16 = 7777;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub port: u16,
    pub backup: BackupConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BackupConfig {
    /// Commit the data folder to its local git repo on an interval.
    pub enabled: bool,
    pub interval_minutes: u64,
    /// Optional remote to push to after each commit (e.g. git@github.com:me/bankan-data.git).
    pub remote: Option<String>,
    pub branch: String,
}

impl Default for Config {
    fn default() -> Self {
        Self { port: DEFAULT_PORT, backup: BackupConfig::default() }
    }
}

impl Default for BackupConfig {
    fn default() -> Self {
        Self { enabled: true, interval_minutes: 15, remote: None, branch: "main".into() }
    }
}

/// `~/.bankan`, or `$BANKAN_HOME` when set.
pub fn home_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("BANKAN_HOME") {
        return PathBuf::from(dir);
    }
    dirs::home_dir().expect("no home directory").join(".bankan")
}

pub fn data_dir(home: &Path) -> PathBuf {
    home.join("data")
}

fn config_path(home: &Path) -> PathBuf {
    home.join("config.toml")
}

impl Config {
    pub fn load(home: &Path) -> Result<Self> {
        let path = config_path(home);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if cfg.backup.interval_minutes == 0 {
            cfg.backup.interval_minutes = 1;
        }
        if cfg.backup.remote.as_deref().is_some_and(|r| r.trim().is_empty()) {
            cfg.backup.remote = None;
        }
        Ok(cfg)
    }

    pub fn save(&self, home: &Path) -> Result<()> {
        std::fs::create_dir_all(home)?;
        let text = toml::to_string_pretty(self)?;
        std::fs::write(config_path(home), text)?;
        Ok(())
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}
