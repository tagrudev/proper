//! Draws the sidebar from its state. No tmux here.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::sidebar::{Mode, Row, State};

const AGENT_NAME_WIDTH: usize = 11;

const HELP: &[&str] = &[
    " j/k   move",
    " Enter open agent",
    " n     new claude",
    " s     new shell",
    " x     kill agent",
    " r     respawn dead",
    " q     detach",
    " ?     toggle help",
    " Esc   close help",
];

pub fn render(frame: &mut Frame, state: &State) {
    let [title, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    frame.render_widget(Paragraph::new(Line::from(" proper").bold()), title);
    frame.render_widget(Paragraph::new(body_lines(state, body)), body);
    frame.render_widget(Paragraph::new(footer_line(state)), footer);
}

fn body_lines(state: &State, area: Rect) -> Vec<Line<'static>> {
    match state.mode() {
        Mode::Help => HELP.iter().map(|h| Line::from(*h)).collect(),
        Mode::Normal if state.rows().is_empty() => {
            vec![
                Line::from(" no projects yet").dim(),
                Line::from(" run: proper add <dir>").dim(),
            ]
        }
        Mode::Normal => state
            .rows()
            .iter()
            .enumerate()
            .map(|(index, row)| row_line(row, index == state.cursor(), area.width))
            .collect(),
    }
}

fn row_line(row: &Row, selected: bool, width: u16) -> Line<'static> {
    let (text, style) = match row {
        Row::Project(project) if project.registered => {
            (format!(" ▾ {}", project.name), Style::new())
        }
        Row::Project(project) => (format!(" ▾ {}*", project.name), Style::new().dim()),
        Row::Agent(agent) => {
            let marker = if agent.current { "▶" } else { " " };
            let (glyph, detail) = match agent.dead {
                Some(status) => ("✗", format!("exit {status}")),
                None => ("●", agent.command.clone()),
            };
            (
                format!(
                    " {marker} {glyph} {:<AGENT_NAME_WIDTH$} {detail}",
                    agent.name
                ),
                Style::new(),
            )
        }
    };
    let padded = format!("{text:<width$}", width = width as usize);
    let style = if selected {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style
    };
    Line::styled(padded, style)
}

fn footer_line(state: &State) -> Line<'static> {
    match state.error() {
        Some(error) => Line::from(format!(" {error}")).red(),
        None if state.in_tmux() => Line::from(" ? help  q detach").dim(),
        None => Line::from(" ? help  q quit").dim(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{ProjectEntry, Tree};
    use crate::sidebar::{Input, State, reduce};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
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

    fn lines(state: &State) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal.draw(|frame| render(frame, state)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn lists_projects_under_the_title_with_a_key_hint_footer() {
        let state = State::new(tree(&["project", "znaiko"]), Some("%1".into()));
        let out = lines(&state);
        assert_eq!(out[0], " proper");
        assert_eq!(out[1], " ▾ project");
        assert_eq!(out[2], " ▾ znaiko");
        assert_eq!(out[11], " ? help  q detach");
    }

    #[test]
    fn footer_says_quit_outside_tmux() {
        let state = State::new(tree(&["project"]), None);
        assert_eq!(lines(&state)[11], " ? help  q quit");
    }

    #[test]
    fn the_cursor_row_is_highlighted() {
        let state = State::new(tree(&["project", "znaiko"]), Some("%1".into()));
        let (state, _) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
        );
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal.draw(|frame| render(frame, &state)).unwrap();
        let buffer = terminal.backend().buffer();
        assert!(
            buffer[(3, 2)].modifier.contains(Modifier::REVERSED),
            "znaiko row"
        );
        assert!(
            !buffer[(3, 1)].modifier.contains(Modifier::REVERSED),
            "project row"
        );
    }

    #[test]
    fn help_overlay_lists_the_keys() {
        let state = State::new(tree(&["project"]), Some("%1".into()));
        let (state, _) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)),
        );
        let text = lines(&state).join("\n");
        for key in ["j/k", "Enter", "n ", "s ", "x ", "r ", "q ", "Esc"] {
            assert!(text.contains(key), "missing {key:?} in\n{text}");
        }
    }

    #[test]
    fn errors_replace_the_footer() {
        let state = State::new(tree(&["project"]), None).with_error("boom");
        assert_eq!(lines(&state)[11], " boom");
    }

    #[test]
    fn empty_registry_explains_how_to_add_a_project() {
        let state = State::new(tree(&[]), None);
        assert!(lines(&state).join("\n").contains("proper add"));
    }
}

#[cfg(test)]
mod agent_tests {
    use super::*;
    use crate::agents::{Agent, ProjectEntry, Tree};
    use crate::sidebar::State;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    fn agent(name: &str, current: bool, dead: Option<i32>) -> Agent {
        Agent {
            name: name.into(),
            project: "project".into(),
            pane_id: "%2".into(),
            window_id: "@1".into(),
            current,
            dead,
            command: "node".into(),
        }
    }

    fn lines(state: &State) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
        terminal.draw(|frame| render(frame, state)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn agent_rows_show_state_glyph_name_and_command_or_exit_status() {
        let tree = Tree {
            projects: vec![
                ProjectEntry {
                    name: "project".into(),
                    path: PathBuf::from("/p/project"),
                    registered: true,
                    agents: vec![agent("claude", true, None), agent("zsh", false, Some(3))],
                },
                ProjectEntry {
                    name: "old".into(),
                    path: PathBuf::from("/p/old"),
                    registered: false,
                    agents: vec![agent("claude", false, None)],
                },
            ],
        };
        let out = lines(&State::new(tree, Some("%1".into())));
        assert_eq!(out[1], " ▾ project");
        assert_eq!(out[2], " ▶ ● claude      node");
        assert_eq!(out[3], "   ✗ zsh         exit 3");
        assert_eq!(out[4], " ▾ old*");
        assert_eq!(out[5], "   ● claude      node");
    }
}
