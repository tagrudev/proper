//! What the sidebar shows: registered projects and the agents living in them.

use std::path::PathBuf;

use crate::config::Projects;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    pub project: String,
    pub pane_id: String,
    pub window_id: String,
    /// The agent's window is the one the client is looking at.
    pub current: bool,
    pub dead: Option<i32>,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    pub name: String,
    pub path: PathBuf,
    /// False when agents exist for a project that is no longer in projects.toml.
    pub registered: bool,
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    pub projects: Vec<ProjectEntry>,
}

impl Tree {
    pub fn from_registry(projects: &Projects) -> Tree {
        Tree {
            projects: projects
                .projects
                .iter()
                .map(|p| ProjectEntry {
                    name: p.name.clone(),
                    path: p.path.clone(),
                    registered: true,
                    agents: vec![],
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Project;

    fn registry(names: &[&str]) -> Projects {
        Projects {
            projects: names
                .iter()
                .map(|n| Project {
                    name: n.to_string(),
                    path: PathBuf::from(format!("/p/{n}")),
                })
                .collect(),
        }
    }

    #[test]
    fn registry_alone_gives_registered_projects_without_agents() {
        let tree = Tree::from_registry(&registry(&["project", "znaiko"]));
        assert_eq!(tree.projects.len(), 2);
        assert_eq!(tree.projects[0].name, "project");
        assert_eq!(tree.projects[0].path, PathBuf::from("/p/project"));
        assert!(tree.projects[0].registered);
        assert!(tree.projects[0].agents.is_empty());
        assert_eq!(tree.projects[1].name, "znaiko");
    }
}
