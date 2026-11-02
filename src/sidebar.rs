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
}

#[derive(Debug)]
pub enum Input {
    Key(KeyEvent),
    Snapshot(Tree),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
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
        State {
            rows: rows(tree),
            cursor: 0,
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

pub fn reduce(state: State, input: Input) -> (State, Option<Command>) {
    match input {
        Input::Snapshot(tree) => {
            let rows = rows(tree);
            let cursor = state.cursor.min(rows.len().saturating_sub(1));
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

fn normal_key(state: State, key: KeyEvent) -> (State, Option<Command>) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => (state.moved(1), None),
        KeyCode::Char('k') | KeyCode::Up => (state.moved(-1), None),
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
