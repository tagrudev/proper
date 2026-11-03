mod support;

use support::{TestServer, marks_on_screen, wait_until};

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
    fn tmux_accepts_every_line_of_the_generated_config() {
        let t = TestServer::start();
        t.server
            .run(&t.server.argv([
                "-f",
                "/dev/null",
                "new-session",
                "-d",
                "-s",
                "x",
                "sleep",
                "1000",
            ]))
            .unwrap();
        let conf = tempfile::NamedTempFile::new().unwrap();
        fs::write(conf.path(), proper::workspace::conf_text(None)).unwrap();

        let sourced = t.server.run(
            &t.server
                .argv(["source-file", conf.path().to_str().unwrap()]),
        );

        assert!(sourced.is_ok(), "{}", sourced.unwrap_err());
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
    fn active_and_inactive_pane_borders_look_alike() {
        let t = TestServer::start();
        let (ws, _config) = workspace(&t);
        ws.bootstrap(None).unwrap();

        let option = |name: &str| {
            t.server
                .run(&t.server.argv(["show-options", "-gv", name]))
                .unwrap()
        };
        assert_eq!(
            option("pane-active-border-style"),
            option("pane-border-style")
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
    use proper::tmux::{Death, Role};
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
                .is_some_and(|a| a.dead == Some(Death::Exit(3)))
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
            wait_until(|| marks_on_screen(&screen(&t, &agent_sidebar.id), "● sleep")),
            "{}",
            screen(&t, &agent_sidebar.id)
        );
        assert!(
            screen(&t, &home_sidebar.id).contains("◂"),
            "every sidebar marks the window the session shows"
        );

        t.server
            .run(&t.server.argv(["select-window", "-t", "proper:home"]))
            .unwrap();
        assert!(
            wait_until(|| !screen(&t, &home_sidebar.id).contains("◂")),
            "{}",
            screen(&t, &home_sidebar.id)
        );
        assert!(wait_until(|| !screen(&t, &agent_sidebar.id).contains("◂")));
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
    fn slash_in_the_sidebar_finds_projects_by_their_letters() {
        let t = TestServer::start();
        let (_ws, _config, _project) = up(&t);
        let sidebar = sidebar_of(&t.server.list_panes().unwrap(), "home");
        let screen = || {
            t.server
                .run(&t.server.argv(["capture-pane", "-p", "-t", &sidebar.id]))
                .unwrap()
        };
        assert!(wait_until(|| screen().contains("? help")), "{}", screen());

        keys(&t, &sidebar.id, &["/", "p", "j"]);
        assert!(
            wait_until(|| screen().contains(" /pj\n project")),
            "{}",
            screen()
        );
        keys(&t, &sidebar.id, &["z"]);
        assert!(wait_until(|| screen().contains("no match")), "{}", screen());
        keys(&t, &sidebar.id, &["BSpace"]);
        assert!(
            wait_until(|| screen().contains(" /pj\n project")),
            "{}",
            screen()
        );
        keys(&t, &sidebar.id, &["Escape"]);
        assert!(wait_until(|| screen().contains("? help")), "{}", screen());
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
        assert!(wait_until(|| {
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .any(|p| p.window_id == shell.window_id && p.role == Some(Role::Sidebar))
        }));
    }
}

mod dead_agents {
    use super::*;
    use proper::config::{Project, Projects};
    use proper::tmux::{Death, Role};
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

    /// Exits 3 the first time it runs and sleeps on every run after that.
    fn dies_once(config: &tempfile::TempDir) -> Vec<String> {
        let flag = config.path().join("ran-once");
        let script = format!(
            "test -e {flag} && exec sleep 1000; touch {flag}; exit 3",
            flag = flag.display()
        );
        vec!["sh".to_string(), "-c".to_string(), script]
    }

    fn dead_status(t: &TestServer, pane_id: &str) -> Option<Death> {
        t.server
            .list_panes()
            .unwrap()
            .iter()
            .find(|p| p.id == pane_id)
            .and_then(|p| p.dead.clone())
    }

    #[test]
    fn respawn_restarts_the_command_in_the_same_pane() {
        let t = TestServer::start();
        let (ws, config, project) = up(&t);
        let agent = ws
            .new_agent(&project, Some("crash"), &dies_once(&config))
            .unwrap();
        assert!(wait_until(
            || dead_status(&t, &agent.pane_id) == Some(Death::Exit(3))
        ));

        ws.respawn(&agent.pane_id).unwrap();

        assert!(wait_until(|| {
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .any(|p| p.id == agent.pane_id && p.dead.is_none() && p.command == "sleep")
        }));
    }

    #[test]
    fn respawning_a_live_agent_is_refused_by_tmux() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(&project, None, &["sleep".to_string(), "1000".to_string()])
            .unwrap();
        assert!(ws.respawn(&agent.pane_id).is_err());
    }

    #[test]
    fn x_then_y_in_the_sidebar_kills_the_agent_and_r_revives_a_dead_one() {
        let t = TestServer::start();
        let (ws, config, project) = up(&t);
        let crash = ws
            .new_agent(&project, Some("crash"), &dies_once(&config))
            .unwrap();
        let sleep = ws
            .new_agent(
                &project,
                Some("sleep"),
                &["sleep".to_string(), "1000".to_string()],
            )
            .unwrap();
        assert!(wait_until(
            || dead_status(&t, &crash.pane_id) == Some(Death::Exit(3))
        ));
        let panes = t.server.list_panes().unwrap();
        let sidebar = panes
            .iter()
            .find(|p| p.window_id == sleep.window_id && p.role == Some(Role::Sidebar))
            .unwrap();
        let screen = || {
            t.server
                .run(&t.server.argv(["capture-pane", "-p", "-t", &sidebar.id]))
                .unwrap()
        };
        assert!(
            wait_until(|| screen().contains("✗ crash") && marks_on_screen(&screen(), "● sleep")),
            "{}",
            screen()
        );

        // rows: project, crash, sleep; the cursor snapped to sleep (current window)
        t.server
            .run(&t.server.argv(["send-keys", "-t", &sidebar.id, "k", "r"]))
            .unwrap();
        assert!(
            wait_until(|| dead_status(&t, &crash.pane_id).is_none()),
            "{}",
            screen()
        );

        t.server
            .run(&t.server.argv(["send-keys", "-t", &sidebar.id, "j", "x"]))
            .unwrap();
        assert!(
            wait_until(|| screen().contains("kill project/sleep? y/n")),
            "{}",
            screen()
        );
        t.server
            .run(&t.server.argv(["send-keys", "-t", &sidebar.id, "y"]))
            .unwrap();
        assert!(wait_until(|| !t
            .server
            .list_panes()
            .unwrap()
            .iter()
            .any(|p| p.window_id == sleep.window_id)));
    }
}

mod heal {
    use super::*;
    use proper::config::{Project, Projects};
    use proper::tmux::{Pane, Role};
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

    fn sidebar_in(panes: &[Pane], window_id: &str) -> Option<Pane> {
        panes
            .iter()
            .find(|p| p.window_id == window_id && p.role == Some(Role::Sidebar))
            .cloned()
    }

    fn sleep() -> Vec<String> {
        vec!["sleep".to_string(), "1000".to_string()]
    }

    #[test]
    fn a_healthy_server_is_left_alone() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        ws.new_agent(&project, None, &sleep()).unwrap();
        let before = t.server.list_panes().unwrap();
        ws.heal().unwrap();
        assert_eq!(t.server.list_panes().unwrap(), before);
    }

    #[test]
    fn a_window_that_lost_its_sidebar_gets_a_new_one() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws.new_agent(&project, None, &sleep()).unwrap();
        let sidebar = sidebar_in(&t.server.list_panes().unwrap(), &agent.window_id).unwrap();
        t.server
            .run(&t.server.argv(["kill-pane", "-t", &sidebar.id]))
            .unwrap();
        assert!(sidebar_in(&t.server.list_panes().unwrap(), &agent.window_id).is_none());

        ws.heal().unwrap();

        let panes = t.server.list_panes().unwrap();
        let healed = sidebar_in(&panes, &agent.window_id).expect("sidebar is back");
        let agent_pane = panes.iter().find(|p| p.id == agent.pane_id).unwrap();
        assert_eq!(healed.width, SIDEBAR_WIDTH);
        assert!(healed.index < agent_pane.index);
        assert!(agent_pane.pane_active, "healing does not steal focus");
        assert!(wait_until(|| t
            .server
            .list_panes()
            .unwrap()
            .iter()
            .any(|p| p.id == healed.id && p.command == "proper")));
    }

    #[test]
    fn a_crashed_sidebar_or_hint_is_respawned() {
        let t = TestServer::start();
        let (ws, _config, _project) = up(&t);
        let panes = t.server.list_panes().unwrap();
        for role in [Role::Sidebar, Role::Hint] {
            let pane = panes.iter().find(|p| p.role == Some(role)).unwrap();
            let pid = t
                .server
                .run(
                    &t.server
                        .argv(["display-message", "-p", "-t", &pane.id, "#{pane_pid}"]),
                )
                .unwrap();
            std::process::Command::new("kill")
                .args(["-9", pid.trim()])
                .status()
                .unwrap();
            assert!(wait_until(|| t
                .server
                .list_panes()
                .unwrap()
                .iter()
                .any(|p| p.id == pane.id && p.dead.is_some())));
        }

        ws.heal().unwrap();

        assert!(wait_until(|| {
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .all(|p| p.dead.is_none() && p.command == "proper")
        }));
        assert_eq!(
            t.server.list_panes().unwrap().len(),
            2,
            "respawned in place, nothing added"
        );
    }

    #[test]
    fn a_window_with_only_a_sidebar_is_closed() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws.new_agent(&project, None, &sleep()).unwrap();
        t.server
            .run(&t.server.argv(["kill-pane", "-t", &agent.pane_id]))
            .unwrap();
        assert!(sidebar_in(&t.server.list_panes().unwrap(), &agent.window_id).is_some());

        ws.heal().unwrap();

        assert!(
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .all(|p| p.window_id != agent.window_id)
        );
        assert_eq!(
            t.server.list_panes().unwrap().len(),
            2,
            "home window untouched"
        );
    }

    #[test]
    fn windows_that_are_not_propers_are_ignored() {
        let t = TestServer::start();
        let (ws, _config, _project) = up(&t);
        t.server
            .run(&t.server.argv([
                "new-window",
                "-d",
                "-t",
                "proper:",
                "-n",
                "mine",
                "sleep",
                "1000",
            ]))
            .unwrap();

        ws.heal().unwrap();

        let panes = t.server.list_panes().unwrap();
        assert_eq!(panes.iter().filter(|p| p.window_name == "mine").count(), 1);
        assert_eq!(panes.len(), 3);
    }
}

mod sidebar_mouse_and_speed {
    use super::*;
    use proper::config::{Project, Projects};
    use proper::tmux::Role;
    use proper::workspace::Workspace;
    use std::time::{Duration, Instant};

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

    fn within(limit: Duration, mut condition: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < limit {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn the_cursor_row_shows_only_while_the_sidebar_is_the_active_pane() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(&project, None, &["sleep".to_string(), "1000".to_string()])
            .unwrap();
        let panes = t.server.list_panes().unwrap();
        let sidebar = panes
            .iter()
            .find(|p| p.window_id == agent.window_id && p.role == Some(Role::Sidebar))
            .unwrap();
        let styled = || {
            t.server
                .run(
                    &t.server
                        .argv(["capture-pane", "-e", "-p", "-t", &sidebar.id]),
                )
                .unwrap()
        };
        let cursor_background = "48;5;237";
        assert!(wait_until(|| screen(&t, &sidebar.id).contains("● sleep")));
        assert!(!styled().contains(cursor_background), "{}", styled());

        // let the one second fallback timer pass so it cannot be what refreshes the screen
        std::thread::sleep(Duration::from_millis(1100));
        t.server
            .run(&t.server.argv(["select-pane", "-t", &sidebar.id]))
            .unwrap();
        assert!(
            within(Duration::from_millis(400), || styled()
                .contains(cursor_background)),
            "{}",
            styled()
        );

        std::thread::sleep(Duration::from_millis(1100));
        t.server
            .run(&t.server.argv(["select-pane", "-t", &agent.pane_id]))
            .unwrap();
        assert!(
            within(Duration::from_millis(400), || !styled()
                .contains(cursor_background)),
            "{}",
            styled()
        );
    }

    #[test]
    fn a_click_on_an_agent_row_opens_that_agent() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(&project, None, &["sleep".to_string(), "1000".to_string()])
            .unwrap();
        t.server
            .run(&t.server.argv(["select-window", "-t", "proper:home"]))
            .unwrap();
        let panes = t.server.list_panes().unwrap();
        let sidebar = panes
            .iter()
            .find(|p| p.window_name == "home" && p.role == Some(Role::Sidebar))
            .unwrap();
        assert!(wait_until(|| screen(&t, &sidebar.id).contains("● sleep")));

        // SGR mouse report: left button press and release on column 5, row 2 (the agent row, 1-based).
        t.server
            .run(&t.server.argv([
                "send-keys",
                "-t",
                &sidebar.id,
                "-l",
                "\x1b[<0;5;2M\x1b[<0;5;2m",
            ]))
            .unwrap();

        assert!(wait_until(|| {
            t.server
                .list_panes()
                .unwrap()
                .iter()
                .any(|p| p.id == agent.pane_id && p.window_active && p.pane_active)
        }));
    }

    #[test]
    fn sidebars_notice_a_respawn_well_under_the_fallback_second() {
        let t = TestServer::start();
        let (ws, config, project) = up(&t);
        let flag = config.path().join("ran-once");
        let script = format!(
            "test -e {flag} && exec sleep 1000; touch {flag}; exit 3",
            flag = flag.display()
        );
        let agent = ws
            .new_agent(
                &project,
                Some("crash"),
                &["sh".to_string(), "-c".to_string(), script],
            )
            .unwrap();
        let panes = t.server.list_panes().unwrap();
        let home_sidebar = panes
            .iter()
            .find(|p| p.window_name == "home" && p.role == Some(Role::Sidebar))
            .unwrap();
        assert!(wait_until(
            || screen(&t, &home_sidebar.id).contains("✗ crash")
        ));
        std::thread::sleep(Duration::from_millis(1100));

        ws.respawn(&agent.pane_id).unwrap();

        assert!(
            within(Duration::from_millis(400), || screen(&t, &home_sidebar.id)
                .contains("● crash")),
            "{}",
            screen(&t, &home_sidebar.id)
        );
    }

    #[test]
    fn sidebars_notice_a_window_switch_well_under_the_fallback_second() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        ws.new_agent(&project, None, &["sleep".to_string(), "1000".to_string()])
            .unwrap();
        let panes = t.server.list_panes().unwrap();
        let home_sidebar = panes
            .iter()
            .find(|p| p.window_name == "home" && p.role == Some(Role::Sidebar))
            .unwrap();
        assert!(wait_until(|| marks_on_screen(
            &screen(&t, &home_sidebar.id),
            "● sleep"
        )));
        // let the one second fallback timer pass so it cannot be what refreshes the screen
        std::thread::sleep(Duration::from_millis(1100));

        t.server
            .run(&t.server.argv(["select-window", "-t", "proper:home"]))
            .unwrap();

        assert!(
            within(Duration::from_millis(400), || !screen(&t, &home_sidebar.id)
                .contains("◂")),
            "{}",
            screen(&t, &home_sidebar.id)
        );
    }
}

mod agent_environment_and_exit {
    use super::*;
    use proper::config::{Project, Projects};
    use proper::tmux::{Death, Role};
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
            .run(&t.server.argv(["capture-pane", "-p", "-S", "-", "-t", pane]))
            .unwrap()
    }

    fn sh(script: &str) -> Vec<String> {
        vec!["sh".to_string(), "-c".to_string(), script.to_string()]
    }

    #[test]
    fn agents_see_no_tmux_variable_so_tmux_inside_them_uses_the_default_server() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(
                &project,
                None,
                &sh("echo \"TMUX=[$TMUX] PANE=[$TMUX_PANE]\"; sleep 1000"),
            )
            .unwrap();
        assert!(
            wait_until(|| screen(&t, &agent.pane_id).contains("TMUX=[] PANE=[%")),
            "{}",
            screen(&t, &agent.pane_id)
        );
    }

    #[test]
    fn a_respawned_agent_still_sees_no_tmux_variable() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(&project, None, &sh("echo \"TMUX=[$TMUX]\"; exit 3"))
            .unwrap();
        assert!(wait_until(|| t.server.list_panes().unwrap().iter().any(
            |p| p.id == agent.pane_id && p.dead == Some(Death::Exit(3))
        )));
        t.server
            .run(&t.server.argv(["clear-history", "-t", &agent.pane_id]))
            .unwrap();

        ws.respawn(&agent.pane_id).unwrap();

        assert!(wait_until(|| t.server.list_panes().unwrap().iter().any(
            |p| p.id == agent.pane_id && p.dead == Some(Death::Exit(3))
        )));
        assert!(
            screen(&t, &agent.pane_id).contains("TMUX=[]"),
            "{}",
            screen(&t, &agent.pane_id)
        );
    }

    #[test]
    fn a_clean_exit_closes_the_whole_window_and_a_crash_keeps_it() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let crash = ws
            .new_agent(&project, Some("crash"), &sh("exit 3"))
            .unwrap();
        let clean = ws
            .new_agent(&project, Some("clean"), &sh("sleep 0.3; exit 0"))
            .unwrap();

        assert!(wait_until(|| t
            .server
            .list_panes()
            .unwrap()
            .iter()
            .all(|p| p.window_id != clean.window_id)));
        let panes = t.server.list_panes().unwrap();
        assert!(
            panes
                .iter()
                .any(|p| p.id == crash.pane_id && p.dead == Some(Death::Exit(3))),
            "crash stays"
        );
        assert!(
            panes
                .iter()
                .any(|p| p.window_id == crash.window_id && p.role == Some(Role::Sidebar)),
            "with its sidebar"
        );
        assert_eq!(
            panes.len(),
            4,
            "home window and the crashed agent's window remain"
        );
    }

    #[test]
    fn a_killed_sidebar_stays_dead_for_heal_to_respawn() {
        let t = TestServer::start();
        let (ws, _config, _project) = up(&t);
        let panes = t.server.list_panes().unwrap();
        let sidebar = panes
            .iter()
            .find(|p| p.role == Some(Role::Sidebar))
            .unwrap();
        let pid = t
            .server
            .run(
                &t.server
                    .argv(["display-message", "-p", "-t", &sidebar.id, "#{pane_pid}"]),
            )
            .unwrap();
        std::process::Command::new("kill")
            .args(["-9", pid.trim()])
            .status()
            .unwrap();

        assert!(wait_until(|| t
            .server
            .list_panes()
            .unwrap()
            .iter()
            .any(|p| p.id == sidebar.id && p.dead.is_some())));
        ws.heal().unwrap();
        assert!(wait_until(|| t
            .server
            .list_panes()
            .unwrap()
            .iter()
            .any(|p| p.id == sidebar.id && p.dead.is_none())));
    }
}

mod status {
    use super::*;
    use proper::config::{Project, Projects};
    use proper::tmux::{Role, Status};
    use proper::workspace::Workspace;
    use std::time::{Duration, Instant};

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
    fn set_status_marks_the_pane_and_sidebars_show_it_quickly() {
        let t = TestServer::start();
        let (ws, _config, project) = up(&t);
        let agent = ws
            .new_agent(&project, None, &["sleep".to_string(), "1000".to_string()])
            .unwrap();
        let panes = t.server.list_panes().unwrap();
        let sidebar = panes
            .iter()
            .find(|p| p.window_id == agent.window_id && p.role == Some(Role::Sidebar))
            .unwrap();
        assert!(wait_until(|| screen(&t, &sidebar.id).contains("● sleep")));
        std::thread::sleep(Duration::from_millis(1100));

        ws.set_status(&agent.pane_id, Status::Attention).unwrap();

        let started = Instant::now();
        assert!(
            wait_until(|| screen(&t, &sidebar.id).contains("! sleep")),
            "{}",
            screen(&t, &sidebar.id)
        );
        assert!(
            started.elapsed() < Duration::from_millis(400),
            "took {:?}",
            started.elapsed()
        );
        let pane = t
            .server
            .list_panes()
            .unwrap()
            .into_iter()
            .find(|p| p.id == agent.pane_id)
            .unwrap();
        assert_eq!(pane.status, Some(Status::Attention));
    }
}
