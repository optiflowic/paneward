# State file format (version 1)

paneward does not talk to agents directly. Agent hooks (writers) record each
session's state in a small JSON file, and paneward (and anything else, such as a
WezTerm tab bar config) reads them. This file is the contract between the two.

## Location

```
${XDG_STATE_HOME:-$HOME/.local/state}/paneward/<terminal>-<pane_id>.json
${XDG_STATE_HOME:-$HOME/.local/state}/paneward/pid-<pid>.json
```

One file per terminal pane, e.g. `~/.local/state/paneward/wezterm-12.json`, or, for
a [session without a pane](#sessions-without-a-pane), one per process, e.g.
`~/.local/state/paneward/pid-4242.json`.

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
| `terminal`   | string | yes*     | Terminal backend: `wezterm` (later `tmux`, ...). |
| `pane_id`    | string | yes*     | Pane id as the terminal reports it, always a string (`"12"`, `"%3"`). |
| `terminal_instance` | string | no | The running terminal the pane belongs to (see below). |
| `pid`        | number | yes*     | Process that lives as long as a session without a pane (see below). |
| `cwd`        | string | no       | Working directory of the session. |
| `session_id` | string | no       | Agent-specific session id. |
| `started_at` | number | no       | Unix seconds when the session started. Preserved across updates. |
| `updated_at` | number | yes      | Unix seconds of the last write. |

\* A file has either `terminal` and `pane_id` (a session in a pane) or `pid` (a
session without a pane).

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

### Sessions without a pane

Some agents run sessions away from any pane, e.g. in a background daemon that a
pane only attaches to for display. Such a session has no pane to jump to and no
pane whose existence shows that it is alive. Its file has `pid` instead of
`terminal` and `pane_id`: a process on the same machine that lives exactly as long
as the session, usually the agent's own process. The file is named after it,
`pid-<pid>.json`.

These files are new in version 1 without a version bump: readers that require
`terminal` and `pane_id` skip them as files they do not understand.

## Rules for writers

- Write atomically: write to a temporary file in the same directory, then rename.
  The temporary file name must not end in `.json` (e.g. `wezterm-12.json.<pid>`).
- Delete the file when the session ends.
- Never block or fail the agent. If anything goes wrong, exit successfully without writing.
- Do nothing when the terminal cannot be identified (e.g. not running inside WezTerm).
- Do not trust the pane variable alone: processes started from a pane inherit it
  even when they run elsewhere (e.g. an agent's background daemon), and two writers
  would then share one file. Check that the writer runs in that pane as the backend
  memo describes. When it does not, do not touch the pane's file: record the
  session [without a pane](#sessions-without-a-pane) instead. When the check
  cannot be made, do nothing.
- Write `terminal_instance` whenever the backend memo's value is available.
- Write `pid` only for a session without a pane, and only a process whose lifetime
  is the session's: a file whose process is gone is never shown.

## Rules for readers

- A missing directory means no sessions, not an error.
- Read only regular files whose name ends in `.json`. Other names are a writer's
  temporary files; symlinks, directories and FIFOs are never state files.
- Skip files that fail to parse, have an unknown `version`, or an unknown `status`.
- A file with `terminal` and `pane_id` is a session in a pane, even if it also has
  `pid`. A file with only `pid` is a session without a pane. Skip files with neither.
- Ignore unknown fields (new optional fields are not a version bump).
- `updated_at` is the time of the last write, not of the last status change: writers
  may rewrite the file without changing `status` (e.g. on every tool call while
  `running`). A reader that shows how long a session has had its status must track
  status changes itself.
- Files can outlive their pane (crash, terminal restart). Show a file with a pane
  only when that pane currently exists in that terminal.
- When both the file and the reader know the terminal instance and the two differ,
  the file belongs to an earlier run of the terminal: skip it. When either side does
  not know it, decide by the pane alone.
- Show a session without a pane only while its `pid` is a running process. It has
  no pane, so it cannot be jumped to. A pid can be reused by an unrelated process
  after a crash; readers accept that rare false positive.
