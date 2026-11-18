//! Real Git repositories and linked worktrees for integration tests (plan §4.1, §4.2): the
//! nested-worktree fixture of IDX-06 is a main checkout with linked worktrees under
//! `.claude/worktrees/`, as agents make them.
//!
//! Git runs with no system or global configuration and a fixed identity, so the user's signing,
//! hooks or templates never reach a fixture.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Runs `git -C dir args...` and returns its stdout, or what went wrong.
pub fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=jfl-testkit",
            "-c",
            "user.email=jfl-testkit@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .map_err(|e| format!("git {args:?}: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `git init` in `dir`, then one commit of everything in it.
pub fn init_and_commit(dir: &Path) -> Result<(), String> {
    git(dir, &["init", "--quiet"])?;
    git(dir, &["add", "--all"])?;
    git(dir, &["commit", "--quiet", "--message", "fixture"])?;
    Ok(())
}

/// `git worktree add` of a new branch `name` at `path` (relative to `repo`).
pub fn add_worktree(repo: &Path, path: &str, name: &str) -> Result<(), String> {
    git(repo, &["worktree", "add", "--quiet", "-b", name, path])?;
    Ok(())
}

/// A real repository's worktree in a caller's scratch directory: the builder the `repo_probe`
/// contract suite, the git adapter's integration tests and its benchmark make repositories with
/// (main checkouts, linked worktrees under `.claude/worktrees/`, nested repositories).
///
/// Every call is [`git`]: no system or global configuration, a fixed identity, no signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    root: PathBuf,
}

impl Repo {
    /// Writes `files` under `root` (made if missing), then `git init` and one commit of them on
    /// `main`.
    pub fn init(root: &Path, files: &[(&str, &[u8])]) -> Result<Self, String> {
        std::fs::create_dir_all(root).map_err(|e| format!("mkdir {}: {e}", root.display()))?;
        let repo = Self {
            root: root.to_path_buf(),
        };
        for (path, bytes) in files {
            repo.write(path, bytes)?;
        }
        repo.git(&["init", "--quiet"])?;
        repo.git(&["add", "--all", "--force"])?;
        repo.git(&["commit", "--quiet", "--allow-empty", "--message", "init"])?;
        Ok(repo)
    }

    /// The worktree at `root`, made earlier.
    pub fn open(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    /// The worktree's root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `git -C root args...`.
    pub fn git(&self, args: &[&str]) -> Result<String, String> {
        git(&self.root, args)
    }

    /// Writes `path` (relative to the root), making its parents.
    pub fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let full = self.root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("mkdir for {path}: {e}"))?;
        }
        std::fs::write(&full, bytes).map_err(|e| format!("write {path}: {e}"))
    }

    /// Stages everything under `path` (`.` for all), ignored files included: `git add -f -A`.
    pub fn stage(&self, path: &str) -> Result<(), String> {
        self.git(&["add", "--force", "--all", "--", path]).map(drop)
    }

    /// Commits the index.
    pub fn commit(&self, message: &str) -> Result<(), String> {
        self.git(&["commit", "--quiet", "--message", message])
            .map(drop)
    }

    /// Stages everything and commits it.
    pub fn commit_all(&self, message: &str) -> Result<(), String> {
        self.stage(".")?;
        self.commit(message)
    }

    /// `git worktree add` of a new branch `branch` at `path` (relative to this root, or absolute).
    pub fn add_worktree(&self, path: &Path, branch: &str) -> Result<Self, String> {
        let target = self.root.join(path);
        let target_text = target
            .to_str()
            .ok_or_else(|| format!("not UTF-8: {}", target.display()))?;
        self.git(&["worktree", "add", "--quiet", "-b", branch, target_text])?;
        Ok(Self { root: target })
    }

    /// A linked worktree as an agent makes one: `.claude/worktrees/<name>` on a new branch `name`.
    pub fn add_agent_worktree(&self, name: &str) -> Result<Self, String> {
        self.add_worktree(&Path::new(".claude/worktrees").join(name), name)
    }
}
