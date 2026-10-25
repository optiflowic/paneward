# State file format (version 1)

paneward does not talk to agents directly. Agent hooks (writers) record each
session's state in a small JSON file, and paneward (and anything else, such as a
WezTerm tab bar config) reads them. This file is the contract between the two.

## Location

```
${XDG_STATE_HOME:-$HOME/.local/state}/paneward/<terminal>-<pane_id>.json
```

One file per terminal pane. Example: `~/.local/state/paneward/wezterm-12.json`.

## Schema

```json
{
  "version": 1,
  "agent": "claude",
  "status": "running",
  "terminal": "wezterm",
  "pane_id": "12",
  "terminal_instance": "/home/me/.local/share/wezterm/gui-sock-example",
  "cwd": "/home/me/src/project",
  "session_id": "00000000-0000-0000-0000-000000000000",
  "started_at": 1700000000,
  "updated_at": 1700000060
}
```

| Field        | Type   | Required | Description |
|--------------|--------|----------|-------------|
| `version`    | number | yes      | Always `1` for this spec. |
| `agent`      | string | yes      | Agent identifier: `claude`, `codex`, ... |
| `status`     | string | yes      | `running`, `waiting` or `idle` (see below). |
| `terminal`   | string | yes      | Terminal backend: `wezterm` (later `tmux`, ...). |
| `pane_id`    | string | yes      | Pane id as the terminal reports it, always a string (`"12"`, `"%3"`). |
| `terminal_instance` | string | no | The running terminal the pane belongs to (see below). |
| `cwd`        | string | no       | Working directory of the session. |
| `session_id` | string | no       | Agent-specific session id. |
| `started_at` | number | no       | Unix seconds when the session started. Preserved across updates. |
| `updated_at` | number | yes      | Unix seconds of the last write. |

### Status

| Value     | Meaning |
|-----------|---------|
| `running` | The agent is working (thinking or running tools). |
| `waiting` | The agent is blocked on the user (e.g. a permission prompt). |
| `idle`    | The agent finished its turn and waits for the next prompt. |

### Terminal instance

Pane ids are only unique within one run of a terminal: after a restart they start
again from the beginning, so a file left behind by a crash can match a new pane.
`terminal_instance` tells runs apart. It is an opaque string that changes whenever
the terminal starts again; each backend memo in `docs/spec/` defines its value
(WezTerm: `WEZTERM_UNIX_SOCKET`). Readers compare it for equality only, and treat
an empty string like a missing field.

## Rules for writers

- Write atomically: write to a temporary file in the same directory, then rename.
  The temporary file name must not end in `.json` (e.g. `wezterm-12.json.<pid>`).
- Delete the file when the session ends.
- Never block or fail the agent. If anything goes wrong, exit successfully without writing.
- Do nothing when the terminal cannot be identified (e.g. not running inside WezTerm).
- Write `terminal_instance` whenever the backend memo's value is available.

## Rules for readers

- A missing directory means no sessions, not an error.
- Read only regular files whose name ends in `.json`. Other names are a writer's
  temporary files; symlinks, directories and FIFOs are never state files.
- Skip files that fail to parse, have an unknown `version`, or an unknown `status`.
- Ignore unknown fields (new optional fields are not a version bump).
- `updated_at` is the time of the last write, not of the last status change: writers
  may rewrite the file without changing `status` (e.g. on every tool call while
  `running`). A reader that shows how long a session has had its status must track
  status changes itself.
- Files can outlive their pane (crash, terminal restart). Show a file only when its
  pane currently exists in that terminal.
- When both the file and the reader know the terminal instance and the two differ,
  the file belongs to an earlier run of the terminal: skip it. When either side does
  not know it, decide by the pane alone.
