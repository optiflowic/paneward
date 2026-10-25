# paneward

Terminal UI that watches over AI coding agent sessions in terminal panes:
see their status, jump to them, and (later) launch them.

## Non-Negotiable Constraints

- **Agent-agnostic.** Never assume Claude Code in core code. Agents only exist as
  the `agent` field of state files written by external hooks.
- **Terminal-agnostic.** Everything that talks to a terminal goes through the
  `Terminal` trait. Only backends in `src/terminal/` may run terminal CLIs
  (`wezterm cli`, ...).
- **The state file format is a public contract.** `docs/spec/state-format.md` is the
  source of truth. Change the spec first, keep readers tolerant (skip what they
  do not understand), and bump `version` only for incompatible changes.
- **Never fail on a machine without agents.** A missing state directory or a
  missing terminal backend is a normal, empty state, not an error.
- **Read-only toward agents.** paneward never writes state files and never sends
  input to agents.

## Architecture

Dependency direction is strictly one-way: `ui → app → {state, terminal}`.
Lower layers never `use` upper layers.

```
src/
  main.rs       -- CLI parsing (clap), terminal setup/restore, event loop
  app.rs        -- application state: session list, selection, key handling.
                -- Knows NOTHING about drawing.
  ui.rs         -- rendering with ratatui (sidebar / dock layouts).
                -- Reads app state only; never runs commands or touches files.
  state.rs      -- state file reading and parsing (docs/spec/state-format.md).
                -- Pure, unit-testable; knows NOTHING about terminals.
  terminal/     -- terminal backends behind the `Terminal` trait
    mod.rs      -- trait + backend detection
    wezterm.rs  -- `wezterm cli` (list panes, focus a pane)
```

## Coding Standards

- Format: `cargo fmt`. Lint: `cargo clippy` with `clippy::pedantic` (configured in
  `Cargo.toml`), warnings are errors. Both must pass before commit.
- `unsafe` is forbidden (`[lints.rust]` in `Cargo.toml`).
- Errors: use `anyhow` with context (`.context("...")`) at boundaries. Expected
  "nothing to show" cases (missing directory, broken file, unknown version) are not
  errors: skip them.
- No `unwrap()` / `expect()` / panicking indexing outside tests, unless the invariant
  is stated in a comment next to it.
- Guard clauses and `let ... else` over nested `if`. Small functions. If a function
  needs comments explaining "sections", split it.
- Readability over cleverness: names state intent, one responsibility per function,
  comments explain *why*. (Principles from "The Art of Readable Code".)
- No speculative code: no unused abstractions, extension points, or options added
  "for later". Every line must serve the change at hand.

## Spec Memo Workflow

Before implementing against an external contract, write/update a memo in `docs/spec/`:

- `docs/spec/state-format.md` — the state file format shared with agent hooks.
- One memo per terminal backend when it is added (CLI commands and output used,
  known quirks).

Implementation must cite the memo, not raw upstream docs. If reality diverges from
the memo, fix the memo in the same PR.

## Testing

- Unit test targets: state parsing (valid, broken, unknown version/status, unknown
  fields), sorting and filtering, time formatting, backend output parsing.
- Tests touching the filesystem use `tempfile`, never the real state directory.
- NOT tested automatically: real terminal interaction and TUI drawing. The manual
  checklist lives in `docs/manual-testing.md` — run it before tagging a release.
- Bug fixes include a regression test when the bug is in pure logic; if
  terminal-dependent, add a step to `docs/manual-testing.md` instead.
- No coverage metric. Quality gates: unit tests for the cases above, `make` clean,
  and the manual checklist before release.
- Run: `make` (`fmt-check`, `lint`, `test`, `deny`, `audit`). CI also runs
  `make build`. Tests must pass on Linux and macOS.

## Definition of Done (per feature)

1. Spec memo updated if the state format or a terminal backend is involved
2. Unit tests for pure logic; manual checklist updated if terminal behavior changed
3. `make` clean (`fmt-check`, `lint`, `test`, `deny`, `audit`)
4. README updated if user-facing (commands, keys, layouts, supported agents/terminals)

## Workflow

- Never commit on `main`. Open an issue, create a branch named
  `<type>/<issue-number>-<short-description>`, where `<type>` is the Conventional
  Commits type of the change (`feat/12-state-module`, `fix/15-…`, `docs/16-…`), and
  open a pull request.
- Commits: Conventional Commits (`feat:`, `fix:`, `docs:`, `refactor:`, `test:`,
  `chore:`, `ci:`). Small, single-purpose commits.
- CI (GitHub Actions): lint (`fmt-check`, `clippy`), security (`cargo deny`),
  workflow audit (zizmor), and test + build on Linux and macOS. All green before merge.
- Versioning follows semver. Before 1.0, a breaking change bumps MINOR. Mark it with
  `!` or a `BREAKING CHANGE:` footer.
- Releases: [release-please](https://github.com/googleapis/release-please)
  (`.github/workflows/release-please.yaml`) keeps a release PR open, bumping
  `Cargo.toml`, `.release-please-manifest.json` and `CHANGELOG.md`. Never edit
  those version fields or `CHANGELOG.md` by hand. Run `docs/manual-testing.md`
  before merging the release PR. When a release is created, the same workflow
  builds the binaries and attaches them to it; to attach them to an existing
  release, run the workflow by hand with its tag.
- The toolchain is pinned in `rust-toolchain.toml`; `rust-version` in `Cargo.toml`
  is the MSRV. `Cargo.lock` is committed.
- Development tools (`cargo-deny`, `zizmor`) are pinned in `mise.toml`, the single
  source of their versions: CI installs them from it with `jdx/mise-action`. Never
  pin a tool version in a workflow.
- Dependabot updates crates and GitHub Actions (with a 7-day cooldown). It does
  NOT update these pins; bump them by hand: the Rust version in
  `rust-toolchain.toml`, the tools in `mise.toml`, and the mise version passed to
  `jdx/mise-action` in `.github/workflows/ci.yaml`.
- When a spec is ambiguous or two valid designs conflict with these rules:
  STOP and ask. Do not pick silently. Present options with trade-offs.

## Out of Scope (do not implement without explicit approval)

- Orchestrating agents: sending prompts, approving permissions, collecting results
- Writing or modifying agent configuration (hooks are installed by the user)
- Terminal backends other than WezTerm before v0.2
- API usage / quota display
