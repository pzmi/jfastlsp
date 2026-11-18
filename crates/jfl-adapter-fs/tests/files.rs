//! Reads and stats refuse a FIFO without blocking on it (a FIFO opened for reading waits for a
//! writer unless it is opened non-blocking).

use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_application::source_tree::{SourceTree, SourceTreeError};
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
