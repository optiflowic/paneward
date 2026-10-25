//! WezTerm backend, driven through `wezterm cli` as described in `docs/spec/wezterm.md`.

use std::collections::HashSet;
use std::env;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::Terminal;

/// Set by WezTerm in every pane it spawns.
const PANE_ENV: &str = "WEZTERM_PANE";

const PROGRAM: &str = "wezterm";

/// Keeps `wezterm cli` from starting a mux server; paneward only talks to the running GUI.
const NO_AUTO_START: &str = "--no-auto-start";

pub struct WezTerm;

/// One entry of `wezterm cli list --format json`. Other fields are ignored.
#[derive(Debug, PartialEq, Eq, Deserialize)]
struct ListedPane {
    pane_id: u64,
    tab_id: u64,
}

impl WezTerm {
    pub fn detect() -> Option<Self> {
        let pane = env::var_os(PANE_ENV)?;
        (!pane.is_empty()).then_some(Self)
    }
}

impl Terminal for WezTerm {
    fn name(&self) -> &'static str {
        "wezterm"
    }

    fn live_pane_ids(&self) -> Result<HashSet<String>> {
        let panes = list_panes()?;
        Ok(panes.iter().map(|pane| pane.pane_id.to_string()).collect())
    }

    fn focus_pane(&self, pane_id: &str) -> Result<()> {
        let panes = list_panes()?;
        let tab_id =
            tab_of(&panes, pane_id).with_context(|| format!("pane {pane_id} no longer exists"))?;
        // `activate-pane` alone is not known to switch tabs (see the memo's quirks).
        run_cli(&["activate-tab", "--tab-id", &tab_id.to_string()])?;
        run_cli(&["activate-pane", "--pane-id", pane_id])?;
        Ok(())
    }
}

fn list_panes() -> Result<Vec<ListedPane>> {
    let output = run_cli(&["list", "--format", "json"])?;
    parse_list(&output)
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

/// Runs `wezterm cli <args>` and returns its stdout.
fn run_cli(args: &[&str]) -> Result<String> {
    let full_args = [&["cli", NO_AUTO_START], args].concat();
    let command_line = format!("{PROGRAM} {}", full_args.join(" "));
    let output = Command::new(PROGRAM)
        .args(&full_args)
        .output()
        .with_context(|| format!("failed to run `{command_line}`"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("`{command_line}` failed: {}", stderr.trim());
    }
    String::from_utf8(output.stdout)
        .with_context(|| format!("`{command_line}` printed invalid UTF-8"))
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
        "is_active": true, "is_zoomed": false, "tty_name": "/dev/ttys001"
      },
      {
        "window_id": 0, "tab_id": 5, "pane_id": 16, "workspace": "default",
        "size": {"rows": 62, "cols": 102, "pixel_width": 1428, "pixel_height": 1736, "dpi": 144},
        "title": "nvim", "cwd": "file://host/Users/me/src/b",
        "is_active": false, "is_zoomed": false, "tty_name": "/dev/ttys016"
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
