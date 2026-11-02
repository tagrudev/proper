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
