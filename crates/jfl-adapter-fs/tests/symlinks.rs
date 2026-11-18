//! Symbolic links that leave the root are never followed (plan §11): not by the walk, not by the
//! watcher.

use std::fs;
use std::os::unix::fs::symlink;
use std::time::Duration;

use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_application::source_tree::{Exclusion, Hint, Hints, ListMode, SourceTree};
use jfl_domain::RelPath;
use jfl_testkit::exec::{InlinePool, block_on};
use jfl_testkit::poll::wait_until;

#[cfg(test)]
mod support;
use support::{Scratch, uri, write};

/// A deadline no healthy watcher comes near; a passing test returns at the hint.
const GENEROUS: Duration = Duration::from_secs(20);

#[test]
fn should_not_follow_a_link_when_it_points_outside_the_root() {
    // given
    let scratch = Scratch::new("symlinks");
    let root = scratch.path().join("root");
    let outside = scratch.path().join("outside");
    write(&root, "a/A.java", b"class A {}").unwrap();
    write(&outside, "Outside.java", b"class Outside {}").unwrap();
    write(&outside, "sub/Deep.java", b"class Deep {}").unwrap();
    symlink(&outside, root.join("escape")).unwrap();
    symlink(outside.join("sub"), root.join("a/deeper")).unwrap();
    symlink(outside.join("Outside.java"), root.join("a/Linked.java")).unwrap();
    let tree = FsSourceTree::new(InlinePool, FsConfig::default());

    // when
    let listing = block_on(tree.list(&uri(&root), ListMode::PathsAndStat)).unwrap();

    // then
    let paths: Vec<&str> = listing.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["a/A.java"]);
    let skipped: Vec<(&str, Exclusion)> = listing
        .skipped
        .iter()
        .map(|s| (s.path.as_str(), s.why))
        .collect();
    assert_eq!(
        skipped,
        [
            ("a/Linked.java", Exclusion::Symlink),
            ("a/deeper", Exclusion::Symlink),
            ("escape", Exclusion::Symlink),
        ]
    );
    assert!(listing.is_complete());
}

#[test]
fn should_not_hint_a_change_when_it_is_behind_a_link_out_of_the_root() {
    // given
    let scratch = Scratch::new("symlinks-watch");
    let root = scratch.path().join("root");
    let outside = scratch.path().join("outside");
    fs::create_dir_all(root.join("a")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    symlink(&outside, root.join("escape")).unwrap();
    let tree = FsSourceTree::new(InlinePool, FsConfig::default());
    let mut watch = block_on(tree.watch(&uri(&root))).unwrap();

    // when
    write(&outside, "New.java", b"class New {}").unwrap();
    write(&root, "a/B.java", b"class B {}").unwrap();

    // then
    let marker = Hint::Changed(RelPath::new("a/B.java").unwrap());
    let mut seen = Vec::new();
    wait_until(GENEROUS, "the hint for a/B.java", || {
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
    assert!(
        seen.iter()
            .all(|h| !matches!(h, Hint::Changed(p) if p.as_str().starts_with("escape/"))),
        "{seen:?}"
    );
}
