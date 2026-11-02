//! A real tmux server on a private socket for integration tests. No mocks.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use proper::tmux::Server;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct TestServer {
    pub server: Server,
}

impl TestServer {
    /// Addresses a not-yet-running server on a socket unique to this process and call.
    pub fn start() -> TestServer {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let socket = format!("proper-test-{}-{n}", std::process::id());
        TestServer {
            server: Server::new(socket),
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        let _ = self.server.run(&self.server.argv(["kill-server"]));
    }
}

/// True when a captured sidebar marks the line containing `row` as the window on screen.
pub fn marks_on_screen(screen: &str, row: &str) -> bool {
    screen
        .lines()
        .any(|line| line.contains(row) && line.ends_with('◂'))
}

/// Polls `condition` every 20ms for up to two seconds.
pub fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if condition() {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
