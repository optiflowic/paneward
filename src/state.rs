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
    pub terminal: String,
    pub pane_id: String,
    /// Which run of the terminal the pane belongs to, if the writer recorded it.
    pub terminal_instance: Option<String>,
    /// Last component of the session's working directory, or `pane <id>` without one.
    pub project: String,
    /// Unix seconds of the last write.
    pub updated_at: u64,
}

/// A state file as written on disk. Unknown fields are ignored by serde's default.
#[derive(Deserialize)]
struct StateFile {
    version: u64,
    agent: String,
    status: Status,
    terminal: String,
    pane_id: String,
    terminal_instance: Option<String>,
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
    parse_session(&text)
}

/// Parses one state file. Returns `None` for anything this reader does not understand.
fn parse_session(text: &str) -> Option<Session> {
    let file: StateFile = serde_json::from_str(text).ok()?;
    if file.version != SUPPORTED_VERSION {
        return None;
    }
    let project = project_name(file.cwd.as_deref(), &file.pane_id);
    Some(Session {
        agent: file.agent,
        status: file.status,
        terminal: file.terminal,
        pane_id: file.pane_id,
        // An empty value says nothing about the instance (the spec treats it as missing).
        terminal_instance: file
            .terminal_instance
            .filter(|instance| !instance.is_empty()),
        project,
        updated_at: file.updated_at,
    })
}

fn project_name(cwd: Option<&str>, pane_id: &str) -> String {
    cwd.and_then(|cwd| Path::new(cwd).file_name()).map_or_else(
        || format!("pane {pane_id}"),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Orders sessions by status (`waiting`, `running`, `idle`), then by project name.
/// The pane id breaks ties so the order does not jump between reloads. It is
/// compared as a string: any fixed order will do, and ids are not always numbers.
fn sort_for_display(sessions: &mut [Session]) {
    sessions.sort_by(|a, b| {
        a.status
            .cmp(&b.status)
            .then_with(|| a.project.cmp(&b.project))
            .then_with(|| a.pane_id.cmp(&b.pane_id))
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
        "terminal_instance": "/run/wezterm/gui-sock-4242",
        "cwd": "/home/me/src/project",
        "session_id": "757d3262-7975-427c-9ef1-7acc74732b57",
        "started_at": 1792858467,
        "updated_at": 1792858641
    }"#;

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

    #[test]
    fn parses_a_valid_file() {
        let session = parse_session(VALID).unwrap();
        assert_eq!(
            session,
            Session {
                agent: "claude".into(),
                status: Status::Running,
                terminal: "wezterm".into(),
                pane_id: "12".into(),
                terminal_instance: Some("/run/wezterm/gui-sock-4242".into()),
                project: "project".into(),
                updated_at: 1_792_858_641,
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
        assert_eq!(session.terminal_instance, None);
        assert_eq!(session.project, "pane %3");
    }

    #[test]
    fn treats_an_empty_terminal_instance_as_unknown() {
        let text = VALID.replace("/run/wezterm/gui-sock-4242", "");
        assert_eq!(parse_session(&text).unwrap().terminal_instance, None);
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
    fn project_name_falls_back_to_the_pane_when_cwd_has_no_name() {
        assert_eq!(project_name(Some("/"), "7"), "pane 7");
        assert_eq!(project_name(Some(""), "7"), "pane 7");
        assert_eq!(project_name(None, "7"), "pane 7");
    }

    #[test]
    fn project_name_ignores_a_trailing_slash() {
        assert_eq!(project_name(Some("/src/paneward/"), "7"), "paneward");
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

        let panes: Vec<&str> = sessions.iter().map(|s| s.pane_id.as_str()).collect();
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
            .map(|s| (s.status, s.project.as_str(), s.pane_id.as_str()))
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
}
