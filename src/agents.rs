//! What the sidebar shows: registered projects and the agents living in them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::config::Projects;
use crate::tmux::{Death, Pane, Role, Status};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    pub project: String,
    pub pane_id: String,
    pub window_id: String,
    /// The agent's window is the one the client is looking at.
    pub current: bool,
    pub dead: Option<Death>,
    pub command: String,
    pub status: Option<Status>,
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
    /// Registered projects in registry order, then any project only known from live panes.
    pub fn build(panes: &[Pane], registry: &Projects) -> Tree {
        let mut tree = Tree::from_registry(registry);
        let agent_panes: Vec<&Pane> = panes
            .iter()
            .filter(|p| p.role == Some(Role::Agent) && !p.project.is_empty() && !p.agent.is_empty())
            .collect();

        let unregistered: BTreeMap<&str, PathBuf> = agent_panes
            .iter()
            .filter(|p| registry.get(&p.project).is_none())
            .fold(BTreeMap::new(), |mut found, p| {
                let path = found.entry(p.project.as_str()).or_default();
                if path.as_os_str().is_empty() {
                    *path = PathBuf::from(&p.path);
                }
                found
            });
        tree.projects
            .extend(unregistered.into_iter().map(|(name, path)| ProjectEntry {
                name: name.to_string(),
                path,
                registered: false,
                agents: vec![],
            }));

        for pane in agent_panes {
            if let Some(project) = tree.projects.iter_mut().find(|p| p.name == pane.project) {
                project.agents.push(Agent {
                    name: pane.agent.clone(),
                    project: pane.project.clone(),
                    pane_id: pane.id.clone(),
                    window_id: pane.window_id.clone(),
                    current: pane.window_active,
                    dead: pane.dead.clone(),
                    command: pane.command.clone(),
                    status: pane.status,
                });
            }
        }
        for project in &mut tree.projects {
            project.agents.sort_by(|a, b| a.name.cmp(&b.name));
        }
        tree
    }

    pub fn agent(&self, project: &str, name: &str) -> Option<&Agent> {
        self.projects
            .iter()
            .find(|p| p.name == project)?
            .agents
            .iter()
            .find(|a| a.name == name)
    }

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

/// True when `own_pane` is a sidebar whose window holds nothing but sidebars: its agent
/// or hint is gone, so the sidebar has nothing left to sit next to.
pub fn sidebar_is_alone(panes: &[Pane], own_pane: &str) -> bool {
    let Some(me) = panes.iter().find(|p| p.id == own_pane) else {
        return false;
    };
    me.role == Some(Role::Sidebar)
        && panes
            .iter()
            .filter(|p| p.window_id == me.window_id && p.id != me.id)
            .all(|p| p.role == Some(Role::Sidebar))
}

/// `base`, or `base-2`, `base-3`, ... when earlier names are taken.
pub fn unique_name(base: &str, taken: &[&str]) -> String {
    if !taken.contains(&base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken.contains(&candidate.as_str()))
        .expect("the counter is unbounded")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Project;
    use crate::tmux::{Role, Status};

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

    const FIXTURE: &str = include_str!("../tests/fixtures/panes.tsv");

    fn panes() -> Vec<Pane> {
        crate::tmux::parse_panes(FIXTURE).unwrap()
    }

    #[test]
    fn agents_land_under_their_project_in_registry_order() {
        let tree = Tree::build(&panes(), &registry(&["znaiko", "project"]));
        let names: Vec<&str> = tree.projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["znaiko", "project"]);
        assert!(tree.projects[0].agents.is_empty());
        let project = &tree.projects[1];
        assert!(project.registered);
        assert_eq!(project.path, PathBuf::from("/p/project"));
        let agents: Vec<&str> = project.agents.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(agents, ["claude", "killed", "zsh"], "sorted by name");
    }

    #[test]
    fn an_agent_carries_its_pane_state() {
        let tree = Tree::build(&panes(), &registry(&["project"]));
        let claude = &tree.projects[0].agents[0];
        assert_eq!(claude.status, Some(Status::Waiting));
        assert_eq!(claude.project, "project");
        assert_eq!(claude.pane_id, "%2");
        assert_eq!(claude.window_id, "@1");
        assert!(!claude.current);
        assert_eq!(claude.dead, None);
        assert_eq!(claude.command, "sleep");
        let killed = &tree.projects[0].agents[1];
        assert_eq!(killed.dead, Some(Death::Signal("kill".into())));
        let zsh = &tree.projects[0].agents[2];
        assert_eq!(zsh.dead, Some(Death::Exit(3)));
    }

    #[test]
    fn current_follows_the_active_window() {
        let mut panes = panes();
        panes[2].window_active = true;
        let tree = Tree::build(&panes, &registry(&["project"]));
        assert!(tree.projects[0].agents[0].current);
    }

    #[test]
    fn agents_of_a_project_missing_from_the_registry_appear_unregistered_after_the_registered_ones()
    {
        let tree = Tree::build(&panes(), &registry(&["znaiko"]));
        assert_eq!(tree.projects.len(), 2);
        let project = &tree.projects[1];
        assert_eq!(project.name, "project");
        assert!(!project.registered);
        assert_eq!(
            project.path,
            PathBuf::from("/private/tmp"),
            "path comes from a live pane"
        );
        assert_eq!(project.agents.len(), 3);
    }

    #[test]
    fn sidebar_hint_unmarked_and_half_marked_panes_are_not_agents() {
        let mut panes = panes();
        panes[5].role = Some(Role::Agent);
        let tree = Tree::build(&panes, &registry(&["project"]));
        let total: usize = tree.projects.iter().map(|p| p.agents.len()).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn agents_are_searchable_by_project_and_name() {
        let tree = Tree::build(&panes(), &registry(&["project"]));
        assert_eq!(tree.agent("project", "zsh").unwrap().pane_id, "%3");
        assert!(tree.agent("project", "nope").is_none());
        assert!(tree.agent("nope", "zsh").is_none());
    }

    #[test]
    fn a_sidebar_is_alone_when_its_window_has_no_other_kind_of_pane() {
        let mut panes = panes();
        assert!(!sidebar_is_alone(&panes, "%1"), "home has a hint pane");
        panes.retain(|p| p.id != "%0");
        assert!(sidebar_is_alone(&panes, "%1"));
        assert!(
            !sidebar_is_alone(&panes, "%2"),
            "an agent pane is never alone in this sense"
        );
        assert!(!sidebar_is_alone(&panes, "%99"), "unknown pane");
    }

    #[test]
    fn a_sidebar_next_to_an_unmarked_pane_is_not_alone() {
        let mut panes = panes();
        panes.retain(|p| p.id != "%0");
        panes.push(Pane {
            id: "%7".into(),
            window_id: "@0".into(),
            window_name: "home".into(),
            window_active: true,
            pane_active: true,
            dead: None,
            command: "sleep".into(),
            path: String::new(),
            width: 80,
            index: 2,
            role: None,
            project: String::new(),
            agent: String::new(),
            status: None,
        });
        assert!(
            !sidebar_is_alone(&panes, "%1"),
            "a pane whose options are not set yet still counts"
        );
    }

    #[test]
    fn unique_names_add_a_counter_only_when_taken() {
        assert_eq!(unique_name("claude", &[]), "claude");
        assert_eq!(unique_name("claude", &["claude"]), "claude-2");
        assert_eq!(unique_name("claude", &["claude", "claude-2"]), "claude-3");
        assert_eq!(unique_name("claude", &["claude-2"]), "claude");
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
