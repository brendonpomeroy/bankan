//! Autostart: a launchd agent on macOS, a systemd user unit on Linux.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const LABEL: &str = "dev.bankan.daemon";

fn exe() -> Result<PathBuf> {
    Ok(std::env::current_exe()?.canonicalize()?)
}

/// True when running from a cargo build folder (the service would break on rebuild).
pub fn running_from_build_dir() -> bool {
    exe().is_ok_and(|e| e.components().any(|c| c.as_os_str() == "target"))
}

fn run(cmd: &mut Command) -> Result<()> {
    let out = cmd.output().with_context(|| format!("running {cmd:?}"))?;
    if !out.status.success() {
        bail!("{cmd:?} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn plist_path() -> PathBuf {
    dirs::home_dir().unwrap().join("Library/LaunchAgents").join(format!("{LABEL}.plist"))
}

#[cfg(target_os = "macos")]
fn domain() -> String {
    // launchd's per-user domain is gui/<uid>.
    let uid = Command::new("id").arg("-u").output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    format!("gui/{uid}")
}

#[cfg(target_os = "macos")]
pub fn install(home: &Path) -> Result<String> {
    let exe = exe()?;
    std::fs::create_dir_all(home)?;
    let log = home.join("bankan.log");
    let esc = |p: &Path| p.display().to_string().replace('&', "&amp;").replace('<', "&lt;");
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array><string>{exe}</string><string>serve</string></array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>BANKAN_HOME</key><string>{home}</string>
    <key>PATH</key><string>/usr/local/bin:/opt/homebrew/bin:/usr/bin:/bin</string>
  </dict>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        exe = esc(&exe),
        home = esc(home),
        log = esc(&log),
    );
    let path = plist_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    // Reinstalling: unload the old agent first (ignore errors if it wasn't loaded).
    let _ = Command::new("launchctl").args(["bootout", &format!("{}/{LABEL}", domain())]).output();
    std::fs::write(&path, plist)?;
    run(Command::new("launchctl").args(["bootstrap", &domain()]).arg(&path))?;
    Ok(format!("Installed launchd agent {LABEL}\n  plist: {}\n  logs:  {}", path.display(), log.display()))
}

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<()> {
    let _ = Command::new("launchctl").args(["bootout", &format!("{}/{LABEL}", domain())]).output();
    let path = plist_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    println!("Removed launchd agent {LABEL}. Your data is untouched.");
    Ok(())
}

#[cfg(target_os = "linux")]
fn unit_path() -> PathBuf {
    dirs::config_dir().unwrap().join("systemd/user/bankan.service")
}

#[cfg(target_os = "linux")]
pub fn install(home: &Path) -> Result<String> {
    let exe = exe()?;
    let unit = format!(
        "[Unit]\nDescription=BanKan tracker\n\n[Service]\nExecStart={} serve\nEnvironment=BANKAN_HOME={}\nRestart=on-failure\n\n[Install]\nWantedBy=default.target\n",
        exe.display(),
        home.display()
    );
    let path = unit_path();
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, unit)?;
    run(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
    run(Command::new("systemctl").args(["--user", "enable", "--now", "bankan.service"]))?;
    Ok(format!("Installed systemd user unit {}", path.display()))
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> Result<()> {
    let _ = Command::new("systemctl").args(["--user", "disable", "--now", "bankan.service"]).output();
    let path = unit_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    println!("Removed systemd user unit. Your data is untouched.");
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn install(_home: &Path) -> Result<String> {
    bail!("autostart is only supported on macOS and Linux; run `bankan serve` yourself")
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn uninstall() -> Result<()> {
    bail!("autostart is only supported on macOS and Linux")
}
