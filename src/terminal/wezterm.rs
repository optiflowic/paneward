//! WezTerm backend, driven through `wezterm cli` as described in `docs/spec/wezterm.md`.

use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::Terminal;

/// Set by WezTerm in every pane it spawns.
const PANE_ENV: &str = "WEZTERM_PANE";

/// Socket of the WezTerm process that owns the panes; it changes on every start, so it
/// tells runs of WezTerm apart (the memo's "Terminal instance").
const INSTANCE_ENV: &str = "WEZTERM_UNIX_SOCKET";

/// Set by WSL in every process of a distribution.
const WSL_ENV: &str = "WSL_DISTRO_NAME";

const PROGRAM: &str = "wezterm";

/// Inside WSL the WezTerm that owns the panes is the Windows one, reached through
/// interop; a Linux `wezterm` is usually not installed there.
const WINDOWS_PROGRAM: &str = "wezterm.exe";

/// Keeps `wezterm cli` from starting a mux server; paneward only talks to the running GUI.
const NO_AUTO_START: &str = "--no-auto-start";

/// `wezterm cli` can hang (e.g. an inherited `WEZTERM_PANE` over ssh), and paneward
/// waits for it on the UI thread; past this it is killed and reported as an error.
const CLI_TIMEOUT: Duration = Duration::from_secs(2);

/// How often to check whether `wezterm cli` has exited.
const EXIT_POLL_INTERVAL: Duration = Duration::from_millis(5);

pub struct WezTerm {
    /// The `wezterm` executable to run.
    program: &'static str,
    instance: Option<String>,
}

/// One entry of `wezterm cli list --format json`. Other fields are ignored.
#[derive(Debug, PartialEq, Eq, Deserialize)]
struct ListedPane {
    pane_id: u64,
    tab_id: u64,
}

impl WezTerm {
    pub fn detect() -> Option<Self> {
        let pane = env::var_os(PANE_ENV)?;
        if pane.is_empty() {
            return None;
        }
        let program = program_for(env::var_os(WSL_ENV).as_deref());
        let instance = instance_from(env::var(INSTANCE_ENV).ok());
        Some(Self { program, instance })
    }

    fn list_panes(&self) -> Result<Vec<ListedPane>> {
        let output = run_cli(self.program, &["list", "--format", "json"])?;
        parse_list(&output)
    }
}

/// An unset or empty `WEZTERM_UNIX_SOCKET` means the instance is unknown.
fn instance_from(socket: Option<String>) -> Option<String> {
    socket.filter(|socket| !socket.is_empty())
}

/// Picks the executable from `WSL_DISTRO_NAME` (see the memo's WSL section).
fn program_for(wsl_distro_name: Option<&OsStr>) -> &'static str {
    match wsl_distro_name {
        Some(name) if !name.is_empty() => WINDOWS_PROGRAM,
        _ => PROGRAM,
    }
}

impl Terminal for WezTerm {
    fn name(&self) -> &'static str {
        "wezterm"
    }

    fn instance(&self) -> Option<&str> {
        self.instance.as_deref()
    }

    fn live_pane_ids(&self) -> Result<HashSet<String>> {
        let panes = self.list_panes()?;
        Ok(panes.iter().map(|pane| pane.pane_id.to_string()).collect())
    }

    fn focus_pane(&self, pane_id: &str) -> Result<()> {
        let panes = self.list_panes()?;
        let tab_id =
            tab_of(&panes, pane_id).with_context(|| format!("pane {pane_id} no longer exists"))?;
        // `activate-pane` alone is not known to switch tabs (see the memo's quirks).
        run_cli(
            self.program,
            &["activate-tab", "--tab-id", &tab_id.to_string()],
        )?;
        run_cli(self.program, &["activate-pane", "--pane-id", pane_id])?;
        Ok(())
    }
}

fn parse_list(json: &str) -> Result<Vec<ListedPane>> {
    serde_json::from_str(json).context("unexpected output from `wezterm cli list`")
}

/// Finds the tab holding `pane_id`, given as the state file's string form.
fn tab_of(panes: &[ListedPane], pane_id: &str) -> Option<u64> {
    panes
        .iter()
        .find(|pane| pane.pane_id.to_string() == pane_id)
        .map(|pane| pane.tab_id)
}

/// Runs `<program> cli <args>` and returns its stdout.
fn run_cli(program: &str, args: &[&str]) -> Result<String> {
    let full_args = [&["cli", NO_AUTO_START], args].concat();
    let command_line = format!("{program} {}", full_args.join(" "));
    let mut child = Command::new(program)
        .args(&full_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to run `{command_line}`"))?;
    // Read both pipes while waiting: a child blocked on a full pipe would never exit.
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());
    let status = wait_or_kill(&mut child, CLI_TIMEOUT)
        .with_context(|| format!("failed to wait for `{command_line}`"))?;
    let Some(status) = status else {
        bail!(
            "`{command_line}` did not finish within {}s",
            CLI_TIMEOUT.as_secs()
        );
    };
    if !status.success() {
        // stderr only explains the failure; show what could be read of it.
        let stderr = collect(stderr).unwrap_or_default();
        bail!(
            "`{command_line}` failed: {}",
            String::from_utf8_lossy(&stderr).trim()
        );
    }
    let stdout =
        collect(stdout).with_context(|| format!("failed to read output of `{command_line}`"))?;
    String::from_utf8(stdout).with_context(|| format!("`{command_line}` printed invalid UTF-8"))
}

fn read_in_background<R: Read + Send + 'static>(
    pipe: Option<R>,
) -> JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            pipe.read_to_end(&mut bytes)?;
        }
        Ok(bytes)
    })
}

/// Returns what a reader thread read.
fn collect(reader: JoinHandle<io::Result<Vec<u8>>>) -> io::Result<Vec<u8>> {
    // The thread only reads into a `Vec`; if it panicked anyway, report it as a read error.
    reader
        .join()
        .unwrap_or_else(|_| Err(io::Error::other("output reader panicked")))
}

/// Waits for `child` to exit. Past `timeout` it is killed and `None` is returned.
fn wait_or_kill(child: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            child.kill()?;
            child.wait()?;
            return Ok(None);
        }
        thread::sleep(EXIT_POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from real `wezterm cli list --format json` output (see the memo).
    const LIST_OUTPUT: &str = r#"[
      {
        "window_id": 0, "tab_id": 0, "pane_id": 1, "workspace": "default",
        "size": {"rows": 30, "cols": 102, "pixel_width": 1428, "pixel_height": 840, "dpi": 144},
        "title": "zsh", "cwd": "file://host/Users/me/src/a",
        "cursor_x": 2, "cursor_y": 27, "cursor_shape": "Default", "cursor_visibility": "Visible",
        "left_col": 103, "top_row": 0, "tab_title": "", "window_title": "zsh",
        "is_active": true, "is_zoomed": false, "tty_name": "/dev/tty-example-1"
      },
      {
        "window_id": 0, "tab_id": 5, "pane_id": 16, "workspace": "default",
        "size": {"rows": 62, "cols": 102, "pixel_width": 1428, "pixel_height": 1736, "dpi": 144},
        "title": "nvim", "cwd": "file://host/Users/me/src/b",
        "is_active": false, "is_zoomed": false, "tty_name": "/dev/tty-example-2"
      }
    ]"#;

    #[test]
    fn parses_pane_and_tab_ids_from_list_output() {
        let panes = parse_list(LIST_OUTPUT).unwrap();
        assert_eq!(
            panes,
            [
                ListedPane {
                    pane_id: 1,
                    tab_id: 0
                },
                ListedPane {
                    pane_id: 16,
                    tab_id: 5
                },
            ]
        );
    }

    #[test]
    fn parses_an_empty_list() {
        assert_eq!(parse_list("[]").unwrap(), []);
    }

    #[test]
    fn rejects_list_output_that_is_not_json() {
        assert!(parse_list("WINID TABID PANEID").is_err());
    }

    #[test]
    fn wait_or_kill_returns_the_status_of_a_quick_command() {
        let mut child = Command::new("true").spawn().unwrap();
        let status = wait_or_kill(&mut child, Duration::from_secs(5)).unwrap();
        assert!(status.is_some_and(|status| status.success()));
    }

    #[test]
    fn wait_or_kill_kills_a_command_that_hangs() {
        let mut child = Command::new("sleep").arg("30").spawn().unwrap();
        let started = Instant::now();

        let status = wait_or_kill(&mut child, Duration::from_millis(50)).unwrap();

        assert_eq!(status, None);
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(child.try_wait().unwrap().is_some(), "the child was reaped");
    }

    #[test]
    fn the_socket_identifies_the_instance() {
        let socket = "/home/me/.local/share/wezterm/gui-sock-example";
        assert_eq!(instance_from(Some(socket.into())).as_deref(), Some(socket));
    }

    #[test]
    fn an_unset_or_empty_socket_leaves_the_instance_unknown() {
        assert_eq!(instance_from(None), None);
        assert_eq!(instance_from(Some(String::new())), None);
    }

    #[test]
    fn uses_the_windows_executable_inside_wsl() {
        assert_eq!(program_for(Some(OsStr::new("Ubuntu"))), "wezterm.exe");
    }

    #[test]
    fn uses_wezterm_outside_wsl() {
        assert_eq!(program_for(None), "wezterm");
        assert_eq!(program_for(Some(OsStr::new(""))), "wezterm");
    }

    #[test]
    fn finds_the_tab_of_a_pane_by_its_state_file_id() {
        let panes = parse_list(LIST_OUTPUT).unwrap();
        assert_eq!(tab_of(&panes, "16"), Some(5));
        assert_eq!(tab_of(&panes, "1"), Some(0));
    }

    #[test]
    fn finds_no_tab_for_a_pane_that_is_gone() {
        let panes = parse_list(LIST_OUTPUT).unwrap();
        assert_eq!(tab_of(&panes, "99"), None);
        assert_eq!(tab_of(&panes, "%1"), None);
    }
}
