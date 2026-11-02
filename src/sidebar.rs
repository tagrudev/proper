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
}

#[derive(Debug)]
pub enum Input {
    Key(KeyEvent),
    Snapshot(Tree),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Claude,
    Shell,
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
}

impl State {
    /// `own_pane` is the sidebar's own tmux pane id; `None` means running outside tmux.
    pub fn new(tree: Tree, own_pane: Option<String>) -> State {
        let rows = rows(tree);
        let cursor = current_agent(&rows).unwrap_or(0);
        State {
            rows,
            cursor,
            mode: Mode::Normal,
            error: None,
            own_pane,
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
        let last = self.rows.len().saturating_sub(1) as isize;
        let cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
        State { cursor, ..self }
    }
}

fn rows(tree: Tree) -> Vec<Row> {
    tree.projects
        .into_iter()
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
            (
                State {
                    rows,
                    cursor,
                    ..state
                },
                None,
            )
        }
        Input::Key(key) => {
            let state = State {
                error: None,
                ..state
            };
            match state.mode {
                Mode::Help => (help_key(state, key), None),
                Mode::ConfirmKill => confirm_kill_key(state, key),
                Mode::Normal => normal_key(state, key),
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
    use crate::agents::ProjectEntry;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::path::PathBuf;

    fn tree(names: &[&str]) -> Tree {
        Tree {
            projects: names
                .iter()
                .map(|n| ProjectEntry {
                    name: n.to_string(),
                    path: PathBuf::from(format!("/p/{n}")),
                    registered: true,
                    agents: vec![],
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
    fn starts_with_one_row_per_project_and_the_cursor_on_the_first() {
        let state = inside_tmux();
        assert_eq!(state.rows().len(), 3);
        assert_eq!(state.cursor(), 0);
        assert!(matches!(state.rows()[0], Row::Project(ref p) if p.name == "project"));
    }

    #[test]
    fn j_and_k_move_the_cursor_and_clamp_at_both_ends() {
        let (state, _) = press(inside_tmux(), KeyCode::Char('k'));
        assert_eq!(state.cursor(), 0);
        let (state, _) = press(state, KeyCode::Char('j'));
        let (state, _) = press(state, KeyCode::Down);
        assert_eq!(state.cursor(), 2);
        let (state, _) = press(state, KeyCode::Char('j'));
        assert_eq!(state.cursor(), 2);
        let (state, _) = press(state, KeyCode::Up);
        assert_eq!(state.cursor(), 1);
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
        assert_eq!(state.rows().len(), 1);
        assert_eq!(state.cursor(), 0);
    }

    #[test]
    fn an_empty_tree_has_no_rows_and_survives_navigation() {
        let state = State::new(tree(&[]), None);
        let (state, _) = press(state, KeyCode::Char('j'));
        assert_eq!(state.cursor(), 0);
        assert!(state.rows().is_empty());
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
                project("znaiko", vec![]),
            ],
        }
    }

    #[test]
    fn agents_are_rows_right_under_their_project() {
        let state = State::new(tree(), Some("%1".into()));
        let rows = state.rows();
        assert_eq!(rows.len(), 4);
        assert!(matches!(&rows[1], Row::Agent(a) if a.name == "claude"));
        assert!(matches!(&rows[2], Row::Agent(a) if a.name == "zsh"));
        assert!(matches!(&rows[3], Row::Project(p) if p.name == "znaiko"));
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
                projects: vec![project("znaiko", vec![]), project("project", vec![])],
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
                projects: vec![project("project", vec![])],
            }),
        );
        assert_eq!(state.cursor(), 0);
    }
}
