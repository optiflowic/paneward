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
    "tty_name": "/dev/ttys016"
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
- **Startup time.** Interop starts a Windows process for every call, which is slower
  than on macOS or Linux. The 2-second timeout (see Quirks) also applies here.

## Quirks

- **Tab switching.** Whether `activate-pane` alone switches to the pane's tab could
  not be confirmed from the CLI: `wezterm cli list-clients` keeps reporting the old
  `focused_pane_id` after either command. paneward therefore always activates the
  tab first. Whether this reaches panes in other tabs is still to be checked by
  hand (`docs/manual-testing.md`, "Jump").
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
