use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use proper::config::{self, Projects};
use proper::tmux::{Server, socket_name};
use proper::tui::Sidebar;
use proper::workspace::Workspace;

#[derive(Parser)]
#[command(name = "proper", version, about = "A tmux workspace for you and your coding agents")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Register a project directory
    Add {
        /// Project directory (default: current directory)
        path: Option<PathBuf>,
        /// Project name (default: directory name)
        #[arg(long)]
        name: Option<String>,
    },
    /// Run the sidebar pane (started by proper itself)
    #[command(hide = true)]
    Sidebar,
    /// Show the placeholder pane (started by proper itself)
    #[command(hide = true)]
    Hint,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("proper: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    let config_dir = config::config_dir(std::env::var_os("PROPER_CONFIG_DIR").map(PathBuf::from));
    match cli.command {
        Some(Command::Add { path, name }) => {
            let path = path.unwrap_or(std::env::current_dir()?);
            let name = name.unwrap_or_else(|| config::default_name(&path));
            let mut projects = Projects::load(&config_dir)?;
            let added = projects.add(&name, &path)?;
            println!("added {} -> {}", added.name, added.path.display());
            projects.save(&config_dir)
        }
        Some(Command::Sidebar) => Sidebar {
            server: Server::from_env(),
            config_dir,
            own_pane: std::env::var("TMUX_PANE").ok(),
        }
        .run(),
        Some(Command::Hint) => proper::hint::run(),
        None => open(config_dir),
    }
}

/// Brings the proper server up if needed and attaches this terminal to it.
fn open(config_dir: PathBuf) -> Result<()> {
    let server = Server::from_env();
    match std::env::var("TMUX") {
        Ok(tmux) if socket_name(&tmux).as_deref() == Some(server.socket()) => {
            println!("already inside proper");
            return Ok(());
        }
        Ok(_) => anyhow::bail!("run proper from a plain terminal, not inside another tmux"),
        Err(_) => {}
    }
    let workspace = Workspace::new(server, config_dir, std::env::current_exe()?);
    if !workspace.is_running()? {
        workspace.bootstrap(dirs::home_dir().as_deref())?;
    }
    workspace.attach()?;
    Ok(())
}
