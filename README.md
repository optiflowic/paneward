# paneward

Watch over the AI coding agents running in your terminal panes.

paneward is a terminal UI that lists every agent session (Claude Code, Codex, ...)
across your terminal panes, shows which ones are running, waiting for you, or idle,
and jumps to the pane you pick.

> [!NOTE]
> Early development. Nothing is released yet.

## Goals

- **See** every agent session and its status at a glance.
- **Jump** to the pane of the session you pick.
- **Launch** a new agent session in a new pane for a project you pick (planned).

Agent-agnostic and terminal-agnostic by design: agents plug in through hooks that
write a [shared state file format](docs/spec/state-format.md), and terminals plug in
through a backend.

## Non-goals

- Orchestrating agents: sending prompts, collecting results, managing their work.
- Replacing your terminal or multiplexer.

## Usage

### Requirements

- [WezTerm](https://wezterm.org). paneward lists its panes and focuses them with
  `wezterm cli`. Other terminals are planned.
  On Windows, run paneward inside WSL and forward the pane id from WezTerm with
  `WSLENV=WEZTERM_PANE/u`; paneward then uses `wezterm.exe`.
- A hook for each agent that records its sessions as state files under
  `${XDG_STATE_HOME:-$HOME/.local/state}/paneward/`, in the
  [state file format](docs/spec/state-format.md). No hook ships with paneward yet:
  write one against that spec (Claude Code, for example, can run a command on its
  hook events). paneward only reads these files; it does not install hooks or
  change agent settings.
- Rust 1.99 or later to build it.

### Install and run

```sh
cargo install --locked --git https://github.com/optiflowic/paneward
paneward
```

Run it in a WezTerm split next to your agents. paneward stays open, so a narrow
split on the side (sidebar) or a short one at the bottom (dock) works well.

```text
paneward [--layout auto|sidebar|dock]
```

| Layout    | Shape |
|-----------|-------|
| `auto`    | Default. Dock when the pane is more than twice as wide as tall (in cells), sidebar otherwise. Follows resizes. |
| `sidebar` | Two lines per session, for a narrow, tall split. |
| `dock`    | One line per session, for a wide, short split. |

### Keys

| Key                     | Action |
|-------------------------|--------|
| `j` / `k`, `↓` / `↑`    | Move the selection |
| `Enter`                 | Jump to the selected session's pane (switching tabs if needed) |
| `1`-`9`                 | Jump to the session with that number |
| `q`, `Esc`, `Ctrl-C`    | Quit |

### What the list shows

Each session shows its number (1-9, for the number keys), status, project (the last
part of its working directory, or `pane <id>` without one), agent, and how long it has had its current status (for sessions already
running when paneward starts, counted from their state file's last update).

| Icon | Status    | Meaning |
|------|-----------|---------|
| `●`  | `waiting` | The agent is waiting for you, e.g. on a permission prompt. |
| `◐`  | `running` | The agent is working. |
| `○`  | `idle`    | The agent finished its turn. |

Sessions needing you come first: `waiting`, then `running`, then `idle`, each by
project name. The list updates as soon as a state file changes, or within a few
seconds when paneward cannot watch the state directory (e.g. it does not exist
yet). Sessions whose
pane is gone (closed pane, terminal restart) are hidden within a few seconds, the
next time paneward lists the panes. If listing the panes fails, paneward keeps the
last list it got and shows the error. Outside WezTerm the list stays empty, because
paneward cannot check which panes exist.

## Roadmap

| Version | Scope |
|---------|-------|
| v0.1    | Session list, status, jump to pane. Claude Code on WezTerm. Sidebar and dock layouts. |
| v0.2    | Launch sessions. Codex support. |
| later   | tmux backend, git worktree launch. |

## Development

The Rust toolchain is pinned in `rust-toolchain.toml`; [rustup](https://rustup.rs)
picks it up automatically. The other development tools (cargo-deny, zizmor) are
pinned in `mise.toml`; install them with [mise](https://mise.jdx.dev):

```sh
mise install
```

`make` needs those tools on `PATH`: either
[activate mise in your shell](https://mise.jdx.dev/getting-started.html#activate-mise)
or run it as `mise exec -- make`.

```sh
make        # fmt-check + lint + test + deny + audit (CI also runs make build)
make fmt    # format
make run    # run from source
```

## License

[MIT](LICENSE)
