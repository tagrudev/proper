//! Pure sidebar state: keys and snapshots go in, commands for tmux come out.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent};

use crate::agents::{Agent, Tree};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    pub name: String,
    pub path: PathBuf,
    pub registered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Project(ProjectRow),
    Agent(Agent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Help,
    ConfirmKill,
    /// Typing narrows the registered projects with nothing running; Enter starts claude in
    /// the picked one.
    Find,
    /// Typing names a worktree; Enter starts claude in a new worktree of that project.
    Worktree,
}

/// Mouse input already translated to sidebar rows; `row` is zero-based from the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouse {
    Click { row: u16 },
    ScrollUp,
    ScrollDown,
}

#[derive(Debug)]
pub enum Input {
    Key(KeyEvent),
    Mouse(Mouse),
    Snapshot(Tree),
    /// Whether the sidebar is the pane that takes keys in its window.
    Focus(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Program {
    Claude,
    Shell,
    /// Claude in a new git worktree of that name.
    Worktree(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Select {
        window_id: String,
        pane_id: String,
    },
    New {
        project: ProjectRow,
        program: Program,
    },
    Kill {
        window_id: String,
    },
    Respawn {
        pane_id: String,
    },
    Detach,
    Quit,
}

#[derive(Debug, Clone)]
pub struct State {
    rows: Vec<Row>,
    cursor: usize,
    mode: Mode,
    error: Option<String>,
    own_pane: Option<String>,
    focused: bool,
    /// Registered projects with no agents, the finder's choices.
    idle: Vec<ProjectRow>,
    query: String,
    /// Index into the finder's matches.
    pick: usize,
    /// The project a worktree prompt starts claude in.
    worktree_project: Option<ProjectRow>,
    worktree_name: String,
}

impl State {
    /// `own_pane` is the sidebar's own tmux pane id; `None` means running outside tmux.
    pub fn new(tree: Tree, own_pane: Option<String>) -> State {
        let idle = idle_projects(&tree);
        let rows = rows(tree);
        let cursor = current_agent(&rows).unwrap_or(0);
        State {
            rows,
            cursor,
            mode: Mode::Normal,
            error: None,
            own_pane,
            focused: true,
            idle,
            query: String::new(),
            pick: 0,
            worktree_project: None,
            worktree_name: String::new(),
        }
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn in_tmux(&self) -> bool {
        self.own_pane.is_some()
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn idle(&self) -> &[ProjectRow] {
        &self.idle
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn pick(&self) -> usize {
        self.pick
    }

    pub fn worktree_project(&self) -> Option<&ProjectRow> {
        self.worktree_project.as_ref()
    }

    pub fn worktree_name(&self) -> &str {
        &self.worktree_name
    }

    /// Idle projects whose names hold the letters of the query in order.
    pub fn found(&self) -> Vec<&ProjectRow> {
        self.idle
            .iter()
            .filter(|project| {
                let mut letters = project.name.chars();
                self.query
                    .chars()
                    .all(|wanted| letters.any(|letter| letter == wanted))
            })
            .collect()
    }

    pub fn with_error(self, message: impl Into<String>) -> State {
        State {
            error: Some(message.into()),
            ..self
        }
    }

    fn row(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    pub fn agent_at_cursor(&self) -> Option<&Agent> {
        match self.row()? {
            Row::Agent(agent) => Some(agent),
            Row::Project(_) => None,
        }
    }

    /// The project the cursor is in: the row itself or the agent's project header.
    fn project_at_cursor(&self) -> Option<ProjectRow> {
        match self.row()? {
            Row::Project(project) => Some(project.clone()),
            Row::Agent(agent) => self.rows.iter().find_map(|row| match row {
                Row::Project(project) if project.name == agent.project => Some(project.clone()),
                _ => None,
            }),
        }
    }

    fn moved(self, delta: isize) -> State {
        let cursor = stepped(self.cursor, delta, self.rows.len());
        State { cursor, ..self }
    }

    fn picked(self, delta: isize) -> State {
        let pick = stepped(self.pick, delta, self.found().len());
        State { pick, ..self }
    }

    /// Opens the worktree prompt for `project` with no name typed yet.
    fn naming_worktree(self, project: ProjectRow) -> State {
        State {
            mode: Mode::Worktree,
            worktree_project: Some(project),
            worktree_name: String::new(),
            ..self
        }
    }
}

/// `index` moved by `delta` and kept inside a list of `len` items.
fn stepped(index: usize, delta: isize, len: usize) -> usize {
    let last = len.saturating_sub(1) as isize;
    (index as isize + delta).clamp(0, last) as usize
}

fn idle_projects(tree: &Tree) -> Vec<ProjectRow> {
    tree.projects
        .iter()
        .filter(|project| project.registered && project.agents.is_empty())
        .map(|project| ProjectRow {
            name: project.name.clone(),
            path: project.path.clone(),
            registered: true,
        })
        .collect()
}

/// Projects with agents, each followed by its agents; projects with none get no row.
fn rows(tree: Tree) -> Vec<Row> {
    tree.projects
        .into_iter()
        .filter(|project| !project.agents.is_empty())
        .flat_map(|project| {
            let header = Row::Project(ProjectRow {
                name: project.name,
                path: project.path,
                registered: project.registered,
            });
            std::iter::once(header).chain(project.agents.into_iter().map(Row::Agent))
        })
        .collect()
}

fn current_agent(rows: &[Row]) -> Option<usize> {
    rows.iter()
        .position(|row| matches!(row, Row::Agent(agent) if agent.current))
}

/// Rows keep their identity across snapshots by project name or agent pane.
fn same_row(a: &Row, b: &Row) -> bool {
    match (a, b) {
        (Row::Project(a), Row::Project(b)) => a.name == b.name,
        (Row::Agent(a), Row::Agent(b)) => a.pane_id == b.pane_id,
        _ => false,
    }
}

pub fn reduce(state: State, input: Input) -> (State, Option<Command>) {
    match input {
        Input::Snapshot(tree) => {
            let idle = idle_projects(&tree);
            let rows = rows(tree);
            let newly_current =
                current_agent(&rows).filter(|now| current_agent(&state.rows) != Some(*now));
            let cursor = newly_current
                .or_else(|| {
                    state
                        .row()
                        .and_then(|current| rows.iter().position(|row| same_row(row, current)))
                })
                .unwrap_or(state.cursor.min(rows.len().saturating_sub(1)));
            let state = State {
                rows,
                cursor,
                idle,
                ..state
            };
            (state.picked(0), None)
        }
        Input::Focus(focused) => (State { focused, ..state }, None),
        Input::Key(key) => {
            let state = State {
                error: None,
                ..state
            };
            match state.mode {
                Mode::Help => (help_key(state, key), None),
                Mode::ConfirmKill => confirm_kill_key(state, key),
                Mode::Find => find_key(state, key),
                Mode::Worktree => worktree_key(state, key),
                Mode::Normal => normal_key(state, key),
            }
        }
        Input::Mouse(mouse) => {
            let state = State {
                error: None,
                ..state
            };
            if state.mode != Mode::Normal {
                return (
                    State {
                        mode: Mode::Normal,
                        ..state
                    },
                    None,
                );
            }
            match mouse {
                Mouse::ScrollUp => (state.moved(-1), None),
                Mouse::ScrollDown => (state.moved(1), None),
                Mouse::Click { row } => {
                    let row = row as usize;
                    if row >= state.rows.len() {
                        return (state, None);
                    }
                    let state = State {
                        cursor: row,
                        ..state
                    };
                    let command = state.agent_at_cursor().map(|agent| Command::Select {
                        window_id: agent.window_id.clone(),
                        pane_id: agent.pane_id.clone(),
                    });
                    (state, command)
                }
            }
        }
    }
}

fn help_key(state: State, key: KeyEvent) -> State {
    match key.code {
        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => State {
            mode: Mode::Normal,
            ..state
        },
        _ => state,
    }
}

fn confirm_kill_key(state: State, key: KeyEvent) -> (State, Option<Command>) {
    let command = match key.code {
        KeyCode::Char('y') => state.agent_at_cursor().map(|agent| Command::Kill {
            window_id: agent.window_id.clone(),
        }),
        _ => None,
    };
    (
        State {
            mode: Mode::Normal,
            ..state
        },
        command,
    )
}

fn find_key(state: State, key: KeyEvent) -> (State, Option<Command>) {
    match key.code {
        KeyCode::Esc => (
            State {
                mode: Mode::Normal,
                ..state
            },
            None,
        ),
        KeyCode::Enter => match state.found().get(state.pick) {
            Some(project) => {
                let command = Command::New {
                    project: (*project).clone(),
                    program: Program::Claude,
                };
                (
                    State {
                        mode: Mode::Normal,
                        ..state
                    },
                    Some(command),
                )
            }
            None => (state, None),
        },
        KeyCode::Tab => match state.found().get(state.pick) {
            Some(project) => {
                let project = (*project).clone();
                (state.naming_worktree(project), None)
            }
            None => (state, None),
        },
        KeyCode::Down => (state.picked(1), None),
        KeyCode::Up => (state.picked(-1), None),
        _ => match edited(&state.query, key) {
            Some(query) => (
                State {
                    query,
                    pick: 0,
                    ..state
                },
                None,
            ),
            None => (state, None),
        },
    }
}

fn worktree_key(state: State, key: KeyEvent) -> (State, Option<Command>) {
    match key.code {
        KeyCode::Esc => (
            State {
                mode: Mode::Normal,
                ..state
            },
            None,
        ),
        KeyCode::Enter if state.worktree_name.is_empty() => {
            (state.with_error("name the worktree"), None)
        }
        KeyCode::Enter => {
            let command = state.worktree_project.clone().map(|project| Command::New {
                project,
                program: Program::Worktree(state.worktree_name.clone()),
            });
            (
                State {
                    mode: Mode::Normal,
                    ..state
                },
                command,
            )
        }
        _ => match edited(&state.worktree_name, key) {
            Some(worktree_name) => (
                State {
                    worktree_name,
                    ..state
                },
                None,
            ),
            None => (state, None),
        },
    }
}

/// `text` after a typing key: Backspace takes back the last letter and a letter is added
/// in lowercase. `None` for keys that do not type.
fn edited(text: &str, key: KeyEvent) -> Option<String> {
    match key.code {
        KeyCode::Backspace => {
            let mut letters = text.chars();
            letters.next_back();
            Some(letters.as_str().to_string())
        }
        KeyCode::Char(letter) => Some(format!("{text}{}", letter.to_ascii_lowercase())),
        _ => None,
    }
}

fn normal_key(state: State, key: KeyEvent) -> (State, Option<Command>) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => (state.moved(1), None),
        KeyCode::Char('k') | KeyCode::Up => (state.moved(-1), None),
        KeyCode::Enter => {
            let command = match state.row() {
                Some(Row::Agent(agent)) => Some(Command::Select {
                    window_id: agent.window_id.clone(),
                    pane_id: agent.pane_id.clone(),
                }),
                _ => None,
            };
            (state, command)
        }
        KeyCode::Char('n') => {
            let command = state.project_at_cursor().map(|project| Command::New {
                project,
                program: Program::Claude,
            });
            (state, command)
        }
        KeyCode::Char('s') => {
            let command = state.project_at_cursor().map(|project| Command::New {
                project,
                program: Program::Shell,
            });
            (state, command)
        }
        KeyCode::Char('w') => match state.project_at_cursor() {
            Some(project) => (state.naming_worktree(project), None),
            None => (state, None),
        },
        KeyCode::Char('x') => match state.agent_at_cursor() {
            Some(_) => (
                State {
                    mode: Mode::ConfirmKill,
                    ..state
                },
                None,
            ),
            None => (state, None),
        },
        KeyCode::Char('r') => match state.agent_at_cursor() {
            Some(agent) if agent.dead.is_some() => {
                let command = Command::Respawn {
                    pane_id: agent.pane_id.clone(),
                };
                (state, Some(command))
            }
            Some(agent) => {
                let error = format!("{} is still running", agent.name);
                (state.with_error(error), None)
            }
            None => (state, None),
        },
        KeyCode::Char('?') => (
            State {
                mode: Mode::Help,
                ..state
            },
            None,
        ),
        KeyCode::Char('/') => (
            State {
                mode: Mode::Find,
                query: String::new(),
                pick: 0,
                ..state
            },
            None,
        ),
        KeyCode::Char('q') => {
            let command = if state.in_tmux() {
                Command::Detach
            } else {
                Command::Quit
            };
            (state, Some(command))
        }
        _ => (state, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{Agent, ProjectEntry};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    /// Each project runs one claude, so each gets a row.
    fn tree(names: &[&str]) -> Tree {
        Tree {
            projects: names
                .iter()
                .enumerate()
                .map(|(index, n)| ProjectEntry {
                    name: n.to_string(),
                    path: PathBuf::from(format!("/p/{n}")),
                    registered: true,
                    agents: vec![Agent {
                        name: "claude".into(),
                        project: n.to_string(),
                        pane_id: format!("%{}", index + 2),
                        window_id: format!("@{}", index + 2),
                        current: false,
                        dead: None,
                        command: "claude".into(),
                        status: None,
                    }],
                })
                .collect(),
        }
    }

    fn key(code: KeyCode) -> Input {
        Input::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn press(state: State, code: KeyCode) -> (State, Option<Command>) {
        reduce(state, key(code))
    }

    fn inside_tmux() -> State {
        State::new(
            tree(&["project", "znaiko", "todor.dev"]),
            Some("%1".to_string()),
        )
    }

    #[test]
    fn starts_with_each_project_above_its_agent_and_the_cursor_on_the_first() {
        let state = inside_tmux();
        assert_eq!(state.rows().len(), 6);
        assert_eq!(state.cursor(), 0);
        assert!(matches!(state.rows()[0], Row::Project(ref p) if p.name == "project"));
        assert!(matches!(state.rows()[1], Row::Agent(ref a) if a.project == "project"));
    }

    #[test]
    fn j_and_k_move_the_cursor_and_clamp_at_both_ends() {
        let (state, _) = press(inside_tmux(), KeyCode::Char('k'));
        assert_eq!(state.cursor(), 0);
        let (state, _) = press(state, KeyCode::Char('j'));
        let (state, _) = press(state, KeyCode::Down);
        assert_eq!(state.cursor(), 2);
        let state = (0..4).fold(state, |state, _| press(state, KeyCode::Char('j')).0);
        assert_eq!(state.cursor(), 5);
        let (state, _) = press(state, KeyCode::Up);
        assert_eq!(state.cursor(), 4);
    }

    #[test]
    fn q_detaches_inside_tmux_and_quits_outside() {
        let (_, command) = press(inside_tmux(), KeyCode::Char('q'));
        assert_eq!(command, Some(Command::Detach));
        let outside = State::new(tree(&["project"]), None);
        let (_, command) = press(outside, KeyCode::Char('q'));
        assert_eq!(command, Some(Command::Quit));
    }

    #[test]
    fn question_mark_toggles_help_and_escape_closes_it() {
        let (state, _) = press(inside_tmux(), KeyCode::Char('?'));
        assert_eq!(state.mode(), Mode::Help);
        let (state, _) = press(state, KeyCode::Char('j'));
        assert_eq!(
            state.cursor(),
            0,
            "keys do not move the cursor while help is open"
        );
        let (state, _) = press(state, KeyCode::Esc);
        assert_eq!(state.mode(), Mode::Normal);
        let (state, _) = press(state, KeyCode::Char('?'));
        let (state, _) = press(state, KeyCode::Char('?'));
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn enter_on_a_project_does_nothing() {
        let (state, command) = press(inside_tmux(), KeyCode::Enter);
        assert_eq!(command, None);
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn a_snapshot_replaces_rows_and_keeps_the_cursor_in_bounds() {
        let (state, _) = press(inside_tmux(), KeyCode::Char('j'));
        let (state, _) = press(state, KeyCode::Char('j'));
        assert_eq!(state.cursor(), 2);
        let (state, command) = reduce(state, Input::Snapshot(tree(&["only"])));
        assert_eq!(command, None);
        assert_eq!(state.rows().len(), 2);
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn an_empty_tree_has_no_rows_and_survives_navigation() {
        let state = State::new(tree(&[]), None);
        let (state, _) = press(state, KeyCode::Char('j'));
        assert_eq!(state.cursor(), 0);
        assert!(state.rows().is_empty());
    }

    #[test]
    fn a_sidebar_starts_focused_and_follows_focus_input() {
        let state = inside_tmux();
        assert!(state.focused());
        let (state, command) = reduce(state, Input::Focus(false));
        assert_eq!(command, None);
        assert!(!state.focused());
        let (state, _) = reduce(state, Input::Focus(true));
        assert!(state.focused());
    }

    #[test]
    fn errors_show_until_the_next_key() {
        let state = inside_tmux().with_error("tmux said no");
        assert_eq!(state.error(), Some("tmux said no"));
        let (state, _) = press(state, KeyCode::Char('j'));
        assert_eq!(state.error(), None);
    }
}

#[cfg(test)]
mod agent_tests {
    use super::*;
    use crate::agents::{Agent, ProjectEntry};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn agent(project: &str, name: &str, pane: &str) -> Agent {
        Agent {
            name: name.into(),
            project: project.into(),
            pane_id: pane.into(),
            window_id: format!("@{}", &pane[1..]),
            current: false,
            dead: None,
            command: "node".into(),
            status: None,
        }
    }

    fn project(name: &str, agents: Vec<Agent>) -> ProjectEntry {
        ProjectEntry {
            name: name.into(),
            path: PathBuf::from(format!("/p/{name}")),
            registered: true,
            agents,
        }
    }

    fn press(state: State, code: KeyCode) -> State {
        reduce(state, Input::Key(KeyEvent::new(code, KeyModifiers::NONE))).0
    }

    fn tree() -> Tree {
        Tree {
            projects: vec![
                project(
                    "project",
                    vec![
                        agent("project", "claude", "%2"),
                        agent("project", "zsh", "%3"),
                    ],
                ),
                project("znaiko", vec![agent("znaiko", "claude", "%4")]),
            ],
        }
    }

    #[test]
    fn projects_without_agents_get_no_row_but_stay_findable() {
        let tree = Tree {
            projects: vec![
                project("idle", vec![]),
                project("busy", vec![agent("busy", "claude", "%2")]),
            ],
        };
        let state = State::new(tree, Some("%1".into()));
        assert_eq!(state.rows().len(), 2);
        assert!(matches!(&state.rows()[0], Row::Project(p) if p.name == "busy"));
        assert!(matches!(&state.rows()[1], Row::Agent(a) if a.pane_id == "%2"));
        let names: Vec<&str> = state.idle().iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["idle"]);
    }

    #[test]
    fn agents_are_rows_right_under_their_project() {
        let state = State::new(tree(), Some("%1".into()));
        let rows = state.rows();
        assert_eq!(rows.len(), 5);
        assert!(matches!(&rows[1], Row::Agent(a) if a.name == "claude"));
        assert!(matches!(&rows[2], Row::Agent(a) if a.name == "zsh"));
        assert!(matches!(&rows[3], Row::Project(p) if p.name == "znaiko"));
        assert!(matches!(&rows[4], Row::Agent(a) if a.project == "znaiko"));
    }

    #[test]
    fn the_cursor_follows_an_agent_when_rows_shift() {
        let state = press(
            press(State::new(tree(), Some("%1".into())), KeyCode::Char('j')),
            KeyCode::Char('j'),
        );
        assert!(matches!(&state.rows()[state.cursor()], Row::Agent(a) if a.pane_id == "%3"));

        let shifted = Tree {
            projects: vec![
                project("alpha", vec![agent("alpha", "claude", "%9")]),
                project(
                    "project",
                    vec![
                        agent("project", "claude", "%2"),
                        agent("project", "zsh", "%3"),
                    ],
                ),
            ],
        };
        let (state, _) = reduce(state, Input::Snapshot(shifted));
        assert!(matches!(&state.rows()[state.cursor()], Row::Agent(a) if a.pane_id == "%3"));
    }

    #[test]
    fn the_cursor_follows_a_project_when_rows_shift() {
        let state = press(
            press(
                press(State::new(tree(), Some("%1".into())), KeyCode::Char('j')),
                KeyCode::Char('j'),
            ),
            KeyCode::Char('j'),
        );
        assert!(matches!(&state.rows()[state.cursor()], Row::Project(p) if p.name == "znaiko"));
        let (state, _) = reduce(
            state,
            Input::Snapshot(Tree {
                projects: vec![
                    project("znaiko", vec![agent("znaiko", "claude", "%4")]),
                    project("project", vec![agent("project", "claude", "%2")]),
                ],
            }),
        );
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn enter_on_an_agent_selects_its_window_and_pane() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('j'));
        let (_, command) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
        );
        assert_eq!(
            command,
            Some(Command::Select {
                window_id: "@2".into(),
                pane_id: "%2".into()
            })
        );
    }

    #[test]
    fn n_and_s_start_agents_in_the_project_under_the_cursor() {
        let state = State::new(tree(), Some("%1".into()));
        let (_, command) = reduce(
            state.clone(),
            Input::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        );
        assert_eq!(
            command,
            Some(Command::New {
                project: project_row("project"),
                program: Program::Claude
            })
        );
        let (_, command) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
        );
        assert_eq!(
            command,
            Some(Command::New {
                project: project_row("project"),
                program: Program::Shell
            })
        );
    }

    #[test]
    fn n_on_an_agent_row_uses_that_agents_project() {
        let state = press(
            press(
                press(State::new(tree(), Some("%1".into())), KeyCode::Char('j')),
                KeyCode::Char('j'),
            ),
            KeyCode::Char('j'),
        );
        let (_, command) = reduce(
            state.clone(),
            Input::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        );
        assert_eq!(
            command,
            Some(Command::New {
                project: project_row("znaiko"),
                program: Program::Claude
            })
        );
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('j'));
        let (_, command) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        );
        assert_eq!(
            command,
            Some(Command::New {
                project: project_row("project"),
                program: Program::Claude
            })
        );
    }

    #[test]
    fn n_with_no_rows_does_nothing() {
        let state = State::new(Tree::default(), Some("%1".into()));
        let (_, command) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        );
        assert_eq!(command, None);
    }

    #[test]
    fn the_cursor_snaps_to_the_agent_whose_window_became_current() {
        let state = State::new(tree(), Some("%1".into()));
        assert_eq!(state.cursor(), 0);
        let mut now_current = tree();
        now_current.projects[0].agents[1].current = true;
        let (state, _) = reduce(state, Input::Snapshot(now_current.clone()));
        assert!(matches!(&state.rows()[state.cursor()], Row::Agent(a) if a.name == "zsh"));

        let state = press(state, KeyCode::Char('k'));
        let (state, _) = reduce(state, Input::Snapshot(now_current));
        assert_eq!(
            state.cursor(),
            1,
            "an unchanged current window leaves the cursor alone"
        );
    }

    #[test]
    fn a_new_state_starts_on_the_agent_whose_window_is_current() {
        let mut current = tree();
        current.projects[0].agents[1].current = true;
        let state = State::new(current, Some("%1".into()));
        assert!(matches!(&state.rows()[state.cursor()], Row::Agent(a) if a.name == "zsh"));
    }

    fn click(state: State, row: u16) -> (State, Option<Command>) {
        reduce(state, Input::Mouse(Mouse::Click { row }))
    }

    #[test]
    fn clicking_an_agent_row_moves_the_cursor_and_opens_it() {
        let (state, command) = click(State::new(tree(), Some("%1".into())), 2);
        assert_eq!(state.cursor(), 2);
        assert_eq!(
            command,
            Some(Command::Select {
                window_id: "@3".into(),
                pane_id: "%3".into()
            })
        );
    }

    #[test]
    fn clicking_a_project_row_only_moves_the_cursor() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('j'));
        let (state, command) = click(state, 3);
        assert_eq!(state.cursor(), 3);
        assert_eq!(command, None);
    }

    #[test]
    fn clicking_below_the_rows_does_nothing() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('j'));
        let (state, command) = click(state, 9);
        assert_eq!(state.cursor(), 1);
        assert_eq!(command, None);
    }

    #[test]
    fn the_wheel_moves_the_cursor() {
        let state = State::new(tree(), Some("%1".into()));
        let (state, _) = reduce(state, Input::Mouse(Mouse::ScrollDown));
        let (state, _) = reduce(state, Input::Mouse(Mouse::ScrollDown));
        assert_eq!(state.cursor(), 2);
        let (state, _) = reduce(state, Input::Mouse(Mouse::ScrollUp));
        assert_eq!(state.cursor(), 1);
    }

    #[test]
    fn a_click_closes_help_and_cancels_a_pending_kill() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('?'));
        let (state, command) = click(state, 1);
        assert_eq!((state.mode(), command), (Mode::Normal, None));
        let state = press(
            press(State::new(tree(), Some("%1".into())), KeyCode::Char('j')),
            KeyCode::Char('x'),
        );
        assert_eq!(state.mode(), Mode::ConfirmKill);
        let (state, command) = click(state, 1);
        assert_eq!((state.mode(), command), (Mode::Normal, None));
    }

    #[test]
    fn x_asks_before_killing_and_y_confirms() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('j'));
        let (state, command) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::ConfirmKill);
        let (state, command) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)),
        );
        assert_eq!(
            command,
            Some(Command::Kill {
                window_id: "@2".into()
            })
        );
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn any_other_key_cancels_the_kill() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('j'));
        let state = press(state, KeyCode::Char('x'));
        for code in [KeyCode::Char('n'), KeyCode::Esc, KeyCode::Char('j')] {
            let (after, command) = reduce(
                state.clone(),
                Input::Key(KeyEvent::new(code, KeyModifiers::NONE)),
            );
            assert_eq!(command, None);
            assert_eq!(after.mode(), Mode::Normal);
            assert_eq!(after.cursor(), 1, "cancelling does not move the cursor");
        }
    }

    #[test]
    fn x_on_a_project_does_nothing() {
        let (state, command) = reduce(
            State::new(tree(), Some("%1".into())),
            Input::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn r_respawns_only_dead_agents() {
        let mut dead = tree();
        dead.projects[0].agents[1].dead = Some(crate::tmux::Death::Exit(3));
        let state = press(
            press(State::new(dead, Some("%1".into())), KeyCode::Char('j')),
            KeyCode::Char('j'),
        );
        let (_, command) = reduce(
            state.clone(),
            Input::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        );
        assert_eq!(
            command,
            Some(Command::Respawn {
                pane_id: "%3".into()
            })
        );
        let alive = press(state, KeyCode::Char('k'));
        let (alive, command) = reduce(
            alive,
            Input::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        );
        assert_eq!(command, None);
        assert_eq!(alive.error(), Some("claude is still running"));
    }

    fn project_row(name: &str) -> ProjectRow {
        ProjectRow {
            name: name.into(),
            path: PathBuf::from(format!("/p/{name}")),
            registered: true,
        }
    }

    #[test]
    fn a_vanished_row_leaves_the_cursor_clamped_in_place() {
        let state = press(
            press(State::new(tree(), Some("%1".into())), KeyCode::Char('j')),
            KeyCode::Char('j'),
        );
        let (state, _) = reduce(
            state,
            Input::Snapshot(Tree {
                projects: vec![project("project", vec![agent("project", "claude", "%2")])],
            }),
        );
        assert_eq!(state.cursor(), 1);
    }
}

#[cfg(test)]
mod finder_tests {
    use super::*;
    use crate::agents::{Agent, ProjectEntry};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn entry(name: &str, registered: bool) -> ProjectEntry {
        ProjectEntry {
            name: name.into(),
            path: PathBuf::from(format!("/p/{name}")),
            registered,
            agents: vec![],
        }
    }

    fn tree(names: &[&str]) -> Tree {
        Tree {
            projects: names.iter().map(|n| entry(n, true)).collect(),
        }
    }

    fn press(state: State, code: KeyCode) -> (State, Option<Command>) {
        reduce(state, Input::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn typed(state: State, text: &str) -> State {
        text.chars()
            .fold(state, |state, c| press(state, KeyCode::Char(c)).0)
    }

    fn finder(names: &[&str]) -> State {
        press(
            State::new(tree(names), Some("%1".into())),
            KeyCode::Char('/'),
        )
        .0
    }

    fn found(state: &State) -> Vec<&str> {
        state.found().iter().map(|p| p.name.as_str()).collect()
    }

    fn project_row(name: &str) -> ProjectRow {
        ProjectRow {
            name: name.into(),
            path: PathBuf::from(format!("/p/{name}")),
            registered: true,
        }
    }

    #[test]
    fn slash_opens_the_finder_on_every_registered_project_in_registry_order() {
        let state = finder(&["proper", "znaiko", "todor.dev"]);
        assert_eq!(state.mode(), Mode::Find);
        assert_eq!(state.query(), "");
        assert_eq!(found(&state), ["proper", "znaiko", "todor.dev"]);
        assert_eq!(state.pick(), 0);
    }

    #[test]
    fn typing_keeps_the_projects_whose_names_hold_the_letters_in_order() {
        let state = finder(&["proper", "znaiko", "todor.dev", "api-router"]);
        let state = typed(state, "pr");
        assert_eq!(state.query(), "pr");
        assert_eq!(found(&state), ["proper", "api-router"]);
        assert_eq!(found(&typed(state, "p")), ["proper"]);
        let state = typed(finder(&["proper", "znaiko", "todor.dev"]), "tdv");
        assert_eq!(found(&state), ["todor.dev"]);
    }

    #[test]
    fn letters_that_move_or_act_elsewhere_are_just_typed_in_the_finder() {
        let state = typed(finder(&["znaiko", "jk"]), "jk");
        assert_eq!(state.mode(), Mode::Find);
        assert_eq!(state.query(), "jk");
        assert_eq!(found(&state), ["jk"]);
        let (state, command) = press(typed(finder(&["proper"]), "nsxrq?"), KeyCode::Char('/'));
        assert_eq!(command, None);
        assert_eq!(state.query(), "nsxrq?/");
    }

    #[test]
    fn capitals_match_the_lowercase_names() {
        assert_eq!(
            found(&typed(finder(&["proper", "znaiko"]), "ZK")),
            ["znaiko"]
        );
    }

    #[test]
    fn backspace_takes_back_the_last_letter() {
        let state = typed(finder(&["proper", "znaiko"]), "zk");
        let (state, _) = press(state, KeyCode::Backspace);
        assert_eq!(state.query(), "z");
        let (state, _) = press(state, KeyCode::Backspace);
        let (state, _) = press(state, KeyCode::Backspace);
        assert_eq!(state.query(), "");
        assert_eq!(state.mode(), Mode::Find);
        assert_eq!(found(&state), ["proper", "znaiko"]);
    }

    #[test]
    fn down_and_up_move_the_pick_within_the_matches_and_typing_resets_it() {
        let state = finder(&["proper", "znaiko", "todor.dev"]);
        let (state, _) = press(state, KeyCode::Up);
        assert_eq!(state.pick(), 0);
        let (state, _) = press(state, KeyCode::Down);
        let (state, _) = press(state, KeyCode::Down);
        let (state, _) = press(state, KeyCode::Down);
        assert_eq!(state.pick(), 2);
        let (state, _) = press(state, KeyCode::Up);
        assert_eq!(state.pick(), 1);
        assert_eq!(typed(state, "o").pick(), 0);
    }

    #[test]
    fn enter_starts_claude_in_the_picked_project_and_closes_the_finder() {
        let state = typed(finder(&["proper", "znaiko", "todor.dev"]), "o");
        let (state, _) = press(state, KeyCode::Down);
        let (state, command) = press(state, KeyCode::Enter);
        assert_eq!(
            command,
            Some(Command::New {
                project: project_row("znaiko"),
                program: Program::Claude
            })
        );
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn enter_with_nothing_matching_does_nothing() {
        let state = typed(finder(&["proper"]), "zzz");
        assert!(state.found().is_empty());
        let (state, command) = press(state, KeyCode::Enter);
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Find);
    }

    #[test]
    fn esc_closes_the_finder_and_reopening_starts_afresh() {
        let state = typed(finder(&["proper", "znaiko"]), "zk");
        let (state, command) = press(state, KeyCode::Esc);
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Normal);
        let (state, _) = press(state, KeyCode::Char('/'));
        assert_eq!(state.query(), "");
        assert_eq!(found(&state), ["proper", "znaiko"]);
    }

    #[test]
    fn a_click_closes_the_finder() {
        let (state, command) = reduce(finder(&["proper"]), Input::Mouse(Mouse::Click { row: 0 }));
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn the_finder_leaves_out_projects_that_already_have_agents() {
        let busy = |name: &str| ProjectEntry {
            agents: vec![Agent {
                name: "claude".into(),
                project: name.into(),
                pane_id: "%2".into(),
                window_id: "@2".into(),
                current: false,
                dead: None,
                command: "claude".into(),
                status: None,
            }],
            ..entry(name, true)
        };
        let tree = Tree {
            projects: vec![busy("proper"), entry("znaiko", true)],
        };
        let (state, _) = press(State::new(tree, Some("%1".into())), KeyCode::Char('/'));
        assert_eq!(found(&state), ["znaiko"]);
        let (state, _) = reduce(
            state,
            Input::Snapshot(Tree {
                projects: vec![entry("proper", true), busy("znaiko")],
            }),
        );
        assert_eq!(found(&state), ["proper"]);
    }

    #[test]
    fn the_finder_leaves_out_projects_no_longer_registered() {
        let tree = Tree {
            projects: vec![entry("proper", true), entry("gone", false)],
        };
        let (state, _) = press(State::new(tree, Some("%1".into())), KeyCode::Char('/'));
        assert_eq!(found(&state), ["proper"]);
    }

    #[test]
    fn a_snapshot_refreshes_the_matches_and_keeps_the_pick_on_one() {
        let state = finder(&["proper", "znaiko", "todor.dev"]);
        let (state, _) = press(state, KeyCode::Down);
        let (state, _) = press(state, KeyCode::Down);
        let (state, _) = reduce(state, Input::Snapshot(tree(&["proper", "zeta"])));
        assert_eq!(state.mode(), Mode::Find);
        assert_eq!(found(&state), ["proper", "zeta"]);
        assert_eq!(state.pick(), 1);
    }
}

#[cfg(test)]
mod worktree_tests {
    use super::*;
    use crate::agents::{Agent, ProjectEntry};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn entry(name: &str, agents: Vec<Agent>) -> ProjectEntry {
        ProjectEntry {
            name: name.into(),
            path: PathBuf::from(format!("/p/{name}")),
            registered: true,
            agents,
        }
    }

    /// `project` runs one claude; `znaiko` and `todor.dev` are idle.
    fn tree() -> Tree {
        Tree {
            projects: vec![
                entry(
                    "project",
                    vec![Agent {
                        name: "claude".into(),
                        project: "project".into(),
                        pane_id: "%2".into(),
                        window_id: "@2".into(),
                        current: false,
                        dead: None,
                        command: "claude".into(),
                        status: None,
                    }],
                ),
                entry("znaiko", vec![]),
                entry("todor.dev", vec![]),
            ],
        }
    }

    fn press(state: State, code: KeyCode) -> (State, Option<Command>) {
        reduce(state, Input::Key(KeyEvent::new(code, KeyModifiers::NONE)))
    }

    fn typed(state: State, text: &str) -> State {
        text.chars()
            .fold(state, |state, c| press(state, KeyCode::Char(c)).0)
    }

    /// The prompt opened with `w` on the agent row of `project`.
    fn prompt() -> State {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('j')).0;
        press(state, KeyCode::Char('w')).0
    }

    fn project_row(name: &str) -> ProjectRow {
        ProjectRow {
            name: name.into(),
            path: PathBuf::from(format!("/p/{name}")),
            registered: true,
        }
    }

    #[test]
    fn w_asks_for_a_worktree_name_for_the_project_under_the_cursor() {
        let state = prompt();
        assert_eq!(state.mode(), Mode::Worktree);
        assert_eq!(state.worktree_project(), Some(&project_row("project")));
        assert_eq!(state.worktree_name(), "");
    }

    #[test]
    fn w_with_no_rows_does_nothing() {
        let (state, command) = press(
            State::new(Tree::default(), Some("%1".into())),
            KeyCode::Char('w'),
        );
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn typing_names_the_worktree_in_lowercase_and_backspace_takes_back_a_letter() {
        let state = typed(prompt(), "AUTH-fixx");
        let (state, _) = press(state, KeyCode::Backspace);
        assert_eq!(state.worktree_name(), "auth-fix");
        let state = typed(prompt(), "jkqxnsrw/?");
        assert_eq!(state.mode(), Mode::Worktree);
        assert_eq!(state.worktree_name(), "jkqxnsrw/?");
    }

    #[test]
    fn enter_starts_claude_in_a_worktree_of_that_project_and_closes_the_prompt() {
        let (state, command) = press(typed(prompt(), "auth-fix"), KeyCode::Enter);
        assert_eq!(
            command,
            Some(Command::New {
                project: project_row("project"),
                program: Program::Worktree("auth-fix".into())
            })
        );
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn enter_without_a_name_stays_in_the_prompt_and_says_so() {
        let (state, command) = press(prompt(), KeyCode::Enter);
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Worktree);
        assert_eq!(state.error(), Some("name the worktree"));
    }

    #[test]
    fn esc_cancels_and_reopening_starts_without_a_name() {
        let (state, command) = press(typed(prompt(), "auth"), KeyCode::Esc);
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Normal);
        let (state, _) = press(state, KeyCode::Char('w'));
        assert_eq!(state.mode(), Mode::Worktree);
        assert_eq!(state.worktree_name(), "");
    }

    #[test]
    fn a_click_closes_the_prompt() {
        assert_eq!(prompt().mode(), Mode::Worktree);
        let (state, command) = reduce(prompt(), Input::Mouse(Mouse::Click { row: 0 }));
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Normal);
    }

    #[test]
    fn tab_in_the_finder_asks_for_a_worktree_name_for_the_picked_project() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('/')).0;
        let (state, _) = press(state, KeyCode::Down);
        let (state, command) = press(state, KeyCode::Tab);
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Worktree);
        assert_eq!(state.worktree_project(), Some(&project_row("todor.dev")));
        assert_eq!(state.worktree_name(), "");
        let (_, command) = press(typed(state, "auth"), KeyCode::Enter);
        assert_eq!(
            command,
            Some(Command::New {
                project: project_row("todor.dev"),
                program: Program::Worktree("auth".into())
            })
        );
    }

    #[test]
    fn tab_in_the_finder_with_nothing_matching_does_nothing() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('/')).0;
        let (state, command) = press(typed(state, "zzz"), KeyCode::Tab);
        assert_eq!(command, None);
        assert_eq!(state.mode(), Mode::Find);
    }

    #[test]
    fn the_prompt_keeps_its_project_when_a_snapshot_takes_it_out_of_the_finder() {
        let state = press(State::new(tree(), Some("%1".into())), KeyCode::Char('/')).0;
        let (state, _) = press(state, KeyCode::Tab);
        let (state, _) = reduce(state, Input::Snapshot(Tree::default()));
        assert_eq!(state.mode(), Mode::Worktree);
        assert_eq!(state.worktree_project(), Some(&project_row("znaiko")));
    }
}
