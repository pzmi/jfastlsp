//! The walk of a worktree, in process (ver §10, doc/bench/README.md's in-process convention):
//! `list` with paths only and with a stat per file, on 1 and 4 walker threads, warm and (asked for,
//! as root) cold; and the start of a watch (on Linux one inotify watch per accepted directory).
//! `just bench-fs SIZE [cold]`. Reports, never gates.
//!
//! Input: when `just bench-fs SIZE COLD` runs it, stdin holds `cold` or `warm`, a newline, the
//! corpus checkout's path, a newline, and `git ls-files -z -- '*.java'` of that checkout. Before timing anything the benchmark lists
//! the checkout and compares the listing with Git's: a tracked `.java` file the listing misses,
//! or a listed file Git does not track, exits 1 and prints both sets (the corpus's paths go to the
//! terminal only, never into a report). Without stdin it times a synthetic tree of 4,000 files in
//! cargo's scratch directory. Cold runs, only when asked for (`just bench-fs SIZE cold`: they disturb
//! everything else on the machine), drop the page cache (`/proc/sys/vm/drop_caches`, Linux, root)
//! before each sample; elsewhere they are skipped with a note.

use std::collections::BTreeSet;
use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use divan::Bencher;
use divan::counter::ItemsCount;
use jfl_adapter_fs::{FsConfig, FsSourceTree};
use jfl_application::source_tree::ListMode;
use jfl_domain::FileUri;
use jfl_testkit::exec::InlinePool;

struct Tree {
    root: FileUri,
    files: usize,
    cold: bool,
}

static TREE: OnceLock<Tree> = OnceLock::new();

fn adapter(walk_threads: usize) -> FsSourceTree<InlinePool> {
    FsSourceTree::new(
        InlinePool,
        FsConfig {
            walk_threads,
            ..FsConfig::default()
        },
    )
}

fn uri(path: &Path) -> FileUri {
    let Some(text) = path.to_str() else {
        eprintln!("bench_fs_root_not_utf8");
        std::process::exit(2);
    };
    FileUri::from_path(text).unwrap_or_else(|err| {
        eprintln!("bench_fs_root_refused: {err}");
        std::process::exit(2)
    })
}

/// 100 packages of 40 small files under cargo's scratch directory, made once.
fn synthetic() -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("bench-fs-synthetic");
    if !root.join("done").exists() {
        let _ = fs::remove_dir_all(&root);
        for p in 0..100 {
            let dir = root.join(format!("src/main/java/org/example/p{p}"));
            fs::create_dir_all(&dir).unwrap_or_else(|err| {
                eprintln!("bench_fs_synthetic_failed: {err}");
                std::process::exit(2)
            });
            for f in 0..40 {
                let _ = fs::write(dir.join(format!("C{f}.java")), b"class C {}\n");
            }
        }
        let _ = fs::write(root.join("done"), b"");
    }
    root
}

/// The corpus checkout from stdin, checked against Git's own list, or the synthetic tree.
fn load() -> Tree {
    let mut stdin = std::io::stdin();
    let mut bytes = Vec::new();
    if !stdin.is_terminal()
        && let Err(err) = stdin.read_to_end(&mut bytes)
    {
        eprintln!("bench_fs_stdin_failed: {err}");
        std::process::exit(2);
    }
    let (cold, bytes) = match bytes.iter().position(|b| *b == b'\n') {
        Some(at) if matches!(bytes.get(..at), Some(b"cold" | b"warm")) => (
            bytes.get(..at) == Some(b"cold"),
            bytes.get(at.saturating_add(1)..).unwrap_or_default(),
        ),
        _ => (false, bytes.as_slice()),
    };
    let Some((first, tracked)) = bytes
        .iter()
        .position(|b| *b == b'\n')
        .map(|at| bytes.split_at(at))
    else {
        let root = uri(&synthetic());
        let files = list_count(&root);
        println!("bench_fs synthetic tree: {files} files");
        return Tree {
            root,
            files,
            cold: false,
        };
    };
    let root = uri(Path::new(&*String::from_utf8_lossy(first)));
    let tracked: BTreeSet<String> = tracked
        .get(1..)
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect();
    let listing = adapter(0)
        .list_blocking(&root, ListMode::Paths)
        .unwrap_or_else(|err| {
            eprintln!("bench_fs_list_failed: {err}");
            std::process::exit(2)
        });
    let listed: BTreeSet<String> = listing
        .files
        .iter()
        .map(|f| f.path.as_str().to_owned())
        .collect();
    let missing: Vec<&String> = tracked.difference(&listed).collect();
    let extra: Vec<&String> = listed.difference(&tracked).collect();
    println!(
        "bench_fs corpus check: {} listed, {} tracked by git, {} skipped, {} gaps",
        listed.len(),
        tracked.len(),
        listing.skipped.len(),
        listing.unlisted_count
    );
    if !missing.is_empty() || !extra.is_empty() || !listing.is_complete() {
        eprintln!("bench_fs_corpus_mismatch: missing {missing:?}, extra {extra:?}");
        eprintln!("skipped {:?}, gaps {:?}", listing.skipped, listing.unlisted);
        std::process::exit(1);
    }
    Tree {
        root,
        files: listed.len(),
        cold,
    }
}

fn list_count(root: &FileUri) -> usize {
    adapter(0)
        .list_blocking(root, ListMode::Paths)
        .map_or(0, |l| l.files.len())
}

fn tree() -> &'static Tree {
    TREE.get_or_init(load)
}

/// Drops the page cache; `false` where that is not allowed.
fn drop_caches() -> bool {
    fs::write("/proc/sys/vm/drop_caches", b"3\n").is_ok()
}

fn main() {
    let tree = tree();
    if !tree.cold {
        println!("bench_fs cold runs skipped: not asked for (`just bench-fs SIZE cold`)");
    } else if !drop_caches() {
        println!("bench_fs cold runs skipped: the page cache cannot be dropped here (Linux, root)");
    }
    let root = Path::new(tree.root.path());
    let (watched, _) = jfl_adapter_fs::walk::watched_dirs(root, root);
    println!(
        "bench_fs root: {} files, {} watched directories",
        tree.files,
        watched.len()
    );
    divan::main();
}

fn bench_list(bencher: Bencher, threads: usize, mode: ListMode) {
    let tree = tree();
    let adapter = adapter(threads);
    bencher.counter(ItemsCount::new(tree.files)).bench(|| {
        adapter
            .list_blocking(&tree.root, mode)
            .map(|l| l.files.len())
    });
}

fn bench_cold(bencher: Bencher, threads: usize, mode: ListMode) {
    let tree = tree();
    let adapter = adapter(threads);
    if !tree.cold || !drop_caches() {
        return;
    }
    bencher
        .counter(ItemsCount::new(tree.files))
        .with_inputs(drop_caches)
        .bench_local_values(|_| {
            adapter
                .list_blocking(&tree.root, mode)
                .map(|l| l.files.len())
        });
}

#[divan::bench(args = [1, 4])]
fn list_paths_warm(bencher: Bencher, threads: usize) {
    bench_list(bencher, threads, ListMode::Paths);
}

#[divan::bench(args = [1, 4])]
fn list_paths_and_stat_warm(bencher: Bencher, threads: usize) {
    bench_list(bencher, threads, ListMode::PathsAndStat);
}

#[divan::bench(args = [4], sample_count = 3, sample_size = 1)]
fn list_paths_cold(bencher: Bencher, threads: usize) {
    bench_cold(bencher, threads, ListMode::Paths);
}

#[divan::bench(args = [4], sample_count = 3, sample_size = 1)]
fn list_paths_and_stat_cold(bencher: Bencher, threads: usize) {
    bench_cold(bencher, threads, ListMode::PathsAndStat);
}

/// A watch's start (on Linux: the walk of the directories and one inotify watch for each) and its
/// stop.
#[divan::bench(sample_count = 5, sample_size = 1)]
fn watch_start(bencher: Bencher) {
    let tree = tree();
    let adapter = adapter(0);
    bencher.bench(|| adapter.watch_blocking(&tree.root).is_ok());
}
