use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use proper::config::{self, Projects};
use proper::tmux::{Server, Status, socket_name};
use proper::tui::Sidebar;
use proper::workspace::{Workspace, play, sound_for};

#[derive(Parser)]
#[command(
    name = "proper",
    version,
    about = "A tmux workspace for you and your coding agents"
)]
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
    /// Start an agent in a project (default command: claude)
    New {
        /// Project name (default: the project containing the current directory)
        project: Option<String>,
        /// Agent name (default: the command's name)
        #[arg(long)]
        name: Option<String>,
        /// Command to run, after `--`
        #[arg(last = true)]
        command: Vec<String>,
    },
    /// List agents: project, name, state, command, pane
    Ls,
    /// Kill an agent given as PROJECT/AGENT
    Kill { agent: String },
    /// Unregister a project that has no agents
    Rm { project: String },
    /// Report an agent's state from inside its pane (meant for Claude Code hooks):
    /// working, waiting or attention. Plays a sound for the last two.
    Status { state: String },
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
    let config_dir = config::config_dir(
        std::env::var_os("PROPER_CONFIG_DIR").map(PathBuf::from),
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
    );
    match cli.command {
        Some(Command::Add { path, name }) => {
            let path = path.unwrap_or(std::env::current_dir()?);
            let name = name.unwrap_or_else(|| config::default_name(&path));
            let mut projects = Projects::load(&config_dir)?;
            let added = projects.add(&name, &path)?;
            println!("added {} -> {}", added.name, added.path.display());
            projects.save(&config_dir)
        }
        Some(Command::New {
            project,
            name,
            command,
        }) => {
            let workspace = workspace(config_dir)?;
            if !workspace.is_running()? {
                workspace.bootstrap(dirs::home_dir().as_deref())?;
            }
            let projects = workspace.projects()?;
            let project = match project {
                Some(name) => projects.get(&name).with_context(|| format!("no project named {name}"))?,
                None => projects.containing(&std::env::current_dir()?).context(
                    "the current directory is not inside a registered project; pass a project name or run `proper add`",
                )?,
            };
            let command = if command.is_empty() {
                vec!["claude".to_string()]
            } else {
                command
            };
            let agent = workspace.new_agent(project, name.as_deref(), &command)?;
            println!(
                "started {}/{} in {}",
                agent.project,
                agent.name,
                project.path.display()
            );
            Ok(())
        }
        Some(Command::Ls) => {
            let tree = workspace(config_dir)?.tree()?;
            for agent in tree.projects.iter().flat_map(|p| p.agents.iter()) {
                let state = match &agent.dead {
                    Some(death) => death.to_string(),
                    None => "alive".to_string(),
                };
                println!(
                    "{}\t{}\t{state}\t{}\t{}",
                    agent.project, agent.name, agent.command, agent.pane_id
                );
            }
            Ok(())
        }
        Some(Command::Kill { agent }) => {
            let workspace = workspace(config_dir)?;
            let (project, name) = agent
                .split_once('/')
                .context("give the agent as PROJECT/AGENT")?;
            let tree = workspace.tree()?;
            let found = tree
                .agent(project, name)
                .with_context(|| format!("no agent {agent}"))?;
            workspace.kill(&found.window_id)
        }
        Some(Command::Rm { project }) => {
            let workspace = workspace(config_dir)?;
            let tree = workspace.tree()?;
            let live: Vec<String> = tree
                .projects
                .iter()
                .filter(|p| p.name == project)
                .flat_map(|p| p.agents.iter().map(|a| format!("{}/{}", a.project, a.name)))
                .collect();
            if !live.is_empty() {
                anyhow::bail!(
                    "{project} still has agents: {}; kill them first",
                    live.join(", ")
                );
            }
            let mut projects = workspace.projects()?;
            let removed = projects.remove(&project)?;
            workspace.save_projects(&projects)?;
            println!("removed {} ({})", removed.name, removed.path.display());
            Ok(())
        }
        Some(Command::Status { state }) => {
            let status =
                Status::parse(&state)?.with_context(|| format!("unknown status {state:?}"))?;
            let Some(pane) = std::env::var("TMUX_PANE").ok() else {
                return Ok(());
            };
            workspace(config_dir)?.set_status(&pane, status)?;
            if let Some(sound) = sound_for(status, std::env::var_os("PROPER_SOUND")) {
                play(&sound);
            }
            Ok(())
        }
        Some(Command::Sidebar) => Sidebar {
            workspace: workspace(config_dir)?,
            own_pane: std::env::var("TMUX_PANE").ok(),
        }
        .run(),
        Some(Command::Hint) => proper::hint::run(),
        None => open(config_dir),
    }
}

fn workspace(config_dir: PathBuf) -> Result<Workspace> {
    Ok(Workspace::new(
        Server::from_env(),
        config_dir,
        std::env::current_exe()?,
    ))
}

/// Brings the proper server up if needed and attaches this terminal to it.
fn open(config_dir: PathBuf) -> Result<()> {
    let server = Server::from_env();
    let workspace = Workspace::new(server, config_dir, std::env::current_exe()?);
    match where_am_i(workspace.server()) {
        Place::InsideProper => {
            workspace.heal()?;
            println!("already inside proper");
            return Ok(());
        }
        Place::InsideOtherTmux => {
            anyhow::bail!("run proper from a plain terminal, not inside another tmux")
        }
        Place::PlainTerminal => {}
    }
    if workspace.is_running()? {
        workspace.heal()?;
    } else {
        workspace.bootstrap(dirs::home_dir().as_deref())?;
    }
    workspace.attach()?;
    Ok(())
}

enum Place {
    InsideProper,
    InsideOtherTmux,
    PlainTerminal,
}

/// Sidebar panes carry TMUX pointing at proper's socket; agent panes have TMUX cleared
/// but still inherit TMUX_PANE and PROPER_SOCKET from the session.
fn where_am_i(server: &Server) -> Place {
    let tmux = std::env::var("TMUX").unwrap_or_default();
    let in_pane = std::env::var_os("TMUX_PANE").is_some();
    let socket_env = std::env::var("PROPER_SOCKET").ok();
    if socket_name(&tmux).as_deref() == Some(server.socket()) {
        Place::InsideProper
    } else if !tmux.is_empty() {
        Place::InsideOtherTmux
    } else if in_pane && socket_env.as_deref() == Some(server.socket()) {
        Place::InsideProper
    } else {
        Place::PlainTerminal
    }
}
