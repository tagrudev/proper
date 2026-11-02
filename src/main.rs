use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use proper::config::{self, Projects};
use proper::tmux::Server;
use proper::tui::Sidebar;

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
        None => anyhow::bail!("nothing to do yet"),
    }
}
