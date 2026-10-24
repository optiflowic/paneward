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

## Roadmap

| Version | Scope |
|---------|-------|
| v0.1    | Session list, status, jump to pane. Claude Code on WezTerm. Sidebar and dock layouts. |
| v0.2    | Launch sessions. Codex support. |
| later   | tmux backend, git worktree launch. |

## Development

The toolchain is pinned in `rust-toolchain.toml`; [rustup](https://rustup.rs)
picks it up automatically.

```sh
make        # fmt-check + lint + test + deny (CI also runs make build)
make fmt    # format
make run    # run from source
```

`make deny` needs [cargo-deny](https://github.com/EmbarkStudios/cargo-deny).

## License

[MIT](LICENSE)
