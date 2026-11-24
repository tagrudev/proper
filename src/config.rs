//! Registered projects, kept in `projects.toml` under the config directory.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "projects.toml";
const LAUNCHERS_FILE: &str = "launchers.toml";
const NAME_MAX_LEN: usize = 32;
/// Keys the sidebar uses itself; a launcher cannot take one.
const RESERVED_KEYS: &[char] = &['j', 'k', 'n', 's', 'w', 'x', 'r', 'q', '?', '/'];

/// A program of the user's own that the sidebar starts on `key`, in the project under
/// the cursor. Its agents are named `name`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launcher {
    pub key: char,
    pub name: String,
    pub command: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Launchers {
    #[serde(default)]
    launchers: Vec<Launcher>,
}

/// Reads `launchers.toml`; a missing file is no launchers. A leading `~/` in a command
/// word becomes the home directory. Errors are one short line, fit for the sidebar footer.
pub fn load_launchers(dir: &Path) -> Result<Vec<Launcher>> {
    let file = dir.join(LAUNCHERS_FILE);
    if !file.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&file).map_err(|err| anyhow!("{LAUNCHERS_FILE}: {err}"))?;
    parse_launchers(&text, dirs::home_dir().as_deref())
        .map_err(|err| anyhow!("{LAUNCHERS_FILE}: {err}"))
}

fn parse_launchers(text: &str, home: Option<&Path>) -> Result<Vec<Launcher>> {
    let parsed: Launchers = toml::from_str(text).map_err(|err| anyhow!("{}", err.message()))?;
    let mut launchers: Vec<Launcher> = Vec::new();
    for launcher in parsed.launchers {
        let key = launcher.key;
        if RESERVED_KEYS.contains(&key) {
            bail!("key {key:?} is taken by proper");
        }
        if !key.is_ascii_graphic() {
            bail!("key {key:?} is not a printable ASCII character");
        }
        if launchers.iter().any(|other| other.key == key) {
            bail!("key {key:?} is used twice");
        }
        validate_name(&launcher.name)?;
        if launcher.command.is_empty() {
            bail!("{} has an empty command", launcher.name);
        }
        let command = launcher
            .command
            .into_iter()
            .map(|word| match (word.strip_prefix("~/"), home) {
                (Some(rest), Some(home)) => home.join(rest).display().to_string(),
                _ => word,
            })
            .collect();
        launchers.push(Launcher {
            command,
            ..launcher
        });
    }
    Ok(launchers)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Projects {
    #[serde(default)]
    pub projects: Vec<Project>,
}

impl Projects {
    /// A missing file is simply no projects yet.
    pub fn load(dir: &Path) -> Result<Projects> {
        let file = dir.join(FILE_NAME);
        if !file.exists() {
            return Ok(Projects::default());
        }
        let text =
            fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", file.display()))
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        let file = dir.join(FILE_NAME);
        fs::write(&file, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", file.display()))
    }

    pub fn add(&mut self, name: &str, path: &Path) -> Result<&Project> {
        validate_name(name)?;
        let path = path
            .canonicalize()
            .with_context(|| format!("{} does not exist", path.display()))?;
        if !path.is_dir() {
            bail!("{} is not a directory", path.display());
        }
        if self.get(name).is_some() {
            bail!("a project named {name} is already registered");
        }
        if let Some(existing) = self.projects.iter().find(|p| p.path == path) {
            bail!(
                "{} is already registered as {}",
                path.display(),
                existing.name
            );
        }
        self.projects.push(Project {
            name: name.to_string(),
            path,
        });
        Ok(self.projects.last().expect("just pushed"))
    }

    pub fn remove(&mut self, name: &str) -> Result<Project> {
        let index = self
            .projects
            .iter()
            .position(|p| p.name == name)
            .with_context(|| format!("no project named {name}"))?;
        Ok(self.projects.remove(index))
    }

    pub fn get(&self, name: &str) -> Option<&Project> {
        self.projects.iter().find(|p| p.name == name)
    }

    /// The most specific registered project whose path contains `dir`.
    pub fn containing(&self, dir: &Path) -> Option<&Project> {
        self.projects
            .iter()
            .filter(|p| dir.starts_with(&p.path))
            .max_by_key(|p| p.path.components().count())
    }
}

/// Lowercase, starts alphanumeric, then alphanumerics, `.`, `_` or `-`; at most 32 chars.
pub fn validate_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let valid_first = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    let valid_rest =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    if !valid_first || !valid_rest || name.len() > NAME_MAX_LEN {
        bail!(
            "invalid name {name:?}: use lowercase letters, digits, '.', '_' or '-', up to {NAME_MAX_LEN} chars"
        );
    }
    Ok(())
}

pub fn default_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// `PROPER_CONFIG_DIR` when set, else `$XDG_CONFIG_HOME/proper`, else `~/.config/proper`.
pub fn config_dir(override_dir: Option<PathBuf>, xdg_config_home: Option<PathBuf>) -> PathBuf {
    override_dir.unwrap_or_else(|| {
        xdg_config_home
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".config")
            })
            .join("proper")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn project_dir(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_file_loads_as_no_projects() {
        let d = dir();
        assert_eq!(Projects::load(d.path()).unwrap(), Projects::default());
    }

    #[test]
    fn added_projects_survive_a_save_and_load() {
        let d = dir();
        let project = project_dir(d.path(), "project");
        let mut projects = Projects::default();
        projects.add("project", &project).unwrap();
        projects.save(d.path()).unwrap();

        let loaded = Projects::load(d.path()).unwrap();
        assert_eq!(loaded.projects.len(), 1);
        assert_eq!(loaded.projects[0].name, "project");
        assert_eq!(loaded.projects[0].path, project.canonicalize().unwrap());
        assert!(
            fs::read_to_string(d.path().join("projects.toml"))
                .unwrap()
                .contains("project")
        );
    }

    #[test]
    fn save_creates_the_config_directory() {
        let d = dir();
        let nested = d.path().join("deeper").join("proper");
        Projects::default().save(&nested).unwrap();
        assert!(nested.join("projects.toml").exists());
    }

    #[test]
    fn add_stores_the_canonical_path() {
        let d = dir();
        let project = project_dir(d.path(), "project");
        let mut projects = Projects::default();
        let added = projects
            .add("project", &project.join("..").join("project"))
            .unwrap();
        assert_eq!(added.path, project.canonicalize().unwrap());
    }

    #[test]
    fn add_rejects_a_path_that_is_not_a_directory() {
        let d = dir();
        let file = d.path().join("file");
        fs::write(&file, "x").unwrap();
        let mut projects = Projects::default();
        assert!(projects.add("file", &file).is_err());
        assert!(projects.add("missing", &d.path().join("missing")).is_err());
    }

    #[test]
    fn add_rejects_duplicate_names_and_paths() {
        let d = dir();
        let project = project_dir(d.path(), "project");
        let other = project_dir(d.path(), "other");
        let mut projects = Projects::default();
        projects.add("project", &project).unwrap();
        assert!(projects.add("project", &other).is_err());
        assert!(projects.add("again", &project).is_err());
        assert_eq!(projects.projects.len(), 1);
    }

    #[test]
    fn names_are_lowercase_kebab_with_dots_allowed() {
        for ok in [
            "project",
            "todor.dev",
            "a",
            "x1",
            "with_under",
            "two-words",
            "0start",
        ] {
            assert!(validate_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "Project",
            "has space",
            "slash/y",
            "-lead",
            ".lead",
            "x".repeat(33).as_str(),
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn default_name_is_the_lowercased_directory_name() {
        assert_eq!(default_name(Path::new("/x/Todor.dev")), "todor.dev");
        assert_eq!(default_name(Path::new("/x/project/")), "project");
    }

    #[test]
    fn remove_returns_the_project_and_errors_on_unknown_names() {
        let d = dir();
        let project = project_dir(d.path(), "project");
        let mut projects = Projects::default();
        projects.add("project", &project).unwrap();
        assert_eq!(projects.remove("project").unwrap().name, "project");
        assert!(projects.projects.is_empty());
        assert!(projects.remove("project").is_err());
    }

    #[test]
    fn containing_picks_the_deepest_project_holding_the_directory() {
        let d = dir();
        let outer = project_dir(d.path(), "outer");
        let inner = project_dir(&outer, "inner");
        let elsewhere = project_dir(d.path(), "elsewhere");
        let mut projects = Projects::default();
        projects.add("outer", &outer).unwrap();
        projects.add("inner", &inner).unwrap();
        let outer = outer.canonicalize().unwrap();
        let inner = inner.canonicalize().unwrap();
        assert_eq!(
            projects.containing(&inner.join("src")).unwrap().name,
            "inner"
        );
        assert_eq!(
            projects.containing(&outer.join("lib")).unwrap().name,
            "outer"
        );
        assert!(
            projects
                .containing(&elsewhere.canonicalize().unwrap())
                .is_none()
        );
    }

    #[test]
    fn launchers_parse_with_home_expanded_in_command_words() {
        let text = r#"
            [[launchers]]
            key = "a"
            name = "reviewer"
            command = ["~/reviewer/bin/start", "--add-dir", "~/reviewer", "~user"]
        "#;
        assert_eq!(
            parse_launchers(text, Some(Path::new("/home/me"))).unwrap(),
            vec![Launcher {
                key: 'a',
                name: "reviewer".into(),
                command: vec![
                    "/home/me/reviewer/bin/start".into(),
                    "--add-dir".into(),
                    "/home/me/reviewer".into(),
                    "~user".into(),
                ],
            }]
        );
        assert_eq!(parse_launchers("", None).unwrap(), vec![]);
    }

    #[test]
    fn launchers_refuse_taken_keys_bad_names_and_empty_commands() {
        let launcher = |key: &str, name: &str, command: &str| {
            format!("[[launchers]]\nkey = \"{key}\"\nname = \"{name}\"\ncommand = {command}\n")
        };
        let error = |text: String| parse_launchers(&text, None).unwrap_err().to_string();
        assert_eq!(
            error(launcher("n", "reviewer", r#"["x"]"#)),
            "key 'n' is taken by proper"
        );
        assert_eq!(
            error(launcher("a", "one", r#"["x"]"#) + &launcher("a", "two", r#"["x"]"#)),
            "key 'a' is used twice"
        );
        assert!(error(launcher("a", "Not Valid", r#"["x"]"#)).contains("invalid name"));
        assert_eq!(
            error(launcher("a", "reviewer", "[]")),
            "reviewer has an empty command"
        );
        assert!(parse_launchers(&launcher("ab", "reviewer", r#"["x"]"#), None).is_err());
        assert!(parse_launchers("[[launchers]]\nkey = \"a\"\n", None).is_err());
    }

    #[test]
    fn a_missing_launchers_file_is_no_launchers_and_a_broken_one_names_the_file() {
        let d = dir();
        assert_eq!(load_launchers(d.path()).unwrap(), vec![]);
        fs::write(d.path().join("launchers.toml"), "launchers = 3").unwrap();
        let error = load_launchers(d.path()).unwrap_err().to_string();
        assert!(error.starts_with("launchers.toml: "), "{error}");
        assert!(!error.contains('\n'), "{error}");
    }

    #[test]
    fn config_dir_is_the_override_then_xdg_then_dot_config() {
        assert_eq!(
            config_dir(Some("/tmp/x".into()), Some("/xdg".into())),
            PathBuf::from("/tmp/x")
        );
        assert_eq!(
            config_dir(None, Some("/xdg".into())),
            PathBuf::from("/xdg/proper")
        );
        let default = config_dir(None, None);
        assert!(default.ends_with(".config/proper"), "{}", default.display());
        assert!(default.is_absolute());
    }
}
