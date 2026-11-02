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
