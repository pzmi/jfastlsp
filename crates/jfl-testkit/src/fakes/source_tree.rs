//! An in-memory `SourceTree`: a tree of files, directories and links under one root, with the
//! same exclusion rules as the filesystem adapter (`jfl_application::source_tree::exclusion`).
//!
//! It is not looser than the adapter (ver §5): it can hold what makes a real walk fail or skip
//! (an unreadable directory, a `.java` name that is not UTF-8, links inside and outside the root)
//! and reports each the same way. Its `.gitignore` support is the subset the contract suite uses:
//! one name per line, `name/` for directories only, matched at any depth below the file (as Git
//! matches a pattern without a slash). Its clock is logical: each write moves a file's mtime one
//! second on and gives a new file a new inode.
//!
//! Every write, removal and directory creation through the fake hints each live watch, unless
//! `hint_excluded` leaves the path out.
//!
//! Pinned by the `source_tree` contract suite (`contracts::source_tree::tests`).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use jfl_application::hints::{HintQueue, HintSink, hint_queue};
use jfl_application::source_tree::{
    BUILD_FILES, Exclusion, FileStat, GitEntry, HintSubject, ListMode, ListedFile, Listing, MTime,
    ReadBytes, RescanReason, Skipped, SourceTree, SourceTreeError, Unlisted, UnlistedReason,
    exclusion, hint_excluded, is_git_metadata, is_java_file_name,
};
use jfl_domain::{FileUri, RelPath};

#[derive(Debug, Clone)]
enum Entry {
    File { bytes: Vec<u8>, stat: FileStat },
    Dir { unreadable: bool },
    Link { to_dir: bool },
    NonUtf8Java,
}

#[derive(Debug)]
struct State {
    /// Keyed by the path under the root; every file's parents are `Dir` entries.
    entries: BTreeMap<String, Entry>,
    next_inode: u64,
    clock: i64,
    /// Each live watch's sink, with the tree path of the root it watches.
    sinks: Vec<(String, HintSink)>,
}

/// The fake tree; clones share it.
#[derive(Debug, Clone)]
pub struct FakeSourceTree {
    root: FileUri,
    hint_capacity: usize,
    state: Arc<Mutex<State>>,
}

fn parent_of(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

fn name_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

fn rel(path: &str) -> Option<RelPath> {
    RelPath::new(path).ok()
}

/// The `.gitignore` patterns in force: (the directory holding the file, its lines).
type Ignores = Vec<(String, Vec<String>)>;

fn ignored(ignores: &Ignores, name: &str, is_dir: bool) -> bool {
    ignores.iter().any(|(_, lines)| {
        lines.iter().any(|line| match line.strip_suffix('/') {
            Some(dir_name) => is_dir && dir_name == name,
            None => line == name,
        })
    })
}

impl FakeSourceTree {
    /// An empty tree rooted at `root`, whose watches queue at most `hint_capacity` paths.
    #[must_use]
    pub fn new(root: FileUri, hint_capacity: usize) -> Self {
        let mut entries = BTreeMap::new();
        entries.insert(String::new(), Entry::Dir { unreadable: false });
        Self {
            root,
            hint_capacity,
            state: Arc::new(Mutex::new(State {
                entries,
                next_inode: 1,
                clock: 1_700_000_000,
                sinks: Vec::new(),
            })),
        }
    }

    /// The root the tree answers for.
    #[must_use]
    pub fn root(&self) -> &FileUri {
        &self.root
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn hint(state: &State, key: &str) {
        let unnamed = matches!(state.entries.get(key), Some(Entry::NonUtf8Java));
        let subject = match state.entries.get(key) {
            Some(Entry::File { .. }) => HintSubject::File,
            Some(Entry::Dir { .. }) => HintSubject::Directory,
            _ => HintSubject::Unknown,
        };
        for (base, sink) in &state.sinks {
            if unnamed {
                sink.rescan(RescanReason::Unnamed);
            } else if let Some(path) = under(base, key)
                && !hint_excluded(&path, subject)
            {
                sink.changed(path);
            }
        }
    }

    fn make_parents(state: &mut State, path: &str) -> Result<(), String> {
        let mut dir = String::new();
        let Some(parent) = parent_of(path) else {
            return Ok(());
        };
        for component in parent.split('/') {
            dir = join(&dir, component);
            match state.entries.get(&dir) {
                Some(Entry::Dir { .. }) => {}
                Some(_) => return Err(format!("{dir} is not a directory")),
                None => {
                    state
                        .entries
                        .insert(dir.clone(), Entry::Dir { unreadable: false });
                    Self::hint(state, &dir);
                }
            }
        }
        Ok(())
    }

    fn put(&self, path: &str, entry: Entry) -> Result<(), String> {
        rel(path).ok_or_else(|| format!("{path:?} is not a relative path"))?;
        let mut state = self.lock();
        Self::make_parents(&mut state, path)?;
        state.entries.insert(path.to_owned(), entry);
        Self::hint(&state, path);
        Ok(())
    }

    /// Writes a file, creating its parents; a rewrite keeps the inode and moves the mtime on.
    pub fn write(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let stat = {
            let mut state = self.lock();
            state.clock = state.clock.saturating_add(1);
            let inode = match state.entries.get(path) {
                Some(Entry::File { stat, .. }) => stat.inode,
                Some(_) => return Err(format!("{path} is not a file")),
                None => {
                    state.next_inode = state.next_inode.saturating_add(1);
                    state.next_inode
                }
            };
            FileStat {
                size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                mtime: MTime {
                    secs: state.clock,
                    nanos: 0,
                },
                inode,
            }
        };
        self.put(
            path,
            Entry::File {
                bytes: bytes.to_vec(),
                stat,
            },
        )
    }

    /// Creates a directory and its parents.
    pub fn mkdir(&self, path: &str) -> Result<(), String> {
        if matches!(self.lock().entries.get(path), Some(Entry::Dir { .. })) {
            return Ok(());
        }
        self.put(path, Entry::Dir { unreadable: false })
    }

    /// Removes a path and everything below it.
    pub fn remove(&self, path: &str) -> Result<(), String> {
        let mut state = self.lock();
        let below = format!("{path}/");
        let before = state.entries.len();
        state
            .entries
            .retain(|key, _| key != path && !key.starts_with(&below));
        if state.entries.len() == before {
            return Err(format!("{path} does not exist"));
        }
        Self::hint(&state, path);
        Ok(())
    }

    /// Renames `from` (and everything below it) to `to`, hinting both names.
    pub fn rename(&self, from: &str, to: &str) -> Result<(), String> {
        rel(to).ok_or_else(|| format!("{to:?} is not a relative path"))?;
        let mut state = self.lock();
        let below = format!("{from}/");
        let moved: Vec<(String, Entry)> = state
            .entries
            .iter()
            .filter(|(key, _)| *key == from || key.starts_with(&below))
            .map(|(key, entry)| (key.clone(), entry.clone()))
            .collect();
        if moved.is_empty() {
            return Err(format!("{from} does not exist"));
        }
        Self::make_parents(&mut state, to)?;
        for (key, entry) in moved {
            state.entries.remove(&key);
            let suffix = key.get(from.len()..).unwrap_or_default();
            state.entries.insert(format!("{to}{suffix}"), entry);
        }
        Self::hint(&state, from);
        Self::hint(&state, to);
        Ok(())
    }

    /// A symbolic link at `path`, to a directory (`to_dir`) or a file, inside or outside the root.
    pub fn link(&self, path: &str, to_dir: bool) -> Result<(), String> {
        self.put(path, Entry::Link { to_dir })
    }

    /// A `.java` file in `dir` whose name is not UTF-8.
    pub fn non_utf8_java(&self, dir: &str) -> Result<(), String> {
        self.put(&join(dir, "\u{fffd}-not-utf-8.java"), Entry::NonUtf8Java)
    }

    /// Makes `dir` unreadable, as `chmod 000` does for a user other than root.
    pub fn unreadable(&self, dir: &str) -> Result<(), String> {
        let mut state = self.lock();
        match state.entries.get_mut(dir) {
            Some(Entry::Dir { unreadable }) => {
                *unreadable = true;
                Ok(())
            }
            _ => Err(format!("{dir} is not a directory")),
        }
    }

    /// The tree's path for a root or a file under it: `Some("")` for the root itself.
    fn key(&self, uri: &FileUri) -> Option<String> {
        if uri == &self.root {
            return Some(String::new());
        }
        uri.relative_to(&self.root).map(|p| p.as_str().to_owned())
    }

    fn base(&self, root: &FileUri) -> Result<String, SourceTreeError> {
        let key = self.key(root).ok_or(SourceTreeError::RootNotFound)?;
        match self.lock().entries.get(&key) {
            Some(Entry::Dir { .. }) => Ok(key),
            Some(_) => Err(SourceTreeError::RootNotADirectory),
            None => Err(SourceTreeError::RootNotFound),
        }
    }

    fn file(&self, root: &FileUri, path: &RelPath) -> Result<(Vec<u8>, FileStat), SourceTreeError> {
        let base = self.base(root)?;
        let key = join(&base, path.as_str());
        match self.lock().entries.get(&key) {
            Some(Entry::File { bytes, stat }) => Ok((bytes.clone(), *stat)),
            Some(_) => Err(SourceTreeError::NotAFile),
            None => Err(SourceTreeError::NotFound),
        }
    }

    /// The listing, as the adapter's walk would produce it.
    pub fn list_now(&self, root: &FileUri, mode: ListMode) -> Result<Listing, SourceTreeError> {
        let base = self.base(root)?;
        let state = self.lock();
        let mut listing = Listing::default();
        let mut ignores = Ignores::new();
        walk(&state, &base, &base, &mut ignores, mode, &mut listing);
        listing.sort();
        Ok(listing)
    }
}

fn children<'s>(state: &'s State, dir: &str) -> Vec<(&'s str, &'s Entry)> {
    let prefix = if dir.is_empty() {
        String::new()
    } else {
        format!("{dir}/")
    };
    state
        .entries
        .range(prefix.clone()..)
        .take_while(|(key, _)| key.starts_with(&prefix))
        .filter(|(key, _)| {
            key.len() > prefix.len() && key.get(prefix.len()..).is_some_and(|r| !r.contains('/'))
        })
        .map(|(key, entry)| (key.as_str(), entry))
        .collect()
}

fn gitignore_lines(state: &State, dir: &str) -> Option<Vec<String>> {
    match state.entries.get(&join(dir, ".gitignore")) {
        Some(Entry::File { bytes, .. }) => Some(
            String::from_utf8_lossy(bytes)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
                .collect(),
        ),
        _ => None,
    }
}

fn under(base: &str, key: &str) -> Option<RelPath> {
    if base.is_empty() {
        return rel(key);
    }
    rel(key.strip_prefix(base)?.strip_prefix('/')?)
}

fn walk(
    state: &State,
    base: &str,
    dir: &str,
    ignores: &mut Ignores,
    mode: ListMode,
    listing: &mut Listing,
) {
    let pushed = gitignore_lines(state, dir).map(|lines| ignores.push((dir.to_owned(), lines)));
    for (key, entry) in children(state, dir) {
        let name = name_of(key);
        if is_git_metadata(name) || ignored(ignores, name, matches!(entry, Entry::Dir { .. })) {
            continue;
        }
        match entry {
            Entry::File { stat, .. } => {
                if let Some(path) = under(base, key).filter(|_| is_java_file_name(name)) {
                    let stat = (mode == ListMode::PathsAndStat).then_some(*stat);
                    listing.files.push(ListedFile { path, stat });
                }
            }
            Entry::NonUtf8Java => listing.record_unlisted(Unlisted {
                path: under(base, dir),
                why: UnlistedReason::NonUtf8Name,
            }),
            Entry::Link { to_dir } => {
                if let Some(path) = under(base, key).filter(|_| *to_dir || is_java_file_name(name))
                {
                    listing.skipped.push(Skipped {
                        path,
                        why: Exclusion::Symlink,
                    });
                }
            }
            Entry::Dir { unreadable } => {
                let parent = (dir != base).then(|| name_of(dir));
                let why = exclusion(
                    parent,
                    name,
                    || match state.entries.get(&join(key, ".git")) {
                        Some(Entry::Dir { .. }) => Some(GitEntry::Dir),
                        Some(_) => Some(GitEntry::File),
                        None => None,
                    },
                    || {
                        BUILD_FILES
                            .iter()
                            .any(|file| state.entries.contains_key(&join(dir, file)))
                    },
                );
                match (why, under(base, key)) {
                    (Some(why), Some(path)) => listing.skipped.push(Skipped { path, why }),
                    (Some(_), None) => {}
                    (None, path) if *unreadable => listing.record_unlisted(Unlisted {
                        path,
                        why: UnlistedReason::Unreadable,
                    }),
                    (None, _) => walk(state, base, key, ignores, mode, listing),
                }
            }
        }
    }
    if pushed.is_some() {
        ignores.pop();
    }
}

/// A watch on the fake.
#[derive(Debug)]
pub struct FakeWatch(HintQueue);

impl jfl_application::source_tree::Hints for FakeWatch {
    fn try_next(&mut self) -> Option<jfl_application::Hint> {
        self.0.try_next()
    }

    fn next(&mut self) -> impl Future<Output = Option<jfl_application::Hint>> + Send {
        self.0.next()
    }
}

impl SourceTree for FakeSourceTree {
    type Watch = FakeWatch;

    async fn list(&self, root: &FileUri, mode: ListMode) -> Result<Listing, SourceTreeError> {
        self.list_now(root, mode)
    }

    async fn read(
        &self,
        root: &FileUri,
        path: &RelPath,
        limit: usize,
    ) -> Result<ReadBytes, SourceTreeError> {
        let (mut bytes, _) = self.file(root, path)?;
        let truncated = bytes.len() > limit;
        bytes.truncate(limit);
        Ok(ReadBytes { bytes, truncated })
    }

    async fn stat(&self, root: &FileUri, path: &RelPath) -> Result<FileStat, SourceTreeError> {
        self.file(root, path).map(|(_, stat)| stat)
    }

    async fn watch(&self, root: &FileUri) -> Result<FakeWatch, SourceTreeError> {
        let base = self.base(root)?;
        let (sink, queue) = hint_queue(self.hint_capacity);
        self.lock().sinks.push((base, sink));
        Ok(FakeWatch(queue))
    }
}
