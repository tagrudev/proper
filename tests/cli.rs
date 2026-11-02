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
