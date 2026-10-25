# Manual Testing Checklist

Automated tests cover pure logic (state parsing, sorting, filtering, formatting,
backend output parsing). Talking to a real terminal and drawing the TUI is not
testable in CI — run this checklist before tagging a release.

Run it on macOS and on Linux (WSL), inside WezTerm, with at least one agent
installed and its hook writing state files (see `docs/spec/state-format.md`).

## Environment without agents

- [ ] With no state directory, `paneward` starts and shows an empty list without errors.
- [ ] Outside a supported terminal, `paneward` starts and explains that jumping is unavailable.

## Session list

- [ ] Starting an agent in a new pane adds it to the list within a second.
- [ ] Status changes (`running` → `waiting` → `idle`) are reflected within a second.
- [ ] Sessions are ordered `waiting`, `running`, `idle`, then by project name.
- [ ] Ending the session (agent exits normally) removes it from the list.
- [ ] Closing the pane without ending the session hides it from the list.
- [ ] After restarting the terminal, sessions from the previous run are not shown.

## Jump

- [ ] Selecting a session in the same tab focuses its pane.
- [ ] Selecting a session in another tab switches to that tab and focuses the pane.
- [ ] Selecting a session whose pane was just closed shows a message instead of crashing.

## Layouts

- [ ] Sidebar layout fits a narrow (about 30 columns) split.
- [ ] Dock layout fits a short (about 8 rows) split.
- [ ] Resizing the pane redraws without artifacts.

## WSL

Run inside a WSL distribution opened by the Windows WezTerm, with `WEZTERM_PANE`
forwarded through `WSLENV` (see `docs/spec/wezterm.md`, "WSL").

- [ ] `echo $WEZTERM_PANE` prints the same id as `wezterm.exe cli list` shows for
      that pane, and the hook's state file uses it.
- [ ] paneward lists the sessions running in WSL panes (`wezterm.exe` is found).
- [ ] Jumping focuses a pane in the same tab and in another tab.
- [ ] The list stays responsive (no repeated timeout messages) with several panes open.

## Exit

- [ ] `q` and `Ctrl-C` restore the terminal (cursor, no raw mode, no alternate screen left behind).
