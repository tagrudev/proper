//! The sidebar process: draws, reads keys, refreshes from tmux once a second.

use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use anyhow::bail;

use crate::agents::Tree;
use crate::config::Project;
use crate::sidebar::{Command, Input, Program, State, reduce};
use crate::view::render;
use crate::workspace::Workspace;

const POLL: Duration = Duration::from_millis(250);
const REFRESH: Duration = Duration::from_secs(1);

pub struct Sidebar {
    pub workspace: Workspace,
    /// This sidebar's own pane id; `None` when running outside tmux.
    pub own_pane: Option<String>,
}

impl Sidebar {
    pub fn run(&self) -> Result<()> {
        let mut terminal = ratatui::init();
        let result = self.event_loop(&mut terminal);
        ratatui::restore();
        result
    }

    fn event_loop(&self, terminal: &mut ratatui::DefaultTerminal) -> Result<()> {
        let mut state = State::new(self.snapshot()?, self.own_pane.clone());
        let mut refreshed = Instant::now();
        loop {
            terminal.draw(|frame| render(frame, &state))?;
            let mut refresh_now = refreshed.elapsed() >= REFRESH;
            if event::poll(POLL)? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        let (next, command) = reduce(state, Input::Key(key));
                        state = next;
                        if let Some(command) = command {
                            match self.execute(&command) {
                                Ok(Flow::Exit) => return Ok(()),
                                Ok(Flow::Continue) => refresh_now = true,
                                Err(err) => state = state.with_error(err.root_cause().to_string()),
                            }
                        }
                    }
                    Event::FocusGained | Event::Resize(..) => refresh_now = true,
                    _ => {}
                }
            }
            if refresh_now {
                state = match self.snapshot() {
                    Ok(tree) => reduce(state, Input::Snapshot(tree)).0,
                    Err(err) => state.with_error(err.root_cause().to_string()),
                };
                refreshed = Instant::now();
            }
        }
    }

    /// Inside tmux the tree comes from live panes; outside there are only registered projects.
    fn snapshot(&self) -> Result<Tree> {
        match self.own_pane {
            Some(_) => self.workspace.tree(),
            None => Ok(Tree::from_registry(&self.workspace.projects()?)),
        }
    }

    fn execute(&self, command: &Command) -> Result<Flow> {
        match command {
            Command::Quit => Ok(Flow::Exit),
            Command::Kill { window_id } => {
                self.workspace.kill(window_id)?;
                Ok(Flow::Continue)
            }
            Command::Respawn { pane_id } => {
                self.workspace.respawn(pane_id)?;
                Ok(Flow::Continue)
            }
            Command::Select { window_id, pane_id } => {
                self.workspace.select(window_id, pane_id)?;
                Ok(Flow::Continue)
            }
            Command::New { project, program } => {
                if project.path.as_os_str().is_empty() {
                    bail!("{} has no directory on record; run proper add", project.name);
                }
                let project = Project { name: project.name.clone(), path: project.path.clone() };
                let command = match program {
                    Program::Claude => vec!["claude".to_string()],
                    Program::Shell => vec![std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())],
                };
                self.workspace.new_agent(&project, None, &command)?;
                Ok(Flow::Continue)
            }
            Command::Detach => {
                self.workspace.detach()?;
                Ok(Flow::Continue)
            }
        }
    }
}

enum Flow {
    Continue,
    Exit,
}
