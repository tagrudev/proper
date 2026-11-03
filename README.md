# proper

A tmux workspace for you and your coding agents.

proper runs its own tmux server with one session. Every agent gets a window
made of two panes: a sidebar on the left listing your projects and the agents
running in each, and the agent itself on the right. The sidebar is a real tmux
pane, the agent is a real tmux pane, so scrollback, mouse, colours and resizing
all come from tmux. Quitting proper only detaches; the agents keep running and
`proper` brings you back to them.

## Install

Requires tmux 3.2 or newer and a Rust toolchain.

```bash
cargo install --path .
```

Install to a stable path (`~/.cargo/bin/proper` is fine). Sidebar panes run the
binary that started them, so a binary that moves breaks respawn.

## Use

```bash
proper add ~/Projects/project          # register a project (name = directory name)
proper add . --name api              # or pick the name
proper                               # bring the server up and attach
proper new project                     # start claude in project
proper new project -- npm run dev      # or any command
proper new --name reviewer           # claude in the project containing the cwd
proper ls                            # project, agent, state, command, pane
proper kill project/claude
proper rm project                      # refuses while project has agents
```

Press `q` in the sidebar, or `prefix d`, to leave. The server stays up. To shut
everything down for real: `tmux -L proper kill-server`.

Your `~/.tmux.conf` (and `~/.config/tmux/tmux.conf`) are sourced first, so your
prefix and bindings work. proper then turns the status bar off, enables the
mouse, keeps crashed panes around, draws every pane border in the same grey
and pins the sidebar at 30 columns.

An agent that exits cleanly closes its window. One that crashes or is killed
stays as a dead pane so you can read what happened (`prefix [` scrolls back)
and press `r` to restart it.

Inside an agent pane `TMUX` is empty, so `tmux` and tools built on it, such as
tmuxinator, talk to your default server exactly as they would from a plain
terminal. A tmuxinator project started from inside proper therefore attaches
nested inside that pane rather than taking over proper's session. Run it from
a separate terminal tab if you want it full size.

## Keys

In the sidebar:

| key       | action                                        |
| --------- | --------------------------------------------- |
| `j` / `k` | move                                          |
| `Enter`   | open the agent under the cursor               |
| `n`       | start claude in the project under the cursor  |
| `s`       | start `$SHELL` in the project under the cursor|
| `/`       | find a project and start claude in it         |
| `x`       | kill the agent under the cursor (asks first)  |
| `r`       | respawn a dead agent in its pane              |
| `q`       | detach                                        |
| `?`       | help                                          |

In the finder, typing keeps the registered projects whose names hold the
letters in order (`tdv` finds `todor.dev`), `↑` / `↓` pick one, `Enter`
starts claude in it and `Esc` closes the finder.

With the mouse: click an agent row to open it, click a project row to move the
cursor, and the wheel scrolls the list.

Anywhere: `M-Tab` or `prefix Tab` toggles focus between sidebar and agent. A
mouse click on the sidebar does the same. The sidebar's cursor row turns grey
while the sidebar has focus and goes plain when the agent does. `M-Tab` needs
your terminal to send Option as Meta (iTerm2: "Esc+", Terminal.app: "Use
Option as Meta key", Ghostty: `macos-option-as-alt`).

Projects with agents are bold, empty ones dim. Agents hang under their
project as `├ ● name command`; the command shows only when it differs from
the name. A crashed agent reads `✗ name exit 3` (or `signal kill`) in red.
Agents that report their state (see below) spin in green while working,
show a yellow `◌` when they have finished and wait for you, and a red `!`
when they need an answer or a permission. A blue `◂` at the right edge marks
the agent whose window is on screen. A project shown dim with `*` has agents
but is no longer in `projects.toml`.

## Sounds and agent state

`proper status <working|waiting|attention>` run inside an agent pane marks
that agent in every sidebar and, for `waiting` and `attention`, plays a sound.
Outside a proper pane it does nothing, so it is safe in any Claude Code
session. Wire it to Claude Code hooks in `~/.claude/settings.json`:

```json
"hooks": {
  "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "proper status working" }] }],
  "Stop":             [{ "hooks": [{ "type": "command", "command": "proper status waiting" }] }],
  "Notification":     [{ "matcher": "permission_prompt|elicitation_dialog|agent_needs_input",
                         "hooks": [{ "type": "command", "command": "proper status attention" }] }]
}
```

The sound is `/System/Library/Sounds/Glass.aiff` played with `afplay`. Set
`PROPER_SOUND` to another file to change it, or to an empty string to turn it
off. The variable is read where the hook runs, so put it in your shell
profile.

## How it fits together

- Server socket `proper` (`tmux -L proper`), session `proper`.
- Window `home` holds the sidebar and a hint pane; it anchors the session so
  killing every agent never kills the server.
- Each agent window is named `project/agent`. Panes carry `@proper_role`,
  `@proper_project` and `@proper_agent` options; that is the only state besides
  `~/.config/proper/projects.toml`.
- Every sidebar re-reads `tmux list-panes -a` when a tmux hook signals a
  change (window switch, focus moving between panes, new or dead pane,
  options set) and once a second as a fallback, so all of them agree without
  talking to each other. The hooks signal the `proper-changed` wait-for
  channel; each sidebar blocks on it.
- `proper` heals before attaching: a window that lost its sidebar gets one
  back, a crashed sidebar is respawned, a window left with only a sidebar is
  closed.

Environment: `PROPER_SOCKET` and `PROPER_CONFIG_DIR` override the socket name
and the config directory (`~/.config/proper`).

## Develop

```bash
cargo test                    # unit tests plus integration tests on throwaway tmux servers
cargo clippy --all-targets -- -D warnings
```

Integration tests start real tmux servers on sockets named
`proper-test-<pid>-<n>` and kill them when done. No mocks.
