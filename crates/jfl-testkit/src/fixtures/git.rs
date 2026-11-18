//! Real Git repositories and linked worktrees for integration tests (plan §4.1, §4.2): the
//! nested-worktree fixture of IDX-06 is a main checkout with linked worktrees under
//! `.claude/worktrees/`, as agents make them.
//!
//! Git runs with no system or global configuration and a fixed identity, so the user's signing,
//! hooks or templates never reach a fixture.

use std::path::Path;
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
