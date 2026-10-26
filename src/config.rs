//! The user's config file, `${XDG_CONFIG_HOME:-$HOME/.config}/paneward/config.toml`.
//! Knows nothing about terminals.

use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const CONFIG_DIR_NAME: &str = "paneward";
const CONFIG_FILE_NAME: &str = "config.toml";

/// Fallback for `XDG_CONFIG_HOME`, relative to `HOME`.
const DEFAULT_CONFIG_HOME: &str = ".config";

/// What paneward can launch.
#[derive(Debug, PartialEq, Eq)]
pub struct Config {
    /// Directories whose subdirectories are the projects, with `~` expanded.
    pub roots: Vec<PathBuf>,
    /// Agents by name, sorted by name.
    pub agents: Vec<Agent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub name: String,
    /// Command line, run through the user's login shell.
    pub command: String,
}

/// The file as written on disk.
#[derive(Deserialize)]
struct ConfigFile {
    #[serde(default)]
    roots: Vec<String>,
    #[serde(default)]
    agents: BTreeMap<String, String>,
}

/// Returns the config file path, or `None` when neither variable is set.
pub fn config_path() -> Option<PathBuf> {
    config_path_from(env::var_os("XDG_CONFIG_HOME"), env::var_os("HOME"))
}

fn config_path_from(xdg_config_home: Option<OsString>, home: Option<OsString>) -> Option<PathBuf> {
    // Like the shell's `${VAR:-default}`, an empty variable counts as unset.
    let non_empty = |value: Option<OsString>| value.filter(|value| !value.is_empty());
    let config_home = match non_empty(xdg_config_home) {
        Some(config_home) => PathBuf::from(config_home),
        None => PathBuf::from(non_empty(home)?).join(DEFAULT_CONFIG_HOME),
    };
    Some(config_home.join(CONFIG_DIR_NAME).join(CONFIG_FILE_NAME))
}

/// Reads the config file. A missing file is `None`, not an error.
pub fn load(path: &Path) -> Result<Option<Config>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("failed to read {}", path.display())),
    };
    let home = env::var_os("HOME").filter(|home| !home.is_empty());
    parse(&text, home.as_deref().map(Path::new))
        .with_context(|| format!("invalid config {}", path.display()))
        .map(Some)
}

fn parse(text: &str, home: Option<&Path>) -> Result<Config> {
    let file: ConfigFile = toml::from_str(text)?;
    let roots = file
        .roots
        .iter()
        .map(|root| expand_home(root, home))
        .collect();
    let agents = file
        .agents
        .into_iter()
        .map(|(name, command)| Agent { name, command })
        .collect();
    Ok(Config { roots, agents })
}

/// Expands a leading `~` to `home`. Without a home the path is kept as written.
fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return PathBuf::from(path);
    };
    if path == "~" {
        return home.to_path_buf();
    }
    match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/home/me";

    fn parse_with_home(text: &str) -> Result<Config> {
        parse(text, Some(Path::new(HOME)))
    }

    #[test]
    fn parses_roots_and_agents_sorted_by_name() {
        let config = parse_with_home(
            r#"
            roots = ["~/src", "/work"]

            [agents]
            codex = "codex"
            claude = "claude --verbose"
            "#,
        )
        .unwrap();
        assert_eq!(
            config,
            Config {
                roots: vec![PathBuf::from("/home/me/src"), PathBuf::from("/work")],
                agents: vec![
                    Agent {
                        name: "claude".into(),
                        command: "claude --verbose".into()
                    },
                    Agent {
                        name: "codex".into(),
                        command: "codex".into()
                    },
                ],
            }
        );
    }

    #[test]
    fn missing_sections_are_empty() {
        let config = parse_with_home("").unwrap();
        assert_eq!(config.roots, Vec::<PathBuf>::new());
        assert_eq!(config.agents, Vec::<Agent>::new());
    }

    #[test]
    fn unknown_fields_are_ignored() {
        assert!(parse_with_home("theme = \"dark\"\nroots = []").is_ok());
    }

    #[test]
    fn broken_toml_or_wrong_types_are_errors() {
        assert!(parse_with_home("roots = [").is_err());
        assert!(parse_with_home("roots = \"~/src\"").is_err());
        assert!(parse_with_home("[agents]\nclaude = 1").is_err());
    }

    #[test]
    fn expands_only_a_leading_tilde() {
        let home = Some(Path::new(HOME));
        assert_eq!(expand_home("~", home), PathBuf::from("/home/me"));
        assert_eq!(expand_home("~/src", home), PathBuf::from("/home/me/src"));
        assert_eq!(expand_home("/src/~", home), PathBuf::from("/src/~"));
        assert_eq!(expand_home("~other/src", home), PathBuf::from("~other/src"));
        assert_eq!(expand_home("~/src", None), PathBuf::from("~/src"));
    }

    #[test]
    fn config_path_prefers_xdg_config_home() {
        assert_eq!(
            config_path_from(Some("/xdg".into()), Some(HOME.into())),
            Some(PathBuf::from("/xdg/paneward/config.toml"))
        );
    }

    #[test]
    fn config_path_falls_back_to_home_when_xdg_is_unset_or_empty() {
        let expected = Some(PathBuf::from("/home/me/.config/paneward/config.toml"));
        assert_eq!(config_path_from(None, Some(HOME.into())), expected);
        assert_eq!(
            config_path_from(Some("".into()), Some(HOME.into())),
            expected
        );
    }

    #[test]
    fn config_path_is_unknown_without_any_variable() {
        assert_eq!(config_path_from(None, None), None);
        assert_eq!(config_path_from(Some("".into()), Some("".into())), None);
    }

    #[test]
    fn a_missing_file_is_no_config() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join(CONFIG_FILE_NAME)).unwrap(), None);
    }

    #[test]
    fn errors_name_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CONFIG_FILE_NAME);
        fs::write(&path, "roots = [").unwrap();
        let err = load(&path).unwrap_err();
        assert!(format!("{err:#}").contains(&path.display().to_string()));
    }
}
