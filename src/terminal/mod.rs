//! Terminal backends. Only code under this module runs terminal CLIs.

mod wezterm;

use std::collections::HashSet;

use anyhow::Result;

/// A terminal whose panes paneward can list and focus.
pub trait Terminal {
    /// Backend name, as written in the `terminal` field of state files.
    fn name(&self) -> &'static str;

    /// Ids of the panes that exist right now, in the form state files use.
    fn live_pane_ids(&self) -> Result<HashSet<String>>;

    /// Brings the pane to the front, switching to its tab if needed.
    fn focus_pane(&self, pane_id: &str) -> Result<()>;
}

/// Returns the backend of the terminal paneward runs in, or `None` outside a
/// supported terminal (a normal state: there is just nothing to check or jump to).
pub fn detect() -> Option<Box<dyn Terminal>> {
    wezterm::WezTerm::detect().map(|wezterm| Box::new(wezterm) as Box<dyn Terminal>)
}
