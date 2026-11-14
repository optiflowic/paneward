# Git worktrees

paneward names a session in a linked git worktree after its repository
(`keyturn (97-pre-pr-review)` instead of `97-pre-pr-review`). It reads the
worktree's files itself and never runs `git`, which may not be installed.

## Layout

A linked worktree (made by `git worktree add`, or by agents such as Claude Code
under `.claude/worktrees/`) has a `.git` *file* at its top, not a directory. Its
first line points into the repository's `.git` directory:

```
gitdir: /src/optiflowic/keyturn/.git/worktrees/97-pre-pr-review
```

- The path is absolute, or relative to the worktree's top when git writes
  relative paths (`worktree.useRelativePaths`, git 2.48 and later).
- `.git/worktrees/<id>` is per worktree; `<id>` is usually the worktree
  directory's name, but git adds a number when names collide.
- The repository is the parent of that `.git` directory, here `keyturn`.

## How paneward reads it

1. From the session's `cwd`, walk up to the first directory that has `.git`
   (like git does), up to the root.
2. Read `.git` only if it is a regular file, and take its first line after `gitdir:`.
3. Resolve a relative path from the directory found in step 1, dropping `..`
   on the path alone (symlinks are not followed).
4. Accept it only if the path ends in `.git/worktrees/<id>`. The project is then
   `<repository> (<worktree>)`, with `<worktree>` the name of the directory found
   in step 1 (what the user sees on disk), not `<id>`.

Anything else keeps the last part of `cwd`, without failing:

- no `.git` up to the root, or `cwd` that is not absolute
- a `.git` directory (a main worktree, or a plain repository)
- a `.git` file pointing elsewhere: a submodule (`.git/modules/<name>`), or a
  worktree of a bare repository (`keyturn.git/worktrees/<id>`)
- an unreadable or malformed `.git` file
