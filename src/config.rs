//! Registered projects, kept in `projects.toml` under the config directory.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const FILE_NAME: &str = "projects.toml";
const NAME_MAX_LEN: usize = 32;

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

/// `PROPER_CONFIG_DIR` when set, otherwise the platform config dir plus `proper`.
pub fn config_dir(override_dir: Option<PathBuf>) -> PathBuf {
    override_dir.unwrap_or_else(|| {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
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
    fn config_dir_honours_the_override_variable() {
        assert_eq!(config_dir(Some("/tmp/x".into())), PathBuf::from("/tmp/x"));
        assert!(config_dir(None).ends_with("proper"));
    }
}
