//! Projects to launch agents in: the directories directly under the configured roots.
//! Knows nothing about terminals.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// Directory name, shown in the picker.
    pub name: String,
    pub path: PathBuf,
}

/// Lists the projects under `roots`, sorted by name ignoring case.
///
/// A root that is missing or unreadable has no projects. Hidden directories and names
/// that are not UTF-8 are skipped.
pub fn list(roots: &[PathBuf]) -> Vec<Project> {
    let mut projects: Vec<Project> = roots.iter().flat_map(|root| list_root(root)).collect();
    projects.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.path.cmp(&b.path))
    });
    projects
}

fn list_root(root: &Path) -> Vec<Project> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        // `is_dir` follows symlinks: a linked project is still a project.
        .filter(|path| path.is_dir())
        .filter_map(|path| {
            let name = path.file_name()?.to_str()?.to_string();
            (!name.starts_with('.')).then_some(Project { name, path })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(projects: &[Project]) -> Vec<&str> {
        projects
            .iter()
            .map(|project| project.name.as_str())
            .collect()
    }

    #[test]
    fn lists_subdirectories_of_every_root_sorted_by_name() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        fs::create_dir(first.path().join("zeta")).unwrap();
        fs::create_dir(first.path().join("alpha")).unwrap();
        fs::create_dir(second.path().join("mid")).unwrap();

        let projects = list(&[first.path().to_path_buf(), second.path().to_path_buf()]);

        assert_eq!(names(&projects), ["alpha", "mid", "zeta"]);
        assert_eq!(projects[1].path, second.path().join("mid"));
    }

    #[test]
    fn sorts_names_ignoring_case() {
        let root = tempfile::tempdir().unwrap();
        for name in ["gamma", "Beta", "alpha"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }

        let projects = list(&[root.path().to_path_buf()]);

        assert_eq!(names(&projects), ["alpha", "Beta", "gamma"]);
    }

    #[test]
    fn skips_files_and_hidden_directories() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("project")).unwrap();
        fs::create_dir(root.path().join(".cache")).unwrap();
        fs::write(root.path().join("notes.txt"), "").unwrap();

        assert_eq!(names(&list(&[root.path().to_path_buf()])), ["project"]);
    }

    #[test]
    fn a_missing_root_has_no_projects() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(list(&[root.path().join("missing")]), Vec::<Project>::new());
    }
}
