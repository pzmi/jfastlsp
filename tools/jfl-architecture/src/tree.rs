//! The files a check reads, by path relative to the workspace root: the repository on disk, or
//! an in-memory tree for the table-driven cases (a fake, not a mock: it holds real contents).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// A walk never enters a `.git` directory, at any depth.
const SKIPPED_ANYWHERE: &str = ".git";
/// Nor, at the root only, cargo's default build output and the work-order scratch directory
/// (gitignored): a module directory named `target` deeper down is ordinary source and is walked.
const SKIPPED_AT_ROOT: [&str; 2] = ["target", ".work"];

/// Read access to a workspace's files.
pub trait Tree {
    /// The contents of `path`, or `None` when it is absent or unreadable.
    fn read(&self, path: &str) -> Option<String>;
    /// The bytes of `path` (a fixture need not be UTF-8), or `None` when it is absent or
    /// unreadable.
    fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
        self.read(path).map(String::into_bytes)
    }
    /// Every file under `dir` (recursively; `""` is the whole tree), as paths relative to the
    /// root, sorted. `.git` directories, and `target` and `.work` at the root, are not entered.
    fn files_under(&self, dir: &str) -> Vec<String>;
    /// What a walk could not read (an unreadable directory, a symlinked directory it did not
    /// follow): each becomes an `ARCH-metadata` finding, so a gap never passes silently.
    fn problems(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The workspace on disk.
#[derive(Debug)]
pub struct DiskTree {
    root: PathBuf,
    skipped: Vec<PathBuf>,
    problems: RefCell<BTreeSet<String>>,
}

impl DiskTree {
    /// The tree rooted at `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            skipped: SKIPPED_AT_ROOT.iter().map(|d| root.join(d)).collect(),
            problems: RefCell::new(BTreeSet::new()),
        }
    }

    /// Also skips `dir` (cargo's resolved target directory, when it lies inside the tree).
    pub fn skipping(mut self, dir: &Path) -> Self {
        self.skipped.push(dir.to_path_buf());
        self
    }
}

impl Tree for DiskTree {
    fn read(&self, path: &str) -> Option<String> {
        std::fs::read_to_string(self.root.join(path)).ok()
    }

    fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.root.join(path)).ok()
    }

    fn files_under(&self, dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        let start = if dir.is_empty() {
            self.root.clone()
        } else {
            self.root.join(dir)
        };
        if !start.exists() {
            return found;
        }
        let mut pending = vec![start];
        while let Some(current) = pending.pop() {
            let relative = |path: &Path| {
                path.strip_prefix(&self.root)
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default()
            };
            let entries = match std::fs::read_dir(&current) {
                Ok(entries) => entries,
                Err(error) => {
                    self.problems
                        .borrow_mut()
                        .insert(format!("{}: cannot be read: {error}", relative(&current)));
                    continue;
                }
            };
            for entry in entries {
                let Ok(entry) = entry else {
                    self.problems
                        .borrow_mut()
                        .insert(format!("{}: an entry cannot be read", relative(&current)));
                    continue;
                };
                let path = entry.path();
                let name = entry.file_name();
                let Ok(kind) = entry.file_type() else {
                    self.problems
                        .borrow_mut()
                        .insert(format!("{}: its type cannot be read", relative(&path)));
                    continue;
                };
                if kind.is_dir() {
                    if name.as_os_str() != SKIPPED_ANYWHERE && !self.skipped.contains(&path) {
                        pending.push(path);
                    }
                } else if kind.is_symlink() && path.is_dir() {
                    self.problems.borrow_mut().insert(format!(
                        "{}: a symlinked directory, not followed: replace it with a directory",
                        relative(&path)
                    ));
                } else {
                    found.push(relative(&path));
                }
            }
        }
        found.sort();
        found
    }

    fn problems(&self) -> Vec<String> {
        self.problems.borrow().iter().cloned().collect()
    }
}

/// An in-memory tree: path to contents.
#[derive(Debug, Default, Clone)]
pub struct MemoryTree {
    files: BTreeMap<String, String>,
}

impl MemoryTree {
    /// An empty tree.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces `path`.
    pub fn with(mut self, path: &str, contents: &str) -> Self {
        self.files.insert(path.to_owned(), contents.to_owned());
        self
    }

    /// Removes `path`.
    pub fn without(mut self, path: &str) -> Self {
        self.files.remove(path);
        self
    }
}

impl Tree for MemoryTree {
    fn read(&self, path: &str) -> Option<String> {
        self.files.get(path).cloned()
    }

    fn files_under(&self, dir: &str) -> Vec<String> {
        let prefix = if dir.is_empty() {
            String::new()
        } else {
            format!("{}/", dir.trim_end_matches('/'))
        };
        self.files
            .keys()
            .filter(|path| path.starts_with(&prefix))
            .filter(|path| {
                !path.split('/').any(|part| part == SKIPPED_ANYWHERE)
                    && !SKIPPED_AT_ROOT
                        .iter()
                        .any(|d| path.starts_with(&format!("{d}/")))
            })
            .cloned()
            .collect()
    }
}
