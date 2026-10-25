# WezTerm backend

What paneward needs from WezTerm and how it gets it. The backend
(`src/terminal/wezterm.rs`) follows this memo; if WezTerm behaves differently, fix
this memo in the same change.

Checked with `wezterm 20260716-195552-76b606ec` on macOS.

## Detection

WezTerm sets `WEZTERM_PANE` (the pane id of the shell) in every pane it spawns.
paneward uses the WezTerm backend when `WEZTERM_PANE` is set and non-empty. Without
it there is no backend: sessions cannot be checked against live panes, and jumping
is unavailable.

The state files name this backend `wezterm` (the `terminal` field).

## Commands

Every command is run as `wezterm cli --no-auto-start <subcommand>`.
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

## Quirks

- **Tab switching.** Whether `activate-pane` alone switches to the pane's tab could
  not be confirmed from the CLI: `wezterm cli list-clients` keeps reporting the old
  `focused_pane_id` after either command. paneward therefore always activates the
  tab first. Verified by hand (`docs/manual-testing.md`, "Jump").
- **Missing pane or tab.** An unknown id exits with status 1 and logs to stderr,
  e.g. `ERROR wezterm > unexpected response ... "Error: pane 99999 not found"` or
  `could not determine which pane should be active for tab 99999`. paneward shows
  the failure as a message.
- **Ids are numbers.** `list` reports `pane_id` and `tab_id` as JSON numbers, while
  the state file stores `pane_id` as a string (`"16"`).
