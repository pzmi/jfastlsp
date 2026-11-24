//! How a request's file is bound to its view when its path goes through a symbolic link, how the
//! answer names files back, and how an open document is found whatever its spelling (plan §4.1).
//!
//! The repository probe resolves a worktree by its canonical root (links resolved; pinned by
//! `jfl-adapter-git`'s `should_resolve_to_the_canonical_root_when_the_path_goes_through_a_link`),
//! so a file spelled through a link at or above the root is not below the root as spelled, and the
//! view service refused it (`NotInWorktree`). macOS's temporary directory is one such spelling:
//! `$TMPDIR` is under `/var`, a link to `/private/var` (FND-17). The daemon therefore:
//!
//! - binds the request by [`canonical`], the probe's own rule
//!   (`jfl_adapter_git::resolve::canonical_path`), so the file is always under the root found;
//! - answers in the request's spelling of the worktree ([`client_root`]: the deepest directory of
//!   the request's path whose canonical path is the root), so the client is told about files by
//!   the names it uses, a link inside the worktree on the way or not (a link from another
//!   worktree into this one is such a directory too); when no directory of the path names the
//!   root, or a respelled name would be too long, the answer is in the root's canonical spelling,
//!   never a mix of the two;
//! - finds the client's open document of a file by its spelling or else by its canonical path
//!   ([`OpenDocuments`]), so the refusal of a hit in a modified open document never depends on
//!   how the request, the hit or the document was spelled. Pinned by
//!   `tests/daemon.rs::should_refuse_a_hit_in_a_modified_document_when_it_was_opened_through_a_link`,
//!   `tests/daemon.rs::should_refuse_a_hit_in_a_modified_document_when_the_request_goes_through_a_link_inside_the_worktree`
//!   and
//!   `tests/daemon.rs::should_refuse_a_hit_in_a_modified_document_when_a_link_leads_from_another_worktree`.
//!
//! Cost: a request spelled as its root's canonical path pays one `realpath` of its directory; a
//! linked spelling one more per directory up to the root's spelling; a request with open documents
//! one per open document and per file its hits name. Each is a few `lstat`s and `readlink`s on the
//! loop, as a request's own stat is (IDX-09): microseconds against the 300 ms deadline.

use std::collections::BTreeMap;
use std::path::Path;

use jfl_domain::FileUri;
use jfl_protocol::DocumentUri;

/// `file` as the views bind it: the deepest existing directory that holds it canonicalised, then
/// the rest as written ([`jfl_adapter_git::resolve::canonical_path`], the probe's rule). `None`
/// when that fails (no part of the path exists, a loop of links, no search permission: the probe
/// refuses such a path too) or the result is no `FileUri` (not UTF-8).
///
/// Pinned by: `spelling::tests::should_resolve_the_directory_s_links_when_a_file_is_reached_through_one`.
#[must_use]
pub fn canonical(file: &FileUri) -> Option<FileUri> {
    let real = jfl_adapter_git::resolve::canonical_path(Path::new(file.path())).ok()?;
    FileUri::from_path(real.to_str()?).ok()
}

/// The request's spelling of its view's `root`, when it is not the root's own: the deepest
/// directory of `asked`'s path whose canonical path is `root`. `None` when `asked` is spelled
/// under the root as the root is, or no directory of its path names the root.
///
/// Pinned by: `spelling::tests::should_name_the_client_s_root_when_a_link_is_on_the_way`.
#[must_use]
pub fn client_root(asked: &FileUri, root: &FileUri) -> Option<FileUri> {
    if asked.relative_to(root).is_some() {
        return None;
    }
    let mut dir = Path::new(asked.path()).parent();
    while let Some(at) = dir {
        let spelled = FileUri::from_path(at.to_str()?).ok()?;
        if canonical(&spelled).as_ref() == Some(root) {
            return Some(spelled);
        }
        dir = at.parent();
    }
    None
}

/// A connection's open documents, found by a file's spelling or else by its canonical path: the
/// canonical paths are taken once, when there is any open document.
#[derive(Debug)]
pub struct OpenDocuments<'a, T> {
    spelled: &'a BTreeMap<DocumentUri, T>,
    canonical: BTreeMap<FileUri, &'a T>,
}

impl<'a, T> OpenDocuments<'a, T> {
    /// The documents of `open`.
    #[must_use]
    pub fn new(open: &'a BTreeMap<DocumentUri, T>) -> Self {
        let canonical = open
            .iter()
            .filter_map(|(uri, doc)| Some((canonical(uri.file())?, doc)))
            .collect();
        Self {
            spelled: open,
            canonical,
        }
    }

    /// Whether no document is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.spelled.is_empty()
    }

    /// The open document of `file`: the one spelled as `file`, else the one whose canonical path
    /// is `file`'s.
    ///
    /// Pinned by: `spelling::tests::should_find_an_open_document_when_it_is_asked_for_by_another_spelling`.
    #[must_use]
    pub fn get(&self, file: &FileUri) -> Option<&'a T> {
        if self.spelled.is_empty() {
            return None;
        }
        if let Some((_, doc)) = self.spelled.iter().find(|(open, _)| open.file() == file) {
            return Some(doc);
        }
        self.canonical.get(&canonical(file)?).copied()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;

    use jfl_testkit::fixtures::ScratchDir;

    use super::*;

    fn uri(path: &Path) -> FileUri {
        FileUri::from_path(path.to_str().unwrap()).unwrap()
    }

    /// `real/wt/src/A.java`, `via` a link to `real`, and `wt/alias` a link to `wt/src`; the
    /// scratch directory canonicalised first, so a link above it (macOS's `/var`) does not count.
    fn world() -> (ScratchDir, PathBuf) {
        let scratch = ScratchDir::new(&std::env::temp_dir(), "jfl-spelling").unwrap();
        let base = fs::canonicalize(scratch.path()).unwrap();
        fs::create_dir_all(base.join("real/wt/src")).unwrap();
        fs::write(base.join("real/wt/src/A.java"), b"class A {}").unwrap();
        symlink(base.join("real"), base.join("via")).unwrap();
        symlink(base.join("real/wt/src"), base.join("real/wt/alias")).unwrap();
        (scratch, base)
    }

    #[test]
    fn should_resolve_the_directory_s_links_when_a_file_is_reached_through_one() {
        // given
        let (_scratch, base) = world();

        // when
        let through = canonical(&uri(&base.join("via/wt/src/A.java")));
        let missing = canonical(&uri(&base.join("via/wt/src/new/B.java")));
        let inside = canonical(&uri(&base.join("real/wt/alias/A.java")));
        let plain = canonical(&uri(&base.join("real/wt/src/A.java")));

        // then: the directory's links resolved, the rest of the path kept as written
        assert_eq!(through, Some(uri(&base.join("real/wt/src/A.java"))));
        assert_eq!(missing, Some(uri(&base.join("real/wt/src/new/B.java"))));
        assert_eq!(inside, Some(uri(&base.join("real/wt/src/A.java"))));
        assert_eq!(plain, Some(uri(&base.join("real/wt/src/A.java"))));
    }

    #[test]
    fn should_name_the_client_s_root_when_a_link_is_on_the_way() {
        // given
        let (_scratch, base) = world();
        let root = uri(&base.join("real/wt"));

        // when
        let through = client_root(&uri(&base.join("via/wt/src/A.java")), &root);
        let both = client_root(&uri(&base.join("via/wt/alias/A.java")), &root);
        let plain = client_root(&uri(&base.join("real/wt/src/A.java")), &root);
        let inside = client_root(&uri(&base.join("real/wt/alias/A.java")), &root);
        let elsewhere = client_root(&uri(&base.join("via/other/src/A.java")), &root);

        // then: the deepest directory naming the root, a link inside the worktree on the way or
        // not; nothing when the root is spelled as it is, or not named at all
        assert_eq!(through, Some(uri(&base.join("via/wt"))));
        assert_eq!(both, Some(uri(&base.join("via/wt"))));
        assert_eq!(plain, None);
        assert_eq!(inside, None);
        assert_eq!(elsewhere, None);
    }

    #[test]
    fn should_find_an_open_document_when_it_is_asked_for_by_another_spelling() {
        // given: A.java open as spelled through `via`
        let (_scratch, base) = world();
        let mut open = BTreeMap::new();
        open.insert(
            DocumentUri::from(uri(&base.join("via/wt/src/A.java"))),
            "open",
        );
        let documents = OpenDocuments::new(&open);

        // when
        let spelled = documents.get(&uri(&base.join("via/wt/src/A.java")));
        let canonical = documents.get(&uri(&base.join("real/wt/src/A.java")));
        let inside = documents.get(&uri(&base.join("real/wt/alias/A.java")));
        let other = documents.get(&uri(&base.join("real/wt/src/B.java")));

        // then
        assert_eq!(spelled, Some(&"open"));
        assert_eq!(canonical, Some(&"open"));
        assert_eq!(inside, Some(&"open"));
        assert_eq!(other, None);
    }
}
