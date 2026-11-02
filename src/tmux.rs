//! The only module that spells "tmux": builds argv for the proper server and
//! parses what it says back.

use std::fmt;
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};

/// One line per pane; fields separated by tabs in this exact order.
pub const PANE_FORMAT: &str = concat!(
    "#{pane_id}\t#{window_id}\t#{window_name}\t#{window_active}\t#{pane_active}\t",
    "#{pane_dead}\t#{pane_dead_status}\t#{pane_dead_signal}\t#{pane_current_command}\t#{pane_current_path}\t",
    "#{pane_width}\t#{pane_index}\t#{@proper_role}\t#{@proper_project}\t#{@proper_agent}"
);
pub const FIELD_COUNT: usize = 15;

/// How a pane's process ended; tmux keeps the pane around because of remain-on-exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Death {
    Exit(i32),
    Signal(String),
}

impl fmt::Display for Death {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Death::Exit(status) => write!(f, "exit {status}"),
            Death::Signal(name) => write!(f, "signal {name}"),
        }
    }
}

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
    pub dead: Option<Death>,
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
    let dead = match (flag(fields[5]), fields[6], fields[7]) {
        (false, _, _) => None,
        (true, status, "") if !status.is_empty() => Some(Death::Exit(
            status
                .parse()
                .with_context(|| format!("pane_dead_status in {line:?}"))?,
        )),
        (true, "", signal) if !signal.is_empty() => Some(Death::Signal(signal.to_string())),
        (true, _, _) => bail!("dead pane without a status or a signal in {line:?}"),
    };
    Ok(Pane {
        id: fields[0].to_string(),
        window_id: fields[1].to_string(),
        window_name: fields[2].to_string(),
        window_active: flag(fields[3]),
        pane_active: flag(fields[4]),
        dead,
        command: fields[8].to_string(),
        path: fields[9].to_string(),
        width: fields[10]
            .parse()
            .with_context(|| format!("pane_width in {line:?}"))?,
        index: fields[11]
            .parse()
            .with_context(|| format!("pane_index in {line:?}"))?,
        role: Role::parse(fields[12])?,
        project: fields[13].to_string(),
        agent: fields[14].to_string(),
    })
}

/// The socket name inside a `$TMUX` value such as `/tmp/tmux-501/proper,123,0`.
pub fn socket_name(tmux_env: &str) -> Option<String> {
    let path = tmux_env.split(',').next()?;
    let name = std::path::Path::new(path).file_name()?;
    Some(name.to_string_lossy().into_owned())
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

    /// `PROPER_SOCKET`, else the socket of the tmux server this process runs inside,
    /// else the default `proper` socket.
    pub fn from_env() -> Server {
        let socket = std::env::var("PROPER_SOCKET")
            .ok()
            .or_else(|| {
                std::env::var("TMUX")
                    .ok()
                    .and_then(|tmux| socket_name(&tmux))
            })
            .unwrap_or_else(|| "proper".to_string());
        Server::new(socket)
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
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let cause = if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            };
            return Err(anyhow!(cause).context(format!("{} failed", argv.join(" "))));
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
        assert_eq!(panes.len(), 6);
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
        assert_eq!(dead.dead, Some(Death::Exit(3)));
        assert_eq!(dead.command, "sh");
        assert_eq!(dead.path, "");
    }

    #[test]
    fn parses_a_pane_killed_by_a_signal() {
        let panes = parse_panes(FIXTURE).unwrap();
        let killed = &panes[4];
        assert_eq!(killed.dead, Some(Death::Signal("kill".into())));
        assert_eq!(killed.agent, "killed");
        assert_eq!(killed.dead.as_ref().unwrap().to_string(), "signal kill");
        assert_eq!(Death::Exit(3).to_string(), "exit 3");
    }

    #[test]
    fn rejects_a_dead_pane_with_neither_status_nor_signal() {
        let line = "%1\t@0\thome\t1\t0\t1\t\t\tsleep\t/x\t30\t0\tagent\tp\ta\n";
        assert!(parse_panes(line).is_err());
    }

    #[test]
    fn unmarked_pane_has_no_role() {
        let panes = parse_panes(FIXTURE).unwrap();
        assert_eq!(panes[5].role, None);
    }

    #[test]
    fn rejects_a_line_with_the_wrong_field_count() {
        assert!(parse_panes("%1\t@0\thome\n").is_err());
    }

    #[test]
    fn rejects_an_unknown_role() {
        let line = "%1\t@0\thome\t1\t0\t0\t\t\tsleep\t/x\t30\t0\tbogus\t\t\n";
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
    fn socket_name_comes_from_the_tmux_variable() {
        assert_eq!(
            socket_name("/private/tmp/tmux-501/proper,4242,0"),
            Some("proper".into())
        );
        assert_eq!(
            socket_name("/tmp/tmux-501/proper-test-1-2,1,0"),
            Some("proper-test-1-2".into())
        );
        assert_eq!(socket_name(""), None);
    }

    #[test]
    fn pane_format_has_one_placeholder_per_field() {
        assert_eq!(PANE_FORMAT.split('\t').count(), FIELD_COUNT);
    }
}
