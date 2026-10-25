mod app;
mod state;
mod terminal;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::Parser;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event as InputEvent, KeyEventKind};

use crate::app::App;
use crate::ui::LayoutChoice;

/// Redraw at least this often, so elapsed times keep counting.
const REDRAW_INTERVAL: Duration = Duration::from_secs(1);

/// Re-list panes and reload state files this often. Closing a pane changes no state
/// file, and a state directory created after startup is not watched yet.
const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

/// Watch over the AI coding agents running in your terminal panes.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Screen layout: `sidebar` for a tall split, `dock` for a wide one.
    #[arg(long, value_enum, default_value_t = LayoutChoice::Auto)]
    layout: LayoutChoice,
}

/// Everything the event loop waits for.
enum Event {
    Input(InputEvent),
    InputFailed(std::io::Error),
    StateChanged,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let state_dir = state::state_dir();
    let mut app = App::new(state_dir.clone(), terminal::detect());
    app.refresh();

    let mut tui = match ratatui::try_init() {
        Ok(tui) => tui,
        Err(err) => {
            // Setup may fail halfway, e.g. after enabling raw mode.
            ratatui::restore();
            return Err(err).context("failed to set up the terminal");
        }
    };
    let result = run(&mut tui, &mut app, state_dir.as_deref(), cli.layout);
    // Panics are covered by the hook `try_init` installs.
    ratatui::restore();
    result
}

fn run(
    tui: &mut DefaultTerminal,
    app: &mut App,
    state_dir: Option<&Path>,
    layout: LayoutChoice,
) -> Result<()> {
    let (sender, events) = mpsc::channel();
    spawn_input_reader(sender.clone());
    let mut watcher = StateWatcher::new(sender, state_dir);
    let mut last_refresh = Instant::now();

    while !app.should_quit() {
        tui.draw(|frame| ui::draw(frame, app, layout, unix_now()))
            .context("failed to draw")?;
        let state_changed = handle_events(app, &events)?;
        if state_changed || last_refresh.elapsed() >= REFRESH_INTERVAL {
            watcher.watch_if_needed();
            app.refresh();
            last_refresh = Instant::now();
        }
    }
    Ok(())
}

/// Waits up to [`REDRAW_INTERVAL`] for an event, then handles it and every event
/// already queued behind it. Returns whether any state file changed.
///
/// One write shows up as several file events (create, modify, rename); handling the
/// whole queue at once turns them into a single reload.
fn handle_events(app: &mut App, events: &Receiver<Event>) -> Result<bool> {
    let first = match events.recv_timeout(REDRAW_INTERVAL) {
        Ok(event) => Some(event),
        Err(RecvTimeoutError::Timeout) => None,
        Err(RecvTimeoutError::Disconnected) => bail!("event sources stopped"),
    };
    let mut state_changed = false;
    for event in first.into_iter().chain(events.try_iter()) {
        // Keys typed after `q` must not jump anywhere on the way out.
        if app.should_quit() {
            break;
        }
        match event {
            Event::Input(InputEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                app.handle_key(key);
            }
            Event::Input(_) => {}
            Event::InputFailed(err) => return Err(err).context("failed to read input"),
            Event::StateChanged => state_changed = true,
        }
    }
    Ok(state_changed)
}

/// Reads terminal input on its own thread, since `event::read` blocks.
fn spawn_input_reader(sender: Sender<Event>) {
    thread::spawn(move || {
        loop {
            let event = match event::read() {
                Ok(input) => Event::Input(input),
                Err(err) => Event::InputFailed(err),
            };
            let failed = matches!(event, Event::InputFailed(_));
            // A send fails only once the event loop has exited.
            if sender.send(event).is_err() || failed {
                return;
            }
        }
    });
}

/// Watches the state directory and reports changes as [`Event::StateChanged`].
///
/// Watching only makes updates instant: the periodic refresh picks up changes anyway.
/// So the directory may not exist yet (watching starts once it does), and a watcher
/// that cannot start (e.g. out of inotify instances) is simply left out.
struct StateWatcher {
    watcher: Option<RecommendedWatcher>,
    dir: Option<PathBuf>,
    is_watching: bool,
}

impl StateWatcher {
    fn new(sender: Sender<Event>, dir: Option<&Path>) -> Self {
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else {
                return;
            };
            if is_state_change(event.kind) {
                // A send fails only once the event loop has exited.
                let _ = sender.send(Event::StateChanged);
            }
        })
        .ok();
        let mut state_watcher = Self {
            watcher,
            dir: dir.map(Path::to_path_buf),
            is_watching: false,
        };
        state_watcher.watch_if_needed();
        state_watcher
    }

    fn watch_if_needed(&mut self) {
        let (Some(watcher), Some(dir)) = (&mut self.watcher, &self.dir) else {
            return;
        };
        if self.is_watching {
            return;
        }
        // Failing here means the directory does not exist (yet); try again later.
        self.is_watching = watcher.watch(dir, RecursiveMode::NonRecursive).is_ok();
    }
}

/// Opening and reading files is reported too (inotify on Linux); reacting to those
/// would make every reload trigger the next one.
fn is_state_change(kind: EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

fn unix_now() -> u64 {
    // A clock before 1970 is not worth an error; elapsed times just read 0s.
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests {
    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind, RenameMode};

    use super::*;

    #[test]
    fn file_writes_renames_and_removals_are_state_changes() {
        assert!(is_state_change(EventKind::Create(CreateKind::File)));
        assert!(is_state_change(EventKind::Modify(ModifyKind::Name(
            RenameMode::To
        ))));
        assert!(is_state_change(EventKind::Remove(RemoveKind::File)));
    }

    #[test]
    fn reading_files_is_not_a_state_change() {
        assert!(!is_state_change(EventKind::Access(AccessKind::Read)));
    }
}
