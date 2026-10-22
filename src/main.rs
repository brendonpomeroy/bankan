mod backup;
mod cli;
mod config;
mod link;
mod mcp;
mod model;
mod server;
mod service;
mod store;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use config::Config;
use console::style;

#[derive(Parser)]
#[command(name = "bankan", version, about = "Local Kanban tickets, docs and proposals for all your projects, with an MCP server for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Guided setup: port, backups, autostart, Claude Code and linking this repo.
    Setup,
    /// Run the server (web UI + API) in the foreground.
    Serve,
    /// Run the MCP server over stdio (started by Claude Code / other MCP clients).
    Mcp,
    /// Start the server automatically at login (launchd on macOS, systemd on Linux).
    Install,
    /// Remove the autostart service. Your data is kept.
    Uninstall,
    /// Link the current repo to a project by writing a `.bankan` file at the repo root.
    Link {
        /// Project key, e.g. RES.
        project: String,
        /// Create the project if it doesn't exist, with this display name.
        #[arg(long, value_name = "NAME")]
        create: Option<String>,
    },
    /// Remove the current repo's `.bankan` link.
    Unlink,
    /// Open the web UI (at the linked project's board when run inside a linked repo).
    Open {
        /// Project key to open.
        project: Option<String>,
    },
    /// Show whether the server is running, backups, and the current repo's link.
    Status,
    /// Run a git backup of the data folder now.
    Backup,
    /// Print the command that registers BanKan's MCP server with Claude Code.
    McpConfig,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("  {} {e:#}", style("✗").red().bold());
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let home = config::home_dir();
    let cfg = Config::load(&home)?;

    match cli.command {
        None => cli::welcome(&home, &cfg)?,
        Some(Cmd::Setup) => cli::setup(&home)?,
        Some(Cmd::Serve) => {
            let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
            rt.block_on(server::serve(home))?;
        }
        Some(Cmd::Mcp) => mcp::run(&cfg)?,
        Some(Cmd::Install) => {
            if service::running_from_build_dir() {
                eprintln!(
                    "  {} Installing from a build folder; rebuilding will break the service. Prefer {}.",
                    style("!").yellow(),
                    style("make install").cyan()
                );
            }
            if !home.join("config.toml").exists() {
                cfg.save(&home)?;
            }
            let summary = service::install(&home)?;
            println!("  {} {}", style("✓").green(), summary.replace('\n', "\n    "));
            println!("  {} BanKan starts at login and is running at {}", style("✓").green(), style(cfg.base_url()).cyan().underlined());
            println!("\n  Connect Claude Code (once, for all repos):\n    {}", style("claude mcp add bankan --scope user -- bankan mcp").cyan());
        }
        Some(Cmd::Uninstall) => service::uninstall()?,
        Some(Cmd::Link { project, create }) => cli::link_repo(&cfg, &project, create)?,
        Some(Cmd::Unlink) => match link::find(&std::env::current_dir()?)? {
            Some(l) => {
                std::fs::remove_file(&l.file)?;
                println!("  {} Removed {}", style("✓").green(), l.file.display());
            }
            None => println!("  No .bankan link found here."),
        },
        Some(Cmd::Open { project }) => {
            let key = project.map(|p| p.to_ascii_uppercase()).or_else(|| link::find(&std::env::current_dir().ok()?).ok().flatten().map(|l| l.project));
            if cli::daemon_up(&cfg).is_none() {
                bail!("BanKan isn't running. Start it with `bankan serve`, or `bankan setup` to install it.");
            }
            let url = match key {
                Some(k) => format!("{}/p/{k}/board", cfg.base_url()),
                None => cfg.base_url(),
            };
            cli::open_url(&url);
        }
        Some(Cmd::Status) => cli::status(&home, &cfg)?,
        Some(Cmd::Backup) => {
            let res: serde_json::Value = ureq::post(&format!("{}/api/backup", cfg.base_url()))
                .send_json(serde_json::json!({}))
                .context("BanKan isn't running")?
                .into_json()?;
            match res["last_error"].as_str() {
                Some(e) => bail!("backup failed: {e}"),
                None => println!("  {} Backup done.", style("✓").green()),
            }
        }
        Some(Cmd::McpConfig) => println!("claude mcp add bankan --scope user -- bankan mcp"),
    }
    Ok(())
}
