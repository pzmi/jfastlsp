//! Worktree roots and the key of each root's jdtls workspace (`-data`) directory.
//!
//! jdtls's own launcher keys `-data` on a SHA-1 of the working directory's *basename*, so two
//! worktrees named alike share one workspace (claude-code#67182, ADR 0028). The key here is the
//! SHA-1 of the **full canonical path** of the innermost worktree root. SHA-1 is a directory key,
//! not a security boundary (the state directory is `0700`). Pinned by
//! `tests::should_give_different_keys_when_worktrees_share_a_basename`.

use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// The innermost directory at or above `start` that holds a `.git` entry (a directory for a
/// main checkout, a file for a linked worktree), so a nested worktree binds to itself and never
/// to its parent. With no `.git` above it, the canonical `start` (or its directory) is the root.
///
/// # Errors
///
/// The error of canonicalizing `start`.
pub fn worktree_root(start: &Path) -> io::Result<PathBuf> {
    let canonical = start.canonicalize()?;
    let base = if canonical.is_dir() {
        canonical
    } else {
        canonical
            .parent()
            .map_or_else(|| canonical.clone(), Path::to_path_buf)
    };
    let mut dir = base.as_path();
    loop {
        if dir.join(".git").symlink_metadata().is_ok() {
            return Ok(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return Ok(base),
        }
    }
}

/// The workspace key of a root: lowercase hex SHA-1 of its full path bytes.
#[must_use]
pub fn data_key(root: &Path) -> String {
    sha1_smol::Sha1::from(root.as_os_str().as_bytes())
        .digest()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_give_different_keys_when_worktrees_share_a_basename() {
        // given
        let a = Path::new("/work/one/repo");
        let b = Path::new("/work/two/repo");

        // when
        let (ka, kb) = (data_key(a), data_key(b));

        // then
        assert_ne!(ka, kb);
        assert_eq!(ka.len(), 40);
        assert_eq!(ka, data_key(Path::new("/work/one/repo")));
    }

    struct Dir(PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn tempdir(name: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("jfl-jdtls-wt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Dir(dir.canonicalize().unwrap())
    }

    #[test]
    fn should_bind_to_the_innermost_git_when_worktrees_nest() {
        // given: a main checkout with a linked worktree nested under .claude/worktrees
        let tmp = tempdir("nest");
        let outer = tmp.0.join("repo");
        let inner = outer.join(".claude/worktrees/agent-1");
        std::fs::create_dir_all(outer.join(".git")).unwrap();
        std::fs::create_dir_all(inner.join("src")).unwrap();
        std::fs::write(
            inner.join(".git"),
            "gitdir: ../../../.git/worktrees/agent-1\n",
        )
        .unwrap();
        std::fs::write(inner.join("src/A.java"), "class A {}\n").unwrap();

        // when
        let from_file = worktree_root(&inner.join("src/A.java")).unwrap();
        let from_outer = worktree_root(&outer).unwrap();

        // then
        assert_eq!(from_file, inner);
        assert_eq!(from_outer, outer);
    }

    #[test]
    fn should_fall_back_to_the_start_when_no_git_above() {
        // given
        let tmp = tempdir("nogit");
        let dir = tmp.0.join("plain");
        std::fs::create_dir_all(&dir).unwrap();

        // when
        let root = worktree_root(&dir).unwrap();

        // then (the system temp directory is not inside a repository)
        assert_eq!(root, dir);
    }
}
