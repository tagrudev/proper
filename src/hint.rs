//! The placeholder pane shown next to the sidebar when no agent is selected.

use crate::config::Launcher;

const BEFORE_LAUNCHERS: &str = "
  proper

    /   find a project and start
        claude in it

  On a project or agent in the sidebar:

    n   start claude there
    s   open a shell there
";

const AFTER_LAUNCHERS: &str = "
  or from any terminal:

    proper new <project> [-- cmd...]

  M-Tab or prefix Tab moves focus
  between sidebar and agent.
  q in the sidebar detaches; agents
  keep running.
";

/// The hint with one line per launcher under the built-in programs.
pub fn text(launchers: &[Launcher]) -> String {
    let launchers: String = launchers
        .iter()
        .map(|l| format!("    {}   start {} there\n", l.key, l.name))
        .collect();
    format!("{BEFORE_LAUNCHERS}{launchers}{AFTER_LAUNCHERS}")
}

/// Prints the hint and parks forever; tmux keeps the pane as long as the process lives.
pub fn run(launchers: &[Launcher]) -> ! {
    print!("{}", text(launchers));
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launchers_are_listed_under_the_built_in_programs() {
        let reviewer = Launcher {
            key: 'a',
            name: "reviewer".into(),
            command: vec!["reviewer".into()],
        };
        assert!(text(&[reviewer]).contains(
            "    s   open a shell there\n    a   start reviewer there\n\n  or from any terminal:"
        ));
        assert!(text(&[]).contains("    s   open a shell there\n\n  or from any terminal:"));
    }
}
