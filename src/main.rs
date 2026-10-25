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
    let mut watcher = StateWatcher::new(sender, state_dir)?;
    let mut last_refresh = Instant::now();

    while !app.should_quit() {
        tui.draw(|frame| ui::draw(frame, app, layout, unix_now()))
            .context("failed to draw")?;
        match events.recv_timeout(REDRAW_INTERVAL) {
            Ok(Event::Input(InputEvent::Key(key))) if key.kind == KeyEventKind::Press => {
                app.handle_key(key);
            }
            Ok(Event::Input(_)) | Err(RecvTimeoutError::Timeout) => {}
            Ok(Event::InputFailed(err)) => return Err(err).context("failed to read input"),
            Ok(Event::StateChanged) => {
                drain_state_changes(&events);
                app.refresh();
                last_refresh = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => bail!("event sources stopped"),
        }
        if last_refresh.elapsed() >= REFRESH_INTERVAL {
            watcher.watch_if_needed();
            app.refresh();
            last_refresh = Instant::now();
        }
    }
    Ok(())
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

/// One write shows up as several file events (create, modify, rename); reload once.
fn drain_state_changes(events: &Receiver<Event>) {
    while let Ok(Event::StateChanged) = events.try_recv() {}
}

/// Watches the state directory and reports changes as [`Event::StateChanged`].
///
/// The directory may not exist yet: watching starts once it does, and until then the
/// periodic refresh picks up new sessions.
struct StateWatcher {
    watcher: RecommendedWatcher,
    dir: Option<PathBuf>,
    is_watching: bool,
}

impl StateWatcher {
    fn new(sender: Sender<Event>, dir: Option<&Path>) -> Result<Self> {
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else {
                return;
            };
            if is_state_change(event.kind) {
                // A send fails only once the event loop has exited.
                let _ = sender.send(Event::StateChanged);
            }
        })
        .context("failed to start watching files")?;
        let mut state_watcher = Self {
            watcher,
            dir: dir.map(Path::to_path_buf),
            is_watching: false,
        };
        state_watcher.watch_if_needed();
        Ok(state_watcher)
    }

    fn watch_if_needed(&mut self) {
        let Some(dir) = &self.dir else {
            return;
        };
        if self.is_watching {
            return;
        }
        // Failing here means the directory does not exist (yet); try again later.
        self.is_watching = self.watcher.watch(dir, RecursiveMode::NonRecursive).is_ok();
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
