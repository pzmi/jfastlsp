//! The git adapter's cost per call, in process (ver §10, doc/bench/README.md's in-process
//! convention): the hot path (`resolve`, cold and cached, and the stat set alone) and each `git`
//! command of the reconcile path (`rev-parse`, `diff --name-only` between two pinned trees,
//! `status -z`, `ls-files -z`), each spawned through `jfl-process` with the adapter's hardening.
//! `just bench-git SIZE`. Reports, never gates.
//!
//! Input: when `just bench-git SIZE` runs it, stdin holds the corpus checkout's path, a newline,
//! and `git ls-files -z -- '*.java'` of that checkout. Before timing anything the benchmark checks
//! the adapter against it: `tracked_java` must equal Git's list, a tracked file must resolve to the
//! checkout, and two snapshots with a status between them must be equal; a difference exits 1.
//! Without stdin it times a synthetic repository of 2,000 files with an agent worktree, made in
//! cargo's scratch directory.
//!
//! The two pinned trees: in a shared, unchecked-out clone of the checkout (in cargo's scratch
//! directory, so the corpus is never written), `HEAD`'s tree and a tree with the first 100 tracked
//! `.java` files replaced by one blob, the shape of a branch switch touching 100 files.

use std::collections::BTreeSet;
use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use divan::Bencher;
use jfl_adapter_git::{DEFAULT_MAX_OUTPUT, GitConfig, GitRepoProbe};
use jfl_application::{GitStep, Worktree};
use jfl_domain::FileUri;
use jfl_testkit::exec::InlinePool;
use jfl_testkit::fixtures::git::{Repo, git};

struct Subject {
    worktree: Worktree,
    /// A tracked file deep in the tree, for `resolve`.
    file: FileUri,
    /// The shared clone holding both pinned trees.
    clone: PathBuf,
    trees: (String, String),
}

static SUBJECT: OnceLock<Subject> = OnceLock::new();

const DEADLINE: Duration = Duration::from_secs(60);

fn fail(event: &str, detail: impl std::fmt::Display) -> ! {
    eprintln!("{event}: {detail}");
    std::process::exit(2)
}

fn probe() -> GitRepoProbe<InlinePool> {
    GitRepoProbe::new(InlinePool, GitConfig::default())
}

fn uri(path: &Path) -> FileUri {
    let text = path
        .to_str()
        .unwrap_or_else(|| fail("bench_git_not_utf8", path.display()));
    FileUri::from_path(text).unwrap_or_else(|e| fail("bench_git_root_refused", e))
}

fn scratch() -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("bench-git")
}

/// 100 packages of 20 files with an agent worktree, made once.
fn synthetic() -> (PathBuf, BTreeSet<String>) {
    let root = scratch().join("synthetic");
    let mut files = BTreeSet::new();
    for p in 0..100 {
        for f in 0..20 {
            files.insert(format!("src/main/java/org/example/p{p}/C{f}.java"));
        }
    }
    if !root.join(".git").exists() {
        let _ = fs::remove_dir_all(&root);
        let entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|f| (f.as_str(), &b"class C {}\n"[..]))
            .collect();
        let repo = Repo::init(&root, &entries).unwrap_or_else(|e| fail("bench_git_init_failed", e));
        repo.add_agent_worktree("agent")
            .unwrap_or_else(|e| fail("bench_git_worktree_failed", e));
    }
    (root, files)
}

/// The corpus checkout and Git's own list from stdin, or the synthetic repository.
fn input() -> (PathBuf, BTreeSet<String>) {
    let mut stdin = std::io::stdin();
    let mut bytes = Vec::new();
    if !stdin.is_terminal()
        && let Err(err) = stdin.read_to_end(&mut bytes)
    {
        fail("bench_git_stdin_failed", err);
    }
    let Some(at) = bytes.iter().position(|b| *b == b'\n') else {
        return synthetic();
    };
    let (first, rest) = bytes.split_at(at);
    let tracked = rest
        .get(1..)
        .unwrap_or_default()
        .split(|b| *b == 0)
        .filter(|name| !name.is_empty())
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .collect();
    (PathBuf::from(&*String::from_utf8_lossy(first)), tracked)
}

/// The shared clone and its two trees.
fn pinned_trees(root: &Path, tracked: &BTreeSet<String>) -> (PathBuf, (String, String)) {
    let clone = scratch().join("clone");
    let _ = fs::remove_dir_all(&clone);
    fs::create_dir_all(scratch()).unwrap_or_else(|e| fail("bench_git_scratch_failed", e));
    let root_text = root
        .to_str()
        .unwrap_or_else(|| fail("bench_git_not_utf8", root.display()));
    git(
        &scratch(),
        &[
            "clone",
            "--quiet",
            "--shared",
            "--no-checkout",
            root_text,
            "clone",
        ],
    )
    .unwrap_or_else(|e| fail("bench_git_clone_failed", e));
    let run = |args: &[&str]| {
        git(&clone, args)
            .map(|out| out.trim().to_owned())
            .unwrap_or_else(|e| fail("bench_git_tree_failed", e))
    };
    let head = run(&["rev-parse", "HEAD^{tree}"]);
    run(&["read-tree", "HEAD"]);
    let blob_file = scratch().join("blob");
    fs::write(&blob_file, b"class Changed {}\n")
        .unwrap_or_else(|e| fail("bench_git_blob_failed", e));
    let blob = run(&["hash-object", "-w", blob_file.to_str().unwrap_or_default()]);
    let infos: Vec<String> = tracked
        .iter()
        .take(100)
        .map(|path| format!("100644,{blob},{path}"))
        .collect();
    let mut args = vec!["update-index"];
    for info in &infos {
        args.extend(["--cacheinfo", info.as_str()]);
    }
    run(&args);
    let changed = run(&["write-tree"]);
    (clone, (head, changed))
}

fn load() -> Subject {
    let (root, tracked) = input();
    let root = fs::canonicalize(&root).unwrap_or_else(|e| fail("bench_git_root_missing", e));
    let probe = probe();
    let file = tracked
        .iter()
        .max_by_key(|p| p.matches('/').count())
        .map(|p| uri(&root.join(p)))
        .unwrap_or_else(|| fail("bench_git_no_files", "nothing tracked"));
    let worktree = probe
        .resolve_blocking(&file)
        .unwrap_or_else(|e| fail("bench_git_resolve_failed", e))
        .unwrap_or_else(|| fail("bench_git_resolve_failed", "no worktree"));
    if worktree.root != uri(&root) {
        fail("bench_git_wrong_root", &worktree.root);
    }
    let listed: BTreeSet<String> = probe
        .tracked_java_blocking(&worktree)
        .unwrap_or_else(|e| fail("bench_git_tracked_failed", e))
        .files
        .iter()
        .map(|p| p.as_str().to_owned())
        .collect();
    if listed != tracked {
        eprintln!(
            "bench_git_tracked_mismatch: {} listed, {} from git",
            listed.len(),
            tracked.len()
        );
        std::process::exit(1);
    }
    let before = probe
        .snapshot_blocking(&worktree)
        .unwrap_or_else(|e| fail("bench_git_snapshot_failed", e));
    let changed = probe
        .changed_since_blocking(&worktree, None)
        .unwrap_or_else(|e| fail("bench_git_changed_failed", e));
    let after = probe
        .snapshot_blocking(&worktree)
        .unwrap_or_else(|e| fail("bench_git_snapshot_failed", e));
    if before != after {
        eprintln!("bench_git_snapshot_moved");
        std::process::exit(1);
    }
    let (clone, trees) = pinned_trees(&root, &tracked);
    println!(
        "bench_git {} tracked .java files; status: {} working, {} untracked, {} unrepresentable; snapshot stat'ed {} files",
        tracked.len(),
        changed.working.len(),
        changed.untracked.len(),
        changed.unrepresentable,
        before.files.len()
    );
    Subject {
        worktree,
        file,
        clone,
        trees,
    }
}

fn subject() -> &'static Subject {
    SUBJECT.get_or_init(load)
}

fn main() {
    subject();
    divan::main();
}

#[divan::bench(sample_count = 200)]
fn resolve_cold(bencher: Bencher) {
    let s = subject();
    bencher.with_inputs(probe).bench_local_values(|p| {
        p.resolve_blocking(&s.file)
            .unwrap_or_else(|e| fail("resolve", e))
    });
}

#[divan::bench(sample_count = 200)]
fn resolve_cached(bencher: Bencher) {
    let s = subject();
    let p = probe();
    bencher.bench_local(|| {
        p.resolve_blocking(&s.file)
            .unwrap_or_else(|e| fail("resolve", e))
    });
}

#[divan::bench(sample_count = 200)]
fn stat_set(bencher: Bencher) {
    let s = subject();
    let p = probe();
    bencher.bench_local(|| {
        p.snapshot_blocking(&s.worktree)
            .unwrap_or_else(|e| fail("snapshot", e))
    });
}

#[divan::bench(sample_count = 100)]
fn rev_parse_head_tree(bencher: Bencher) {
    let s = subject();
    let p = probe();
    bencher.bench_local(|| {
        p.head_tree_blocking(&s.worktree)
            .unwrap_or_else(|e| fail("rev-parse", e))
    });
}

#[divan::bench(sample_count = 100)]
fn diff_name_only_100_files(bencher: Bencher) {
    let s = subject();
    let p = probe();
    let args = [
        "diff",
        "--name-only",
        "-z",
        "--no-renames",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-relative",
        "--end-of-options",
        s.trees.0.as_str(),
        s.trees.1.as_str(),
    ];
    bencher.bench_local(|| {
        p.git()
            .run(
                GitStep::Diff,
                &s.clone,
                &args,
                DEFAULT_MAX_OUTPUT,
                DEADLINE,
                &[],
            )
            .unwrap_or_else(|e| fail("diff", e))
    });
}

#[divan::bench(sample_count = 50)]
fn status_z(bencher: Bencher) {
    let s = subject();
    let p = probe();
    let args = [
        "status",
        "-z",
        "--porcelain=v2",
        "--untracked-files=all",
        "--no-renames",
        "--ignore-submodules=all",
    ];
    let root = Path::new(s.worktree.root.path());
    bencher.bench_local(|| {
        p.git()
            .run(
                GitStep::Status,
                root,
                &args,
                DEFAULT_MAX_OUTPUT,
                DEADLINE,
                &[],
            )
            .unwrap_or_else(|e| fail("status", e))
    });
}

#[divan::bench(sample_count = 50)]
fn ls_files_z(bencher: Bencher) {
    let s = subject();
    let p = probe();
    bencher.bench_local(|| {
        p.tracked_java_blocking(&s.worktree)
            .unwrap_or_else(|e| fail("ls-files", e))
    });
}

#[divan::bench(sample_count = 50)]
fn changed_since_head(bencher: Bencher) {
    let s = subject();
    let p = probe();
    let head = p
        .head_tree_blocking(&s.worktree)
        .unwrap_or_else(|e| fail("rev-parse", e));
    bencher.bench_local(|| {
        p.changed_since_blocking(&s.worktree, head.as_ref())
            .unwrap_or_else(|e| fail("changed", e))
    });
}
