//! The sidebar process: draws, reads keys and clicks, refreshes from tmux when
//! a hook says something changed and once a second regardless.

use std::io::stdout;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use ratatui::crossterm::event::{
    self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event,
    KeyEventKind, MouseButton, MouseEventKind,
};
use ratatui::crossterm::execute;

use crate::agents::{Tree, sidebar_is_alone};
use crate::config::Project;
use crate::sidebar::{Command, Input, Mouse, Program, State, reduce};
use crate::view::render;
use crate::workspace::Workspace;

const POLL: Duration = Duration::from_millis(50);
const REFRESH: Duration = Duration::from_secs(1);

pub struct Sidebar {
    pub workspace: Workspace,
    /// This sidebar's own pane id; `None` when running outside tmux.
    pub own_pane: Option<String>,
}

impl Sidebar {
    pub fn run(&self) -> Result<()> {
        let mut terminal = ratatui::init();
        let _ = execute!(stdout(), EnableMouseCapture, EnableFocusChange);
        let changed = self.watch_changes();
        let result = self.event_loop(&mut terminal, &changed);
        let _ = execute!(stdout(), DisableMouseCapture, DisableFocusChange);
        ratatui::restore();
        changed.stop();
        result
    }

    fn event_loop(&self, terminal: &mut ratatui::DefaultTerminal, changed: &Changes) -> Result<()> {
        let (tree, focused) = match self.snapshot()? {
            Snapshot::Alone => return Ok(()),
            Snapshot::Tree { tree, focused } => (tree, focused),
        };
        let (mut state, _) = reduce(
            State::new(tree, self.own_pane.clone()),
            Input::Focus(focused),
        );
        let started = Instant::now();
        let mut refreshed = started;
        loop {
            terminal.draw(|frame| render(frame, &state, started.elapsed()))?;
            let mut refresh_now = refreshed.elapsed() >= REFRESH || changed.take();
            if event::poll(POLL)? {
                let input = match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => Some(Input::Key(key)),
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            Some(Input::Mouse(Mouse::Click { row: mouse.row }))
                        }
                        MouseEventKind::ScrollUp => Some(Input::Mouse(Mouse::ScrollUp)),
                        MouseEventKind::ScrollDown => Some(Input::Mouse(Mouse::ScrollDown)),
                        _ => None,
                    },
                    Event::FocusGained | Event::Resize(..) => {
                        refresh_now = true;
                        None
                    }
                    _ => None,
                };
                if let Some(input) = input {
                    let (next, command) = reduce(state, input);
                    state = next;
                    if let Some(command) = command {
                        match self.execute(&command) {
                            Ok(Flow::Exit) => return Ok(()),
                            Ok(Flow::Continue) => refresh_now = true,
                            Err(err) => state = state.with_error(err.root_cause().to_string()),
                        }
                    }
                }
            }
            if refresh_now {
                state = match self.snapshot() {
                    Ok(Snapshot::Alone) => return Ok(()),
                    Ok(Snapshot::Tree { tree, focused }) => {
                        let (state, _) = reduce(state, Input::Snapshot(tree));
                        reduce(state, Input::Focus(focused)).0
                    }
                    Err(err) => state.with_error(err.root_cause().to_string()),
                };
                refreshed = Instant::now();
            }
        }
    }

    /// Inside tmux the tree comes from live panes; outside there are only registered projects
    /// and the sidebar always has focus.
    /// A sidebar whose agent is gone reports `Alone` and exits, which closes its window.
    fn snapshot(&self) -> Result<Snapshot> {
        let projects = self.workspace.projects()?;
        let Some(own_pane) = &self.own_pane else {
            return Ok(Snapshot::Tree {
                tree: Tree::from_registry(&projects),
                focused: true,
            });
        };
        let panes = self.workspace.server().list_panes()?;
        if sidebar_is_alone(&panes, own_pane) {
            return Ok(Snapshot::Alone);
        }
        Ok(Snapshot::Tree {
            tree: Tree::build(&panes, &projects),
            focused: panes.iter().any(|p| &p.id == own_pane && p.pane_active),
        })
    }

    /// A thread blocked in `tmux wait-for` raises a flag each time a hook fires.
    fn watch_changes(&self) -> Changes {
        let changes = Changes {
            flag: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            server: self.workspace.server().clone(),
        };
        if self.own_pane.is_some() {
            let flag = Arc::clone(&changes.flag);
            let stopped = Arc::clone(&changes.stopped);
            let server = changes.server.clone();
            let workspace_server = server;
            std::thread::spawn(move || {
                let waiter =
                    Workspace::new(workspace_server, Default::default(), Default::default());
                while !stopped.load(Ordering::SeqCst) {
                    match waiter.wait_for_change() {
                        Ok(()) => flag.store(true, Ordering::SeqCst),
                        Err(_) => std::thread::sleep(REFRESH),
                    }
                }
            });
        }
        changes
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
                    bail!(
                        "{} has no directory on record; run proper add",
                        project.name
                    );
                }
                let project = Project {
                    name: project.name.clone(),
                    path: project.path.clone(),
                };
                let (name, command) = match program {
                    Program::Claude => (None, vec!["claude".to_string()]),
                    Program::Shell => (
                        None,
                        vec![std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string())],
                    ),
                    Program::Worktree(worktree) => (
                        Some(worktree.as_str()),
                        vec!["claude".to_string(), "-w".to_string(), worktree.clone()],
                    ),
                };
                self.workspace.new_agent(&project, name, &command)?;
                Ok(Flow::Continue)
            }
            Command::Detach => {
                self.workspace.detach()?;
                Ok(Flow::Continue)
            }
        }
    }
}

enum Snapshot {
    /// `focused` is true when the sidebar is the active pane of its window.
    Tree {
        tree: Tree,
        focused: bool,
    },
    Alone,
}

struct Changes {
    flag: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    server: crate::tmux::Server,
}

impl Changes {
    fn take(&self) -> bool {
        self.flag.swap(false, Ordering::SeqCst)
    }

    /// Ends the waiter thread by letting its pending `wait-for` return.
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = Workspace::new(self.server.clone(), Default::default(), Default::default())
            .signal_change();
    }
}

enum Flow {
    Continue,
    Exit,
}
