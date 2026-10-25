# paneward

Watch over the AI coding agents running in your terminal panes.

paneward is a terminal UI that lists every agent session (Claude Code, Codex, ...)
across your terminal panes, shows which ones are running, waiting for you, or idle,
and jumps to the pane you pick.

> [!NOTE]
> Early development: expect breaking changes before 1.0.

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
  `WSLENV=WEZTERM_PANE/u`; paneward then uses `wezterm.exe`. WSL support is not
  tested yet.
- A hook for each agent that records its sessions as state files under
  `${XDG_STATE_HOME:-$HOME/.local/state}/paneward/`, in the
  [state file format](docs/spec/state-format.md). No hook ships with paneward yet:
  write one against that spec (Claude Code, for example, can run a command on its
  hook events). Have the hook write `terminal_instance` (for WezTerm, the value of
  `WEZTERM_UNIX_SOCKET`): without it, an old session left by a crash can show up
  again after WezTerm restarts and reuses its pane id. paneward only reads these
  files; it does not install hooks or change agent settings.
- Rust 1.99 or later, only to build from source.

### Install

Each [release](https://github.com/optiflowic/paneward/releases) from v0.1.1 on has
an archive per platform, named `paneward-<version>-<target>.tar.gz` (`<version>`
includes the `v`, e.g. `v0.1.1`):

| Platform                | `<target>` |
|-------------------------|------------|
| macOS (Apple silicon)   | `aarch64-apple-darwin` |
| macOS (Intel)           | `x86_64-apple-darwin` |
| Linux, WSL (x86_64)     | `x86_64-unknown-linux-musl` |
| Linux, WSL (arm64)      | `aarch64-unknown-linux-musl` |

The Linux binaries are static, so they run on any distribution. To install the
latest release with the [GitHub CLI](https://cli.github.com), for example on Apple
silicon:

```sh
target=aarch64-apple-darwin
cd "$(mktemp -d)"
gh release download --repo optiflowic/paneward --pattern "paneward-*-$target.tar.gz*"
shasum -a 256 -c paneward-*-"$target".tar.gz.sha256   # Linux: sha256sum -c
gh attestation verify paneward-*-"$target".tar.gz --repo optiflowic/paneward
tar -xzf paneward-*-"$target".tar.gz
mkdir -p ~/.local/bin && mv paneward ~/.local/bin/
```

`gh attestation verify` checks that the archive was built by this repository's
release workflow. Rarely, a release has no attestation; the checksum still applies.
If `paneward` is not found afterwards, add `~/.local/bin` to your
`PATH`, e.g. with `export PATH="$HOME/.local/bin:$PATH"` in your shell's startup
file.

The macOS binaries are not notarized. If you download an archive with a browser,
macOS may refuse to run it; remove the quarantine flag with
`xattr -d com.apple.quarantine ~/.local/bin/paneward`.

Or build from source (`<version>` as above; v0.1.0 is only available this way):

```sh
cargo install --locked --git https://github.com/optiflowic/paneward --tag paneward-<version>
```

### Run

```sh
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
part of its working directory, or `pane <id>` without one), agent, and how long it
has had its current status. For sessions that existed before paneward started, that
time counts from their state file's last update.

| Icon | Status    | Meaning |
|------|-----------|---------|
| `●`  | `waiting` | The agent is waiting for you, e.g. on a permission prompt. |
| `◐`  | `running` | The agent is working. |
| `○`  | `idle`    | The agent finished its turn. |

Sessions needing you come first: `waiting`, then `running`, then `idle`, each by
project name. The list updates as soon as a state file changes, or within a few
seconds when paneward cannot watch the state directory (e.g. it does not exist
yet). Sessions whose pane is gone (closed pane, terminal restart) are hidden within
a few seconds, the next time paneward lists the panes. If listing the panes fails,
paneward shows the error and keeps using the last pane list it got: state file
changes still show up, but panes opened or closed since then are not noticed.
Without `WEZTERM_PANE` (outside WezTerm, or in WSL without the `WSLENV` forwarding)
the list stays empty, because paneward cannot check which panes exist.

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
