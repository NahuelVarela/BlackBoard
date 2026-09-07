//! #7/namespace — resolve where `.blackboard/` lives for this invocation.
//!
//! Resolution order: `--repo` (explicit, exact, skips discovery) > nearest
//! ancestor `.git` (dir or file — worktrees/submodules use a `gitdir:`
//! pointer file, presence is all that matters) > fixed machine-wide default
//! under `$XDG_DATA_HOME` (or `~/.local/share`). This is a filesystem stat
//! on `.git`'s presence, never a `git` subprocess — the "no git on the
//! read/write path" invariant is unchanged.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Explicit,
    Git(PathBuf),
    Default,
}

#[derive(Debug, Clone)]
pub struct Namespace {
    pub root: PathBuf,
    pub origin: Origin,
}

pub fn resolve(explicit: &Option<String>) -> Result<Namespace> {
    if let Some(r) = explicit {
        return Ok(Namespace { root: PathBuf::from(r), origin: Origin::Explicit });
    }
    let cwd = std::env::current_dir().context("current dir")?;
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    if let Some(root) = find_git_root(&cwd) {
        return Ok(Namespace { root: root.clone(), origin: Origin::Git(root) });
    }
    Ok(Namespace { root: default_root()?, origin: Origin::Default })
}

/// Walk up from `start` looking for a `.git` entry (dir or file) at each
/// level. First hit wins.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// `$XDG_DATA_HOME/blackboard/default`, falling back to
/// `$HOME/.local/share/blackboard/default` when unset.
fn default_root() -> Result<PathBuf> {
    let base = match std::env::var("XDG_DATA_HOME") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").context("HOME unset")?).join(".local/share"),
    };
    Ok(base.join("blackboard").join("default"))
}

/// Scan `$PATH` for a `bb` whose canonicalized path equals `exe`'s.
/// Read-only, no subprocess — used by `bb help`'s install self-check.
pub fn on_path(exe: &Path) -> bool {
    let exe = exe.canonicalize().unwrap_or_else(|_| exe.to_path_buf());
    let path = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return false,
    };
    std::env::split_paths(&path).any(|dir| {
        let candidate = dir.join("bb");
        candidate.canonicalize().map(|c| c == exe).unwrap_or(false)
    })
}

impl Origin {
    /// Human-readable description for `explain()` / `bb help`.
    pub fn describe(&self) -> String {
        match self {
            Origin::Explicit => "explicit --repo".to_string(),
            Origin::Git(root) => format!("git root ({})", root.display()),
            Origin::Default => "default (no git repo found)".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bb-ns-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn explicit_short_circuits_without_touching_filesystem() {
        let ns = resolve(&Some("/does/not/exist".to_string())).unwrap();
        assert_eq!(ns.root, PathBuf::from("/does/not/exist"));
        assert_eq!(ns.origin, Origin::Explicit);
    }

    #[test]
    fn walk_up_finds_git_dir_from_nested_subdir() {
        let root = tmp_dir("walkup-dir");
        fs::create_dir_all(root.join(".git")).unwrap();
        let nested = root.join("a").join("b").join("c");
        fs::create_dir_all(&nested).unwrap();
        let found = find_git_root(&nested).unwrap();
        assert_eq!(found, root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn walk_up_finds_git_file_worktree_style() {
        let root = tmp_dir("walkup-file");
        // Worktrees/submodules use a `.git` *file* containing `gitdir: ...`;
        // only presence is checked, content is irrelevant.
        fs::write(root.join(".git"), "gitdir: /somewhere/else\n").unwrap();
        let nested = root.join("x").join("y");
        fs::create_dir_all(&nested).unwrap();
        let found = find_git_root(&nested).unwrap();
        assert_eq!(found, root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_git_anywhere_returns_none() {
        let root = tmp_dir("no-git");
        let nested = root.join("p").join("q");
        fs::create_dir_all(&nested).unwrap();
        // No `.git` under this scratch tree; walk stops at `root`'s parent
        // (temp_dir()), which also has no `.git` in a sane test environment.
        // We only assert the immediate scratch subtree has none by checking
        // that the first hit (if any) is not under `root`.
        let found = find_git_root(&nested);
        if let Some(hit) = &found {
            assert!(!hit.starts_with(&root), "unexpectedly found .git inside scratch tree");
        }
        let _ = fs::remove_dir_all(&root);
    }

    // Both env-var cases live in one test (not two) so parallel `cargo test`
    // threads can't interleave mutations of the same process-global vars.
    #[test]
    fn default_root_xdg_and_home_fallback() {
        let saved_xdg = std::env::var("XDG_DATA_HOME").ok();
        let saved_home = std::env::var("HOME").ok();

        std::env::set_var("XDG_DATA_HOME", "/tmp/bb-xdg-test");
        let root = default_root().unwrap();
        assert_eq!(root, PathBuf::from("/tmp/bb-xdg-test/blackboard/default"));

        std::env::remove_var("XDG_DATA_HOME");
        std::env::set_var("HOME", "/tmp/bb-home-test");
        let root = default_root().unwrap();
        assert_eq!(root, PathBuf::from("/tmp/bb-home-test/.local/share/blackboard/default"));

        match saved_xdg {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match saved_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}
