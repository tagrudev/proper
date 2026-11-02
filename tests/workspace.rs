mod support;

use support::{TestServer, wait_until};

#[test]
fn a_fresh_server_lists_its_single_pane() {
    let t = TestServer::start();
    let server = &t.server;
    server
        .run(&server.argv(["-f", "/dev/null", "new-session", "-d", "-s", "proper", "sleep", "1000"]))
        .unwrap();

    assert!(wait_until(|| server.list_panes().unwrap()[0].command == "sleep"));
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
            .run(&t.server.argv(["-f", "/dev/null", "new-session", "-d", "-s", "proper", "sleep", "1000"]))
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
    assert!(!root.contains("list-panes"), "root cause is tmux's own message: {root}");
}

mod bootstrap {
    use super::*;
    use proper::tmux::Role;
    use proper::workspace::{SIDEBAR_WIDTH, Workspace};
    use std::fs;

    fn workspace(t: &TestServer) -> (Workspace, tempfile::TempDir) {
        let config = tempfile::tempdir().unwrap();
        let ws = Workspace::new(t.server.clone(), config.path().to_path_buf(), env!("CARGO_BIN_EXE_proper").into());
        (ws, config)
    }

    #[test]
    fn bootstrap_builds_a_home_window_with_sidebar_and_hint() {
        let t = TestServer::start();
        let (ws, config) = workspace(&t);
        assert!(!ws.is_running().unwrap());

        ws.bootstrap(None).unwrap();

        assert!(ws.is_running().unwrap());
        assert!(fs::read_to_string(config.path().join("tmux.conf")).unwrap().contains("status off"));

        let panes = t.server.list_panes().unwrap();
        assert_eq!(panes.len(), 2);
        let sidebar = panes.iter().find(|p| p.role == Some(Role::Sidebar)).expect("sidebar pane");
        let hint = panes.iter().find(|p| p.role == Some(Role::Hint)).expect("hint pane");
        assert_eq!(sidebar.window_name, "home");
        assert_eq!(sidebar.window_id, hint.window_id);
        assert!(sidebar.index < hint.index, "sidebar is the first pane so main-vertical keeps it left");
        assert_eq!(sidebar.width, SIDEBAR_WIDTH);
        assert!(hint.pane_active, "focus starts in the hint pane, not the sidebar");
    }

    #[test]
    fn sidebar_and_hint_processes_stay_alive() {
        let t = TestServer::start();
        let (ws, _config) = workspace(&t);
        ws.bootstrap(None).unwrap();

        assert!(wait_until(|| {
            t.server.list_panes().unwrap().iter().all(|p| p.command == "proper" && p.dead.is_none())
        }));
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(t.server.list_panes().unwrap().iter().all(|p| p.dead.is_none()), "still alive after half a second");
    }

    #[test]
    fn resizing_the_window_keeps_the_sidebar_at_its_width() {
        let t = TestServer::start();
        let (ws, _config) = workspace(&t);
        ws.bootstrap(None).unwrap();

        t.server.run(&t.server.argv(["resize-window", "-t", "proper:home", "-x", "120", "-y", "40"])).unwrap();

        assert!(wait_until(|| {
            let panes = t.server.list_panes().unwrap();
            let sidebar = panes.iter().find(|p| p.role == Some(Role::Sidebar)).unwrap();
            let hint = panes.iter().find(|p| p.role == Some(Role::Hint)).unwrap();
            sidebar.width == SIDEBAR_WIDTH && hint.width == 120 - SIDEBAR_WIDTH - 1
        }));
    }

    #[test]
    fn sidebar_panes_see_the_config_dir_and_socket_through_the_session_environment() {
        let t = TestServer::start();
        let (ws, config) = workspace(&t);
        ws.bootstrap(None).unwrap();

        let env = t.server.run(&t.server.argv(["show-environment", "-t", "proper"])).unwrap();
        assert!(env.contains(&format!("PROPER_CONFIG_DIR={}", config.path().display())), "{env}");
        assert!(env.contains(&format!("PROPER_SOCKET={}", t.server.socket())), "{env}");
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
