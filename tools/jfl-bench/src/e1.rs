//! The E1 parse pipeline, in process: list, then per file read, blake3, tree-sitter parse and a
//! declaration walk, on a rayon pool with one parser per worker.
//!
//! Rewritten from the archived spike (`spikes/e1-parse/`, doc/bench-corpus.md) under the
//! workspace's rules: no panics on input, no unchecked arithmetic, one grammar (the orchard
//! fork, ADR 0022; the spike's comparison against upstream was a one-off finding). Stages are
//! cumulative, so `--upto parse` reads, hashes and parses. The stage that matters for a
//! budget is `extract`, the full index of a file. The pipeline lives here and not in a product
//! crate on purpose: the harness is independent of what it judges (fnd §2.1), and the product's
//! own indexer (IDX-07) is benchmarked beside this one, not through it.
//!
//! Pinned by: `tests/e1.rs` (counts on a synthetic tree, errors counted, stage order).

use std::path::{Path, PathBuf};
use std::time::Instant;

use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Tree};

use crate::error::{BenchError, Context, Result};
use crate::procfs;
use crate::stats;

/// A cumulative pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Read each file.
    Read,
    /// Read and blake3-hash each file.
    Hash,
    /// Read, hash and parse each file.
    Parse,
    /// Read, hash, parse and walk the tree for package, imports and declarations.
    Extract,
}

impl Stage {
    /// The stage's name as written in suites and on the command line.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Hash => "hash",
            Self::Parse => "parse",
            Self::Extract => "extract",
        }
    }

    /// The stage named `s`.
    pub fn from_name(s: &str) -> Result<Self> {
        match s {
            "read" => Ok(Self::Read),
            "hash" => Ok(Self::Hash),
            "parse" => Ok(Self::Parse),
            "extract" => Ok(Self::Extract),
            other => Err(BenchError::new(format!(
                "unknown stage {other:?} (read, hash, parse, extract)"
            ))),
        }
    }
}

/// One run's knobs.
#[derive(Debug, Clone)]
pub struct Options {
    /// The checkout (or worktree) to index.
    pub root: PathBuf,
    /// Worker threads.
    pub threads: usize,
    /// The last stage to run.
    pub upto: Stage,
}

/// One run's measurements, printed as a single JSON line by `jfl-bench parse`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunResult {
    /// The grammar's name.
    pub grammar: String,
    /// Worker threads.
    pub threads: usize,
    /// The last stage run.
    pub upto: Stage,
    /// Java files found.
    pub files: u64,
    /// Total size of those files in MB (10^6 bytes).
    pub mb: f64,
    /// Time to list the files (the `.gitignore`-aware walk), ms.
    pub list_ms: f64,
    /// Time of the per-file stages across the pool, ms.
    pub work_ms: f64,
    /// `list_ms + work_ms`, ms.
    pub total_ms: f64,
    /// Files per second of the per-file stages.
    pub files_per_s: f64,
    /// MB per second of the per-file stages.
    pub mb_per_s: f64,
    /// Median per-file time, ms.
    pub p50_ms: f64,
    /// 95th percentile per-file time, ms.
    pub p95_ms: f64,
    /// Slowest file, ms.
    pub max_ms: f64,
    /// Files whose tree has an ERROR or MISSING node.
    pub err_files: u64,
    /// Files that could not be read or parsed at all.
    pub failed_files: u64,
    /// Declarations, imports and package clauses extracted.
    pub decls: u64,
    /// An order-independent digest of what was extracted: equal across runs and thread counts.
    pub checksum: u64,
    /// Peak resident set of this process at the end, KiB (Linux).
    pub rss_peak_kib: Option<u64>,
    /// Resident set of this process at the end, KiB (Linux).
    pub rss_end_kib: Option<u64>,
}

/// What a declaration is: the key a declaration diff compares, beside the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    Package,
    Import,
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
    Method,
    Constructor,
    Field,
}

/// One extracted entry: a declaration, an import or the package clause.
pub(crate) struct Decl {
    pub(crate) kind: Kind,
    pub(crate) name: Box<str>,
    start: (u32, u32),
    end: (u32, u32),
}

/// What one file leaves behind. The declarations are kept until the run's end, as an in-memory
/// index would keep them: dropping them at once would understate the RSS the spike reported.
struct FileOut {
    bytes: usize,
    nanos: u64,
    has_error: bool,
    failed: bool,
    hash_byte: u8,
    decls: Vec<Decl>,
}

/// The largest file the harness reads (16 MiB; the biggest `.java` of the pinned corpus is
/// 1.6 MB). A larger file is counted as failed, never read whole: corpus files are untrusted
/// input (plan §11).
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;

/// A file's bytes, or `None` when it is over [`MAX_FILE_BYTES`] (checked on its metadata and again
/// on what was read, since a file can grow between the two).
pub fn read_capped(path: &Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if file.metadata()?.len() > MAX_FILE_BYTES {
        return Ok(None);
    }
    let mut buf = Vec::new();
    file.take(MAX_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut buf)?;
    Ok((buf.len() as u64 <= MAX_FILE_BYTES).then_some(buf))
}

/// The `.java` files under `root`, sorted.
///
/// Only ignore files **inside the checkout** count (`.gitignore`, `.ignore`, `.rgignore`): not the
/// user's global gitignore, not `.git/info/exclude`, not the ignore files of parent directories.
/// Otherwise one person's `~/.config/git/ignore` changes the file set, every e1 number and every
/// seed-picked query, and the corpus measured is no longer the pinned one. The `rg` floor is
/// told the same (`--no-config --no-ignore-global --no-ignore-parent --no-ignore-exclude`).
/// Hidden directories are skipped, so nested `.claude/worktrees/` are never entered.
///
/// Pinned by: `tests/cli.rs::should_ignore_a_global_gitignore_and_parent_ignore_files_when_listing`.
pub fn list_java(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = WalkBuilder::new(root)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .build()
        .flatten()
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter(|e| e.path().extension().is_some_and(|x| x == "java"))
        .map(|e| e.into_path())
        .collect();
    out.sort();
    out
}

fn text<'a>(n: Node<'_>, src: &'a [u8]) -> &'a str {
    src.get(n.byte_range())
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("")
}

fn pos(p: tree_sitter::Point) -> (u32, u32) {
    (
        u32::try_from(p.row).unwrap_or(u32::MAX),
        u32::try_from(p.column).unwrap_or(u32::MAX),
    )
}

fn push(decls: &mut Vec<Decl>, kind: Kind, n: Node<'_>, src: &[u8]) {
    decls.push(Decl {
        kind,
        name: text(n, src).into(),
        start: pos(n.start_position()),
        end: pos(n.end_position()),
    });
}

fn extract(tree: &Tree, src: &[u8]) -> Vec<Decl> {
    let mut decls = Vec::new();
    let mut c = tree.walk();
    'outer: loop {
        let n = c.node();
        let mut descend = true; // a full walk also finds local and anonymous classes
        let kind = match n.kind() {
            "package_declaration" | "import_declaration" => {
                descend = false; // the whole qualified name is one entry
                let k = if n.kind() == "package_declaration" {
                    Kind::Package
                } else {
                    Kind::Import
                };
                let mut nc = n.walk();
                if let Some(q) = n
                    .named_children(&mut nc)
                    .find(|x| matches!(x.kind(), "scoped_identifier" | "identifier"))
                {
                    push(&mut decls, k, q, src);
                }
                None
            }
            "class_declaration" => Some(Kind::Class),
            "interface_declaration" => Some(Kind::Interface),
            "enum_declaration" => Some(Kind::Enum),
            "record_declaration" => Some(Kind::Record),
            "annotation_type_declaration" => Some(Kind::Annotation),
            "method_declaration" | "annotation_type_element_declaration" => Some(Kind::Method),
            "constructor_declaration" | "compact_constructor_declaration" => {
                Some(Kind::Constructor)
            }
            "field_declaration" | "constant_declaration" => {
                let mut nc = n.walk();
                for d in n.children_by_field_name("declarator", &mut nc) {
                    if let Some(name) = d.child_by_field_name("name") {
                        push(&mut decls, Kind::Field, name, src);
                    }
                }
                None
            }
            _ => None,
        };
        if let Some(k) = kind
            && let Some(name) = n.child_by_field_name("name")
        {
            push(&mut decls, k, name, src);
        }
        if descend && c.goto_first_child() {
            continue;
        }
        loop {
            if c.goto_next_sibling() {
                continue 'outer;
            }
            if !c.goto_parent() {
                break 'outer;
            }
        }
    }
    decls
}

/// Fold the extraction into a number the optimiser cannot discard and that does not depend on
/// the order files finish in.
fn digest(decls: &[Decl]) -> u64 {
    decls.iter().fold(0u64, |acc, d| {
        let part = (d.kind as u64)
            .wrapping_mul(0x9E37_79B9)
            .wrapping_add(d.name.len() as u64)
            .wrapping_add(u64::from(d.start.0))
            .wrapping_add(u64::from(d.start.1).wrapping_mul(31))
            .wrapping_add(u64::from(d.end.0).wrapping_mul(131))
            .wrapping_add(u64::from(d.end.1).wrapping_mul(7));
        acc.wrapping_add(part)
    })
}

fn process(parser: Option<&mut Parser>, path: &Path, upto: Stage) -> FileOut {
    let t0 = Instant::now();
    let mut out = FileOut {
        bytes: 0,
        nanos: 0,
        has_error: false,
        failed: false,
        hash_byte: 0,
        decls: Vec::new(),
    };
    match read_capped(path) {
        Ok(Some(src)) => {
            out.bytes = src.len();
            if upto >= Stage::Hash {
                let hash = blake3::hash(&src);
                out.hash_byte = hash.as_bytes().first().copied().unwrap_or(0);
            }
            if upto >= Stage::Parse {
                match parser.and_then(|p| p.parse(&src, None)) {
                    Some(tree) => {
                        out.has_error = tree.root_node().has_error();
                        if upto >= Stage::Extract {
                            out.decls = extract(&tree, &src);
                        }
                    }
                    None => out.failed = true,
                }
            }
        }
        Ok(None) | Err(_) => out.failed = true,
    }
    out.nanos = u64::try_from(t0.elapsed().as_nanos()).unwrap_or(u64::MAX);
    out
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// A parser for the orchard grammar.
pub(crate) fn java_parser() -> Result<Parser> {
    let language: tree_sitter::Language = tree_sitter_java_orchard::LANGUAGE.into();
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .context("load the Java grammar")?;
    Ok(parser)
}

/// Parse `src` from scratch and extract its entries; `None` when the parser gives up.
pub(crate) fn parse_extract(parser: &mut Parser, src: &[u8]) -> Option<Vec<Decl>> {
    parser.parse(src, None).map(|tree| extract(&tree, src))
}

/// The per-file stages up to `upto` over `files` on `pool`, one parser per worker.
fn process_all(pool: &rayon::ThreadPool, files: &[PathBuf], upto: Stage) -> Vec<FileOut> {
    let language: tree_sitter::Language = tree_sitter_java_orchard::LANGUAGE.into();
    pool.install(|| {
        files
            .par_iter()
            .map_init(
                || {
                    let mut p = Parser::new();
                    p.set_language(&language).is_ok().then_some(p)
                },
                |p, f| process(p.as_mut(), f, upto),
            )
            .collect()
    })
}

/// What re-indexing a set of files cost: the pool's wall time, and what it found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Batch {
    /// Wall time of the per-file stages across the pool, ms (the pool is built before it).
    pub work_ms: f64,
    /// Files given.
    pub files: u64,
    /// Files that could not be read or parsed.
    pub failed: u64,
    /// Entries extracted (0 below the extract stage).
    pub decls: u64,
}

/// Run the per-file stages up to `upto` over exactly `files` on `threads` workers: what an index
/// does to the files a change touched.
///
/// Pinned by: `tests/incremental.rs` (a re-parse of the changed set counts what the diff found).
pub fn index_files(files: &[PathBuf], threads: usize, upto: Stage) -> Result<Batch> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads.max(1))
        .build()
        .context("build the thread pool")?;
    let t = Instant::now();
    let results = process_all(&pool, files, upto);
    let work = t.elapsed();
    Ok(Batch {
        work_ms: ms(work),
        files: files.len() as u64,
        failed: results.iter().filter(|r| r.failed).count() as u64,
        decls: results
            .iter()
            .map(|r| r.decls.len() as u64)
            .fold(0, u64::saturating_add),
    })
}

/// Run the pipeline once and measure it.
pub fn run(options: &Options) -> Result<RunResult> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(options.threads)
        .build()
        .context("build the thread pool")?;

    let t_all = Instant::now();
    let files = list_java(&options.root);
    let list = t_all.elapsed();

    let t_work = Instant::now();
    let results = process_all(&pool, &files, options.upto);
    let work = t_work.elapsed();
    let total = t_all.elapsed();

    let bytes: usize = results
        .iter()
        .map(|r| r.bytes)
        .fold(0, usize::saturating_add);
    let mut nanos: Vec<f64> = results.iter().map(|r| r.nanos as f64 / 1e6).collect();
    nanos.sort_by(f64::total_cmp);
    let work_s = work.as_secs_f64().max(f64::MIN_POSITIVE);
    let decls: u64 = results
        .iter()
        .map(|r| r.decls.len() as u64)
        .fold(0, u64::saturating_add);
    let checksum = results.iter().fold(0u64, |acc, r| {
        acc.wrapping_add(digest(&r.decls))
            .wrapping_add(u64::from(r.hash_byte))
    });
    // Read while `results` is alive: the steady figure is the resident set with the index held.
    let mem = procfs::read_status(None);
    let result = RunResult {
        grammar: "orchard".to_string(),
        threads: options.threads,
        upto: options.upto,
        files: files.len() as u64,
        mb: bytes as f64 / 1e6,
        list_ms: ms(list),
        work_ms: ms(work),
        total_ms: ms(total),
        files_per_s: files.len() as f64 / work_s,
        mb_per_s: bytes as f64 / 1e6 / work_s,
        p50_ms: stats::percentile_sorted(&nanos, 0.5),
        p95_ms: stats::percentile_sorted(&nanos, 0.95),
        max_ms: stats::percentile_sorted(&nanos, 1.0),
        err_files: results.iter().filter(|r| r.has_error).count() as u64,
        failed_files: results.iter().filter(|r| r.failed).count() as u64,
        decls,
        checksum,
        rss_peak_kib: mem.and_then(|m| m.peak_kib),
        rss_end_kib: mem.and_then(|m| m.rss_kib),
    };
    drop(results);
    Ok(result)
}
