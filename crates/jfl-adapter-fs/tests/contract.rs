//! The `source_tree` contract suite (`jfl_testkit::contracts::source_tree`, ver §5) against the
//! filesystem adapter, on real directories, links and a real watcher.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;

use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_domain::FileUri;
use jfl_testkit::contracts::source_tree::{Factory, Fixture, check};
use jfl_testkit::contracts::{NonUtf8Name, PLATFORM_NON_UTF8_NAME};
use jfl_testkit::exec::InlinePool;

#[cfg(test)]
mod support;
use support::{Scratch, uri, write};

struct Real {
    /// Directories a case made unreadable, given their permissions back before the scratch
    /// directory is removed.
    locked: RefCell<Vec<PathBuf>>,
    _scratch: Scratch,
    root: PathBuf,
    outside: PathBuf,
    root_uri: FileUri,
    tree: FsSourceTree<InlinePool>,
}

impl Fixture for Real {
    type Tree = FsSourceTree<InlinePool>;

    fn tree(&self) -> &Self::Tree {
        &self.tree
    }

    fn root(&self) -> &FileUri {
        &self.root_uri
    }

    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        write(&self.root, path, bytes)
    }

    fn mkdir(&self, path: &str) -> Result<(), String> {
        fs::create_dir_all(self.root.join(path)).map_err(|e| e.to_string())
    }

    fn remove(&self, path: &str) -> Result<(), String> {
        let full = self.root.join(path);
        if full.is_dir() {
            fs::remove_dir_all(full)
        } else {
            fs::remove_file(full)
        }
        .map_err(|e| e.to_string())
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        fs::rename(self.root.join(from), self.root.join(to)).map_err(|e| e.to_string())
    }

    fn link_outside(&self, path: &str) -> Result<(), String> {
        write(&self.outside, "Outside.java", b"class Outside {}")?;
        symlink(&self.outside, self.root.join(path)).map_err(|e| e.to_string())
    }

    fn link_inside(&self, path: &str, target: &str) -> Result<(), String> {
        symlink(self.root.join(target), self.root.join(path)).map_err(|e| e.to_string())
    }

    fn non_utf8_java(&self, dir: &str) -> Result<NonUtf8Name, String> {
        let name = OsStr::from_bytes(b"\xff-not-utf-8.java");
        match fs::write(self.root.join(dir).join(name), b"class X {}") {
            Ok(()) => Ok(NonUtf8Name::Written),
            Err(e) if e.raw_os_error() == Some(libc::EILSEQ) => Ok(NonUtf8Name::Refused),
            Err(e) => Err(e.to_string()),
        }
    }

    fn unreadable(&self, dir: &str) -> Result<(), String> {
        let dir = self.root.join(dir);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o000)).map_err(|e| e.to_string())?;
        self.locked.borrow_mut().push(dir);
        Ok(())
    }
}

impl Drop for Real {
    fn drop(&mut self) {
        for dir in self.locked.borrow().iter() {
            let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o755));
        }
    }
}

struct Reals;

impl Factory for Reals {
    type Fixture = Real;

    fn fixture(&self, hint_capacity: usize) -> Result<Real, String> {
        let scratch = Scratch::new("contract");
        let root = scratch.path().join("root");
        let outside = scratch.path().join("outside");
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let config = FsConfig {
            hint_capacity,
            ..FsConfig::default()
        };
        Ok(Real {
            root_uri: uri(&root),
            root,
            outside,
            locked: RefCell::default(),
            _scratch: scratch,
            tree: FsSourceTree::new(InlinePool, config),
        })
    }
}

#[test]
fn should_pass_the_source_tree_contract_when_the_tree_is_on_disk() {
    // given
    let factory = Reals;

    // when
    let violations = check(&factory);

    // then
    assert_eq!(violations, []);
}

#[test]
fn should_write_or_refuse_a_non_utf8_name_as_the_platform_does_when_the_fixture_makes_one() {
    // given: Linux holds the name; APFS refuses it with EILSEQ (PLATFORM_NON_UTF8_NAME)
    let fixture = Reals.fixture(1).unwrap();
    fixture.mkdir("a").unwrap();

    // when
    let outcome = fixture.non_utf8_java("a");

    // then
    assert_eq!(outcome, Ok(PLATFORM_NON_UTF8_NAME));
}
