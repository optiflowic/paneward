//! Linked git worktrees, so that sessions in one are named after their repository.
//!
//! The layout is described in `docs/spec/git-worktree.md`.

use std::fs;
use std::path::{Component, Path, PathBuf};

const DOT_GIT: &str = ".git";
const GITDIR_PREFIX: &str = "gitdir:";
const WORKTREES_DIR: &str = "worktrees";

/// A linked worktree of a repository.
#[derive(Debug, PartialEq, Eq)]
pub struct Worktree {
    /// Name of the repository's main worktree directory.
    pub repository: String,
    /// Name of the linked worktree's top directory.
    pub name: String,
}

/// The linked worktree that `dir` is in, found at the first `.git` above it.
///
/// `None` for anything else: no `.git`, a `.git` directory, or a `.git` file that
/// does not point into a repository's `.git/worktrees`.
pub fn find(dir: &Path) -> Option<Worktree> {
    // A relative path would be looked up from paneward's own directory.
    if !dir.is_absolute() {
        return None;
    }
    let top = dir
        .ancestors()
        .find(|dir| fs::symlink_metadata(dir.join(DOT_GIT)).is_ok())?;
    let dot_git = top.join(DOT_GIT);
    // Only regular files are read: opening a FIFO with no writer would block forever.
    if !fs::metadata(&dot_git).is_ok_and(|metadata| metadata.is_file()) {
        return None;
    }
    let text = fs::read_to_string(dot_git).ok()?;
    Some(Worktree {
        repository: repository_name(top, &text)?,
        name: top.file_name()?.to_string_lossy().into_owned(),
    })
}

/// The repository that a `.git` file at `top` points into, from its `gitdir:` line.
fn repository_name(top: &Path, dot_git_file: &str) -> Option<String> {
    let gitdir = dot_git_file
        .lines()
        .next()?
        .strip_prefix(GITDIR_PREFIX)?
        .trim();
    if gitdir.is_empty() {
        return None;
    }
    // Joining keeps an absolute path as is and resolves a relative one from `top`.
    let gitdir = without_parent_dirs(&top.join(gitdir));
    let worktrees = gitdir.parent()?;
    if worktrees.file_name()? != WORKTREES_DIR {
        return None;
    }
    let dot_git = worktrees.parent()?;
    if dot_git.file_name()? != DOT_GIT {
        return None;
    }
    let name = dot_git.parent()?.file_name()?;
    Some(name.to_string_lossy().into_owned())
}

/// Removes `..` by dropping the component before it, without touching the disk.
fn without_parent_dirs(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normal.pop();
            }
            Component::CurDir => {}
            other => normal.push(other),
        }
    }
    normal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worktree(repository: &str, name: &str) -> Worktree {
        Worktree {
            repository: repository.into(),
            name: name.into(),
        }
    }

    #[test]
    fn names_the_repository_of_an_absolute_gitdir() {
        let gitdir = "gitdir: /src/keyturn/.git/worktrees/97-pre-pr-review\n";
        assert_eq!(
            repository_name(Path::new("/src/wt/97-pre-pr-review"), gitdir).as_deref(),
            Some("keyturn")
        );
    }

    #[test]
    fn resolves_a_relative_gitdir_from_the_top() {
        let gitdir = "gitdir: ../../../.git/worktrees/adr-40-41\n";
        assert_eq!(
            repository_name(
                Path::new("/src/keyturn/.claude/worktrees/adr-40-41"),
                gitdir
            )
            .as_deref(),
            Some("keyturn")
        );
    }

    #[test]
    fn rejects_gitdirs_outside_a_worktrees_directory() {
        let top = Path::new("/src/keyturn/sub");
        for text in [
            "gitdir: /src/keyturn/.git/modules/sub",
            "gitdir: /src/keyturn.git/worktrees/x",
            "gitdir:",
            "gitdir: ",
            "not a gitdir",
            "",
        ] {
            assert_eq!(repository_name(top, text), None, "{text:?}");
        }
    }

    #[test]
    fn finds_a_linked_worktree_from_below_its_top() {
        let root = tempfile::tempdir().unwrap();
        let top = root.path().join("keyturn-worktrees/97-pre-pr-review");
        fs::create_dir_all(top.join("src")).unwrap();
        let gitdir = root.path().join("keyturn/.git/worktrees/97-pre-pr-review");
        fs::write(top.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();

        assert_eq!(
            find(&top.join("src")),
            Some(worktree("keyturn", "97-pre-pr-review"))
        );
    }

    #[test]
    fn a_main_worktree_is_not_a_linked_worktree() {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("keyturn");
        fs::create_dir_all(repository.join(".git")).unwrap();

        assert_eq!(find(&repository), None);
    }

    #[test]
    fn stops_at_the_first_dot_git() {
        let root = tempfile::tempdir().unwrap();
        let top = root.path().join("wt");
        let inner = top.join("inner");
        fs::create_dir_all(inner.join(".git")).unwrap();
        let gitdir = root.path().join("keyturn/.git/worktrees/wt");
        fs::write(top.join(".git"), format!("gitdir: {}", gitdir.display())).unwrap();

        assert_eq!(find(&inner), None);
        assert_eq!(find(&top), Some(worktree("keyturn", "wt")));
    }

    #[test]
    fn a_relative_dir_is_not_looked_up() {
        assert_eq!(find(Path::new("src")), None);
    }
}
