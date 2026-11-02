//! The only module that spells "tmux": builds argv for the proper server and
//! parses what it says back.

use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};

/// One line per pane; fields separated by tabs in this exact order.
pub const PANE_FORMAT: &str = concat!(
    "#{pane_id}\t#{window_id}\t#{window_name}\t#{window_active}\t#{pane_active}\t",
    "#{pane_dead}\t#{pane_dead_status}\t#{pane_current_command}\t#{pane_current_path}\t",
    "#{pane_width}\t#{pane_index}\t#{@proper_role}\t#{@proper_project}\t#{@proper_agent}"
);
pub const FIELD_COUNT: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Sidebar,
    Hint,
    Agent,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Sidebar => "sidebar",
            Role::Hint => "hint",
            Role::Agent => "agent",
        }
    }

    fn parse(value: &str) -> Result<Option<Role>> {
        match value {
            "" => Ok(None),
            "sidebar" => Ok(Some(Role::Sidebar)),
            "hint" => Ok(Some(Role::Hint)),
            "agent" => Ok(Some(Role::Agent)),
            other => bail!("unknown @proper_role {other:?}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub window_id: String,
    pub window_name: String,
    pub window_active: bool,
    pub pane_active: bool,
    /// Exit status when the pane's process has exited and remain-on-exit kept it.
    pub dead: Option<i32>,
    pub command: String,
    pub path: String,
    pub width: u16,
    pub index: u16,
    pub role: Option<Role>,
    pub project: String,
    pub agent: String,
}

pub fn parse_panes(listing: &str) -> Result<Vec<Pane>> {
    listing.lines().map(parse_pane).collect()
}

fn parse_pane(line: &str) -> Result<Pane> {
    let fields: Vec<&str> = line.split('\t').collect();
    if fields.len() != FIELD_COUNT {
        bail!(
            "expected {FIELD_COUNT} fields, got {} in {line:?}",
            fields.len()
        );
    }
    let flag = |value: &str| value == "1";
    let dead = if flag(fields[5]) {
        Some(
            fields[6]
                .parse()
                .with_context(|| format!("pane_dead_status in {line:?}"))?,
        )
    } else {
        None
    };
    Ok(Pane {
        id: fields[0].to_string(),
        window_id: fields[1].to_string(),
        window_name: fields[2].to_string(),
        window_active: flag(fields[3]),
        pane_active: flag(fields[4]),
        dead,
        command: fields[7].to_string(),
        path: fields[8].to_string(),
        width: fields[9]
            .parse()
            .with_context(|| format!("pane_width in {line:?}"))?,
        index: fields[10]
            .parse()
            .with_context(|| format!("pane_index in {line:?}"))?,
        role: Role::parse(fields[11])?,
        project: fields[12].to_string(),
        agent: fields[13].to_string(),
    })
}

/// A tmux server addressed by socket name (`tmux -L <socket>`).
#[derive(Debug, Clone)]
pub struct Server {
    socket: String,
}

impl Server {
    pub fn new(socket: impl Into<String>) -> Server {
        Server {
            socket: socket.into(),
        }
    }

    pub fn socket(&self) -> &str {
        &self.socket
    }

    pub fn argv<I, S>(&self, args: I) -> Vec<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        ["tmux", "-L", &self.socket]
            .into_iter()
            .map(str::to_string)
            .chain(args.into_iter().map(|a| a.as_ref().to_string()))
            .collect()
    }

    pub fn argv_list_panes(&self) -> Vec<String> {
        self.argv(["list-panes", "-a", "-F", PANE_FORMAT])
    }

    /// Runs a tmux command and returns its stdout; tmux's stderr becomes the error.
    pub fn run(&self, argv: &[String]) -> Result<String> {
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .with_context(|| format!("failed to spawn {}", argv.join(" ")))?;
        if !output.status.success() {
            return Err(anyhow!(
                "{} failed: {}",
                argv.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn list_panes(&self) -> Result<Vec<Pane>> {
        parse_panes(&self.run(&self.argv_list_panes())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../tests/fixtures/panes.tsv");

    #[test]
    fn parses_every_pane_from_a_real_listing() {
        let panes = parse_panes(FIXTURE).unwrap();
        assert_eq!(panes.len(), 5);
    }

    #[test]
    fn parses_a_sidebar_pane() {
        let panes = parse_panes(FIXTURE).unwrap();
        let sidebar = &panes[0];
        assert_eq!(sidebar.id, "%1");
        assert_eq!(sidebar.window_id, "@0");
        assert_eq!(sidebar.window_name, "home");
        assert!(sidebar.window_active);
        assert!(!sidebar.pane_active);
        assert_eq!(sidebar.dead, None);
        assert_eq!(sidebar.command, "sleep");
        assert_eq!(sidebar.path, "/Users/tagrudev/Projects/proper");
        assert_eq!(sidebar.width, 30);
        assert_eq!(sidebar.index, 0);
        assert_eq!(sidebar.role, Some(Role::Sidebar));
        assert_eq!(sidebar.project, "");
        assert_eq!(sidebar.agent, "");
    }

    #[test]
    fn parses_an_agent_pane() {
        let panes = parse_panes(FIXTURE).unwrap();
        let agent = &panes[2];
        assert_eq!(agent.window_name, "project/claude");
        assert!(!agent.window_active);
        assert_eq!(agent.role, Some(Role::Agent));
        assert_eq!(agent.project, "project");
        assert_eq!(agent.agent, "claude");
    }

    #[test]
    fn parses_a_dead_pane_with_its_exit_status_and_empty_path() {
        let panes = parse_panes(FIXTURE).unwrap();
        let dead = &panes[3];
        assert_eq!(dead.dead, Some(3));
        assert_eq!(dead.command, "sh");
        assert_eq!(dead.path, "");
    }

    #[test]
    fn unmarked_pane_has_no_role() {
        let panes = parse_panes(FIXTURE).unwrap();
        assert_eq!(panes[4].role, None);
    }

    #[test]
    fn rejects_a_line_with_the_wrong_field_count() {
        assert!(parse_panes("%1\t@0\thome\n").is_err());
    }

    #[test]
    fn rejects_an_unknown_role() {
        let line = "%1\t@0\thome\t1\t0\t0\t\tsleep\t/x\t30\t0\tbogus\t\t\n";
        assert!(parse_panes(line).is_err());
    }

    #[test]
    fn empty_listing_is_no_panes() {
        assert_eq!(parse_panes("").unwrap().len(), 0);
    }

    #[test]
    fn list_panes_argv_targets_the_socket_and_uses_the_pane_format() {
        let server = Server::new("proper");
        assert_eq!(
            server.argv_list_panes(),
            vec![
                "tmux",
                "-L",
                "proper",
                "list-panes",
                "-a",
                "-F",
                PANE_FORMAT
            ]
        );
    }

    #[test]
    fn pane_format_has_one_placeholder_per_field() {
        assert_eq!(PANE_FORMAT.split('\t').count(), FIELD_COUNT);
    }
}
