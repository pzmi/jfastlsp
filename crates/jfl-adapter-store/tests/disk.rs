//! The on-disk store's own promises (plan §5.5, §5.10, fnd §6), on real files: a torn, truncated,
//! flipped, misplaced or other-version entry is a miss and is removed, never a summary; a store of
//! another format version is discarded whole; a crash's temporary files are removed at open; the
//! cap evicts the least recently used entry on a fake clock, after a restart the oldest written,
//! and an entry evicted while it was being read stays evicted; entries are private to the owner;
//! `open` follows no symbolic link (a linked root is refused, a linked `objects/` replaced, a
//! linked marker temporary removed), and an entry no read would accept is never written.

#[cfg(test)]
mod support;

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use jfl_adapter_store::DiskSummaryStore;
use jfl_adapter_store::blobs::{Blobs, FORMAT_FILE, OBJECTS};
use jfl_adapter_store::codec::{HEADER_LEN, MAX_ENTRY_BYTES};
use jfl_application::{LruClock, SummaryStore};
use jfl_domain::FileSummary;
use jfl_testkit::contracts::summary_store::{Harness, corpus_summaries, key};
use jfl_testkit::exec::{InlinePool, block_on};
use jfl_testkit::fakes::summary_store::FakeClock;
use support::{DiskHarness, Scratch, open, truncate};

#[cfg(test)]
mod given {
    use super::*;

    /// The corpus summary with the most reference sites.
    pub(crate) fn summary() -> FileSummary {
        corpus_summaries()
            .unwrap()
            .into_iter()
            .max_by_key(|s| s.references().len())
            .unwrap()
    }

    pub(crate) fn clock() -> Arc<dyn LruClock> {
        Arc::new(FakeClock::at(1))
    }
}
use given::{clock, summary};

#[test]
fn should_miss_and_discard_when_an_entry_is_truncated_at_any_length() {
    // given
    let scratch = Scratch::new("truncated");
    let store = open(&scratch, u64::MAX, clock()).unwrap();
    let summary = summary();
    store.put_blocking(key(1), &summary).unwrap();
    let path = store.path_of(&key(1));
    let whole = fs::read(&path).unwrap();
    let cuts = [
        0,
        1,
        HEADER_LEN - 1,
        HEADER_LEN,
        whole.len() / 2,
        whole.len() - 1,
    ];
    for cut in cuts {
        fs::write(&path, &whole[..cut]).unwrap();
        // when
        let got = store.get_blocking(&key(1));
        let declarations = store.get_declarations_blocking(&key(1));
        // then
        assert_eq!(got, None, "cut at {cut}");
        assert_eq!(declarations, None, "cut at {cut}");
        assert!(!path.exists(), "cut at {cut}: the entry was not removed");
        store.put_blocking(key(1), &summary).unwrap();
        fs::write(&path, &whole).unwrap();
        assert_eq!(store.get_blocking(&key(1)), Some(summary.clone()));
    }
    assert_eq!(store.stats().discarded, 6);
}

#[test]
fn should_miss_when_an_entry_is_flipped_misplaced_or_of_another_version() {
    // given
    let scratch = Scratch::new("corrupt");
    let store = open(&scratch, u64::MAX, clock()).unwrap();
    let summary = summary();
    store.put_blocking(key(1), &summary).unwrap();
    let whole = fs::read(store.path_of(&key(1))).unwrap();
    let mut flipped = whole.clone();
    flipped[whole.len() / 2] ^= 0x10;
    let mut version = whole.clone();
    version[4] = 0;
    let mut grown = whole.clone();
    grown.push(0);
    let cases = [(2, flipped), (3, version), (4, whole), (5, grown)];
    for (n, bytes) in &cases {
        fs::create_dir_all(store.path_of(&key(*n)).parent().unwrap()).unwrap();
        fs::write(store.path_of(&key(*n)), bytes).unwrap();
    }
    // when
    let got: Vec<Option<FileSummary>> = cases
        .iter()
        .map(|(n, _)| store.get_blocking(&key(*n)))
        .collect();
    // then
    assert_eq!(got, vec![None; 4], "key 4 holds key 1's entry");
    assert_eq!(store.get_blocking(&key(1)), Some(summary));
}

#[test]
fn should_discard_the_store_when_its_format_marker_is_of_another_version() {
    // given
    let scratch = Scratch::new("format");
    let store = open(&scratch, u64::MAX, clock()).unwrap();
    store.put_blocking(key(1), &summary()).unwrap();
    drop(store);
    fs::write(scratch.root().join(FORMAT_FILE), "jfl summary store 0\n").unwrap();
    // when
    let (store, opened) =
        DiskSummaryStore::open(&scratch.root(), u64::MAX, clock(), InlinePool).unwrap();
    // then
    assert!(opened.discarded_format);
    assert_eq!(store.stats().entries, 0);
    assert_eq!(store.get_blocking(&key(1)), None);
    let (_, reopened) =
        DiskSummaryStore::open(&scratch.root(), u64::MAX, clock(), InlinePool).unwrap();
    assert!(!reopened.discarded_format);
}

#[test]
fn should_remove_a_crash_s_temporary_files_and_strays_when_opened() {
    // given
    let scratch = Scratch::new("strays");
    let store = open(&scratch, u64::MAX, clock()).unwrap();
    store.put_blocking(key(1), &summary()).unwrap();
    let dir = store.path_of(&key(1)).parent().unwrap().to_owned();
    fs::write(dir.join(".tmp-abc-0"), b"torn").unwrap();
    fs::write(dir.join("not-a-key"), b"x").unwrap();
    fs::write(scratch.root().join(OBJECTS).join("stray"), b"x").unwrap();
    drop(store);
    // when
    let (store, opened) =
        DiskSummaryStore::open(&scratch.root(), u64::MAX, clock(), InlinePool).unwrap();
    // then
    assert_eq!(opened.removed_strays, 3);
    assert!(!dir.join(".tmp-abc-0").exists());
    assert_eq!(store.stats().entries, 1);
    assert_eq!(store.get_blocking(&key(1)), Some(summary()));
}

#[test]
fn should_evict_the_least_recently_used_entry_when_a_put_passes_the_cap() {
    // given
    let scratch = Scratch::new("lru");
    let summary = summary();
    let size = {
        let probe = Scratch::new("lru-probe");
        let store = open(&probe, u64::MAX, clock()).unwrap();
        store.put_blocking(key(0), &summary).unwrap();
        store.stats().bytes
    };
    let clock = Arc::new(FakeClock::at(0));
    let store = open(&scratch, size * 3, Arc::clone(&clock) as Arc<dyn LruClock>).unwrap();
    for n in 1..=3 {
        clock.set(u64::from(n) * 10);
        store.put_blocking(key(n), &summary).unwrap();
    }
    clock.set(40);
    assert!(store.get_declarations_blocking(&key(1)).is_some());
    clock.set(50);
    assert!(store.get_blocking(&key(2)).is_some());
    // when
    clock.set(60);
    store.put_blocking(key(4), &summary).unwrap();
    // then
    assert!(
        !store.path_of(&key(3)).exists(),
        "key 3 was the least recently used"
    );
    let held: Vec<u32> = [1, 2, 4]
        .into_iter()
        .filter(|n| store.path_of(&key(*n)).exists())
        .collect();
    assert_eq!(held, [1, 2, 4]);
    let stats = store.stats();
    assert_eq!(
        (stats.evicted, stats.entries, stats.bytes),
        (1, 3, size * 3)
    );
}

#[test]
fn should_evict_the_oldest_written_entry_when_reopened_over_a_smaller_cap() {
    // given
    let scratch = Scratch::new("restart");
    let summary = summary();
    let store = open(&scratch, u64::MAX, clock()).unwrap();
    for n in 1..=3 {
        store.put_blocking(key(n), &summary).unwrap();
    }
    let size = store.stats().bytes / 3;
    for (n, secs) in [(1, 300), (2, 100), (3, 200)] {
        let file = fs::File::options()
            .write(true)
            .open(store.path_of(&key(n)))
            .unwrap();
        file.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }
    drop(store);
    // when
    let store = open(&scratch, size * 2, clock()).unwrap();
    // then
    let held: Vec<u32> = [1, 2, 3]
        .into_iter()
        .filter(|n| store.path_of(&key(*n)).exists())
        .collect();
    assert_eq!(held, [1, 3]);
    assert_eq!(store.stats().evicted, 1);
}

#[test]
fn should_keep_entries_private_when_written() {
    // given
    let scratch = Scratch::new("modes");
    let store = open(&scratch, u64::MAX, clock()).unwrap();
    // when
    block_on(store.put(key(1), Arc::new(summary()))).unwrap();
    // then
    let mode = fs::metadata(store.path_of(&key(1)))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "mode {mode:o}");
}

#[test]
fn should_miss_after_a_truncation_through_the_harness_when_the_port_reads() {
    // given
    let mut harness = DiskHarness(Scratch::new("port"));
    let store = harness.open(u64::MAX, clock()).unwrap();
    block_on(store.put(key(1), Arc::new(summary()))).unwrap();
    truncate(&store, &key(1)).unwrap();
    // when
    let got = block_on(store.get(key(1))).unwrap();
    // then
    assert_eq!(got, None);
    harness.wipe().unwrap();
}

#[test]
fn should_keep_an_entry_evicted_when_it_was_evicted_while_being_read() {
    // given
    let scratch = Scratch::new("evicted-read");
    let entry = jfl_adapter_store::codec::encode(&key(1), &summary()).unwrap();
    let size = u64::try_from(entry.len()).unwrap();
    let (blobs, _) = Blobs::open(&scratch.root(), size * 2, clock()).unwrap();
    blobs.write(key(1), &entry).unwrap();
    let read = blobs.read(&key(1));
    blobs.write(key(2), &entry).unwrap();
    blobs.write(key(3), &entry).unwrap();
    // when
    blobs.used(key(1));
    // then
    assert!(read.is_some());
    let stats = blobs.stats();
    assert_eq!(
        (stats.entries, stats.bytes, stats.evicted),
        (2, size * 2, 1)
    );
    assert!(!blobs.path_of(&key(1)).exists());
}

#[test]
fn should_replace_a_linked_objects_directory_when_opened_without_touching_its_target() {
    // given
    let scratch = Scratch::new("linked-objects");
    let outside = scratch.root().with_file_name("outside");
    fs::create_dir_all(outside.join("not-an-entry-dir")).unwrap();
    fs::write(outside.join("not-a-key"), b"keep me").unwrap();
    fs::create_dir_all(scratch.root()).unwrap();
    std::os::unix::fs::symlink(&outside, scratch.root().join(OBJECTS)).unwrap();
    // when
    let store = open(&scratch, u64::MAX, clock()).unwrap();
    // then
    assert_eq!(fs::read(outside.join("not-a-key")).unwrap(), b"keep me");
    assert!(outside.join("not-an-entry-dir").is_dir());
    let objects = fs::symlink_metadata(scratch.root().join(OBJECTS)).unwrap();
    assert!(objects.is_dir(), "objects/ is still a link");
    store.put_blocking(key(1), &summary()).unwrap();
    assert_eq!(store.get_blocking(&key(1)), Some(summary()));
    assert!(fs::read_dir(&outside).unwrap().count() == 2);
}

#[test]
fn should_leave_a_linked_temporary_marker_s_target_whole_when_the_marker_is_written() {
    // given
    let scratch = Scratch::new("linked-marker");
    let outside = scratch.root().with_file_name("precious");
    fs::write(&outside, b"do not truncate").unwrap();
    fs::create_dir_all(scratch.root()).unwrap();
    std::os::unix::fs::symlink(&outside, scratch.root().join(format!(".tmp-{FORMAT_FILE}")))
        .unwrap();
    // when
    let (_, opened) =
        DiskSummaryStore::open(&scratch.root(), u64::MAX, clock(), InlinePool).unwrap();
    // then
    assert!(opened.discarded_format);
    assert_eq!(fs::read(&outside).unwrap(), b"do not truncate");
    let marker = fs::read_to_string(scratch.root().join(FORMAT_FILE)).unwrap();
    assert!(marker.starts_with("jfl summary store "), "{marker}");
    let mode = fs::metadata(scratch.root().join(FORMAT_FILE))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "mode {mode:o}");
}

#[test]
fn should_refuse_a_root_when_it_is_a_symbolic_link() {
    // given
    let scratch = Scratch::new("linked-root");
    let real = scratch.root().with_file_name("real");
    fs::create_dir_all(&real).unwrap();
    std::os::unix::fs::symlink(&real, scratch.root()).unwrap();
    // when
    let opened = DiskSummaryStore::open(&scratch.root(), u64::MAX, clock(), InlinePool);
    // then
    assert_eq!(
        opened.map(|_| ()).map_err(|e| e.kind()),
        Err(std::io::ErrorKind::NotADirectory)
    );
    assert_eq!(fs::read_dir(&real).unwrap().count(), 0);
}

#[test]
fn should_refuse_an_entry_when_it_is_larger_than_any_read_accepts() {
    // given
    let scratch = Scratch::new("too-large");
    let (blobs, _) = Blobs::open(&scratch.root(), u64::MAX, clock()).unwrap();
    let entry = vec![0_u8; usize::try_from(MAX_ENTRY_BYTES).unwrap() + 1];
    // when
    blobs.write(key(1), &entry).unwrap();
    // then
    let stats = blobs.stats();
    assert_eq!((stats.refused, stats.entries), (1, 0));
    assert!(!blobs.path_of(&key(1)).exists());
}
