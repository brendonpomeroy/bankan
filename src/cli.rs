//! Terminal UX: the interactive `bankan setup` wizard and styled output for
//! the other commands.

use crate::config::{self, Config};
use crate::{link, service, store};
use anyhow::{bail, Result};
use cliclack::{confirm, input, intro, log, note, outro, select, spinner};
use console::style;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

// --- helpers ------------------------------------------------------------------

fn api(cfg: &Config, method: &str, path: &str, body: Option<Value>) -> Result<Value> {
    let req = ureq::request(method, &format!("{}/api{path}", cfg.base_url())).timeout(Duration::from_secs(5));
    let res = match body {
        Some(b) => req.send_json(b),
        None => req.call(),
    };
    match res {
        Ok(r) => Ok(r.into_json()?),
        Err(ureq::Error::Status(_, r)) => {
            let v: Value = r.into_json().unwrap_or(Value::Null);
            bail!("{}", v["error"].as_str().unwrap_or("request failed"))
        }
        Err(e) => Err(e.into()),
    }
}

pub fn daemon_up(cfg: &Config) -> Option<Value> {
    api(cfg, "GET", "/status", None).ok()
}

fn tilde(p: &Path) -> String {
    match dirs::home_dir() {
        Some(h) if p.starts_with(&h) => format!("~/{}", p.strip_prefix(&h).unwrap().display()),
        _ => p.display().to_string(),
    }
}

pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let out = Command::new("git").args(["rev-parse", "--show-toplevel"]).current_dir(dir).output().ok()?;
    out.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&out.stdout).trim()))
}

fn which(bin: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {bin}")]).output().is_ok_and(|o| o.status.success())
}

/// "resure-platform" → "Resure Platform"
fn title_case(s: &str) -> String {
    s.split(['-', '_', ' '])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// "Resure Platform" → "RP", "Resure" → "RES"
fn suggest_key(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let raw: String = if words.len() > 1 { words.iter().filter_map(|w| w.chars().next()).collect() } else { name.chars().take(3).collect() };
    let key: String = raw.to_ascii_uppercase().chars().filter(|c| c.is_ascii_alphanumeric()).skip_while(|c| c.is_ascii_digit()).take(10).collect();
    if store::valid_key(&key) { key } else { "PRJ".into() }
}

fn wait_for_daemon(cfg: &Config) -> bool {
    for _ in 0..40 {
        if daemon_up(cfg).is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    false
}

fn global_ignore_file() -> PathBuf {
    let configured = Command::new("git").args(["config", "--global", "core.excludesFile"]).output().ok().filter(|o| o.status.success());
    match configured {
        Some(o) => {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            PathBuf::from(p.strip_prefix("~/").map(|rest| dirs::home_dir().unwrap().join(rest).display().to_string()).unwrap_or(p))
        }
        None => dirs::config_dir().unwrap_or_else(|| dirs::home_dir().unwrap().join(".config")).join("git/ignore"),
    }
}

fn ignore_bankan_globally() -> Result<PathBuf> {
    let file = global_ignore_file();
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    if !existing.lines().any(|l| l.trim() == ".bankan") {
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let sep = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&file, format!("{existing}{sep}.bankan\n"))?;
    }
    Ok(file)
}

// --- setup wizard -------------------------------------------------------------

pub fn setup(home: &Path) -> Result<()> {
    intro(style(" BanKan setup ").black().on_cyan().bold())?;
    log::remark(format!(
        "Kanban tickets, docs and proposals for all your projects,\n{}",
        style("kept out of your repos and available to AI agents over MCP.").dim()
    ))?;

    let exe = std::env::current_exe()?.canonicalize()?;
    if exe.components().any(|c| c.as_os_str() == "target") {
        log::warning(format!(
            "You're running BanKan from a build folder:\n{}\nInstall it first so the background service survives rebuilds:\n{}",
            style(tilde(&exe)).dim(),
            style("make install   (or: cargo install --path .)").cyan()
        ))?;
        if !confirm("Continue anyway?").initial_value(false).interact()? {
            outro("Run `make install`, then `bankan setup` again.")?;
            return Ok(());
        }
    }

    // 1. Server settings
    let mut cfg = Config::load(home)?;
    let old = cfg.clone();
    let port: u16 = input("Which port should the web UI use?")
        .default_input(&cfg.port.to_string())
        .validate(|s: &String| s.trim().parse::<u16>().map(|_| ()).map_err(|_| "Enter a port number, e.g. 7777"))
        .interact()?;
    cfg.port = port;

    // 2. Backups
    let current = if !cfg.backup.enabled { "off" } else if cfg.backup.remote.is_some() { "remote" } else { "local" };
    let mode = select("How should BanKan back up your data?")
        .item("local", "Local git history", "commits ~/.bankan/data on a timer")
        .item("remote", "Git history + push to a remote", "e.g. a private GitHub repo")
        .item("off", "No backups", "")
        .initial_value(current)
        .interact()?;
    cfg.backup.enabled = mode != "off";
    if mode == "remote" {
        let remote: String = input("Remote URL")
            .placeholder("git@github.com:you/bankan-data.git")
            .default_input(cfg.backup.remote.as_deref().unwrap_or(""))
            .validate(|s: &String| if s.trim().is_empty() { Err("Enter a git remote URL") } else { Ok(()) })
            .interact()?;
        cfg.backup.remote = Some(remote.trim().to_string());
    } else {
        cfg.backup.remote = None;
    }
    if cfg.backup.enabled {
        cfg.backup.interval_minutes = input("Back up every how many minutes?")
            .default_input(&cfg.backup.interval_minutes.to_string())
            .validate(|s: &String| s.trim().parse::<u64>().ok().filter(|n| *n > 0).map(|_| ()).ok_or("Enter a number of minutes"))
            .interact()?;
    }
    let restart_needed = old.port != cfg.port && daemon_up(&old).is_some();
    cfg.save(home)?;
    log::success(format!("Saved settings to {}", style(tilde(&home.join("config.toml"))).dim()))?;

    // 3. Autostart
    let mut running = daemon_up(&cfg).is_some();
    if confirm("Start BanKan automatically when you log in?").initial_value(true).interact()? {
        let sp = spinner();
        sp.start("Installing the background service…");
        match service::install(home) {
            Ok(_) => {
                running = wait_for_daemon(&cfg);
                if running {
                    sp.stop(format!("BanKan is running at {}", style(cfg.base_url()).cyan().underlined()));
                } else {
                    sp.error(format!("Installed, but BanKan didn't answer yet. Check {}", tilde(&home.join("bankan.log"))));
                }
            }
            Err(e) => sp.error(format!("Couldn't install the service: {e:#}")),
        }
    } else if restart_needed {
        log::warning("Restart BanKan for the new port to take effect.")?;
    } else if !running {
        log::info(format!("Start it yourself with {}", style("bankan serve").cyan()))?;
    }

    // 4. Claude Code
    let mcp_cmd = format!("claude mcp add bankan --scope user -- {} mcp", exe.display());
    if which("claude") {
        if confirm("Connect Claude Code to BanKan (MCP server for every repo)?").initial_value(true).interact()? {
            let _ = Command::new("claude").args(["mcp", "remove", "bankan", "--scope", "user"]).output();
            let out = Command::new("claude").args(["mcp", "add", "bankan", "--scope", "user", "--"]).arg(&exe).arg("mcp").output();
            match out {
                Ok(o) if o.status.success() => log::success("Claude Code can now use BanKan. Try /bankan:next in a linked repo.")?,
                Ok(o) => log::error(format!("`claude mcp add` failed: {}", String::from_utf8_lossy(&o.stderr).trim()))?,
                Err(e) => log::error(format!("Couldn't run claude: {e}"))?,
            }
        }
    } else {
        log::info(format!("To connect Claude Code later, run:\n{}", style(&mcp_cmd).cyan()))?;
    }

    // 5. First project / link this repo
    if running {
        let cwd = std::env::current_dir()?;
        let repo = repo_root(&cwd);
        let existing_link = link::find(&cwd)?;
        let projects = api(&cfg, "GET", "/projects", None)?.as_array().cloned().unwrap_or_default();
        let key = match (&repo, &existing_link) {
            (Some(_), Some(l)) => {
                log::info(format!("This repo is already linked to {}", style(&l.project).bold()))?;
                None
            }
            (Some(root), None) => {
                let name = root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if confirm(format!("Link this repo ({}) to a BanKan project?", style(&name).bold())).initial_value(true).interact()? {
                    Some(pick_or_create_project(&cfg, &projects, &title_case(&name))?)
                } else {
                    None
                }
            }
            (None, _) if projects.is_empty() => {
                if confirm("Create your first project?").initial_value(true).interact()? {
                    pick_or_create_project(&cfg, &projects, "")?;
                }
                None
            }
            _ => None,
        };
        if let (Some(key), Some(root)) = (key, &repo) {
            let file = link::write(root, &key, None)?;
            log::success(format!("Linked {} → {}", style(tilde(&file)).dim(), style(&key).bold()))?;
            let keep_local = select("Should the .bankan link file be committed?")
                .item(false, "Commit it", "teammates and other machines get the link too")
                .item(true, "Keep it local", "adds .bankan to your global gitignore")
                .initial_value(false)
                .interact()?;
            if keep_local {
                let f = ignore_bankan_globally()?;
                log::success(format!("Added .bankan to {}", style(tilde(&f)).dim()))?;
            }
        }
    }

    // 6. Done
    let mut next = vec![
        format!("{}  open the web UI", style("bankan open      ").cyan()),
        format!("{}  link a repo to a project", style("bankan link KEY  ").cyan()),
        format!("{}  check everything is healthy", style("bankan status    ").cyan()),
    ];
    if !running {
        next.insert(0, format!("{}  start the server", style("bankan serve     ").cyan()));
    }
    note("Next steps", next.join("\n"))?;
    if running && confirm("Open BanKan in your browser now?").initial_value(true).interact()? {
        open_url(&cfg.base_url());
    }
    outro(format!("You're all set. {}", style(cfg.base_url()).cyan().underlined()))?;
    Ok(())
}

fn pick_or_create_project(cfg: &Config, projects: &[Value], suggested_name: &str) -> Result<String> {
    if !projects.is_empty() {
        let mut sel = select("Which project?");
        for p in projects {
            let key = p["key"].as_str().unwrap_or("?").to_string();
            sel = sel.item(key.clone(), format!("{key}  {}", p["name"].as_str().unwrap_or("")), "");
        }
        sel = sel.item("__new__".to_string(), "Create a new project…", "");
        let choice = sel.interact()?;
        if choice != "__new__" {
            return Ok(choice);
        }
    }
    let name: String = input("Project name")
        .default_input(suggested_name)
        .validate(|s: &String| if s.trim().is_empty() { Err("Give the project a name") } else { Ok(()) })
        .interact()?;
    let key: String = input("Project key (prefix for ticket ids, e.g. RES → RES-12)")
        .default_input(&suggest_key(&name))
        .validate(|s: &String| if store::valid_key(&s.trim().to_ascii_uppercase()) { Ok(()) } else { Err("2–10 letters/digits, starting with a letter") })
        .interact()?;
    let key = key.trim().to_ascii_uppercase();
    api(cfg, "POST", "/projects", Some(json!({ "key": key, "name": name.trim() })))?;
    log::success(format!("Created project {} ({})", style(&key).bold(), name.trim()))?;
    Ok(key)
}

// --- other commands -----------------------------------------------------------

pub fn open_url(url: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    if Command::new(opener).arg(url).spawn().is_err() {
        println!("{url}");
    }
}

fn row(icon: console::StyledObject<&str>, label: &str, value: impl std::fmt::Display) {
    println!("  {icon} {:<11}{value}", style(label).dim());
}

pub fn status(home: &Path, cfg: &Config) -> Result<()> {
    println!();
    println!("  {} {}", style("BanKan").bold(), style(format!("v{}", env!("CARGO_PKG_VERSION"))).dim());
    println!();
    match daemon_up(cfg) {
        Some(s) => {
            row(style("●").green(), "server", format!("{}  {}", style(cfg.base_url()).cyan().underlined(), style(format!("{} projects", s["projects"])).dim()));
            let b = &s["backup"];
            if b["config"]["enabled"].as_bool() == Some(true) {
                let remote = b["config"]["remote"].as_str().map(|r| format!(" → {r}")).unwrap_or_default();
                let last = b["status"]["last_run"].as_str().map(|t| t.replace('T', " ").chars().take(16).collect::<String>()).unwrap_or_else(|| "not yet".into());
                match b["status"]["last_error"].as_str() {
                    Some(e) => row(style("✗").red(), "backup", format!("every {} min{remote} · {}", b["config"]["interval_minutes"], style(format!("last run failed: {e}")).red())),
                    None => row(style("✓").green(), "backup", format!("every {} min{remote} {}", b["config"]["interval_minutes"], style(format!("· last run {last}")).dim())),
                }
            } else {
                row(style("○").dim(), "backup", style("off").dim());
            }
        }
        None => row(style("●").red(), "server", format!("not running  {}", style("start it with `bankan serve` or `bankan setup`").dim())),
    }
    row(style("◆").cyan(), "data", tilde(&config::data_dir(home)));
    match link::find(&std::env::current_dir()?)? {
        Some(l) => row(style("⛓").cyan(), "this repo", format!("{}  {}", style(&l.project).bold(), style(tilde(&l.file)).dim())),
        None => row(style("○").dim(), "this repo", style("not linked (bankan link KEY)").dim()),
    }
    println!();
    Ok(())
}

pub fn link_repo(cfg: &Config, project: &str, create: Option<String>, component: Option<String>) -> Result<()> {
    let key = project.trim().to_ascii_uppercase();
    if !store::valid_key(&key) {
        bail!("'{project}' is not a valid project key (2–10 uppercase letters/digits, e.g. RES)");
    }
    let component = component.map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
    if let Some(c) = &component {
        if !store::valid_component(c) {
            bail!("'{c}' is not a valid component (up to 40 letters, digits, '-', '_' or '.')");
        }
    }
    if daemon_up(cfg).is_none() {
        eprintln!("  {} BanKan isn't running, so {key} couldn't be checked.", style("!").yellow());
    } else if api(cfg, "GET", &format!("/projects/{key}"), None).is_err() {
        match create {
            Some(name) => {
                api(cfg, "POST", "/projects", Some(json!({ "key": key, "name": name })))?;
                println!("  {} Created project {} ({name})", style("✓").green(), style(&key).bold());
            }
            None => bail!("project {key} doesn't exist. Create it in the UI, or run `bankan link {key} --create \"Project name\"`"),
        }
    }
    let cwd = std::env::current_dir()?;
    let root = repo_root(&cwd).unwrap_or(cwd);
    let file = link::write(&root, &key, component.as_deref())?;
    let to = match &component {
        Some(c) => format!("{} ({c})", style(&key).bold()),
        None => style(&key).bold().to_string(),
    };
    println!("  {} Linked {} → {to}", style("✓").green(), style(tilde(&file)).dim());
    println!("    {}", style("Commit it, or keep it local with: echo .bankan >> ~/.config/git/ignore").dim());
    Ok(())
}

/// One Markdown file to import: where it goes and what it holds.
struct ImportFile {
    source: PathBuf,
    path: String,
    title: String,
    tags: Option<Vec<String>>,
    body: String,
}

/// `Research Notes/AI native.md` → `research-notes/ai-native`, under `prefix` if given.
fn import_path(rel: &Path, prefix: Option<&str>) -> String {
    let mut parts: Vec<String> = prefix.into_iter().flat_map(|p| p.split('/')).filter(|p| !p.trim().is_empty()).map(store::slugify).collect();
    let rel = rel.with_extension("");
    parts.extend(rel.iter().map(|seg| store::slugify(&seg.to_string_lossy())));
    parts.join("/")
}

/// Title from front matter, else the first `# ` heading, else the file name.
fn read_import(source: &Path, path: String) -> Result<ImportFile> {
    let text = std::fs::read_to_string(source)?;
    let (fm, body) = store::split_front_matter(&text);
    let meta: Value = if fm.trim().is_empty() { Value::Null } else { serde_yaml::from_str(fm).unwrap_or(Value::Null) };
    let heading = body.lines().find_map(|l| l.strip_prefix("# ")).map(|h| h.trim().to_string());
    let stem = source.file_stem().map(|s| s.to_string_lossy().replace(['-', '_'], " ")).unwrap_or_default();
    let title = meta["title"].as_str().map(str::to_string).or(heading).filter(|t| !t.trim().is_empty()).unwrap_or(stem);
    let tags = meta["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect());
    Ok(ImportFile { source: source.to_path_buf(), path, title, tags, body: body.trim_start_matches(['\n', '\r']).trim_end().to_string() })
}

fn collect_md(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for p in entries {
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            collect_md(&p, out)?;
        } else if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("md")) {
            out.push(p);
        }
    }
    Ok(())
}

/// `bankan import <dir>`: copies Markdown files into a project's docs through the daemon. Each doc
/// records its source file and a hash of what was imported, so a re-run updates docs whose source
/// changed and leaves alone (unless `force`) docs that were edited in BanKan since.
pub fn import_docs(cfg: &Config, dir: &Path, project: Option<String>, prefix: Option<String>, dry_run: bool, force: bool) -> Result<()> {
    let key = match project {
        Some(p) => p.trim().to_ascii_uppercase(),
        None => match link::find(&std::env::current_dir()?)? {
            Some(l) => l.project,
            None => bail!("this repo isn't linked to a project; pass --project KEY"),
        },
    };
    if daemon_up(cfg).is_none() {
        bail!("BanKan isn't running. Start it with `bankan serve`, or `bankan setup` to install it.");
    }
    let dir = dir.canonicalize().map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
    if !dir.is_dir() {
        bail!("{} is not a folder", dir.display());
    }
    let mut sources = Vec::new();
    collect_md(&dir, &mut sources)?;
    if sources.is_empty() {
        bail!("no .md files in {}", dir.display());
    }
    let existing: Vec<Value> = api(cfg, "GET", &format!("/projects/{key}/docs"), None)?.as_array().cloned().unwrap_or_default();
    let by_path = |p: &str| existing.iter().find(|d| d["path"].as_str().is_some_and(|x| x.eq_ignore_ascii_case(p)));

    let mut files = Vec::new();
    for src in &sources {
        let rel = src.strip_prefix(&dir).unwrap_or(src);
        files.push(read_import(src, import_path(rel, prefix.as_deref()))?);
    }
    let mut seen = std::collections::HashMap::new();
    for f in &files {
        if let Some(other) = seen.insert(f.path.to_lowercase(), &f.source) {
            bail!("{} and {} would both import as {}; rename one", tilde(other), tilde(&f.source), f.path);
        }
    }

    println!("  Importing {} into {}{}", style(tilde(&dir)).dim(), style(&key).bold(), if dry_run { style(" (dry run)").yellow().to_string() } else { String::new() });
    let (mut created, mut updated, mut unchanged, mut skipped) = (0, 0, 0, 0);
    for f in &files {
        let hash = store::content_hash(&f.body);
        let url = format!("/projects/{key}/docs/{}", f.path.split('/').map(encode_segment).collect::<Vec<_>>().join("/"));
        let (verb, why) = match by_path(&f.path) {
            None => ("create", None),
            Some(d) => match d["source_hash"].as_str() {
                // Source unchanged: nothing to do, and edits made in BanKan stay (unless forced back).
                Some(h) if h == hash && !force => ("same", None),
                Some(h) => {
                    let current = api(cfg, "GET", &url, None)?;
                    let edited = store::content_hash(current["body"].as_str().unwrap_or("")) != h;
                    match (h != hash, edited) {
                        (false, false) => ("same", None),
                        (_, true) if !force => ("skip", Some("edited in BanKan since the last import; --force replaces it")),
                        _ => ("update", None),
                    }
                }
                None if force => ("update", None),
                None => ("skip", Some("a doc that wasn't imported is already at this path; --force replaces it")),
            },
        };
        let mark = match verb {
            "create" => style("+").green(),
            "update" => style("~").cyan(),
            "same" => style("=").dim(),
            _ => style("!").yellow(),
        };
        if verb != "same" {
            println!("  {mark} {}{}", f.path, why.map(|w| style(format!("  ({w})")).dim().to_string()).unwrap_or_default());
        }
        match verb {
            "create" => created += 1,
            "update" => updated += 1,
            "same" => unchanged += 1,
            _ => skipped += 1,
        }
        if dry_run || !matches!(verb, "create" | "update") {
            continue;
        }
        let mut body = json!({ "title": f.title, "body": f.body, "source": f.source.display().to_string(), "source_hash": hash });
        if let Some(tags) = &f.tags {
            body["tags"] = json!(tags);
        }
        api(cfg, "PUT", &url, Some(body))?;
    }
    println!(
        "  {} {created} created, {updated} updated, {unchanged} unchanged, {skipped} skipped{}",
        style("✓").green(),
        if dry_run { " (nothing written)" } else { "" }
    );
    Ok(())
}

fn encode_segment(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn welcome(home: &Path, cfg: &Config) -> Result<()> {
    if !home.join("config.toml").exists() {
        println!();
        println!("  {} {}", style("Welcome to BanKan").bold(), style(format!("v{}", env!("CARGO_PKG_VERSION"))).dim());
        println!("  Run {} to get started.", style("bankan setup").cyan());
        println!("  {}", style("All commands: bankan --help").dim());
        println!();
        return Ok(());
    }
    status(home, cfg)?;
    println!("  {}", style("Commands: setup · open · link · status · backup · --help").dim());
    println!();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_keys() {
        assert_eq!(title_case("resure-platform"), "Resure Platform");
        assert_eq!(suggest_key("Resure Platform"), "RP");
        assert_eq!(suggest_key("resure"), "RES");
        assert_eq!(suggest_key("3d"), "PRJ");
    }
}
