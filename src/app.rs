//! Application state: the sessions on screen, the selection and key handling.
//! Knows nothing about drawing.

use std::collections::HashSet;
use std::mem;
use std::path::PathBuf;

use anyhow::Result;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{self, Agent, Config};
use crate::projects::{self, Project};
use crate::state::{self, Session};
use crate::terminal::Terminal;

/// Number keys jump straight to the first sessions on screen.
pub const MAX_JUMP_NUMBER: usize = 9;

const NO_TERMINAL_MESSAGE: &str =
    "Not running in a supported terminal: sessions cannot be checked or jumped to.";

/// A session on screen.
#[derive(Debug, PartialEq, Eq)]
pub struct ListedSession {
    pub session: Session,
    /// Unix seconds since which the session has had its current status.
    ///
    /// Hooks rewrite the file (and `updated_at`) while the status stays the same,
    /// e.g. on every tool call while `running`, so paneward remembers when it saw
    /// the status change.
    pub status_since: u64,
}

const NO_CONFIG_PATH_MESSAGE: &str =
    "Cannot launch: neither XDG_CONFIG_HOME nor HOME is set to find the config file.";

/// What a key asks for on the session list.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    SelectPrevious,
    SelectNext,
    JumpToSelected,
    /// Jump to the session at this index on screen.
    JumpTo(usize),
    Launch,
    Quit,
}

/// What a key asks for in a picker.
#[derive(Debug, PartialEq, Eq)]
enum PickerAction {
    Edit(Edit),
    Choose,
    Back,
    Quit,
}

/// A change to a picker's query or selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    Type(char),
    Erase,
    SelectPrevious,
    SelectNext,
}

/// What the screen shows.
enum Screen {
    Sessions,
    PickProject {
        picker: Picker<Project>,
        agents: Vec<Agent>,
    },
    PickAgent {
        project: Project,
        picker: Picker<Agent>,
    },
}

/// Something a picker can list by name.
trait Named {
    fn name(&self) -> &str;
}

impl Named for Project {
    fn name(&self) -> &str {
        &self.name
    }
}

impl Named for Agent {
    fn name(&self) -> &str {
        &self.name
    }
}

/// A list filtered by what the user types.
struct Picker<T> {
    items: Vec<T>,
    query: String,
    /// Index into the matching items.
    selected: usize,
}

impl<T: Named> Picker<T> {
    fn new(items: Vec<T>) -> Self {
        Self {
            items,
            query: String::new(),
            selected: 0,
        }
    }

    fn matches(&self) -> Vec<&T> {
        self.items
            .iter()
            .filter(|item| name_matches(item.name(), &self.query))
            .collect()
    }

    fn chosen(&self) -> Option<&T> {
        self.matches().get(self.selected).copied()
    }

    fn edit(&mut self, edit: Edit) {
        match edit {
            Edit::Type(c) => {
                self.query.push(c);
                self.selected = 0;
            }
            Edit::Erase => {
                self.query.pop();
                self.selected = 0;
            }
            Edit::SelectPrevious => self.selected = self.selected.saturating_sub(1),
            Edit::SelectNext => {
                let last = self.matches().len().saturating_sub(1);
                self.selected = (self.selected + 1).min(last);
            }
        }
    }

    fn view(&self, title: String) -> PickerView<'_> {
        PickerView {
            title,
            query: &self.query,
            names: self.matches().into_iter().map(Named::name).collect(),
            selected: self.selected,
        }
    }
}

/// A picker as the screen shows it.
pub struct PickerView<'a> {
    pub title: String,
    pub query: &'a str,
    /// Names of the matching items.
    pub names: Vec<&'a str>,
    /// Index into `names`. Meaningless while nothing matches.
    pub selected: usize,
}

pub struct App {
    state_dir: Option<PathBuf>,
    config_path: Option<PathBuf>,
    terminal: Option<Box<dyn Terminal>>,
    /// Last pane list the terminal reported, kept when listing fails.
    live_pane_ids: HashSet<String>,
    sessions: Vec<ListedSession>,
    selected: usize,
    refresh_error: Option<String>,
    /// Why the last jump or launch failed, shown until the next key.
    action_error: Option<String>,
    screen: Screen,
    should_quit: bool,
}

impl App {
    pub fn new(
        state_dir: Option<PathBuf>,
        config_path: Option<PathBuf>,
        terminal: Option<Box<dyn Terminal>>,
    ) -> Self {
        Self {
            state_dir,
            config_path,
            terminal,
            live_pane_ids: HashSet::new(),
            sessions: Vec::new(),
            selected: 0,
            refresh_error: None,
            action_error: None,
            screen: Screen::Sessions,
            should_quit: false,
        }
    }

    pub fn sessions(&self) -> &[ListedSession] {
        &self.sessions
    }

    /// Index of the selected session. Meaningless while there are no sessions.
    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    /// The message to show, the most recent problem first.
    pub fn message(&self) -> Option<&str> {
        if self.terminal.is_none() {
            return Some(NO_TERMINAL_MESSAGE);
        }
        self.action_error
            .as_deref()
            .or(self.refresh_error.as_deref())
    }

    /// The open picker, or `None` while the session list is shown.
    pub fn picker(&self) -> Option<PickerView<'_>> {
        match &self.screen {
            Screen::Sessions => None,
            Screen::PickProject { picker, .. } => Some(picker.view("project".to_string())),
            Screen::PickAgent { project, picker } => {
                Some(picker.view(format!("agent for {}", project.name)))
            }
        }
    }

    /// Reloads the state files and the terminal's panes.
    ///
    /// Without a terminal there is nothing to show: no pane can be confirmed to exist.
    /// On failure the last known panes and sessions stay on screen with an error message,
    /// so a passing hiccup does not empty the list.
    pub fn refresh(&mut self) {
        let Some(terminal) = self.terminal.as_deref() else {
            return;
        };
        let terminal_name = terminal.name();
        let instance = terminal.instance().map(str::to_owned);
        let panes_result = terminal.live_pane_ids();
        let sessions_result = self.load_sessions();
        let mut errors = Vec::new();
        match panes_result {
            Ok(ids) => self.live_pane_ids = ids,
            Err(err) => errors.push(format!("Cannot list panes: {err:#}")),
        }
        match sessions_result {
            Ok(sessions) => self.show_sessions(sessions, terminal_name, instance.as_deref()),
            Err(err) => errors.push(format!("Cannot read sessions: {err:#}")),
        }
        self.refresh_error = (!errors.is_empty()).then(|| errors.join(" "));
    }

    fn load_sessions(&self) -> Result<Vec<Session>> {
        match &self.state_dir {
            Some(dir) => state::load_sessions(dir),
            None => Ok(Vec::new()),
        }
    }

    fn show_sessions(
        &mut self,
        sessions: Vec<Session>,
        terminal_name: &str,
        instance: Option<&str>,
    ) {
        let live = sessions_in_live_panes(sessions, terminal_name, instance, &self.live_pane_ids);
        let listed = with_status_since(&self.sessions, live);
        self.selected = reselect(&self.sessions, self.selected, &listed);
        self.sessions = listed;
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if matches!(self.screen, Screen::Sessions) {
            self.handle_session_key(key);
        } else {
            self.handle_picker_key(key);
        }
    }

    fn handle_session_key(&mut self, key: KeyEvent) {
        let Some(action) = action_for(key) else {
            return;
        };
        self.action_error = None;
        match action {
            Action::SelectPrevious => self.selected = self.selected.saturating_sub(1),
            Action::SelectNext => {
                self.selected = (self.selected + 1).min(self.sessions.len().saturating_sub(1));
            }
            Action::JumpToSelected => self.jump_to(self.selected),
            Action::JumpTo(index) => self.jump_to(index),
            Action::Launch => self.open_project_picker(),
            Action::Quit => self.should_quit = true,
        }
    }

    fn handle_picker_key(&mut self, key: KeyEvent) {
        let Some(action) = picker_action_for(key) else {
            return;
        };
        self.action_error = None;
        match action {
            PickerAction::Edit(edit) => self.edit_picker(edit),
            PickerAction::Choose => self.choose(),
            PickerAction::Back => self.screen = Screen::Sessions,
            PickerAction::Quit => self.should_quit = true,
        }
    }

    fn jump_to(&mut self, index: usize) {
        let Some(listed) = self.sessions.get(index) else {
            return;
        };
        self.selected = index;
        let Some(terminal) = self.terminal.as_deref() else {
            return;
        };
        if let Err(err) = terminal.focus_pane(&listed.session.pane_id) {
            self.action_error = Some(format!(
                "Cannot jump to {}: {err:#}",
                listed.session.project
            ));
        }
    }

    /// Opens the project picker, or explains why launching is unavailable.
    fn open_project_picker(&mut self) {
        // Without a terminal there is nowhere to launch; the message already says so.
        if self.terminal.is_none() {
            return;
        }
        let config = match self.launch_config() {
            Ok(config) => config,
            Err(message) => {
                self.action_error = Some(message);
                return;
            }
        };
        let found = projects::list(&config.roots);
        if found.is_empty() {
            self.action_error = Some("No projects under the roots in the config.".to_string());
            return;
        }
        self.screen = Screen::PickProject {
            picker: Picker::new(found),
            agents: config.agents,
        };
    }

    /// Reads the config, or returns the message that explains why launching is unavailable.
    fn launch_config(&self) -> Result<Config, String> {
        let Some(path) = &self.config_path else {
            return Err(NO_CONFIG_PATH_MESSAGE.to_string());
        };
        let config = config::load(path).map_err(|err| format!("Cannot launch: {err:#}"))?;
        let Some(config) = config else {
            return Err(format!(
                "Create {} to launch sessions (see the README).",
                path.display()
            ));
        };
        if config.agents.is_empty() {
            return Err(format!("No agents in {}.", path.display()));
        }
        Ok(config)
    }

    fn edit_picker(&mut self, edit: Edit) {
        match &mut self.screen {
            Screen::Sessions => {}
            Screen::PickProject { picker, .. } => picker.edit(edit),
            Screen::PickAgent { picker, .. } => picker.edit(edit),
        }
    }

    /// Takes the selected item: a project moves on to the agent (or launches when there
    /// is only one), an agent launches. Nothing happens while nothing matches.
    fn choose(&mut self) {
        match mem::replace(&mut self.screen, Screen::Sessions) {
            Screen::Sessions => {}
            Screen::PickProject { picker, agents } => {
                let Some(project) = picker.chosen().cloned() else {
                    self.screen = Screen::PickProject { picker, agents };
                    return;
                };
                if let [agent] = agents.as_slice() {
                    self.launch(&project, agent);
                    return;
                }
                self.screen = Screen::PickAgent {
                    project,
                    picker: Picker::new(agents),
                };
            }
            Screen::PickAgent { project, picker } => {
                let Some(agent) = picker.chosen() else {
                    self.screen = Screen::PickAgent { project, picker };
                    return;
                };
                self.launch(&project, agent);
            }
        }
    }

    fn launch(&mut self, project: &Project, agent: &Agent) {
        let Some(terminal) = self.terminal.as_deref() else {
            return;
        };
        if let Err(err) = terminal.spawn_tab(&project.path, &agent.command) {
            self.action_error = Some(format!(
                "Cannot launch {} in {}: {err:#}",
                agent.name, project.name
            ));
        }
    }
}

fn action_for(key: KeyEvent) -> Option<Action> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(Action::Quit);
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Some(Action::Quit),
        KeyCode::Char('k') | KeyCode::Up => Some(Action::SelectPrevious),
        KeyCode::Char('j') | KeyCode::Down => Some(Action::SelectNext),
        KeyCode::Enter => Some(Action::JumpToSelected),
        KeyCode::Char('n') => Some(Action::Launch),
        KeyCode::Char(digit) => jump_number_index(digit).map(Action::JumpTo),
        _ => None,
    }
}

/// Letters type into the query, so only keys without text move or quit.
fn picker_action_for(key: KeyEvent) -> Option<PickerAction> {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return (key.code == KeyCode::Char('c')).then_some(PickerAction::Quit);
    }
    match key.code {
        KeyCode::Esc => Some(PickerAction::Back),
        KeyCode::Enter => Some(PickerAction::Choose),
        KeyCode::Up => Some(PickerAction::Edit(Edit::SelectPrevious)),
        KeyCode::Down => Some(PickerAction::Edit(Edit::SelectNext)),
        KeyCode::Backspace => Some(PickerAction::Edit(Edit::Erase)),
        KeyCode::Char(c) => Some(PickerAction::Edit(Edit::Type(c))),
        _ => None,
    }
}

/// Whether `name` contains `query`, ignoring case. An empty query matches everything.
fn name_matches(name: &str, query: &str) -> bool {
    name.to_lowercase().contains(&query.to_lowercase())
}

/// Maps `'1'..='9'` to indices `0..=8`.
fn jump_number_index(digit: char) -> Option<usize> {
    let number = usize::try_from(digit.to_digit(10)?).ok()?;
    (1..=MAX_JUMP_NUMBER).contains(&number).then(|| number - 1)
}

/// Keeps the sessions whose pane exists in this run of this terminal. A state file
/// can outlive its pane, panes of other terminals cannot be checked, and after a
/// terminal restart a new pane can reuse the id of an old one.
fn sessions_in_live_panes(
    sessions: Vec<Session>,
    terminal_name: &str,
    instance: Option<&str>,
    live_pane_ids: &HashSet<String>,
) -> Vec<Session> {
    sessions
        .into_iter()
        .filter(|session| {
            session.terminal == terminal_name
                && is_same_run(session.terminal_instance.as_deref(), instance)
                && live_pane_ids.contains(&session.pane_id)
        })
        .collect()
}

/// Whether a state file comes from the running terminal. When either side does not
/// know the instance, only the pane can decide (`docs/spec/state-format.md`).
fn is_same_run(file_instance: Option<&str>, current_instance: Option<&str>) -> bool {
    match (file_instance, current_instance) {
        (Some(file), Some(current)) => file == current,
        _ => true,
    }
}

/// Carries over when each session's status started from the previous list. A new
/// session, or one whose status changed, starts at its file's last write.
fn with_status_since(previous: &[ListedSession], sessions: Vec<Session>) -> Vec<ListedSession> {
    sessions
        .into_iter()
        .map(|session| {
            let unchanged = previous.iter().find(|listed| {
                is_same_pane(&listed.session, &session) && listed.session.status == session.status
            });
            let status_since = unchanged.map_or(session.updated_at, |listed| listed.status_since);
            ListedSession {
                session,
                status_since,
            }
        })
        .collect()
}

/// Keeps the selection on the same pane across reloads. If that session is gone,
/// keeps the position, clamped to the new list.
fn reselect(previous: &[ListedSession], selected: usize, current: &[ListedSession]) -> usize {
    let same_pane = previous.get(selected).and_then(|selected| {
        current
            .iter()
            .position(|listed| is_same_pane(&listed.session, &selected.session))
    });
    same_pane.unwrap_or_else(|| selected.min(current.len().saturating_sub(1)))
}

fn is_same_pane(a: &Session, b: &Session) -> bool {
    a.terminal == b.terminal && a.pane_id == b.pane_id
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::Path;
    use std::rc::Rc;

    use anyhow::bail;
    use tempfile::TempDir;

    use super::*;
    use crate::state::Status;

    fn session(pane_id: &str, status: Status, updated_at: u64) -> Session {
        Session {
            agent: "claude".into(),
            status,
            terminal: "wezterm".into(),
            pane_id: pane_id.into(),
            terminal_instance: None,
            project: format!("project-{pane_id}"),
            updated_at,
        }
    }

    fn listed(pane_id: &str, status: Status, status_since: u64) -> ListedSession {
        ListedSession {
            session: session(pane_id, status, status_since),
            status_since,
        }
    }

    fn ids(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(ToString::to_string).collect()
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn maps_keys_to_actions() {
        assert_eq!(
            action_for(key(KeyCode::Char('j'))),
            Some(Action::SelectNext)
        );
        assert_eq!(action_for(key(KeyCode::Down)), Some(Action::SelectNext));
        assert_eq!(
            action_for(key(KeyCode::Char('k'))),
            Some(Action::SelectPrevious)
        );
        assert_eq!(action_for(key(KeyCode::Up)), Some(Action::SelectPrevious));
        assert_eq!(
            action_for(key(KeyCode::Enter)),
            Some(Action::JumpToSelected)
        );
        assert_eq!(action_for(key(KeyCode::Char('n'))), Some(Action::Launch));
        assert_eq!(action_for(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(action_for(key(KeyCode::Esc)), Some(Action::Quit));
        assert_eq!(action_for(key(KeyCode::Char('x'))), None);
    }

    #[test]
    fn ctrl_c_quits_and_other_ctrl_keys_do_nothing() {
        let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        assert_eq!(action_for(ctrl('c')), Some(Action::Quit));
        assert_eq!(action_for(ctrl('j')), None);
        assert_eq!(action_for(ctrl('1')), None);
    }

    #[test]
    fn picker_keys_type_letters_instead_of_moving_or_quitting() {
        let edit = |code| picker_action_for(key(code));
        assert_eq!(
            edit(KeyCode::Char('q')),
            Some(PickerAction::Edit(Edit::Type('q')))
        );
        assert_eq!(
            edit(KeyCode::Char('j')),
            Some(PickerAction::Edit(Edit::Type('j')))
        );
        assert_eq!(
            edit(KeyCode::Down),
            Some(PickerAction::Edit(Edit::SelectNext))
        );
        assert_eq!(
            edit(KeyCode::Backspace),
            Some(PickerAction::Edit(Edit::Erase))
        );
        assert_eq!(edit(KeyCode::Enter), Some(PickerAction::Choose));
        assert_eq!(edit(KeyCode::Esc), Some(PickerAction::Back));
        let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        assert_eq!(picker_action_for(ctrl('c')), Some(PickerAction::Quit));
        assert_eq!(picker_action_for(ctrl('n')), None);
    }

    #[test]
    fn name_matching_ignores_case_and_accepts_an_empty_query() {
        assert!(name_matches("paneward", "WARD"));
        assert!(name_matches("PaneWard", "pane"));
        assert!(name_matches("paneward", ""));
        assert!(!name_matches("paneward", "kumolo"));
    }

    #[test]
    fn number_keys_one_to_nine_jump_to_the_matching_row() {
        assert_eq!(action_for(key(KeyCode::Char('1'))), Some(Action::JumpTo(0)));
        assert_eq!(action_for(key(KeyCode::Char('9'))), Some(Action::JumpTo(8)));
        assert_eq!(action_for(key(KeyCode::Char('0'))), None);
    }

    #[test]
    fn keeps_only_sessions_in_live_panes_of_this_terminal() {
        let mut tmux = session("3", Status::Idle, 1);
        tmux.terminal = "tmux".into();
        let sessions = vec![
            session("1", Status::Idle, 1),
            session("2", Status::Idle, 1),
            tmux,
        ];

        let kept = sessions_in_live_panes(sessions, "wezterm", None, &ids(&["1", "3"]));

        let panes: Vec<&str> = kept.iter().map(|s| s.pane_id.as_str()).collect();
        assert_eq!(panes, ["1"]);
    }

    #[test]
    fn hides_sessions_from_an_earlier_run_of_the_terminal() {
        let mut old_run = session("1", Status::Idle, 1);
        old_run.terminal_instance = Some("gui-sock-old-run".into());
        let mut this_run = session("2", Status::Idle, 1);
        this_run.terminal_instance = Some("gui-sock-this-run".into());
        let sessions = vec![old_run, this_run];

        let kept = sessions_in_live_panes(
            sessions,
            "wezterm",
            Some("gui-sock-this-run"),
            &ids(&["1", "2"]),
        );

        let panes: Vec<&str> = kept.iter().map(|s| s.pane_id.as_str()).collect();
        assert_eq!(panes, ["2"]);
    }

    #[test]
    fn decides_by_pane_alone_when_an_instance_is_unknown() {
        assert!(is_same_run(None, Some("gui-sock-this-run")));
        assert!(is_same_run(Some("gui-sock-old-run"), None));
        assert!(is_same_run(None, None));
        assert!(is_same_run(
            Some("gui-sock-this-run"),
            Some("gui-sock-this-run")
        ));
        assert!(!is_same_run(
            Some("gui-sock-old-run"),
            Some("gui-sock-this-run")
        ));
    }

    #[test]
    fn status_since_starts_at_the_last_write_for_new_sessions() {
        let listed = with_status_since(&[], vec![session("1", Status::Running, 100)]);
        assert_eq!(listed[0].status_since, 100);
    }

    #[test]
    fn status_since_survives_rewrites_with_the_same_status() {
        let previous = [listed("1", Status::Running, 100)];
        let listed = with_status_since(&previous, vec![session("1", Status::Running, 160)]);
        assert_eq!(listed[0].status_since, 100);
    }

    #[test]
    fn status_since_restarts_when_the_status_changes() {
        let previous = [listed("1", Status::Running, 100)];
        let listed = with_status_since(&previous, vec![session("1", Status::Waiting, 160)]);
        assert_eq!(listed[0].status_since, 160);
    }

    #[test]
    fn selection_follows_its_pane_when_the_order_changes() {
        let previous = [listed("1", Status::Idle, 1), listed("2", Status::Idle, 1)];
        let current = [
            listed("3", Status::Waiting, 1),
            listed("1", Status::Idle, 1),
            listed("2", Status::Idle, 1),
        ];
        assert_eq!(reselect(&previous, 1, &current), 2);
    }

    #[test]
    fn selection_stays_in_range_when_its_session_disappears() {
        let previous = [listed("1", Status::Idle, 1), listed("2", Status::Idle, 1)];
        let current = [listed("1", Status::Idle, 1)];
        assert_eq!(reselect(&previous, 1, &current), 0);
        assert_eq!(reselect(&previous, 1, &[]), 0);
    }

    const CURRENT_INSTANCE: &str = "gui-sock-this-run";

    /// A terminal that reports fixed panes and records what it was asked to focus
    /// and spawn. Spawning fails when `spawn_error` is set.
    struct FakeTerminal {
        instance: Option<&'static str>,
        live_pane_ids: Result<HashSet<String>, String>,
        focused: Rc<RefCell<Vec<String>>>,
        spawned: Rc<RefCell<Vec<(PathBuf, String)>>>,
        spawn_error: Option<&'static str>,
    }

    impl FakeTerminal {
        fn with_panes(live_pane_ids: Result<HashSet<String>, String>) -> Self {
            Self {
                instance: Some(CURRENT_INSTANCE),
                live_pane_ids,
                focused: Rc::default(),
                spawned: Rc::default(),
                spawn_error: None,
            }
        }
    }

    impl Terminal for FakeTerminal {
        fn name(&self) -> &'static str {
            "wezterm"
        }

        fn instance(&self) -> Option<&str> {
            self.instance
        }

        fn live_pane_ids(&self) -> Result<HashSet<String>> {
            match &self.live_pane_ids {
                Ok(ids) => Ok(ids.clone()),
                Err(message) => bail!("{message}"),
            }
        }

        fn focus_pane(&self, pane_id: &str) -> Result<()> {
            if !self
                .live_pane_ids
                .as_ref()
                .is_ok_and(|ids| ids.contains(pane_id))
            {
                bail!("pane {pane_id} no longer exists");
            }
            self.focused.borrow_mut().push(pane_id.to_string());
            Ok(())
        }

        fn spawn_tab(&self, cwd: &Path, command: &str) -> Result<()> {
            if let Some(message) = self.spawn_error {
                bail!("{message}");
            }
            self.spawned
                .borrow_mut()
                .push((cwd.to_path_buf(), command.to_string()));
            Ok(())
        }
    }

    struct Fixture {
        app: App,
        dir: TempDir,
        focused: Rc<RefCell<Vec<String>>>,
        spawned: Rc<RefCell<Vec<(PathBuf, String)>>>,
    }

    /// Where the fixture's config file goes, inside its temporary directory.
    const CONFIG_FILE: &str = "config.toml";

    /// An app over a temporary state directory holding one idle session per pane id.
    fn fixture(state_panes: &[&str], live_panes: &[&str]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        for pane in state_panes {
            write_state(&dir, pane, "idle");
        }
        let terminal = FakeTerminal::with_panes(Ok(ids(live_panes)));
        let focused = Rc::clone(&terminal.focused);
        let spawned = Rc::clone(&terminal.spawned);
        let mut app = App::new(
            Some(dir.path().to_path_buf()),
            Some(dir.path().join(CONFIG_FILE)),
            Some(Box::new(terminal)),
        );
        app.refresh();
        Fixture {
            app,
            dir,
            focused,
            spawned,
        }
    }

    fn write_state(dir: &TempDir, pane: &str, status: &str) {
        let text = serde_json::json!({
            "version": 1, "agent": "claude", "status": status, "terminal": "wezterm",
            "pane_id": pane, "cwd": format!("/src/project-{pane}"), "updated_at": 100,
        });
        fs::write(
            dir.path().join(format!("wezterm-{pane}.json")),
            text.to_string(),
        )
        .unwrap();
    }

    fn shown_panes(app: &App) -> Vec<&str> {
        app.sessions()
            .iter()
            .map(|listed| listed.session.pane_id.as_str())
            .collect()
    }

    #[test]
    fn refresh_hides_sessions_whose_pane_is_gone() {
        let Fixture { app, .. } = fixture(&["1", "2"], &["2"]);
        assert_eq!(shown_panes(&app), ["2"]);
        assert_eq!(app.message(), None);
    }

    #[test]
    fn refresh_hides_a_session_left_by_an_earlier_run_in_a_reused_pane() {
        let Fixture { mut app, dir, .. } = fixture(&["1"], &["1", "2"]);
        let text = serde_json::json!({
            "version": 1, "agent": "claude", "status": "idle", "terminal": "wezterm",
            "pane_id": "2", "terminal_instance": "gui-sock-old-run", "updated_at": 50,
        });
        fs::write(dir.path().join("wezterm-2.json"), text.to_string()).unwrap();

        app.refresh();

        assert_eq!(shown_panes(&app), ["1"]);
    }

    #[test]
    fn refresh_picks_up_new_state_files() {
        let Fixture { mut app, dir, .. } = fixture(&["1"], &["1", "2"]);
        write_state(&dir, "2", "waiting");
        app.refresh();
        assert_eq!(shown_panes(&app), ["2", "1"]);
    }

    #[test]
    fn refresh_keeps_the_last_panes_when_listing_fails() {
        let Fixture { mut app, dir, .. } = fixture(&["1"], &["1"]);
        app.terminal = Some(Box::new(FakeTerminal::with_panes(Err(
            "wezterm is gone".into()
        ))));
        write_state(&dir, "1", "waiting");

        app.refresh();

        assert_eq!(shown_panes(&app), ["1"]);
        assert_eq!(app.sessions()[0].session.status, Status::Waiting);
        assert_eq!(app.message(), Some("Cannot list panes: wezterm is gone"));
    }

    #[test]
    fn without_a_terminal_nothing_is_shown_and_the_reason_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        write_state(&dir, "1", "idle");
        let mut app = App::new(Some(dir.path().to_path_buf()), None, None);

        app.refresh();

        assert_eq!(shown_panes(&app), Vec::<&str>::new());
        assert_eq!(app.message(), Some(NO_TERMINAL_MESSAGE));
    }

    #[test]
    fn enter_focuses_the_selected_pane() {
        let Fixture {
            mut app, focused, ..
        } = fixture(&["1", "2"], &["1", "2"]);
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Enter));
        assert_eq!(*focused.borrow(), ["2"]);
        assert!(!app.should_quit());
    }

    #[test]
    fn number_key_selects_and_focuses_that_row() {
        let Fixture {
            mut app, focused, ..
        } = fixture(&["1", "2"], &["1", "2"]);
        app.handle_key(key(KeyCode::Char('2')));
        assert_eq!(app.selected(), 1);
        assert_eq!(*focused.borrow(), ["2"]);
    }

    #[test]
    fn number_key_past_the_list_does_nothing() {
        let Fixture {
            mut app, focused, ..
        } = fixture(&["1"], &["1"]);
        app.handle_key(key(KeyCode::Char('5')));
        assert_eq!(app.selected(), 0);
        assert_eq!(*focused.borrow(), Vec::<String>::new());
    }

    #[test]
    fn a_failed_jump_shows_a_message_until_the_next_key() {
        let Fixture { mut app, .. } = fixture(&["1"], &["1"]);
        app.terminal = Some(Box::new(FakeTerminal::with_panes(Ok(HashSet::new()))));

        app.handle_key(key(KeyCode::Enter));
        assert_eq!(
            app.message(),
            Some("Cannot jump to project-1: pane 1 no longer exists")
        );

        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.message(), None);
    }

    #[test]
    fn selection_stays_within_the_list() {
        let Fixture { mut app, .. } = fixture(&["1", "2"], &["1", "2"]);
        app.handle_key(key(KeyCode::Up));
        assert_eq!(app.selected(), 0);
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Down));
        assert_eq!(app.selected(), 1);
    }

    #[test]
    fn q_quits() {
        let Fixture { mut app, .. } = fixture(&[], &[]);
        app.handle_key(key(KeyCode::Char('q')));
        assert!(app.should_quit());
    }

    /// Writes a config whose single root holds `projects`, and returns that root.
    fn write_config(dir: &TempDir, projects: &[&str], agents: &[(&str, &str)]) -> PathBuf {
        let root = dir.path().join("src");
        for project in projects {
            fs::create_dir_all(root.join(project)).unwrap();
        }
        let agents: String = agents
            .iter()
            .map(|(name, command)| format!("{name} = \"{command}\"\n"))
            .collect::<Vec<_>>()
            .concat();
        let text = format!("roots = [\"{}\"]\n\n[agents]\n{agents}", root.display());
        fs::write(dir.path().join(CONFIG_FILE), text).unwrap();
        root
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            app.handle_key(key(KeyCode::Char(c)));
        }
    }

    fn picker_names(app: &App) -> Vec<String> {
        app.picker()
            .unwrap()
            .names
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn n_opens_the_project_picker() {
        let Fixture { mut app, dir, .. } = fixture(&[], &[]);
        write_config(&dir, &["kumolo", "paneward"], &[("claude", "claude")]);

        app.handle_key(key(KeyCode::Char('n')));

        let picker = app.picker().unwrap();
        assert_eq!(picker.title, "project");
        assert_eq!(picker.names, ["kumolo", "paneward"]);
    }

    #[test]
    fn typing_filters_the_projects_and_backspace_widens_them_again() {
        let Fixture { mut app, dir, .. } = fixture(&[], &[]);
        write_config(&dir, &["kumolo", "paneward"], &[("claude", "claude")]);
        app.handle_key(key(KeyCode::Char('n')));

        type_text(&mut app, "WARDx");
        assert_eq!(picker_names(&app), Vec::<String>::new());

        app.handle_key(key(KeyCode::Backspace));
        assert_eq!(picker_names(&app), ["paneward"]);
        assert_eq!(app.picker().unwrap().query, "WARD");
    }

    #[test]
    fn with_one_agent_picking_a_project_launches_it() {
        let Fixture {
            mut app,
            dir,
            spawned,
            ..
        } = fixture(&[], &[]);
        let root = write_config(&dir, &["kumolo", "paneward"], &[("claude", "claude -c")]);
        app.handle_key(key(KeyCode::Char('n')));

        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            *spawned.borrow(),
            [(root.join("paneward"), "claude -c".to_string())]
        );
        assert!(app.picker().is_none());
        assert_eq!(app.message(), None);
    }

    #[test]
    fn with_several_agents_the_agent_is_picked_next() {
        let Fixture {
            mut app,
            dir,
            spawned,
            ..
        } = fixture(&[], &[]);
        let root = write_config(
            &dir,
            &["paneward"],
            &[("claude", "claude"), ("codex", "codex")],
        );
        app.handle_key(key(KeyCode::Char('n')));
        app.handle_key(key(KeyCode::Enter));

        let picker = app.picker().unwrap();
        assert_eq!(picker.title, "agent for paneward");
        assert_eq!(picker.names, ["claude", "codex"]);

        type_text(&mut app, "co");
        app.handle_key(key(KeyCode::Enter));

        assert_eq!(
            *spawned.borrow(),
            [(root.join("paneward"), "codex".to_string())]
        );
        assert!(app.picker().is_none());
    }

    #[test]
    fn enter_with_nothing_matching_keeps_the_picker_open() {
        let Fixture {
            mut app,
            dir,
            spawned,
            ..
        } = fixture(&[], &[]);
        write_config(&dir, &["paneward"], &[("claude", "claude")]);
        app.handle_key(key(KeyCode::Char('n')));
        type_text(&mut app, "zzz");

        app.handle_key(key(KeyCode::Enter));

        assert!(app.picker().is_some());
        assert_eq!(*spawned.borrow(), Vec::<(PathBuf, String)>::new());
    }

    #[test]
    fn esc_goes_back_to_the_sessions_without_quitting() {
        let Fixture { mut app, dir, .. } = fixture(&[], &[]);
        write_config(&dir, &["paneward"], &[("claude", "claude")]);
        app.handle_key(key(KeyCode::Char('n')));

        app.handle_key(key(KeyCode::Esc));

        assert!(app.picker().is_none());
        assert!(!app.should_quit());
    }

    #[test]
    fn a_failed_launch_shows_a_message() {
        let Fixture { mut app, dir, .. } = fixture(&[], &[]);
        write_config(&dir, &["paneward"], &[("claude", "claude")]);
        let mut terminal = FakeTerminal::with_panes(Ok(HashSet::new()));
        terminal.spawn_error = Some("wezterm is gone");
        app.terminal = Some(Box::new(terminal));
        app.handle_key(key(KeyCode::Char('n')));

        app.handle_key(key(KeyCode::Enter));

        assert!(app.picker().is_none());
        assert_eq!(
            app.message(),
            Some("Cannot launch claude in paneward: wezterm is gone")
        );
    }

    #[test]
    fn without_a_config_file_n_explains_how_to_set_one_up() {
        let Fixture { mut app, dir, .. } = fixture(&[], &[]);

        app.handle_key(key(KeyCode::Char('n')));

        assert!(app.picker().is_none());
        let expected = format!(
            "Create {} to launch sessions (see the README).",
            dir.path().join(CONFIG_FILE).display()
        );
        assert_eq!(app.message(), Some(expected.as_str()));
    }

    #[test]
    fn a_broken_config_file_is_shown_as_a_message() {
        let Fixture { mut app, dir, .. } = fixture(&[], &[]);
        fs::write(dir.path().join(CONFIG_FILE), "roots = [").unwrap();

        app.handle_key(key(KeyCode::Char('n')));

        assert!(app.picker().is_none());
        assert!(
            app.message().unwrap().starts_with(
                "Cannot launch: unclosed array, expected `]` at line 1, column 10 in "
            )
        );
    }

    #[test]
    fn a_config_without_agents_cannot_launch() {
        let Fixture { mut app, dir, .. } = fixture(&[], &[]);
        write_config(&dir, &["paneward"], &[]);

        app.handle_key(key(KeyCode::Char('n')));

        assert!(app.picker().is_none());
        assert!(app.message().unwrap().starts_with("No agents in"));
    }

    #[test]
    fn a_config_without_projects_cannot_launch() {
        let Fixture { mut app, dir, .. } = fixture(&[], &[]);
        write_config(&dir, &[], &[("claude", "claude")]);

        app.handle_key(key(KeyCode::Char('n')));

        assert!(app.picker().is_none());
        assert_eq!(
            app.message(),
            Some("No projects under the roots in the config.")
        );
    }

    #[test]
    fn without_a_terminal_n_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(None, Some(dir.path().join(CONFIG_FILE)), None);
        write_config(&dir, &["paneward"], &[("claude", "claude")]);

        app.handle_key(key(KeyCode::Char('n')));

        assert!(app.picker().is_none());
        assert_eq!(app.message(), Some(NO_TERMINAL_MESSAGE));
    }
}
