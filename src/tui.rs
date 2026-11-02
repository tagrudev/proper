//! The sidebar process: draws, reads keys, refreshes from tmux once a second.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

use crate::agents::Tree;
use crate::config::Projects;
use crate::sidebar::{Command, Input, State, reduce};
use crate::tmux::Server;
use crate::view::render;

const POLL: Duration = Duration::from_millis(250);
const REFRESH: Duration = Duration::from_secs(1);

pub struct Sidebar {
    pub server: Server,
    pub config_dir: PathBuf,
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

    fn snapshot(&self) -> Result<Tree> {
        Ok(Tree::from_registry(&Projects::load(&self.config_dir)?))
    }

    fn execute(&self, command: &Command) -> Result<Flow> {
        match command {
            Command::Quit => Ok(Flow::Exit),
            Command::Detach => {
                self.server.run(&self.server.argv(["detach-client"]))?;
                Ok(Flow::Continue)
            }
        }
    }
}

enum Flow {
    Continue,
    Exit,
}
