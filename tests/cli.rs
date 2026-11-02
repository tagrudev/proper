use std::fs;
use std::path::Path;
use std::process::Command;

fn proper(config_dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_proper"))
        .env("PROPER_CONFIG_DIR", config_dir)
        .args(args)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn add_registers_a_project_in_the_config_dir() {
    let d = tempfile::tempdir().unwrap();
    let config = d.path().join("config");
    let project_dir = d.path().join("project");
    fs::create_dir(&project_dir).unwrap();

    let out = proper(&config, &["add", project_dir.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(text(&out.stdout).contains("project"));

    let toml = fs::read_to_string(config.join("projects.toml")).unwrap();
    assert!(toml.contains("name = \"project\""), "{toml}");
    assert!(
        toml.contains(&project_dir.canonicalize().unwrap().display().to_string()),
        "{toml}"
    );
}

#[test]
fn add_takes_an_explicit_name_and_rejects_bad_ones() {
    let d = tempfile::tempdir().unwrap();
    let config = d.path().join("config");
    let dir = d.path().join("Some Dir");
    fs::create_dir(&dir).unwrap();

    let out = proper(&config, &["add", dir.to_str().unwrap(), "--name", "some"]);
    assert!(out.status.success(), "{}", text(&out.stderr));

    let out = proper(
        &config,
        &["add", dir.to_str().unwrap(), "--name", "Bad Name"],
    );
    assert!(!out.status.success());
    assert!(text(&out.stderr).contains("Bad Name"));
}

#[test]
fn add_defaults_to_the_current_directory() {
    let d = tempfile::tempdir().unwrap();
    let config = d.path().join("config");
    let here = d.path().join("here");
    fs::create_dir(&here).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_proper"))
        .env("PROPER_CONFIG_DIR", &config)
        .current_dir(&here)
        .arg("add")
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        fs::read_to_string(config.join("projects.toml"))
            .unwrap()
            .contains("name = \"here\"")
    );
}

#[test]
fn running_proper_without_a_terminal_still_brings_the_server_up() {
    let d = tempfile::tempdir().unwrap();
    let socket = format!("proper-cli-{}", std::process::id());
    let out = Command::new(env!("CARGO_BIN_EXE_proper"))
        .env("PROPER_CONFIG_DIR", d.path())
        .env("PROPER_SOCKET", &socket)
        .env_remove("TMUX")
        .output()
        .unwrap();
    let server = proper::tmux::Server::new(socket);
    let panes = server.list_panes();
    let _ = server.run(&server.argv(["kill-server"]));

    assert!(!out.status.success(), "attach cannot work without a tty");
    assert!(
        text(&out.stderr).contains("not a terminal"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(
        panes.unwrap().len(),
        2,
        "home window with sidebar and hint exists"
    );
}

#[test]
fn running_proper_inside_a_foreign_tmux_is_refused() {
    let d = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_proper"))
        .env("PROPER_CONFIG_DIR", d.path())
        .env("PROPER_SOCKET", "proper-never-started")
        .env("TMUX", "/tmp/tmux-501/default,1,0")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("plain terminal"),
        "{}",
        text(&out.stderr)
    );
}

mod with_server {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    use proper::config::Projects;
    use proper::tmux::Server;
    use proper::workspace::Workspace;

    struct Up {
        config: tempfile::TempDir,
        server: Server,
        project_dir: std::path::PathBuf,
    }

    impl Up {
        fn start() -> Up {
            let config = tempfile::tempdir().unwrap();
            let project_dir = config.path().join("project");
            fs::create_dir(&project_dir).unwrap();
            let mut projects = Projects::default();
            projects.add("project", &project_dir).unwrap();
            projects.save(config.path()).unwrap();
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let server = Server::new(format!("proper-cli-{}-{n}", std::process::id()));
            Workspace::new(
                server.clone(),
                config.path().to_path_buf(),
                env!("CARGO_BIN_EXE_proper").into(),
            )
            .bootstrap(None)
            .unwrap();
            Up {
                config,
                server,
                project_dir,
            }
        }

        fn proper(&self, args: &[&str], cwd: &Path) -> std::process::Output {
            Command::new(env!("CARGO_BIN_EXE_proper"))
                .env("PROPER_CONFIG_DIR", self.config.path())
                .env("PROPER_SOCKET", self.server.socket())
                .env_remove("TMUX")
                .current_dir(cwd)
                .args(args)
                .output()
                .unwrap()
        }
    }

    impl Drop for Up {
        fn drop(&mut self) {
            let _ = self.server.run(&self.server.argv(["kill-server"]));
        }
    }

    #[test]
    fn new_ls_kill_round_trip() {
        let up = Up::start();
        let home = up.config.path();

        let out = up.proper(&["new", "project", "--", "sleep", "1000"], home);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert!(
            text(&out.stdout).contains("project/sleep"),
            "{}",
            text(&out.stdout)
        );

        let out = up.proper(&["ls"], home);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let line = text(&out.stdout);
        let fields: Vec<&str> = line.trim_end().split('\t').collect();
        assert_eq!(fields[0], "project");
        assert_eq!(fields[1], "sleep");
        assert_eq!(fields[2], "alive");
        assert!(fields[4].starts_with('%'), "{line}");

        let out = up.proper(&["kill", "project/sleep"], home);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_eq!(text(&up.proper(&["ls"], home).stdout), "");

        let out = up.proper(&["kill", "project/sleep"], home);
        assert!(!out.status.success());
        assert!(text(&out.stderr).contains("project/sleep"));
    }

    #[test]
    fn new_defaults_to_claude_in_the_project_containing_the_cwd() {
        let up = Up::start();
        let out = up.proper(
            &["new", "--name", "pending", "--", "sleep", "1000"],
            &up.project_dir,
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert!(
            text(&up.proper(&["ls"], &up.project_dir).stdout).starts_with("project\tpending\t")
        );

        let out = up.proper(&["new"], up.config.path());
        assert!(!out.status.success(), "cwd is not inside a project");
        assert!(
            text(&out.stderr).contains("proper add"),
            "{}",
            text(&out.stderr)
        );
    }

    #[test]
    fn ls_marks_dead_agents_with_their_exit_status() {
        let up = Up::start();
        let home = up.config.path();
        assert!(
            up.proper(
                &[
                    "new", "project", "--name", "crash", "--", "sh", "-c", "exit 3"
                ],
                home
            )
            .status
            .success()
        );
        std::thread::sleep(std::time::Duration::from_millis(300));
        let line = text(&up.proper(&["ls"], home).stdout);
        assert!(line.starts_with("project\tcrash\texit 3\t"), "{line}");
    }

    #[test]
    fn rm_refuses_while_agents_live_and_works_after_they_are_killed() {
        let up = Up::start();
        let home = up.config.path();
        assert!(
            up.proper(&["new", "project", "--", "sleep", "1000"], home)
                .status
                .success()
        );

        let out = up.proper(&["rm", "project"], home);
        assert!(!out.status.success());
        assert!(
            text(&out.stderr).contains("project/sleep"),
            "{}",
            text(&out.stderr)
        );

        assert!(up.proper(&["kill", "project/sleep"], home).status.success());
        let out = up.proper(&["rm", "project"], home);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert!(
            !fs::read_to_string(home.join("projects.toml"))
                .unwrap()
                .contains("project")
        );
    }
}
