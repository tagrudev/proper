mod support;

use support::{TestServer, wait_until};

#[test]
fn a_fresh_server_lists_its_single_pane() {
    let t = TestServer::start();
    let server = &t.server;
    server
        .run(&server.argv([
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "proper",
            "sleep",
            "1000",
        ]))
        .unwrap();

    assert!(wait_until(
        || server.list_panes().unwrap()[0].command == "sleep"
    ));
    let panes = server.list_panes().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0].role, None);
    assert_eq!(panes[0].dead, None);
    assert!(panes[0].window_active);
}

#[test]
fn dropping_the_test_server_kills_it() {
    let socket;
    {
        let t = TestServer::start();
        socket = t.server.socket().to_string();
        t.server
            .run(&t.server.argv([
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                "proper",
                "sleep",
                "1000",
            ]))
            .unwrap();
        assert!(t.server.list_panes().is_ok());
    }
    let gone = proper::tmux::Server::new(socket);
    assert!(gone.list_panes().is_err());
}

#[test]
fn a_server_that_is_not_running_is_an_error() {
    let t = TestServer::start();
    let err = t.server.list_panes().unwrap_err();
    let full = format!("{err:#}");
    assert!(full.contains("list-panes"), "{full}");
    let root = err.root_cause().to_string();
    assert!(root.contains(t.server.socket()), "{root}");
    assert!(
        !root.contains("list-panes"),
        "root cause is tmux's own message: {root}"
    );
}

mod bootstrap {
    use super::*;
    use proper::tmux::Role;
    use proper::workspace::{SIDEBAR_WIDTH, Workspace};
    use std::fs;

    fn workspace(t: &TestServer) -> (Workspace, tempfile::TempDir) {
        let config = tempfile::tempdir().unwrap();
        let ws = Workspace::new(
            t.server.clone(),
            config.path().to_path_buf(),
            env!("CARGO_BIN_EXE_proper").into(),
        );
        (ws, config)
    }

    #[test]
    fn bootstrap_builds_a_home_window_with_sidebar_and_hint() {
        let t = TestServer::start();
        let (ws, config) = workspace(&t);
        assert!(!ws.is_running().unwrap());

        ws.bootstrap(None).unwrap();

        assert!(ws.is_running().unwrap());
        assert!(
            fs::read_to_string(config.path().join("tmux.conf"))
                .unwrap()
                .contains("status off")
        );

        let panes = t.server.list_panes().unwrap();
        assert_eq!(panes.len(), 2);
        let sidebar = panes
            .iter()
            .find(|p| p.role == Some(Role::Sidebar))
            .expect("sidebar pane");
        let hint = panes
            .iter()
            .find(|p| p.role == Some(Role::Hint))
            .expect("hint pane");
        assert_eq!(sidebar.window_name, "home");
        assert_eq!(sidebar.window_id, hint.window_id);
        assert!(
            sidebar.index < hint.index,
            "sidebar is the first pane so main-vertical keeps it left"
        );
        assert_eq!(sidebar.width, SIDEBAR_WIDTH);
        assert!(
            hint.pane_active,
            "focus starts in the hint pane, not the sidebar"
        );
    }

    #[test]
    fn sidebar_and_hint_processes_stay_alive() {
        let t = TestServer::start();
        let (ws, _config) = workspace(&t);
        ws.bootstrap(None).unwrap();

        assert!(wait_until(|| {
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .all(|p| p.command == "proper" && p.dead.is_none())
        }));
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .all(|p| p.dead.is_none()),
            "still alive after half a second"
        );
    }

    #[test]
    fn resizing_the_window_keeps_the_sidebar_at_its_width() {
        let t = TestServer::start();
        let (ws, _config) = workspace(&t);
        ws.bootstrap(None).unwrap();

        t.server
            .run(&t.server.argv([
                "resize-window",
                "-t",
                "proper:home",
                "-x",
                "120",
                "-y",
                "40",
            ]))
            .unwrap();

        assert!(wait_until(|| {
            let panes = t.server.list_panes().unwrap();
            let sidebar = panes
                .iter()
                .find(|p| p.role == Some(Role::Sidebar))
                .unwrap();
            let hint = panes.iter().find(|p| p.role == Some(Role::Hint)).unwrap();
            sidebar.width == SIDEBAR_WIDTH && hint.width == 120 - SIDEBAR_WIDTH - 1
        }));
    }

    #[test]
    fn sidebar_panes_see_the_config_dir_and_socket_through_the_session_environment() {
        let t = TestServer::start();
        let (ws, config) = workspace(&t);
        ws.bootstrap(None).unwrap();

        let env = t
            .server
            .run(&t.server.argv(["show-environment", "-t", "proper"]))
            .unwrap();
        assert!(
            env.contains(&format!("PROPER_CONFIG_DIR={}", config.path().display())),
            "{env}"
        );
        assert!(
            env.contains(&format!("PROPER_SOCKET={}", t.server.socket())),
            "{env}"
        );
    }

    #[test]
    fn bootstrapping_twice_is_an_error_rather_than_a_second_session() {
        let t = TestServer::start();
        let (ws, _config) = workspace(&t);
        ws.bootstrap(None).unwrap();
        assert!(ws.bootstrap(None).is_err());
        assert_eq!(t.server.list_panes().unwrap().len(), 2);
    }
}

mod agents {
    use super::*;
    use proper::agents::Tree;
    use proper::config::{Project, Projects};
    use proper::tmux::Role;
    use proper::workspace::{SIDEBAR_WIDTH, Workspace};

    fn up(t: &TestServer) -> (Workspace, tempfile::TempDir, Project) {
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("project");
        std::fs::create_dir(&path).unwrap();
        let mut projects = Projects::default();
        let project = projects.add("project", &path).unwrap().clone();
        projects.save(config.path()).unwrap();
        let ws = Workspace::new(
            t.server.clone(),
            config.path().to_path_buf(),
            env!("CARGO_BIN_EXE_proper").into(),
        );
        ws.bootstrap(None).unwrap();
        (ws, config, project)
    }

    fn cmd(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|p| p.to_string()).collect()
    }

    #[test]
    fn new_agent_opens_a_named_window_with_sidebar_and_focus_in_the_agent() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);

        let agent = ws
            .new_agent(&project, None, &cmd(&["sleep", "1000"]))
            .unwrap();
        assert_eq!(agent.name, "sleep");

        assert!(wait_until(|| t
            .server
            .list_panes()
            .unwrap()
            .iter()
            .any(|p| p.id == agent.pane_id && p.command == "sleep")));
        let panes = t.server.list_panes().unwrap();
        let pane = panes.iter().find(|p| p.id == agent.pane_id).unwrap();
        assert_eq!(pane.window_name, "project/sleep");
        assert_eq!(pane.role, Some(Role::Agent));
        assert_eq!(pane.project, "project");
        assert_eq!(pane.agent, "sleep");
        assert_eq!(pane.path, project.path.display().to_string());
        assert!(pane.pane_active, "focus is in the agent pane");
        assert!(pane.window_active, "the new window is the current one");

        let sidebar = panes
            .iter()
            .find(|p| p.window_id == pane.window_id && p.role == Some(Role::Sidebar))
            .expect("sidebar");
        assert_eq!(sidebar.width, SIDEBAR_WIDTH);
        assert!(sidebar.index < pane.index);
        assert!(wait_until(|| t
            .server
            .list_panes()
            .unwrap()
            .iter()
            .any(|p| p.id == sidebar.id && p.command == "proper")));
    }

    #[test]
    fn a_second_agent_with_the_same_command_gets_a_numbered_name() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        ws.new_agent(&project, None, &cmd(&["sleep", "1000"]))
            .unwrap();
        let second = ws
            .new_agent(&project, None, &cmd(&["sleep", "1000"]))
            .unwrap();
        assert_eq!(second.name, "sleep-2");
        let named = ws
            .new_agent(&project, Some("reviewer"), &cmd(&["sleep", "1000"]))
            .unwrap();
        assert_eq!(named.name, "reviewer");
        assert!(
            ws.new_agent(&project, Some("reviewer"), &cmd(&["sleep", "1000"]))
                .is_err(),
            "taken"
        );
        assert!(
            ws.new_agent(&project, Some("Bad Name"), &cmd(&["sleep", "1000"]))
                .is_err()
        );
    }

    #[test]
    fn the_tree_shows_live_and_dead_agents() {
        let t = TestServer::start();
        let (ws, config, project) = up(&t);
        ws.new_agent(&project, None, &cmd(&["sleep", "1000"]))
            .unwrap();
        ws.new_agent(&project, Some("crash"), &cmd(&["sh", "-c", "exit 3"]))
            .unwrap();

        let registry = Projects::load(config.path()).unwrap();
        assert!(wait_until(|| {
            let tree = Tree::build(&t.server.list_panes().unwrap(), &registry);
            tree.agent("project", "crash")
                .is_some_and(|a| a.dead == Some(3))
        }));
        let tree = Tree::build(&t.server.list_panes().unwrap(), &registry);
        assert_eq!(tree.agent("project", "sleep").unwrap().dead, None);
        assert_eq!(tree.projects.len(), 1);
    }

    #[test]
    fn kill_removes_the_window_and_its_sidebar() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(&project, None, &cmd(&["sleep", "1000"]))
            .unwrap();
        assert_eq!(t.server.list_panes().unwrap().len(), 4);

        ws.kill(&agent.window_id).unwrap();

        let panes = t.server.list_panes().unwrap();
        assert_eq!(panes.len(), 2);
        assert!(panes.iter().all(|p| p.window_name == "home"));
    }
}

mod sidebar_screen {
    use super::*;
    use proper::config::{Project, Projects};
    use proper::tmux::Role;
    use proper::workspace::Workspace;

    fn up(t: &TestServer) -> (Workspace, tempfile::TempDir, Project) {
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("project");
        std::fs::create_dir(&path).unwrap();
        let mut projects = Projects::default();
        let project = projects.add("project", &path).unwrap().clone();
        projects.save(config.path()).unwrap();
        let ws = Workspace::new(
            t.server.clone(),
            config.path().to_path_buf(),
            env!("CARGO_BIN_EXE_proper").into(),
        );
        ws.bootstrap(None).unwrap();
        (ws, config, project)
    }

    fn screen(t: &TestServer, pane: &str) -> String {
        t.server
            .run(&t.server.argv(["capture-pane", "-p", "-t", pane]))
            .unwrap()
    }

    #[test]
    fn the_sidebar_in_the_home_window_lists_a_running_agent() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(&project, None, &["sleep".to_string(), "1000".to_string()])
            .unwrap();

        let panes = t.server.list_panes().unwrap();
        let home_sidebar = panes
            .iter()
            .find(|p| p.window_name == "home" && p.role == Some(Role::Sidebar))
            .unwrap();
        let agent_sidebar = panes
            .iter()
            .find(|p| p.window_id == agent.window_id && p.role == Some(Role::Sidebar))
            .unwrap();

        assert!(
            wait_until(|| screen(&t, &home_sidebar.id).contains("● sleep")),
            "{}",
            screen(&t, &home_sidebar.id)
        );
        assert!(
            wait_until(|| screen(&t, &agent_sidebar.id).contains("▶ ● sleep")),
            "{}",
            screen(&t, &agent_sidebar.id)
        );
        assert!(
            screen(&t, &home_sidebar.id).contains("▶"),
            "every sidebar marks the window the session shows"
        );

        t.server
            .run(&t.server.argv(["select-window", "-t", "proper:home"]))
            .unwrap();
        assert!(
            wait_until(|| !screen(&t, &home_sidebar.id).contains("▶")),
            "{}",
            screen(&t, &home_sidebar.id)
        );
        assert!(wait_until(|| !screen(&t, &agent_sidebar.id).contains("▶")));
    }
}

mod sidebar_keys {
    use super::*;
    use proper::config::{Project, Projects};
    use proper::tmux::{Pane, Role};
    use proper::workspace::Workspace;

    fn up(t: &TestServer) -> (Workspace, tempfile::TempDir, Project) {
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("project");
        std::fs::create_dir(&path).unwrap();
        let mut projects = Projects::default();
        let project = projects.add("project", &path).unwrap().clone();
        projects.save(config.path()).unwrap();
        let ws = Workspace::new(
            t.server.clone(),
            config.path().to_path_buf(),
            env!("CARGO_BIN_EXE_proper").into(),
        );
        ws.bootstrap(None).unwrap();
        (ws, config, project)
    }

    fn sidebar_of(panes: &[Pane], window: &str) -> Pane {
        panes
            .iter()
            .find(|p| p.window_name == window && p.role == Some(Role::Sidebar))
            .unwrap()
            .clone()
    }

    fn keys(t: &TestServer, pane: &str, keys: &[&str]) {
        let mut argv = vec!["send-keys", "-t", pane];
        argv.extend(keys);
        t.server.run(&t.server.argv(argv)).unwrap();
    }

    #[test]
    fn select_switches_window_and_focuses_the_agent_pane() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let first = ws
            .new_agent(
                &project,
                Some("first"),
                &["sleep".to_string(), "1000".to_string()],
            )
            .unwrap();
        let second = ws
            .new_agent(
                &project,
                Some("second"),
                &["sleep".to_string(), "1000".to_string()],
            )
            .unwrap();
        assert!(
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .any(|p| p.id == second.pane_id && p.window_active)
        );

        ws.select(&first.window_id, &first.pane_id).unwrap();

        let panes = t.server.list_panes().unwrap();
        let pane = panes.iter().find(|p| p.id == first.pane_id).unwrap();
        assert!(pane.window_active && pane.pane_active);
    }

    #[test]
    fn enter_in_the_sidebar_opens_the_agent_under_the_cursor() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(&project, None, &["sleep".to_string(), "1000".to_string()])
            .unwrap();
        t.server
            .run(&t.server.argv(["select-window", "-t", "proper:home"]))
            .unwrap();
        let sidebar = sidebar_of(&t.server.list_panes().unwrap(), "home");
        assert!(wait_until(|| t
            .server
            .run(&t.server.argv(["capture-pane", "-p", "-t", &sidebar.id]))
            .unwrap()
            .contains("● sleep")));

        keys(&t, &sidebar.id, &["j", "Enter"]);

        assert!(wait_until(|| {
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .any(|p| p.id == agent.pane_id && p.window_active && p.pane_active)
        }));
    }

    #[test]
    fn s_in_the_sidebar_starts_a_shell_in_the_project() {
        let t = TestServer::start();
        let (_ws, _config, project) = up(&t);
        let sidebar = sidebar_of(&t.server.list_panes().unwrap(), "home");
        assert!(wait_until(|| t
            .server
            .run(&t.server.argv(["capture-pane", "-p", "-t", &sidebar.id]))
            .unwrap()
            .contains("project")));

        keys(&t, &sidebar.id, &["s"]);

        assert!(wait_until(|| {
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .any(|p| p.role == Some(Role::Agent) && p.project == "project" && p.window_active)
        }));
        let panes = t.server.list_panes().unwrap();
        let shell = panes.iter().find(|p| p.role == Some(Role::Agent)).unwrap();
        assert_eq!(shell.path, project.path.display().to_string());
        assert!(shell.pane_active, "focus moved into the new shell");
        assert!(
            panes
                .iter()
                .any(|p| p.window_id == shell.window_id && p.role == Some(Role::Sidebar))
        );
    }
}
