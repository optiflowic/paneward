# Manual Testing Checklist

Automated tests cover pure logic (state parsing, sorting, filtering, formatting,
backend output parsing). Talking to a real terminal and drawing the TUI is not
testable in CI — run this checklist before tagging a release.

Run it inside WezTerm, with at least one agent installed and its hook writing state
files (see `docs/spec/state-format.md`): on macOS, and on Linux (WSL) once the WSL
section has been run. That section has not been run yet, so the README marks WSL
support as not tested. Once it passes, remove that note from the README and the
"Not checked yet" notes from `docs/spec/wezterm.md`.

## Environment without agents

- [ ] With no state directory, `paneward` starts and shows an empty list without errors.
- [ ] Outside a supported terminal, `paneward` starts and explains that jumping is unavailable.
- [ ] Inside tmux in WezTerm (where `wezterm cli` may fail or hang), `paneward` shows
      an error message within a few seconds and `q` still quits.
- [ ] Starting paneward before the state directory exists, then starting an agent,
      adds the session within about two seconds.

## Session list

- [ ] Starting an agent in a new pane adds it to the list within a second.
- [ ] Status changes (`running` → `waiting` → `idle`) are reflected within a second.
- [ ] While an agent runs several tools in a row, its `running` time keeps counting
      instead of restarting at each tool call.
- [ ] Sessions are ordered `waiting`, `running`, `idle`, then by project name
      ignoring case.
- [ ] Ending the session (agent exits normally) removes it from the list.
- [ ] Closing the pane without ending the session hides it from the list.
- [ ] After restarting the terminal, sessions from the previous run are not shown,
      even once a new pane gets the same id as an old one (needs a hook that writes
      `terminal_instance`; leave a state file behind by force-quitting WezTerm).

## Jump

- [ ] Selecting a session in the same tab focuses its pane.
- [ ] Selecting a session in another tab switches to that tab and focuses the pane.
- [ ] Selecting a session whose pane was just closed shows a message instead of crashing.
- [ ] Number keys `1`-`9` jump to the session with that number.
- [ ] paneward stays open after a jump.

## Launch

Run with a config file listing at least two roots and one agent (see the README,
"Launch").

- [ ] Without the config file, `n` explains where to create it and paneward keeps running.
- [ ] With a broken config file (e.g. `roots = [`), `n` shows why it is invalid and
      where (also in a 30-column sidebar), and paneward keeps running.
- [ ] `n` lists the projects under every root, sorted by name
      ignoring case, without hidden directories.
- [ ] Typing filters the list (ignoring case), `Backspace` widens it again, and the
      cursor sits after the query.
- [ ] Letters such as `q`, `j` and `n` type into the query instead of quitting or moving.
- [ ] `Enter` opens a new tab in the same window, in the project's directory, running
      the agent, and switches to that tab.
- [ ] The agent finds commands on the usual `PATH`, also when WezTerm was started from
      the Dock (macOS).
- [ ] With two agents configured, the agent picker follows the project picker.
- [ ] `Esc` goes back to the session list; `Ctrl-C` quits.
- [ ] The launched session shows up in the list once its hook writes a state file.
- [ ] Exiting the agent closes its tab.

## Layouts

- [ ] Sidebar layout fits a narrow (about 30 columns) split.
- [ ] Dock layout fits a short (about 8 rows) split.
- [ ] Resizing the pane redraws without artifacts.
- [ ] Without `--layout`, making the pane much wider than tall switches to the dock,
      and back to the sidebar when it is tall again.

## WSL

Not run yet (see above). Run inside a WSL distribution opened by the Windows
WezTerm, with `WEZTERM_PANE` forwarded through `WSLENV` (see `docs/spec/wezterm.md`,
"WSL").

- [ ] `echo $WEZTERM_PANE` prints the same id as `wezterm.exe cli list` shows for
      that pane, and the hook's state file uses it.
- [ ] paneward lists the sessions running in WSL panes (`wezterm.exe` is found).
- [ ] Jumping focuses a pane in the same tab and in another tab.
- [ ] Launching (`n`, then `Enter`) opens a tab in the WSL domain, in the project's
      Linux directory, running the agent.
- [ ] The list stays responsive (no repeated timeout messages) with several panes open.
- [ ] Without the `WSLENV` forwarding, paneward starts without errors and explains
      that jumping is unavailable.

## Exit

- [ ] `q` and `Ctrl-C` restore the terminal (cursor, no raw mode, no alternate screen left behind).
