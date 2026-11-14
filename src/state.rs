//! Session state files written by agent hooks.
//!
//! The format is a public contract described in `docs/spec/state-format.md`.
//! Readers are tolerant: anything they do not understand is skipped, never an error.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::worktree::{self, Worktree};

/// The only `version` this reader understands.
const SUPPORTED_VERSION: u64 = 1;

/// Directory under the XDG state home that holds the state files.
const STATE_DIR_NAME: &str = "paneward";

/// Fallback for `XDG_STATE_HOME`, relative to `HOME`.
const DEFAULT_STATE_HOME: &str = ".local/state";

/// Extension of complete state files. Writers give temporary files another name
/// and rename them into place, so other files are still being written.
const STATE_FILE_EXTENSION: &str = "json";

/// What the agent of a session is doing.
///
/// The declaration order is the display order: sessions that need the user come first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Waiting,
    Running,
    Idle,
}

/// One agent session, as recorded by its hook.
#[derive(Debug, PartialEq, Eq)]
pub struct Session {
    pub agent: String,
    pub status: Status,
    pub place: Place,
    /// Last component of the session's working directory, `<repository> (<worktree>)`
    /// in a linked git worktree, or `pane <id>` / `pid <pid>` without one.
    pub project: String,
    /// Unix seconds of the last write.
    pub updated_at: u64,
}

/// Where a session runs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Place {
    Pane {
        terminal: String,
        pane_id: String,
        /// Which run of the terminal the pane belongs to, if the writer recorded it.
        terminal_instance: Option<String>,
    },
    /// Away from any pane, alive as long as this process.
    Process { pid: u32 },
}

/// A state file as written on disk. Unknown fields are ignored by serde's default.
#[derive(Deserialize)]
struct StateFile {
    version: u64,
    agent: String,
    status: Status,
    terminal: Option<String>,
    pane_id: Option<String>,
    terminal_instance: Option<String>,
    pid: Option<u32>,
    cwd: Option<String>,
    updated_at: u64,
}

/// Returns the state directory, `${XDG_STATE_HOME:-$HOME/.local/state}/paneward`,
/// or `None` when neither variable is set.
pub fn state_dir() -> Option<PathBuf> {
    state_dir_from(env::var_os("XDG_STATE_HOME"), env::var_os("HOME"))
}

fn state_dir_from(xdg_state_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    // Like the shell's `${VAR:-default}`, an empty variable counts as unset.
    let non_empty = |value: Option<OsString>| value.filter(|value| !value.is_empty());
    let state_home = match non_empty(xdg_state_home) {
        Some(state_home) => PathBuf::from(state_home),
        None => PathBuf::from(non_empty(home)?).join(DEFAULT_STATE_HOME),
    };
    Some(state_home.join(STATE_DIR_NAME))
}

/// Reads every session in `dir`, sorted for display.
///
/// A missing directory means no sessions. Files that cannot be read or parsed are skipped.
pub fn load_sessions(dir: &Path) -> Result<Vec<Session>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", dir.display()));
        }
    };
    let mut sessions: Vec<Session> = entries
        .filter_map(Result::ok)
        // Writers rename regular files into place. Anything else is skipped without
        // opening it: opening a FIFO with no writer would block forever.
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .filter(|path| is_state_file(path))
        .filter_map(|path| read_session(&path))
        .collect();
    sort_for_display(&mut sessions);
    Ok(sessions)
}

fn is_state_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == STATE_FILE_EXTENSION)
}

fn read_session(path: &Path) -> Option<Session> {
    // The file may vanish between listing and reading when its session ends.
    let text = fs::read_to_string(path).ok()?;
    let file = parse_state_file(&text)?;
    let worktree = file
        .cwd
        .as_deref()
        .and_then(|cwd| worktree::find(Path::new(cwd)));
    session_from(file, worktree.as_ref())
}

/// Parses one state file. Returns `None` for anything this reader does not understand.
fn parse_state_file(text: &str) -> Option<StateFile> {
    let file: StateFile = serde_json::from_str(text).ok()?;
    (file.version == SUPPORTED_VERSION).then_some(file)
}

/// The session of a parsed state file, given the linked worktree its `cwd` is in.
fn session_from(file: StateFile, worktree: Option<&Worktree>) -> Option<Session> {
    let place = place(
        file.terminal,
        file.pane_id,
        file.terminal_instance,
        file.pid,
    )?;
    let project = project_name(file.cwd.as_deref(), worktree, &place);
    Some(Session {
        agent: file.agent,
        status: file.status,
        place,
        project,
        updated_at: file.updated_at,
    })
}

/// A pane when the file names one, else the process of a session without a pane.
fn place(
    terminal: Option<String>,
    pane_id: Option<String>,
    terminal_instance: Option<String>,
    pid: Option<u32>,
) -> Option<Place> {
    if let (Some(terminal), Some(pane_id)) = (terminal, pane_id) {
        return Some(Place::Pane {
            terminal,
            pane_id,
            // An empty value says nothing about the instance (the spec treats it as missing).
            terminal_instance: terminal_instance.filter(|instance| !instance.is_empty()),
        });
    }
    pid.map(|pid| Place::Process { pid })
}

fn project_name(cwd: Option<&str>, worktree: Option<&Worktree>, place: &Place) -> String {
    if let Some(worktree) = worktree {
        return format!("{} ({})", worktree.repository, worktree.name);
    }
    let Some(cwd) = cwd.map(Path::new) else {
        return place_name(place);
    };
    cwd.file_name().map_or_else(
        || place_name(place),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn place_name(place: &Place) -> String {
    match place {
        Place::Pane { pane_id, .. } => format!("pane {pane_id}"),
        Place::Process { pid } => format!("pid {pid}"),
    }
}

/// Orders sessions in panes before those without one, each by status (`waiting`,
/// `running`, `idle`), then by project name ignoring case, then as written so that
/// `Foo` and `foo` keep a fixed order. The place breaks ties so the order does not
/// jump between reloads. Pane ids are compared as strings: any fixed order will do,
/// and ids are not always numbers.
fn sort_for_display(sessions: &mut [Session]) {
    let has_pane = |session: &Session| matches!(session.place, Place::Pane { .. });
    sessions.sort_by(|a, b| {
        has_pane(b)
            .cmp(&has_pane(a))
            .then_with(|| a.status.cmp(&b.status))
            .then_with(|| a.project.to_lowercase().cmp(&b.project.to_lowercase()))
            .then_with(|| a.project.cmp(&b.project))
            .then_with(|| a.place.cmp(&b.place))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"{
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
    }"#;

    fn parse_session(text: &str) -> Option<Session> {
        session_from(parse_state_file(text)?, None)
    }

    fn state_json(status: &str, pane_id: &str, cwd: &str) -> String {
        serde_json::json!({
            "version": 1,
            "agent": "claude",
            "status": status,
            "terminal": "wezterm",
            "pane_id": pane_id,
            "cwd": cwd,
            "updated_at": 100,
        })
        .to_string()
    }

    fn pane_less_json(status: &str, pid: u32, cwd: &str) -> String {
        serde_json::json!({
            "version": 1,
            "agent": "claude",
            "status": status,
            "pid": pid,
            "cwd": cwd,
            "updated_at": 100,
        })
        .to_string()
    }

    fn pane_id(session: &Session) -> &str {
        match &session.place {
            Place::Pane { pane_id, .. } => pane_id,
            Place::Process { .. } => "-",
        }
    }

    fn terminal_instance(session: &Session) -> Option<&str> {
        match &session.place {
            Place::Pane {
                terminal_instance, ..
            } => terminal_instance.as_deref(),
            Place::Process { .. } => None,
        }
    }

    #[test]
    fn parses_a_valid_file() {
        let session = parse_session(VALID).unwrap();
        assert_eq!(
            session,
            Session {
                agent: "claude".into(),
                status: Status::Running,
                place: Place::Pane {
                    terminal: "wezterm".into(),
                    pane_id: "12".into(),
                    terminal_instance: Some(
                        "/home/me/.local/share/wezterm/gui-sock-example".into()
                    ),
                },
                project: "project".into(),
                updated_at: 1_700_000_060,
            }
        );
    }

    #[test]
    fn parses_every_status() {
        for (text, status) in [
            ("waiting", Status::Waiting),
            ("running", Status::Running),
            ("idle", Status::Idle),
        ] {
            let session = parse_session(&state_json(text, "1", "/p")).unwrap();
            assert_eq!(session.status, status);
        }
    }

    #[test]
    fn accepts_a_file_without_optional_fields() {
        let text = r#"{"version":1,"agent":"codex","status":"idle","terminal":"wezterm",
                       "pane_id":"%3","updated_at":5}"#;
        let session = parse_session(text).unwrap();
        assert_eq!(session.agent, "codex");
        assert_eq!(terminal_instance(&session), None);
        assert_eq!(session.project, "pane %3");
    }

    #[test]
    fn treats_an_empty_terminal_instance_as_unknown() {
        let text = VALID.replace("/home/me/.local/share/wezterm/gui-sock-example", "");
        assert_eq!(terminal_instance(&parse_session(&text).unwrap()), None);
    }

    #[test]
    fn ignores_unknown_fields() {
        let text = VALID.replacen('{', r#"{"future_field": {"nested": true},"#, 1);
        assert!(parse_session(&text).is_some());
    }

    #[test]
    fn skips_an_unknown_version() {
        let text = VALID.replace(r#""version": 1"#, r#""version": 2"#);
        assert_eq!(parse_session(&text), None);
    }

    #[test]
    fn skips_an_unknown_status() {
        assert_eq!(parse_session(&state_json("sleeping", "1", "/p")), None);
    }

    #[test]
    fn skips_a_file_missing_a_required_field() {
        let text = VALID.replace(r#""pane_id": "12","#, "");
        assert_eq!(parse_session(&text), None);
    }

    #[test]
    fn parses_a_session_without_a_pane() {
        let session = parse_session(&pane_less_json("idle", 4242, "/src/project")).unwrap();
        assert_eq!(session.place, Place::Process { pid: 4242 });
        assert_eq!(session.project, "project");
    }

    #[test]
    fn a_pane_wins_over_a_pid() {
        let text = VALID.replacen('{', r#"{"pid": 4242,"#, 1);
        assert!(matches!(
            parse_session(&text).unwrap().place,
            Place::Pane { .. }
        ));
    }

    #[test]
    fn a_terminal_without_a_pane_id_counts_as_no_pane() {
        let text = VALID
            .replace(r#""pane_id": "12","#, "")
            .replacen('{', r#"{"pid": 4242,"#, 1);
        assert_eq!(
            parse_session(&text).unwrap().place,
            Place::Process { pid: 4242 }
        );
    }

    #[test]
    fn skips_a_pid_that_is_not_a_number() {
        let text = pane_less_json("idle", 4242, "/p").replace("4242", r#""4242""#);
        assert_eq!(parse_session(&text), None);
    }

    #[test]
    fn skips_a_pane_id_that_is_not_a_string() {
        let text = VALID.replace(r#""pane_id": "12""#, r#""pane_id": 12"#);
        assert_eq!(parse_session(&text), None);
    }

    #[test]
    fn skips_broken_json() {
        assert_eq!(parse_session(r#"{"version":1,"agent":"#), None);
        assert_eq!(parse_session(""), None);
    }

    #[test]
    fn project_name_falls_back_to_the_place_when_cwd_has_no_name() {
        let pane = Place::Pane {
            terminal: "wezterm".into(),
            pane_id: "7".into(),
            terminal_instance: None,
        };
        assert_eq!(project_name(Some("/"), None, &pane), "pane 7");
        assert_eq!(project_name(Some(""), None, &pane), "pane 7");
        assert_eq!(project_name(None, None, &pane), "pane 7");
        assert_eq!(
            project_name(None, None, &Place::Process { pid: 42 }),
            "pid 42"
        );
    }

    #[test]
    fn project_name_ignores_a_trailing_slash() {
        let place = Place::Process { pid: 42 };
        assert_eq!(
            project_name(Some("/src/paneward/"), None, &place),
            "paneward"
        );
    }

    #[test]
    fn state_dir_prefers_xdg_state_home() {
        let dir = state_dir_from(Some("/xdg".into()), Some("/home/me".into()));
        assert_eq!(dir, Some(PathBuf::from("/xdg/paneward")));
    }

    #[test]
    fn state_dir_falls_back_to_home_when_xdg_state_home_is_unset_or_empty() {
        let expected = Some(PathBuf::from("/home/me/.local/state/paneward"));
        assert_eq!(state_dir_from(None, Some("/home/me".into())), expected);
        assert_eq!(
            state_dir_from(Some(OsString::new()), Some("/home/me".into())),
            expected
        );
    }

    #[test]
    fn state_dir_is_none_without_any_home() {
        assert_eq!(state_dir_from(None, None), None);
        assert_eq!(state_dir_from(None, Some(OsString::new())), None);
    }

    #[test]
    fn missing_directory_has_no_sessions() {
        let dir = tempfile::tempdir().unwrap();
        let sessions = load_sessions(&dir.path().join("missing")).unwrap();
        assert_eq!(sessions, []);
    }

    #[test]
    fn loads_only_valid_json_files() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| fs::write(dir.path().join(name), text).unwrap();
        write("wezterm-1.json", &state_json("idle", "1", "/src/a"));
        write("wezterm-2.json", "{ broken");
        write(
            "wezterm-3.json.4242",
            &state_json("idle", "3", "/src/in-progress"),
        );
        write("notes.txt", &state_json("idle", "4", "/src/text"));
        fs::create_dir(dir.path().join("subdir.json")).unwrap();

        let sessions = load_sessions(dir.path()).unwrap();

        let panes: Vec<&str> = sessions.iter().map(pane_id).collect();
        assert_eq!(panes, ["1"]);
    }

    #[cfg(unix)]
    #[test]
    fn skips_symlinks_to_state_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("elsewhere");
        fs::write(&target, state_json("idle", "1", "/src/a")).unwrap();
        std::os::unix::fs::symlink(&target, dir.path().join("wezterm-1.json")).unwrap();

        assert_eq!(load_sessions(dir.path()).unwrap(), []);
    }

    #[test]
    fn project_name_in_a_linked_worktree_names_the_repository() {
        let root = tempfile::tempdir().unwrap();
        let top = root.path().join("keyturn-worktrees/97-pre-pr-review");
        fs::create_dir_all(&top).unwrap();
        let gitdir = root.path().join("keyturn/.git/worktrees/97-pre-pr-review");
        fs::write(top.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        let states = root.path().join("states");
        fs::create_dir(&states).unwrap();
        let state = state_json("idle", "1", top.to_str().unwrap());
        fs::write(states.join("wezterm-1.json"), state).unwrap();

        let sessions = load_sessions(&states).unwrap();
        assert_eq!(sessions[0].project, "keyturn (97-pre-pr-review)");
    }

    #[test]
    fn project_name_keeps_characters_that_need_json_escaping() {
        let session = parse_session(&state_json("idle", "1", r#"/src/say "hi"\x"#)).unwrap();
        assert_eq!(session.project, r#"say "hi"\x"#);
    }

    #[test]
    fn sorts_by_status_then_project_then_pane() {
        let dir = tempfile::tempdir().unwrap();
        let write = |pane: &str, status: &str, cwd: &str| {
            let path = dir.path().join(format!("wezterm-{pane}.json"));
            fs::write(path, state_json(status, pane, cwd)).unwrap();
        };
        write("1", "idle", "/src/alpha");
        write("2", "running", "/src/zeta");
        write("3", "waiting", "/src/zeta");
        write("4", "running", "/src/beta");
        write("5", "running", "/other/beta");

        let sessions = load_sessions(dir.path()).unwrap();

        let order: Vec<(Status, &str, &str)> = sessions
            .iter()
            .map(|s| (s.status, s.project.as_str(), pane_id(s)))
            .collect();
        assert_eq!(
            order,
            [
                (Status::Waiting, "zeta", "3"),
                (Status::Running, "beta", "4"),
                (Status::Running, "beta", "5"),
                (Status::Running, "zeta", "2"),
                (Status::Idle, "alpha", "1"),
            ]
        );
    }

    #[test]
    fn sorts_sessions_without_a_pane_after_those_in_panes() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: String| fs::write(dir.path().join(name), text).unwrap();
        write("pid-20.json", pane_less_json("waiting", 20, "/src/alpha"));
        write("pid-10.json", pane_less_json("idle", 10, "/src/alpha"));
        write("wezterm-1.json", state_json("idle", "1", "/src/zeta"));

        let sessions = load_sessions(dir.path()).unwrap();

        let places: Vec<&Place> = sessions.iter().map(|s| &s.place).collect();
        assert_eq!(
            places,
            [
                &Place::Pane {
                    terminal: "wezterm".into(),
                    pane_id: "1".into(),
                    terminal_instance: None,
                },
                &Place::Process { pid: 20 },
                &Place::Process { pid: 10 },
            ]
        );
    }

    #[test]
    fn sorts_project_names_ignoring_case() {
        let dir = tempfile::tempdir().unwrap();
        for (pane, cwd) in [
            ("1", "/src/gamma"),
            ("2", "/src/beta"),
            ("3", "/src/Beta"),
            ("4", "/src/alpha"),
        ] {
            let path = dir.path().join(format!("wezterm-{pane}.json"));
            fs::write(path, state_json("idle", pane, cwd)).unwrap();
        }

        let sessions = load_sessions(dir.path()).unwrap();

        let projects: Vec<&str> = sessions.iter().map(|s| s.project.as_str()).collect();
        assert_eq!(projects, ["alpha", "Beta", "beta", "gamma"]);
    }
}
