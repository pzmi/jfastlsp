//! Reads and stats refuse a FIFO without blocking on it (a FIFO opened for reading waits for a
//! writer unless it is opened non-blocking); a stale cookie is swept and nothing else; an
//! existing cookie name is left alone; a directory on another device is a gap, not walked.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jfl_adapter_fs::walk::list_on;
use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_application::source_tree::{ListMode, MTime, SourceTree, SourceTreeError, UnlistedReason};
use jfl_domain::RelPath;
use jfl_testkit::exec::{InlinePool, block_on};
use jfl_testkit::fixtures::fifo;

#[cfg(test)]
mod support;
use support::{Scratch, uri, write};

#[test]
fn should_refuse_a_fifo_without_blocking_when_it_is_read_or_stated() {
    // given
    let scratch = Scratch::new("fifo");
    let root = scratch.path().join("root");
    write(&root, "A.java", b"class A {}").unwrap();
    fifo(&root.join("Pipe.java")).unwrap();
    let adapter = FsSourceTree::new(InlinePool, FsConfig::default());
    let path = RelPath::new("Pipe.java").unwrap();

    // when
    let read = block_on(adapter.read(&uri(&root), &path, 1024));
    let stat = block_on(adapter.stat(&uri(&root), &path));

    // then
    assert_eq!(read, Err(SourceTreeError::NotAFile));
    assert_eq!(stat, Err(SourceTreeError::NotAFile));
}

#[cfg(test)]
mod given {
    use super::*;

    /// 1970, plus a thousand seconds: long before any cookie a test writes now.
    pub(crate) fn long_ago() -> SystemTime {
        UNIX_EPOCH.checked_add(Duration::from_secs(1000)).unwrap()
    }

    /// Puts `dir` (any directory named `mnt`) on device 2 and everything else on device 1.
    pub(crate) fn mounted(path: &std::path::Path) -> Option<u64> {
        Some(if path.file_name().is_some_and(|n| n == "mnt") {
            2
        } else {
            1
        })
    }
}

#[test]
fn should_sweep_only_stale_regular_cookies_when_a_view_opens() {
    // given
    let scratch = Scratch::new("sweep");
    let root = scratch.path().join("root");
    write(&root, "A.java", b"class A {}").unwrap();
    for name in [
        ".jfl-cookie-d1-7",
        ".jfl-cookie-d2-8",
        ".jfl-cookie-x",
        ".jfl-cookie-d3-9-1",
    ] {
        write(&root, name, b"").unwrap();
    }
    for name in [".jfl-cookie-d1-7", ".jfl-cookie-x", ".jfl-cookie-d3-9-1"] {
        std::fs::File::options()
            .write(true)
            .open(root.join(name))
            .unwrap()
            .set_modified(given::long_ago())
            .unwrap();
    }
    write(&scratch.path().join("outside"), "kept", b"").unwrap();
    std::os::unix::fs::symlink(
        scratch.path().join("outside/kept"),
        root.join(".jfl-cookie-l-1"),
    )
    .unwrap();
    let adapter = FsSourceTree::new(InlinePool, FsConfig::default());
    let before = MTime {
        secs: 1_000_000_000,
        nanos: 0,
    };

    // when
    let removed = block_on(adapter.sweep_cookies(&uri(&root), before)).unwrap();

    // then
    assert_eq!(removed, 2);
    assert!(!root.join(".jfl-cookie-d1-7").exists());
    assert!(!root.join(".jfl-cookie-d3-9-1").exists());
    assert!(
        root.join(".jfl-cookie-d2-8").exists(),
        "a fresh cookie is kept"
    );
    assert!(
        root.join(".jfl-cookie-x").exists(),
        "not a cookie's exact name"
    );
    assert!(std::fs::symlink_metadata(root.join(".jfl-cookie-l-1")).is_ok());
    assert!(scratch.path().join("outside/kept").exists());
}

#[test]
fn should_leave_an_existing_file_alone_when_a_cookie_takes_its_name() {
    // given
    let scratch = Scratch::new("cookie-exists");
    let root = scratch.path().join("root");
    write(&root, ".jfl-cookie-d1-7", b"someone's").unwrap();
    let adapter = FsSourceTree::new(InlinePool, FsConfig::default());
    let name = RelPath::new(".jfl-cookie-d1-7").unwrap();

    // when
    let written = block_on(adapter.write_cookie(&uri(&root), &name));

    // then
    assert_eq!(
        written,
        Err(SourceTreeError::Io(std::io::ErrorKind::AlreadyExists))
    );
    assert_eq!(
        std::fs::read(root.join(".jfl-cookie-d1-7")).unwrap(),
        b"someone's"
    );
}

#[test]
fn should_report_a_gap_and_not_walk_it_when_a_directory_is_another_filesystem() {
    // given
    let scratch = Scratch::new("mount");
    let root = scratch.path().join("root");
    write(&root, "src/A.java", b"class A {}").unwrap();
    write(&root, "mnt/Remote.java", b"class Remote {}").unwrap();

    // when
    let listing = list_on(&root, ListMode::Paths, &FsConfig::default(), given::mounted);

    // then
    let files: Vec<&str> = listing.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(files, ["src/A.java"]);
    assert_eq!(listing.unlisted_count, 1);
    assert_eq!(
        listing
            .unlisted
            .first()
            .map(|u| (u.path.as_ref().map(RelPath::as_str), u.why)),
        Some((Some("mnt"), UnlistedReason::OtherFilesystem))
    );
}
