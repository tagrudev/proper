//! Draws the sidebar from its state. No tmux here.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::sidebar::{Mode, Row, State};
use crate::tmux::Status;

/// Background of the row under the cursor, a soft grey that lifts it off a dark theme.
const CURSOR: Color = Color::Indexed(237);

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_FRAME: Duration = Duration::from_millis(100);

const HELP: &[&str] = &[
    " j/k   move",
    " Enter open agent",
    " n     new claude",
    " s     new shell",
    " w     new worktree",
    " /     find project",
    " x     kill agent",
    " r     respawn dead",
    " q     detach",
    " ?     toggle help",
    " Esc   close help",
];

/// `elapsed` is how long the sidebar has been running; it turns the spinner of working agents.
pub fn render(frame: &mut Frame, state: &State, elapsed: Duration) {
    let [body, footer] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

    frame.render_widget(Paragraph::new(body_lines(state, body, elapsed)), body);
    frame.render_widget(Paragraph::new(footer_line(state)), footer);
}

fn body_lines(state: &State, area: Rect, elapsed: Duration) -> Vec<Line<'static>> {
    match state.mode() {
        Mode::Help => HELP.iter().map(|h| Line::from(*h)).collect(),
        Mode::Worktree => {
            let project = state
                .worktree_project()
                .map(|project| project.name.clone())
                .unwrap_or_default();
            vec![
                Line::from(vec![" worktree in ".dim(), project.into()]),
                Line::from(vec![
                    " name: ".dim(),
                    state.worktree_name().to_string().into(),
                ]),
            ]
        }
        Mode::Find => {
            let query = Line::from(vec![" /".dim(), state.query().to_string().into()]);
            let found = state.found();
            if found.is_empty() {
                return vec![query, Line::from(" no match").dim()];
            }
            let matches = found.into_iter().enumerate().map(|(index, project)| {
                let text = format!(" {}", project.name);
                let line = Line::from(format!("{text:<width$}", width = area.width as usize));
                if index == state.pick() {
                    line.bg(CURSOR)
                } else {
                    line
                }
            });
            std::iter::once(query).chain(matches).collect()
        }
        Mode::Normal | Mode::ConfirmKill if state.rows().is_empty() && state.idle().is_empty() => {
            vec![
                Line::from(" no projects yet").dim(),
                Line::from(" run: proper add <dir>").dim(),
            ]
        }
        Mode::Normal | Mode::ConfirmKill if state.rows().is_empty() => {
            vec![
                Line::from(" nothing running").dim(),
                Line::from(" / finds a project").dim(),
            ]
        }
        Mode::Normal | Mode::ConfirmKill => {
            let rows = state.rows();
            rows.iter()
                .enumerate()
                .map(|(index, row)| {
                    let next_is_agent = matches!(rows.get(index + 1), Some(Row::Agent(_)));
                    let line = row_line(row, next_is_agent, area.width, elapsed);
                    if index == state.cursor() && state.focused() {
                        line.bg(CURSOR)
                    } else {
                        line
                    }
                })
                .collect()
        }
    }
}

/// `next_is_agent` tells an agent that more follow it.
/// Rows are padded to `width` so the cursor background spans the pane.
fn row_line(row: &Row, next_is_agent: bool, width: u16, elapsed: Duration) -> Line<'static> {
    match row {
        Row::Project(project) => {
            let (mark, style) = if project.registered {
                ("", Style::new().bold())
            } else {
                ("*", Style::new().dim())
            };
            let text = format!(" {}{mark}", project.name);
            Line::styled(format!("{text:<width$}", width = width as usize), style)
        }
        Row::Agent(agent) => {
            let guide = if next_is_agent { "├" } else { "└" };
            let command = if agent.command == agent.name {
                Span::default()
            } else {
                agent.command.clone().dim()
            };
            let (glyph, detail) = match (&agent.dead, agent.status) {
                (Some(death), _) => ("✗".red(), death.to_string().red()),
                (None, Some(Status::Working)) => {
                    let frame = elapsed.as_millis() / SPINNER_FRAME.as_millis();
                    (SPINNER[frame as usize % SPINNER.len()].green(), command)
                }
                (None, Some(Status::Waiting)) => ("◌".yellow(), command),
                (None, Some(Status::Attention)) => ("!".red().bold(), command),
                (None, None) => ("●".dim(), command),
            };
            let marker = if agent.current {
                "◂".blue()
            } else {
                " ".into()
            };
            let name = [
                " ".into(),
                guide.dim(),
                " ".into(),
                glyph,
                " ".into(),
                agent.name.clone().into(),
            ];
            let tail = [" ".into(), detail, " ".into(), marker, " ".into()];
            let used: usize = name.iter().chain(&tail).map(Span::width).sum();
            let fill = Span::raw(" ".repeat((width as usize).saturating_sub(used)));
            Line::from_iter(name.into_iter().chain([fill]).chain(tail))
        }
    }
}

fn footer_line(state: &State) -> Line<'static> {
    if state.mode() == Mode::ConfirmKill
        && let Some(agent) = state.agent_at_cursor()
    {
        return Line::from(format!(" kill {}/{}? y/n", agent.project, agent.name)).yellow();
    }
    match state.error() {
        Some(error) => Line::from(format!(" {error}")).red(),
        None if state.mode() == Mode::Find => Line::from(vec![
            " enter".into(),
            " claude  ".dim(),
            "tab".into(),
            " worktree".dim(),
        ]),
        None if state.mode() == Mode::Worktree => Line::from(vec![
            " enter".into(),
            " start  ".dim(),
            "esc".into(),
            " cancel".dim(),
        ]),
        None => {
            let leave = if state.in_tmux() { " detach" } else { " quit" };
            Line::from(vec![
                " ?".into(),
                " help  ".dim(),
                "/".into(),
                " find  ".dim(),
                "q".into(),
                leave.dim(),
            ])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{Agent, ProjectEntry, Tree};
    use crate::sidebar::{Input, State, reduce};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::style::Modifier;
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

    fn lines(state: &State) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal
            .draw(|frame| render(frame, state, Duration::ZERO))
            .unwrap();
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
    fn lists_projects_from_the_first_row_with_a_key_hint_footer() {
        let state = State::new(tree(&["project", "znaiko"]), Some("%1".into()));
        let out = lines(&state);
        assert_eq!(out[0], " project");
        assert_eq!(out[1], " └ ● claude");
        assert_eq!(out[2], " znaiko");
        assert_eq!(out[3], " └ ● claude");
        assert_eq!(out[10], "");
        assert_eq!(out[11], " ? help  / find  q detach");
    }

    #[test]
    fn footer_says_quit_outside_tmux() {
        let state = State::new(tree(&["project"]), None);
        assert_eq!(lines(&state)[11], " ? help  / find  q quit");
    }

    #[test]
    fn the_cursor_row_has_a_soft_background_across_the_whole_width() {
        let j = || Input::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        let state = State::new(tree(&["project", "znaiko"]), Some("%1".into()));
        let (state, _) = reduce(state, j());
        let (state, _) = reduce(state, j());
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal
            .draw(|frame| render(frame, &state, Duration::ZERO))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(3, 2)].bg, CURSOR, "znaiko row");
        assert_eq!(buffer[(29, 2)].bg, CURSOR, "znaiko row, right edge");
        assert_ne!(buffer[(3, 0)].bg, CURSOR, "project row");
        assert!(!buffer[(3, 2)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn the_cursor_row_is_plain_while_the_sidebar_is_not_focused() {
        let state = State::new(tree(&["project", "znaiko"]), Some("%1".into()));
        let (state, _) = reduce(state, Input::Focus(false));
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal
            .draw(|frame| render(frame, &state, Duration::ZERO))
            .unwrap();
        assert_ne!(terminal.backend().buffer()[(3, 0)].bg, CURSOR);
    }

    #[test]
    fn help_overlay_lists_the_keys() {
        let state = State::new(tree(&["project"]), Some("%1".into()));
        let (state, _) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)),
        );
        let text = lines(&state).join("\n");
        for key in [
            "j/k", "Enter", "n ", "s ", " w ", "/ ", "x ", "r ", "q ", "Esc",
        ] {
            assert!(text.contains(key), "missing {key:?} in\n{text}");
        }
    }

    /// The finder opened on idle projects named `names`, with `text` typed.
    fn find(names: &[&str], text: &str) -> State {
        let key = |c| Input::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        let idle = Tree {
            projects: tree(names)
                .projects
                .into_iter()
                .map(|project| ProjectEntry {
                    agents: vec![],
                    ..project
                })
                .collect(),
        };
        let state = State::new(idle, Some("%1".into()));
        let (state, _) = reduce(state, key('/'));
        text.chars().fold(state, |state, c| reduce(state, key(c)).0)
    }

    #[test]
    fn the_finder_shows_the_query_then_the_matches_and_how_to_use_them() {
        let out = lines(&find(&["proper", "znaiko", "api"], "o"));
        assert_eq!(out[0], " /o");
        assert_eq!(out[1], " proper");
        assert_eq!(out[2], " znaiko");
        assert_eq!(out[3], "");
        assert_eq!(out[11], " enter claude  tab worktree");
    }

    /// The worktree prompt opened on `project` with `keys` pressed.
    fn worktree(keys: &[KeyCode]) -> State {
        let key = |code| Input::Key(KeyEvent::new(code, KeyModifiers::NONE));
        let state = State::new(tree(&["project"]), Some("%1".into()));
        let (state, _) = reduce(state, key(KeyCode::Char('w')));
        keys.iter()
            .fold(state, |state, code| reduce(state, key(*code)).0)
    }

    #[test]
    fn the_worktree_prompt_names_the_project_then_the_name_typed_so_far() {
        let out = lines(&worktree(&[KeyCode::Char('a'), KeyCode::Char('b')]));
        assert_eq!(out[0], " worktree in project");
        assert_eq!(out[1], " name: ab");
        assert_eq!(out[2], "");
        assert_eq!(out[11], " enter start  esc cancel");
    }

    #[test]
    fn the_worktree_prompt_says_when_it_needs_a_name() {
        let out = lines(&worktree(&[KeyCode::Enter]));
        assert_eq!(out[0], " worktree in project");
        assert_eq!(out[11], " name the worktree");
    }

    #[test]
    fn the_pick_has_the_cursor_background_across_the_whole_width() {
        let state = find(&["proper", "znaiko"], "");
        let (state, _) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
        );
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal
            .draw(|frame| render(frame, &state, Duration::ZERO))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(3, 2)].bg, CURSOR, "znaiko row");
        assert_eq!(buffer[(29, 2)].bg, CURSOR, "znaiko row, right edge");
        assert_ne!(buffer[(3, 1)].bg, CURSOR, "proper row");
        assert_ne!(buffer[(3, 0)].bg, CURSOR, "query row");
    }

    #[test]
    fn the_finder_says_when_nothing_matches() {
        let out = lines(&find(&["proper"], "zzz"));
        assert_eq!(out[0], " /zzz");
        assert_eq!(out[1], " no match");
    }

    #[test]
    fn errors_replace_the_footer() {
        let state = State::new(tree(&["project"]), None).with_error("boom");
        assert_eq!(lines(&state)[11], " boom");
    }

    #[test]
    fn with_nothing_running_the_sidebar_points_at_the_finder() {
        let tree = Tree {
            projects: vec![ProjectEntry {
                name: "project".into(),
                path: PathBuf::from("/p/project"),
                registered: true,
                agents: vec![],
            }],
        };
        let out = lines(&State::new(tree, Some("%1".into())));
        assert_eq!(out[0], " nothing running");
        assert_eq!(out[1], " / finds a project");
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
    use crate::tmux::{Death, Status};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Modifier;
    use std::path::PathBuf;

    fn agent(name: &str, current: bool, dead: Option<Death>) -> Agent {
        Agent {
            name: name.into(),
            project: "project".into(),
            pane_id: "%2".into(),
            window_id: "@1".into(),
            current,
            dead,
            command: "node".into(),
            status: None,
        }
    }

    fn buffer(state: &State, elapsed: Duration) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
        terminal
            .draw(|frame| render(frame, state, elapsed))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn lines(state: &State) -> Vec<String> {
        let buffer = buffer(state, Duration::ZERO);
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

    fn one_project(agents: Vec<Agent>) -> State {
        let tree = Tree {
            projects: vec![ProjectEntry {
                name: "project".into(),
                path: PathBuf::from("/p/project"),
                registered: true,
                agents,
            }],
        };
        State::new(tree, Some("%1".into()))
    }

    fn with(status: Option<Status>) -> Agent {
        Agent {
            status,
            ..agent("claude", false, None)
        }
    }

    #[test]
    fn agent_rows_show_working_waiting_and_attention_glyphs() {
        let out = lines(&one_project(vec![
            with(Some(Status::Working)),
            with(Some(Status::Waiting)),
            with(Some(Status::Attention)),
            with(None),
        ]));
        assert_eq!(out[1], " ├ ⠋ claude            node");
        assert_eq!(out[2], " ├ ◌ claude            node");
        assert_eq!(out[3], " ├ ! claude            node");
        assert_eq!(out[4], " └ ● claude            node");
    }

    #[test]
    fn state_glyphs_are_coloured_by_how_much_they_need_you() {
        let state = one_project(vec![
            with(Some(Status::Working)),
            with(Some(Status::Waiting)),
            with(Some(Status::Attention)),
            with(None),
            agent("zsh", false, Some(Death::Exit(3))),
        ]);
        let buffer = buffer(&state, Duration::ZERO);
        assert_eq!(buffer[(3, 1)].fg, Color::Green, "working");
        assert_eq!(buffer[(3, 2)].fg, Color::Yellow, "waiting");
        assert_eq!(buffer[(3, 3)].fg, Color::Red, "attention");
        assert!(
            buffer[(3, 3)].modifier.contains(Modifier::BOLD),
            "attention"
        );
        assert!(buffer[(3, 4)].modifier.contains(Modifier::DIM), "no state");
        assert_eq!(buffer[(3, 5)].fg, Color::Red, "dead");
        assert_eq!(buffer[(24, 5)].fg, Color::Red, "exit status");
        assert!(
            buffer[(1, 1)].modifier.contains(Modifier::DIM),
            "tree guide"
        );
    }

    #[test]
    fn a_working_agent_spins() {
        let state = one_project(vec![with(Some(Status::Working))]);
        let glyph = |millis| {
            buffer(&state, Duration::from_millis(millis))[(3, 1)]
                .symbol()
                .to_string()
        };
        assert_eq!(glyph(0), "⠋");
        assert_eq!(glyph(150), "⠙");
        assert_eq!(glyph(1000), "⠋", "ten frames to a second, then round again");
    }

    #[test]
    fn the_command_shows_only_when_it_differs_from_the_name() {
        let out = lines(&one_project(vec![
            Agent {
                command: "claude".into(),
                ..agent("claude", false, None)
            },
            Agent {
                command: "bash".into(),
                ..agent("zsh", false, None)
            },
        ]));
        assert_eq!(out[1], " ├ ● claude");
        assert_eq!(out[2], " └ ● zsh               bash");
    }

    #[test]
    fn projects_without_agents_are_left_out_and_the_rest_are_bold() {
        let tree = Tree {
            projects: vec![
                ProjectEntry {
                    name: "busy".into(),
                    path: PathBuf::from("/p/busy"),
                    registered: true,
                    agents: vec![agent("claude", false, None)],
                },
                ProjectEntry {
                    name: "idle".into(),
                    path: PathBuf::from("/p/idle"),
                    registered: true,
                    agents: vec![],
                },
            ],
        };
        let state = State::new(tree, Some("%1".into()));
        let out = lines(&state);
        assert_eq!(out[0], " busy");
        assert_eq!(out[1], " └ ● claude            node");
        assert_eq!(out[2], "");
        let buffer = buffer(&state, Duration::ZERO);
        assert!(buffer[(1, 0)].modifier.contains(Modifier::BOLD), "busy");
    }

    #[test]
    fn the_footer_asks_before_a_kill() {
        use crate::sidebar::{Input, reduce};
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let tree = Tree {
            projects: vec![ProjectEntry {
                name: "project".into(),
                path: PathBuf::from("/p/project"),
                registered: true,
                agents: vec![agent("zsh", false, None)],
            }],
        };
        let state = State::new(tree, Some("%1".into()));
        let (state, _) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
        );
        let (state, _) = reduce(
            state,
            Input::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE)),
        );
        assert_eq!(lines(&state)[7], " kill project/zsh? y/n");
    }

    #[test]
    fn agent_rows_show_state_glyph_name_and_command_or_exit_status() {
        let tree = Tree {
            projects: vec![
                ProjectEntry {
                    name: "project".into(),
                    path: PathBuf::from("/p/project"),
                    registered: true,
                    agents: vec![
                        agent("claude", true, None),
                        agent("zsh", false, Some(Death::Exit(3))),
                        agent("gone", false, Some(Death::Signal("kill".into()))),
                    ],
                },
                ProjectEntry {
                    name: "old".into(),
                    path: PathBuf::from("/p/old"),
                    registered: false,
                    agents: vec![agent("claude", false, None)],
                },
            ],
        };
        let state = State::new(tree, Some("%1".into()));
        let out = lines(&state);
        assert_eq!(out[0], " project");
        assert_eq!(out[1], " ├ ● claude            node ◂");
        assert_eq!(out[2], " ├ ✗ zsh             exit 3");
        assert_eq!(out[3], " └ ✗ gone       signal kill");
        assert_eq!(out[4], " old*");
        assert_eq!(out[5], " └ ● claude            node");
        let buffer = buffer(&state, Duration::ZERO);
        assert_eq!(buffer[(28, 1)].fg, Color::Blue, "on-screen marker");
        assert!(buffer[(1, 4)].modifier.contains(Modifier::DIM), "old*");
    }
}
