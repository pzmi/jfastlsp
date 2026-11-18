//! `SummaryStore`, `LibraryStore` and `ProjectModelCache`: the on-disk caches. Role: adapter.
//!
//! - [`DiskSummaryStore`] (IDX-08): the `SummaryStore` port over a content-addressed directory of
//!   postcard entries keyed by `SyntaxKey` ([`blobs`], [`codec`]), with a least-recently-used cap
//!   of bytes, and the large tier's name → files index ([`postings`]); the storage tier of DEC-03
//!   (plan §5.5, ADR 0025). Every blocking call runs on the injected `BlockingPool` (fnd §4.4).
//! - [`Blake3`]: the `KeyHasher` the cache keys are derived with (plan §5.4, ADR 0025).
//!
//! The crates of ADR 0025 that are used: `blake3` (keys and entry digests) and `postcard`
//! (entries). `rusqlite` is not: DEC-03's tier holds no manifest or baseline (plan §5.5's second
//! row), the postings fit in memory, and a flat directory needs no database; it arrives with a
//! tier that needs one. Nor is `fst`: the postings look names up exactly and are rebuilt at every
//! open, where a sorted table builds faster ([`postings`] has the measurement); `fst` stays ADR
//! 0025's choice for `workspace/symbol`'s prefix and fuzzy search (IDX-10).

pub mod blobs;
pub mod codec;
pub mod postings;

use std::io;
use std::path::Path;
use std::sync::Arc;

use jfl_application::{
    BlockingPool, Declarations, LruClock, StoreStats, SummaryStore, SummaryStoreError,
};
use jfl_domain::{Digest, FileSummary, KeyHasher, SyntaxKey};

pub use blobs::Opened;
pub use postings::SortedPostings;

/// The default cap: 1 GiB of entries. The pinned large corpus (elasticsearch) encodes to about a
/// tenth of that (`just bench-store`, projected from medium), so several worktrees of it at
/// different commits fit; REL-02 sets the number from measurements (plan §5.9).
pub const DEFAULT_CAP: u64 = 1024 * 1024 * 1024;

/// A blake3 [`KeyHasher`].
#[derive(Debug, Clone, Default)]
pub struct Blake3(blake3::Hasher);

impl Blake3 {
    /// A fresh hasher.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The blake3 digest of `bytes`: a file's content digest for its `SyntaxKey`.
    #[must_use]
    pub fn digest(bytes: &[u8]) -> Digest {
        Digest::from_bytes(*blake3::hash(bytes).as_bytes())
    }
}

impl KeyHasher for Blake3 {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }

    fn finish(self) -> Digest {
        Digest::from_bytes(*self.0.finalize().as_bytes())
    }
}

/// The on-disk [`SummaryStore`]: see the crate documentation, [`blobs`] and [`codec`].
#[derive(Debug)]
pub struct DiskSummaryStore<P> {
    blobs: Arc<blobs::Blobs>,
    pool: P,
}

impl<P: Clone> Clone for DiskSummaryStore<P> {
    fn clone(&self) -> Self {
        Self {
            blobs: Arc::clone(&self.blobs),
            pool: self.pool.clone(),
        }
    }
}

impl<P> DiskSummaryStore<P> {
    /// Opens the store under `root` with a cap of `cap` bytes, ticking uses on `clock`: a
    /// blocking call, made once when the daemon starts. A store of another format version is
    /// discarded (fnd §6).
    pub fn open(
        root: &Path,
        cap: u64,
        clock: Arc<dyn LruClock>,
        pool: P,
    ) -> io::Result<(Self, Opened)> {
        let (blobs, opened) = blobs::Blobs::open(root, cap, clock)?;
        Ok((
            Self {
                blobs: Arc::new(blobs),
                pool,
            },
            opened,
        ))
    }

    /// Where `key`'s entry lives: for the tests that damage one.
    #[must_use]
    pub fn path_of(&self, key: &SyntaxKey) -> std::path::PathBuf {
        self.blobs.path_of(key)
    }

    /// [`SummaryStore::get`], on the calling thread.
    #[must_use]
    pub fn get_blocking(&self, key: &SyntaxKey) -> Option<FileSummary> {
        read(&self.blobs, key, codec::decode)
    }

    /// [`SummaryStore::get_declarations`], on the calling thread.
    #[must_use]
    pub fn get_declarations_blocking(&self, key: &SyntaxKey) -> Option<Declarations> {
        read(&self.blobs, key, codec::decode_declarations)
    }

    /// [`SummaryStore::put`], on the calling thread.
    pub fn put_blocking(
        &self,
        key: SyntaxKey,
        summary: &FileSummary,
    ) -> Result<(), SummaryStoreError> {
        put(&self.blobs, key, summary)
    }
}

fn read<T>(
    blobs: &blobs::Blobs,
    key: &SyntaxKey,
    decode: fn(&SyntaxKey, &[u8]) -> Result<T, codec::Corrupt>,
) -> Option<T> {
    let entry = blobs.read(key)?;
    match decode(key, &entry) {
        Ok(value) => {
            blobs.used(*key);
            Some(value)
        }
        Err(_) => {
            blobs.discard(key);
            None
        }
    }
}

fn put(
    blobs: &blobs::Blobs,
    key: SyntaxKey,
    summary: &FileSummary,
) -> Result<(), SummaryStoreError> {
    let entry = codec::encode(&key, summary)
        .map_err(|_| SummaryStoreError::Write(io::ErrorKind::FileTooLarge))?;
    blobs
        .write(key, &entry)
        .map_err(|e| SummaryStoreError::Write(e.kind()))
}

impl<P: BlockingPool> SummaryStore for DiskSummaryStore<P> {
    type Postings = SortedPostings;

    fn get(
        &self,
        key: SyntaxKey,
    ) -> impl Future<Output = Result<Option<FileSummary>, SummaryStoreError>> + Send {
        let blobs = Arc::clone(&self.blobs);
        let job = self.pool.run(move || read(&blobs, &key, codec::decode));
        async move { job.await.map_err(|_| SummaryStoreError::Interrupted) }
    }

    fn get_declarations(
        &self,
        key: SyntaxKey,
    ) -> impl Future<Output = Result<Option<Declarations>, SummaryStoreError>> + Send {
        let blobs = Arc::clone(&self.blobs);
        let job = self
            .pool
            .run(move || read(&blobs, &key, codec::decode_declarations));
        async move { job.await.map_err(|_| SummaryStoreError::Interrupted) }
    }

    fn put(
        &self,
        key: SyntaxKey,
        summary: Arc<FileSummary>,
    ) -> impl Future<Output = Result<(), SummaryStoreError>> + Send {
        let blobs = Arc::clone(&self.blobs);
        let job = self.pool.run(move || put(&blobs, key, &summary));
        async move {
            job.await
                .map_err(|_| SummaryStoreError::Interrupted)
                .and_then(|written| written)
        }
    }

    fn stats(&self) -> StoreStats {
        self.blobs.stats()
    }

    fn postings<'a, I>(&self, files: I) -> SortedPostings
    where
        I: IntoIterator<Item = (u32, &'a Declarations)>,
    {
        SortedPostings::build(files)
    }
}

#[cfg(test)]
mod tests {
    use jfl_domain::SyntaxInputs;

    use super::*;

    #[test]
    fn should_derive_the_reference_digest_when_hashing_with_blake3() {
        // given
        let inputs = SyntaxInputs {
            file: Blake3::digest(b"class A {}"),
            grammar: "tree-sitter-java-orchard",
            grammar_version: "0.5.25",
            summariser_version: 1,
        };
        // when
        let key = SyntaxKey::derive(&inputs, Blake3::new());
        let again = SyntaxKey::derive(&inputs, Blake3::new());
        // then
        assert_eq!(key, again);
        assert_eq!(
            Blake3::digest(b"").to_string(),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }
}
