//! The watched set: one watch per directory the exclusion rules accept, so a nested worktree, an
//! agent worktree, `.git` and build output are never watched, and their changes never reach the
//! parent's hints (plan §4.2); and every directory that holds a listed file is watched.

use std::os::unix::fs::symlink;
use std::path::Path;

use jfl_adapter_fs::walk::watched_dirs;
use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_application::source_tree::{ListMode, SourceTree};
use jfl_testkit::exec::{InlinePool, block_on};

#[cfg(test)]
mod support;
use support::{Scratch, uri, write};

fn tree(root: &Path, outside: &Path) -> Result<(), String> {
    write(root, "a/b/B.java", b"class B {}")?;
    write(root, ".git/objects/x", b"")?;
    write(root, ".claude/worktrees/agent/src/A.java", b"class A {}")?;
    write(root, ".claude/notes/n.md", b"")?;
    write(root, "nested/.git", b"gitdir: /elsewhere\n")?;
    write(root, "nested/src/N.java", b"class N {}")?;
    write(root, "build.gradle", b"")?;
    write(root, "build/classes/A.class", b"")?;
    write(root, ".gitignore", b"ignored/\n")?;
    write(root, "ignored/I.java", b"class I {}")?;
    write(outside, "O.java", b"class O {}")?;
    symlink(outside, root.join("link")).map_err(|e| e.to_string())
}

fn names(root: &Path, dirs: &[std::path::PathBuf]) -> Vec<String> {
    let mut names: Vec<String> = dirs
        .iter()
        .map(|d| {
            d.strip_prefix(root)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn should_watch_only_what_the_rules_accept_when_a_tree_is_walked() {
    // given
    let scratch = Scratch::new("watched-dirs");
    let root = scratch.path().join("root");
    tree(&root, &scratch.path().join("outside")).unwrap();

    // when
    let (all, errors) = watched_dirs(&root, &root, 0);
    let (below_a, _) = watched_dirs(&root, &root.join("a"), 0);
    let (nested, _) = watched_dirs(&root, &root.join("nested"), 0);
    let (agents, _) = watched_dirs(&root, &root.join(".claude/worktrees/agent"), 0);

    // then
    assert_eq!(errors, []);
    assert_eq!(
        names(&root, &all),
        ["", ".claude", ".claude/notes", "a", "a/b", "ignored"]
    );
    assert_eq!(names(&root, &below_a), ["a", "a/b"]);
    assert!(nested.is_empty());
    assert!(agents.is_empty());
    let adapter = FsSourceTree::new(InlinePool, FsConfig::default());
    let listing = block_on(adapter.list(&uri(&root), ListMode::Paths)).unwrap();
    for file in &listing.files {
        let dir = root.join(file.path.as_str());
        let dir = dir.parent().unwrap();
        assert!(
            all.iter().any(|d| d == dir),
            "{dir:?} holds a listed file but is not watched"
        );
    }
}

/// Linux only: on macOS one recursive FSEvents stream covers the whole root (`watch`'s doc), so a
/// nested worktree's changes are hinted there and filtered by the reconcile.
#[cfg(not(target_os = "macos"))]
#[test]
fn should_not_hint_a_change_when_it_is_inside_a_nested_worktree() {
    use std::fs;
    use std::time::Duration;

    use jfl_application::source_tree::{Hint, Hints};
    use jfl_domain::RelPath;
    use jfl_testkit::poll::wait_until;

    /// A deadline no healthy watcher comes near; a passing test returns at the hint.
    const GENEROUS: Duration = Duration::from_secs(20);

    // given
    let scratch = Scratch::new("watched-nested");
    let root = scratch.path().join("root");
    tree(&root, &scratch.path().join("outside")).unwrap();
    let adapter = FsSourceTree::new(InlinePool, FsConfig::default());
    let mut watch = block_on(adapter.watch(&uri(&root))).unwrap();

    // when
    fs::write(root.join("nested/src/Changed.java"), b"class C {}").unwrap();
    fs::write(root.join("build/classes/B.class"), b"").unwrap();
    write(&root, "a/Marker.java", b"class M {}").unwrap();

    // then
    let marker = Hint::Changed(RelPath::new("a/Marker.java").unwrap());
    let mut seen = Vec::new();
    wait_until(GENEROUS, "the hint for a/Marker.java", || {
        while let Some(hint) = watch.try_next() {
            let done = hint == marker;
            seen.push(hint);
            if done {
                return true;
            }
        }
        false
    })
    .unwrap();
    assert_eq!(seen, [marker], "only the marker is hinted");
}

/// A directory the watch cannot read or watch is a rescan that says the watch is degraded, never
/// skipped. A superuser reads a `chmod 000` directory, so then the directory is simply watched;
/// whichever it is, the outcome must agree with whether the directory can be read.
#[test]
fn should_report_a_degraded_watch_when_a_directory_cannot_be_watched() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use jfl_application::source_tree::{Hint, Hints};

    // given
    let scratch = Scratch::new("watched-locked");
    let root = scratch.path().join("root");
    write(&root, "locked/inner/L.java", b"class L {}").unwrap();
    let locked = root.join("locked");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let readable = fs::read_dir(&locked).is_ok();
    let adapter = FsSourceTree::new(InlinePool, FsConfig::default());

    // when
    let watch = block_on(adapter.watch(&uri(&root)));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    // then
    let mut watch = watch.unwrap();
    let first = watch.try_next();
    let degraded = matches!(&first, Some(Hint::Rescan(r)) if r.reasons.degrade_the_watch());
    assert_eq!(
        degraded, !readable,
        "readable: {readable}, first hint: {first:?}"
    );
}
