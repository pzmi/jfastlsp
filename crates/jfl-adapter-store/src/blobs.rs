//! The content-addressed blob directory (ADR 0025: cacache's layout, not the crate) and its
//! least-recently-used cap.
//!
//! ```text
//! <root>/FORMAT                 "jfl summary store <FORMAT_VERSION>\n"
//! <root>/objects/ab/cdef…       one entry per SyntaxKey: the first two hex digits, then the rest
//! <root>/objects/ab/.tmp-cdef…-N   a write in progress, renamed over its entry when whole
//! ```
//!
//! **Versioned** (fnd §6): `open` reads `FORMAT`; a missing or different one discards `objects/`
//! whole and starts empty, never migrating an entry (each entry's own header carries the version
//! too, `codec`). **Atomic writes**: an entry is written to a temporary file in its own directory
//! and renamed over its name, so a reader sees the old entry, no entry, or the whole new one.
//! There is no fsync: after a crash or power loss an entry may be empty or torn, and its header's
//! length and digest make it a miss, which is all a cache owes (plan §5.10); an fsync per entry
//! would cost a full index of the large corpus thousands of disk flushes for no correctness.
//! Temporary files left by a crash are removed by the next `open`.
//!
//! **The cap** counts entry bytes on disk. Every read that answers and every write is a use,
//! ticked by the injected [`LruClock`]; a write that passes the cap evicts least recently used
//! entries first. Entries found by `open` rank before any use of this process, oldest
//! modification first: a cache reopened after a restart forgets which entries were read, not
//! which were written. Pinned by `tests/disk.rs` and the `summary_store` contract suite.
//!
//! The single daemon owns its state directory (plan §5.2), which only its user can enter (plan
//! §5.3), so no other process writes here. Still, `open` follows no symbolic link: a root that is
//! one is refused; an `objects/` that is one, or a file, is replaced by a directory (the link
//! removed, never its target); the format marker's temporary file is opened with `create_new`
//! after removing whatever held its name; and the sweep removes whatever under `objects/` is not
//! an entry (a link, a directory, a name that is no key), each by its own name, without reading
//! through a link (`tests/disk.rs`). Within the process,
//! the table and every rename and removal are under one lock, and reads run outside it: a read
//! racing an eviction misses or answers from the file it had opened, and the entry stays evicted
//! (`tests/disk.rs`).

use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read as _, Write as _};
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::SystemTime;

use jfl_application::{LruClock, StoreStats};
use jfl_domain::{Digest, SyntaxKey};

use crate::codec::{FORMAT_VERSION, MAX_ENTRY_BYTES};

/// The marker file's name.
pub const FORMAT_FILE: &str = "FORMAT";
/// The entries' directory.
pub const OBJECTS: &str = "objects";
const TEMP_PREFIX: &str = ".tmp-";

/// The marker's text for this format version.
#[must_use]
pub fn format_marker() -> String {
    format!("jfl summary store {FORMAT_VERSION}\n")
}

/// Where an entry ranks for eviction: entries found at open first (by modification time), then
/// those used by this process (by tick).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Found(u64),
    Used(u64),
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    size: u64,
    rank: Rank,
}

#[derive(Debug, Default)]
struct Table {
    entries: HashMap<SyntaxKey, Entry>,
    order: BTreeSet<(Rank, SyntaxKey)>,
    stats: StoreStats,
}

impl Table {
    fn insert(&mut self, key: SyntaxKey, entry: Entry) {
        if let Some(old) = self.entries.insert(key, entry) {
            self.order.remove(&(old.rank, key));
            self.stats.bytes = self.stats.bytes.saturating_sub(old.size);
        }
        self.order.insert((entry.rank, key));
        self.stats.bytes = self.stats.bytes.saturating_add(entry.size);
        self.stats.entries = u64::try_from(self.entries.len()).unwrap_or(u64::MAX);
    }

    fn remove(&mut self, key: &SyntaxKey) -> bool {
        let Some(old) = self.entries.remove(key) else {
            return false;
        };
        self.order.remove(&(old.rank, *key));
        self.stats.bytes = self.stats.bytes.saturating_sub(old.size);
        self.stats.entries = u64::try_from(self.entries.len()).unwrap_or(u64::MAX);
        true
    }

    fn touch(&mut self, key: SyntaxKey, size: u64, tick: u64) {
        self.insert(
            key,
            Entry {
                size,
                rank: Rank::Used(tick),
            },
        );
    }
}

/// The directory of entries.
pub struct Blobs {
    objects: PathBuf,
    cap: u64,
    clock: Arc<dyn LruClock>,
    table: Mutex<Table>,
    temps: AtomicU64,
}

impl core::fmt::Debug for Blobs {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Blobs")
            .field("objects", &self.objects)
            .field("cap", &self.cap)
            .finish_non_exhaustive()
    }
}

/// Checks that `root` is a directory itself, not a link to one, creating it (and its parents) if
/// it is missing. The store's root is the caller's: a link there is refused, never followed.
fn real_dir(root: &Path) -> io::Result<()> {
    match fs::symlink_metadata(root) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            "the summary store's root is not a directory (a symbolic link is refused)",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => private_dir(root, true),
        Err(e) => Err(e),
    }
}

/// Creates `path` (and its missing parents when `recursive`) with mode 0700: the store's
/// directories are its owner's alone, wherever its root is placed (IDX-08's review).
///
/// Pinned by: `tests/disk.rs::should_keep_directories_private_when_the_store_creates_them`.
fn private_dir(path: &Path, recursive: bool) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt as _;
    fs::DirBuilder::new()
        .recursive(recursive)
        .mode(0o700)
        .create(path)
}

/// Makes `path`, inside the store, a directory of its own: a link or a file there is removed
/// (the link itself, never its target) and a directory created in its place.
fn replace_unless_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => return Ok(()),
        Ok(_) => fs::remove_file(path)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    private_dir(path, false)
}

fn remove_stray(path: &Path, is_dir: bool) -> io::Result<()> {
    if is_dir {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// What `open` found and removed: for the tests and the bench.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Opened {
    /// The directory was discarded: no marker, or another version's.
    pub discarded_format: bool,
    /// Temporary files and names that are no entry, removed.
    pub removed_strays: u64,
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => byte.checked_sub(b'0'),
        b'a'..=b'f' => byte.checked_sub(b'a').and_then(|v| v.checked_add(10)),
        _ => None,
    }
}

/// The key named by a fan-out directory and a file name, if they name one.
fn key_of(dir: &str, file: &str) -> Option<SyntaxKey> {
    if dir.len() != 2 || file.len() != 62 {
        return None;
    }
    let mut bytes = [0_u8; 32];
    let digits = dir.bytes().chain(file.bytes());
    let mut high = None;
    let mut out = bytes.iter_mut();
    for digit in digits {
        let value = hex_digit(digit)?;
        match high.take() {
            None => high = Some(value),
            Some(h) => *out.next()? = h.checked_mul(16)?.checked_add(value)?,
        }
    }
    Some(SyntaxKey::from_digest(Digest::from_bytes(bytes)))
}

/// Reads at most `limit` bytes of `file`; `None` when it holds more.
fn read_bounded(file: File, limit: u64, hint: u64) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::with_capacity(usize::try_from(hint.min(limit)).unwrap_or(0));
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).map_or(true, |len| len > limit) {
        return Ok(None);
    }
    Ok(Some(bytes))
}

impl Blobs {
    /// Opens the store under `root` (created if missing), discarding it if its format marker is
    /// missing or of another version, and scanning its entries for the cap.
    pub fn open(root: &Path, cap: u64, clock: Arc<dyn LruClock>) -> io::Result<(Self, Opened)> {
        real_dir(root)?;
        let objects = root.join(OBJECTS);
        replace_unless_dir(&objects)?;
        let marker = root.join(FORMAT_FILE);
        let mut opened = Opened::default();
        let expected = format_marker();
        let limit = u64::try_from(expected.len()).unwrap_or(u64::MAX);
        let current = match File::open(&marker).and_then(|f| read_bounded(f, limit, limit)) {
            Ok(bytes) => bytes.is_some_and(|b| b == expected.as_bytes()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => return Err(e),
        };
        if !current {
            opened.discarded_format = true;
            match fs::remove_dir_all(&objects) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            private_dir(&objects, true)?;
            let temp = root.join(format!("{TEMP_PREFIX}{FORMAT_FILE}"));
            // A stale temporary, or a link planted in its place, is removed (a link, not its
            // target); `create_new` then never opens through one.
            match fs::remove_file(&temp) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => {}
            }
            let mut out = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            out.write_all(expected.as_bytes())?;
            drop(out);
            fs::rename(&temp, &marker)?;
        }
        replace_unless_dir(&objects)?;
        let mut found = Vec::new();
        for dir in fs::read_dir(&objects)? {
            let dir = dir?;
            let is_dir = dir.file_type()?.is_dir();
            let dir_name = dir.file_name();
            let Some(dir_name) = dir_name.to_str().filter(|name| is_dir && name.len() == 2) else {
                remove_stray(&dir.path(), is_dir)?;
                opened.removed_strays = opened.removed_strays.saturating_add(1);
                continue;
            };
            for file in fs::read_dir(dir.path())? {
                let file = file?;
                let meta = file.metadata()?;
                let key = file
                    .file_name()
                    .to_str()
                    .filter(|_| meta.is_file())
                    .and_then(|name| key_of(dir_name, name));
                match key {
                    Some(key) if meta.len() <= MAX_ENTRY_BYTES => {
                        let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                        found.push((modified, key, meta.len()));
                    }
                    _ => {
                        remove_stray(&file.path(), meta.is_dir())?;
                        opened.removed_strays = opened.removed_strays.saturating_add(1);
                    }
                }
            }
        }
        found.sort_unstable();
        let mut table = Table::default();
        for (rank, (_, key, size)) in (0_u64..).zip(found) {
            table.insert(
                key,
                Entry {
                    size,
                    rank: Rank::Found(rank),
                },
            );
        }
        table.stats.cap = cap;
        let blobs = Self {
            objects,
            cap,
            clock,
            table: Mutex::new(table),
            temps: AtomicU64::new(0),
        };
        {
            let mut table = blobs.lock();
            blobs.evict(&mut table, None);
        }
        Ok((blobs, opened))
    }

    fn lock(&self) -> MutexGuard<'_, Table> {
        // A panic under the lock leaves the table consistent at worst with one entry's bytes
        // counted twice or not at all: a cache may keep serving.
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn dir_of(&self, key: &SyntaxKey) -> (PathBuf, String) {
        let hex = key.to_string();
        let (dir, file) = hex.split_at_checked(2).unwrap_or_default();
        (self.objects.join(dir), file.to_owned())
    }

    /// The path of `key`'s entry.
    #[must_use]
    pub fn path_of(&self, key: &SyntaxKey) -> PathBuf {
        let (dir, file) = self.dir_of(key);
        dir.join(file)
    }

    /// The bytes of `key`'s entry, bounded by [`MAX_ENTRY_BYTES`]; `None` for no entry or one too
    /// long (removed). Not a use until [`Blobs::used`] says the bytes decoded.
    pub fn read(&self, key: &SyntaxKey) -> Option<Vec<u8>> {
        match Self::read_path(&self.path_of(key)) {
            Ok(Some(bytes)) => Some(bytes),
            Ok(None) => {
                self.discard(key, None);
                None
            }
            Err(e) => {
                self.missed(key, &e);
                None
            }
        }
    }

    /// The bytes at `path`, bounded; `None` when longer than [`MAX_ENTRY_BYTES`].
    fn read_path(path: &Path) -> io::Result<Option<Vec<u8>>> {
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        read_bounded(file, MAX_ENTRY_BYTES, len)
    }

    /// A read of `key` failed with `error`: a miss. An entry gone from disk is forgotten, but only
    /// if it is still gone under the lock: a write of the same key renames its file in under the
    /// lock, so a reader whose open lost that race must not drop the live entry from the cap's
    /// table. Any other error (out of descriptors) leaves the entry counted, so the cap does not
    /// lose track of a file that is still there.
    ///
    /// Pinned by: `tests/disk.rs::should_keep_counting_an_entry_when_a_read_missed_its_write`.
    pub fn missed(&self, key: &SyntaxKey, error: &io::Error) {
        let mut table = self.lock();
        if error.kind() == io::ErrorKind::NotFound
            && matches!(fs::symlink_metadata(self.path_of(key)), Err(e) if e.kind() == io::ErrorKind::NotFound)
        {
            table.remove(key);
        }
        table.stats.misses = table.stats.misses.saturating_add(1);
    }

    /// Records a read of `key` that decoded: a hit, and a use if the entry is still held. An entry
    /// evicted while it was being read stays evicted: the reader's open file kept its bytes, the
    /// table must not count a file that is gone.
    pub fn used(&self, key: SyntaxKey) {
        let tick = self.clock.now();
        let mut table = self.lock();
        if let Some(entry) = table.entries.get(&key).copied() {
            table.touch(key, entry.size, tick);
        }
        table.stats.hits = table.stats.hits.saturating_add(1);
    }

    /// Removes `key`'s entry, found unreadable: a miss, counted as discarded. `torn` is what the
    /// reader found (`None`: longer than any read accepts). The entry is removed only if the file
    /// still holds just that, checked under the lock every write renames under: if a write has put
    /// a good entry there since (two readers saw the same torn bytes, one discarded it, a write
    /// replaced it), the good entry stays. Neither the inode nor the times can tell: the new file
    /// may get the freed inode back, within the same timestamp tick.
    ///
    /// Pinned by: `tests/disk.rs::should_keep_a_rewritten_entry_when_a_late_discard_names_the_torn_one`.
    pub fn discard(&self, key: &SyntaxKey, torn: Option<&[u8]>) {
        let mut table = self.lock();
        let path = self.path_of(key);
        match Self::read_path(&path) {
            Ok(now) if now.as_deref() == torn => {
                table.remove(key);
                let _ = fs::remove_file(&path);
                table.stats.discarded = table.stats.discarded.saturating_add(1);
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                table.remove(key);
            }
            Err(_) => {}
        }
        table.stats.misses = table.stats.misses.saturating_add(1);
    }

    /// Whether `key` has an entry, as far as the table knows; a use if it does.
    fn use_if_held(&self, key: SyntaxKey) -> bool {
        let tick = self.clock.now();
        let mut table = self.lock();
        let Some(entry) = table.entries.get(&key).copied() else {
            return false;
        };
        table.touch(key, entry.size, tick);
        true
    }

    /// Writes `entry` as `key`'s, atomically, then evicts until the cap holds. A key already held
    /// is only used: a key names one content. An entry larger than the cap or than
    /// [`MAX_ENTRY_BYTES`], which no read would accept, is not written (`refused`).
    pub fn write(&self, key: SyntaxKey, entry: &[u8]) -> io::Result<()> {
        if self.use_if_held(key) {
            return Ok(());
        }
        let size = u64::try_from(entry.len()).unwrap_or(u64::MAX);
        if size > self.cap || size > MAX_ENTRY_BYTES {
            let mut table = self.lock();
            table.stats.refused = table.stats.refused.saturating_add(1);
            return Ok(());
        }
        let (dir, file) = self.dir_of(&key);
        let target = dir.join(&file);
        let temp = match self.write_temp(&dir, &file, entry) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                private_dir(&dir, true)?;
                self.write_temp(&dir, &file, entry)?
            }
            other => other?,
        };
        let tick = self.clock.now();
        let mut table = self.lock();
        if let Err(e) = fs::rename(&temp, &target) {
            let _ = fs::remove_file(&temp);
            return Err(e);
        }
        table.touch(key, size, tick);
        self.evict(&mut table, Some(key));
        Ok(())
    }

    fn write_temp(&self, dir: &Path, file: &str, entry: &[u8]) -> io::Result<PathBuf> {
        loop {
            let n = self.temps.fetch_add(1, Ordering::Relaxed);
            let temp = dir.join(format!("{TEMP_PREFIX}{file}-{n}"));
            let opened = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp);
            let mut out = match opened {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                other => other?,
            };
            if let Err(e) = out.write_all(entry) {
                let _ = fs::remove_file(&temp);
                return Err(e);
            }
            return Ok(temp);
        }
    }

    /// Evicts least recently used entries, never `keep`, until the cap holds.
    fn evict(&self, table: &mut Table, keep: Option<SyntaxKey>) {
        while table.stats.bytes > self.cap {
            let Some(&(rank, key)) = table.order.iter().find(|(_, k)| Some(*k) != keep) else {
                break;
            };
            table.order.remove(&(rank, key));
            table.remove(&key);
            let _ = fs::remove_file(self.path_of(&key));
            table.stats.evicted = table.stats.evicted.saturating_add(1);
        }
    }

    /// What the store holds and did.
    pub fn stats(&self) -> StoreStats {
        self.lock().stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_read_back_the_key_when_its_path_names_it() {
        // given
        let key = SyntaxKey::from_digest(Digest::from_bytes(core::array::from_fn(|i| {
            u8::try_from(i).unwrap().wrapping_mul(7)
        })));
        let hex = key.to_string();
        // when
        let parsed = key_of(&hex[..2], &hex[2..]);
        // then
        assert_eq!(parsed, Some(key));
        assert_eq!(key_of("0g", &hex[2..]), None);
        assert_eq!(key_of(&hex[..2], &hex[3..]), None);
        assert_eq!(key_of("AB", &hex[2..]), None);
    }
}
