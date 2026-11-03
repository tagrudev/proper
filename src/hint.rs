//! The placeholder pane shown next to the sidebar when no agent is selected.

const TEXT: &str = "
  proper

    /   find a project and start
        claude in it

  On a project or agent in the sidebar:

    n   start claude there
    s   open a shell there

  or from any terminal:

    proper new <project> [-- cmd...]

  M-Tab or prefix Tab moves focus
  between sidebar and agent.
  q in the sidebar detaches; agents
  keep running.
";

/// Prints the hint and parks forever; tmux keeps the pane as long as the process lives.
pub fn run() -> ! {
    print!("{TEXT}");
    loop {
        std::thread::park();
    }
}
