# WezTerm backend

What paneward needs from WezTerm and how it gets it. The backend
(`src/terminal/wezterm.rs`) follows this memo; if WezTerm behaves differently, fix
this memo in the same change.

Checked with `wezterm 20260716-195552-76b606ec` on macOS. The WSL section below is
not checked yet; see `docs/manual-testing.md`.

## Detection

WezTerm sets `WEZTERM_PANE` (the pane id of the shell) in every pane it spawns.
paneward uses the WezTerm backend when `WEZTERM_PANE` is set and non-empty. Without
it there is no backend: sessions cannot be checked against live panes, and jumping
is unavailable.

The state files name this backend `wezterm` (the `terminal` field).

## Terminal instance

The state file's `terminal_instance` for WezTerm is the value of
`WEZTERM_UNIX_SOCKET`, which WezTerm also sets in every pane: the socket of the
process that owns the panes, e.g. `$HOME/.local/share/wezterm/gui-sock-<pid>`.
`<pid>` is that process's id, so the value changes when WezTerm starts again.
The hook reads it from its environment; paneward reads its own, since it runs in a
pane of the same WezTerm. Without the variable (empty or unset), the instance is
unknown.

Not checked: panes of a separate `wezterm-mux-server` are expected to carry the
server's socket, which survives GUI restarts, as their panes do.

## Commands

Every command is run as `wezterm cli --no-auto-start <subcommand>` (`wezterm.exe`
inside WSL, see below).
`--no-auto-start` keeps `wezterm cli` from starting a mux server when none is
running; paneward only talks to the running GUI.

### List panes

```sh
wezterm cli --no-auto-start list --format json
```

Prints a JSON array with one object per pane, across all windows, tabs and
workspaces. paneward reads two fields and ignores the rest:

| Field     | Type   | Use |
|-----------|--------|-----|
| `pane_id` | number | Live pane id. Compared with the state file's `pane_id` (a string) as its decimal form. |
| `tab_id`  | number | Tab that holds the pane, needed to focus panes in another tab. |

Excerpt:

```json
[
  {
    "window_id": 0,
    "tab_id": 5,
    "pane_id": 16,
    "workspace": "default",
    "size": { "rows": 62, "cols": 102, "pixel_width": 1428, "pixel_height": 1736, "dpi": 144 },
    "title": "zsh",
    "cwd": "file://host/Users/me/src/paneward",
    "is_active": true,
    "is_zoomed": false,
    "tty_name": "/dev/tty-example"
  }
]
```

### Focus a pane

```sh
wezterm cli --no-auto-start activate-tab --tab-id <tab_id>
wezterm cli --no-auto-start activate-pane --pane-id <pane_id>
```

`<tab_id>` comes from `list` above. Both exit with status 0 on success and print
nothing.

### Spawn a tab

```sh
wezterm cli --no-auto-start spawn --cwd <dir> -- <shell> -lic <command>
```

Opens a new tab in the window of the current pane (`WEZTERM_PANE`), in the same
domain, and prints the new pane's id. paneward does not use the id. The new tab is
expected to become the active tab of that window; not checked yet
(`docs/manual-testing.md`, "Launch").
`wezterm cli` resolves a relative `--cwd` against its own working directory
(inherited from paneward) before sending it to the GUI (`wezterm/src/cli/mod.rs`,
`resolve_relative_cwd`).

The program runs in the environment of the WezTerm GUI, not of the `wezterm cli`
caller. A GUI started from the macOS Dock has a minimal `PATH`, so the command is
run through the user's login shell: `<shell>` is paneward's `SHELL`, or `/bin/sh`
when it is unset or empty. `-lic` makes it a login, interactive shell, so both
login files and rc files (where `PATH` is often set, e.g. `~/.zshrc`) are read.
sh, bash, zsh and fish accept the combined `-lic`; shells that do not (e.g.
nushell) are not supported.
When `<command>` exits, the shell exits and WezTerm closes the tab.

Not checked: inside WSL the tab is expected to open in the WSL domain of the
current pane, with `<dir>` a Linux path.

## WSL

Not checked yet. This describes the expected setup; fix it once the WSL items in
`docs/manual-testing.md` have been run.

- **Setup.** The Windows WezTerm runs a WSL distribution as its domain. WezTerm sets
  `WEZTERM_PANE` on the Windows side only; the user must forward it into WSL with
  `WSLENV` (e.g. `WSLENV=WEZTERM_PANE/u`), or neither the hook nor paneward sees it.
- **Executable.** Inside WSL, paneward runs `wezterm.exe` (the Windows binary,
  through WSL interop) instead of `wezterm`, which is usually not installed there.
  WSL is detected by `WSL_DISTRO_NAME` being set and non-empty. A Linux WezTerm
  running inside WSL (WSLg) is not supported by this rule.
- **Ids.** `wezterm.exe cli list` is expected to report the same `pane_id` as the
  forwarded `WEZTERM_PANE` (and so as the state files). Unverified.
- **Terminal instance.** `WSLENV=WEZTERM_PANE/u` forwards only the pane id, so
  `WEZTERM_UNIX_SOCKET` is unknown inside WSL and files are matched by pane alone.
  Forwarding it as well (with `/p` to translate the path) is untested.
- **Startup time.** Interop starts a Windows process for every call, which is slower
  than on macOS or Linux. The 2-second timeout (see Quirks) also applies here.

## Quirks

- **Tab switching.** Whether `activate-pane` alone switches to the pane's tab could
  not be confirmed from the CLI: `wezterm cli list-clients` keeps reporting the old
  `focused_pane_id` after either command. paneward therefore always activates the
  tab first. Checked by hand on macOS: jumping to a session in another tab switches
  to that tab and focuses the pane (`docs/manual-testing.md`, "Jump").
- **Missing pane or tab.** An unknown id exits with status 1 and logs to stderr,
  e.g. `ERROR wezterm > unexpected response ... "Error: pane 99999 not found"` or
  `could not determine which pane should be active for tab 99999`. paneward shows
  the failure as a message.
- **Inherited `WEZTERM_PANE`.** The variable survives into tmux inside WezTerm and
  into `ssh` sessions that forward it, where `wezterm cli` may be missing or unable
  to reach the GUI. Detection only checks the variable; a failing `wezterm cli`
  surfaces as an error message in paneward, not as a crash.
- **Hangs.** When `wezterm cli` cannot reach the GUI it may hang instead of failing
  (e.g. over ssh). paneward waits at most 2 seconds, then kills it and shows an
  error.
- **Ids are numbers.** `list` reports `pane_id` and `tab_id` as JSON numbers, while
  the state file stores `pane_id` as a string (`"16"`).
