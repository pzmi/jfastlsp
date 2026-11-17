//! The incremental suite: what keeping a syntax index fresh costs, measured before the index
//! exists (BENCH-03, plan §1.7's incremental rows, *proposed*).
//!
//! Every series this module writes is a **proxy** and says so in its name
//! (`incremental/<size>/<case>/proxy/...`): the syntax-tier work an index cannot avoid, timed with
//! the E1 pipeline's own stages. The full metrics (a published generation that answers, postings
//! updated, dependants re-resolved) need the product and are written by the task that makes them
//! measurable, under the same case without `proxy` (`incremental/<size>/edit/generation_ms`);
//! `doc/bench/baseline.md` maps each proxy to its full metric and card. A proxy series is never
//! renamed into a full one: the history keeps both.
//!
//! The cases:
//! - `edit`: one file changed on disk: stat, read and re-hash it, re-parse it from scratch and
//!   extract its declarations, and diff them against the file's previous declarations. The edit
//!   is applied in memory (a method added before the last `}`) so the corpus stays clean.
//! - `rebase`, `switch`: the checkout moves from a pinned older commit to the pin
//!   (`scripts/bench-corpus.sh pairs`): `git diff --name-only` between the two, the size of the
//!   changed set, and a re-parse of the changed files present at the pin.
//! - `fanout`: the most-imported project types (picked from the corpus, not by hand; series keyed
//!   by slot `kNN`): how many files import or name each (the files a change to it re-resolves),
//!   the scan that finds them and a re-parse of them.
//! - `reopen`: an indexed worktree reopened warm: the walk, a stat of every file, and a full
//!   re-hash (and the re-hash after a page-cache drop, where allowed).
//!
//! Pinned by: `tests/incremental.rs` (every case on a two-commit repository in a tempdir) and this
//! module's unit tests.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use crate::e1::{self, Decl, Kind, Stage};
use crate::error::{BenchError, Context, Result};
use crate::report::{Better, Metric, QuerySlot};
use crate::rng::SplitMix64;
use crate::runner::CacheDrop;

/// The method the synthetic edit adds: a new declaration, so the diff has exactly one entry.
pub const PROBE: &str = "\n    void jflBenchProbe() {}\n";

/// A pinned older commit a checkout moves from to its pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// `rebase` or `switch`: the series name.
    pub role: String,
    /// The full commit id.
    pub commit: String,
}

/// The suite's knobs, from the suite file's `[incremental]` and `[run]`.
#[derive(Debug, Clone)]
pub struct Options {
    /// Worker threads for re-parsing a set of files and re-hashing a worktree.
    pub threads: usize,
    /// Untimed repetitions before the timed ones.
    pub warmup: u32,
    /// Timed repetitions.
    pub runs: u32,
    /// Repetitions after a page-cache drop (`reopen` only).
    pub cold_runs: u32,
    /// Seed for picking the edited files.
    pub seed: u64,
    /// Files edited by the `edit` case.
    pub edits: usize,
    /// Widely used types measured by the `fanout` case.
    pub fanout: usize,
    /// The pair roles to measure, in order.
    pub roles: Vec<String>,
}

/// Parse the `role commit` lines `scripts/bench-corpus.sh pairs SIZE` prints. A commit must be 40
/// hex digits, so nothing on the line can reach `git` as an option.
pub fn parse_pairs(text: &str) -> Result<Vec<Pair>> {
    let mut out = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut parts = line.split_whitespace();
        let (Some(role), Some(commit), None) = (parts.next(), parts.next(), parts.next()) else {
            return Err(BenchError::new(format!("pairs: malformed line {line:?}")));
        };
        let role_ok = !role.is_empty() && role.chars().all(|c| c.is_ascii_lowercase());
        let commit_ok = commit.len() == 40 && commit.chars().all(|c| c.is_ascii_hexdigit());
        if !role_ok || !commit_ok {
            return Err(BenchError::new(format!("pairs: malformed line {line:?}")));
        }
        out.push(Pair {
            role: role.to_string(),
            commit: commit.to_string(),
        });
    }
    Ok(out)
}

/// The pinned pairs of `size`, from `scripts/bench-corpus.sh pairs SIZE` in `repo_root`.
pub fn read_pairs(repo_root: &Path, size: &str) -> Result<Vec<Pair>> {
    let script = repo_root.join("scripts/bench-corpus.sh");
    let out = Command::new(&script)
        .args(["pairs", size])
        .output()
        .context(format!("run {} pairs {size}", script.display()))?;
    if !out.status.success() {
        return Err(BenchError::new(format!(
            "bench-corpus.sh pairs {size}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    parse_pairs(&String::from_utf8_lossy(&out.stdout))
}

/// The `.java` files that differ between a commit and the checkout's `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    /// Changed files present in the checkout, absolute: what an index re-parses.
    pub present: Vec<PathBuf>,
    /// Changed files absent from the checkout: what an index drops.
    pub deleted: u64,
}

/// `git diff --name-only --no-renames` between `from` and `HEAD` of `root`, `.java` only. `from`
/// must be a full commit id, so it never reaches `git` as an option.
pub fn changed_java(root: &Path, from: &str) -> Result<Changed> {
    if from.len() != 40 || !from.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(BenchError::new(format!("{from:?} is not a full commit id")));
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "diff",
            "--name-only",
            "--no-renames",
            "-z",
            from,
            "HEAD",
            "--",
            "*.java",
        ])
        .output()
        .context("run git diff")?;
    if !out.status.success() {
        return Err(BenchError::new(format!(
            "git diff {from} HEAD: {} (fetch the pairs: scripts/bench-corpus.sh fetch-pairs SIZE)",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    let mut present = Vec::new();
    let mut deleted = 0u64;
    for name in out.stdout.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let path = root.join(String::from_utf8_lossy(name).as_ref());
        if path.is_file() {
            present.push(path);
        } else {
            deleted = deleted.saturating_add(1);
        }
    }
    Ok(Changed { present, deleted })
}

/// `src` with [`PROBE`] inserted before its last `}`; `None` when it has none.
pub fn apply_edit(src: &[u8]) -> Option<Vec<u8>> {
    let at = src.iter().rposition(|b| *b == b'}')?;
    let mut out = Vec::with_capacity(src.len().saturating_add(PROBE.len()));
    out.extend_from_slice(src.get(..at)?);
    out.extend_from_slice(PROBE.as_bytes());
    out.extend_from_slice(src.get(at..)?);
    Some(out)
}

/// Entries added and removed between two extractions, keyed by kind and name (a multiset): what
/// an index would publish for the file. Positions are not compared, so an insertion that moves
/// later declarations is one addition, not a rewrite of the file.
pub(crate) fn decl_diff(old: &[Decl], new: &[Decl]) -> (u64, u64) {
    let mut count: BTreeMap<(Kind, &str), i64> = BTreeMap::new();
    for d in new {
        let e = count.entry((d.kind, &d.name)).or_insert(0);
        *e = e.saturating_add(1);
    }
    for d in old {
        let e = count.entry((d.kind, &d.name)).or_insert(0);
        *e = e.saturating_sub(1);
    }
    count.values().fold((0, 0), |(a, r), v| {
        if *v > 0 {
            (a.saturating_add(v.unsigned_abs()), r)
        } else {
            (a, r.saturating_add(v.unsigned_abs()))
        }
    })
}

/// One timed edit: each stage in ms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EditSample {
    /// Stat, read and blake3 of the file.
    pub rehash_ms: f64,
    /// Parse from scratch and extract the edited text.
    pub reparse_ms: f64,
    /// Diff the declarations against the previous extraction.
    pub diff_ms: f64,
}

/// What the `edit` case measured.
#[derive(Debug, Clone, PartialEq)]
pub struct Edits {
    /// One sample per edited file and timed run.
    pub samples: Vec<EditSample>,
    /// Files edited.
    pub files: usize,
    /// Edited files whose diff was not exactly one addition (a parse that lost the probe).
    pub unexpected: u64,
}

fn ms_since(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// The `edit` case on `count` files of `root` picked by `seed`.
pub fn edits(root: &Path, count: usize, seed: u64, warmup: u32, runs: u32) -> Result<Edits> {
    let mut files = e1::list_java(root);
    let mut rng = SplitMix64::new(seed);
    let mut parser = e1::java_parser()?;
    let mut out = Edits {
        samples: Vec::new(),
        files: 0,
        unexpected: 0,
    };
    let mut next = 0usize;
    while out.files < count && next < files.len() {
        let j = next.saturating_add(rng.below(files.len().saturating_sub(next)));
        files.swap(next, j);
        let Some(path) = files.get(next).cloned() else {
            break;
        };
        next = next.saturating_add(1);
        let Ok(Some(original)) = e1::read_capped(&path) else {
            continue;
        };
        let (Some(edited), Some(old)) = (
            apply_edit(&original),
            e1::parse_extract(&mut parser, &original),
        ) else {
            continue;
        };
        out.files = out.files.saturating_add(1);
        for i in 0..warmup.saturating_add(runs) {
            let t = Instant::now();
            let meta = std::fs::metadata(&path).context(format!("stat {}", path.display()))?;
            let bytes = e1::read_capped(&path)
                .context(format!("read {}", path.display()))?
                .context(format!("{} grew past the cap", path.display()))?;
            black_box((meta.len(), meta.modified().ok(), blake3::hash(&bytes)));
            let rehash_ms = ms_since(t);
            let t = Instant::now();
            let new = e1::parse_extract(&mut parser, &edited)
                .context(format!("parse the edited {}", path.display()))?;
            let reparse_ms = ms_since(t);
            let t = Instant::now();
            let (added, removed) = black_box(decl_diff(&old, &new));
            let diff_ms = ms_since(t);
            if i == 0 && (added, removed) != (1, 0) {
                out.unexpected = out.unexpected.saturating_add(1);
            }
            if i >= warmup {
                out.samples.push(EditSample {
                    rehash_ms,
                    reparse_ms,
                    diff_ms,
                });
            }
        }
    }
    Ok(out)
}

/// A file's `package` and `import` clauses, read line by line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Header {
    /// The package, if declared.
    pub package: Option<String>,
    /// Types imported by name: single-type imports, and the type of a static import.
    pub imports: BTreeSet<String>,
    /// Packages (or types) imported on demand (`import a.b.*;`).
    pub wildcards: BTreeSet<String>,
}

/// The header of `src`. A line scan, not a parse: it counts imports for picking the widely used
/// types and finding their importers, and a commented-out import costs one false importer.
pub fn header(src: &str) -> Header {
    let mut h = Header::default();
    for line in src.lines() {
        let t = line.trim();
        let (rest, is_import) = if let Some(r) = t.strip_prefix("package ") {
            (r, false)
        } else if let Some(r) = t.strip_prefix("import ") {
            (r, true)
        } else {
            continue;
        };
        let Some(body) = rest.split(';').next() else {
            continue;
        };
        let body = body.trim();
        if !is_import {
            h.package = Some(body.to_string());
            continue;
        }
        let (body, is_static) = match body.strip_prefix("static ") {
            Some(b) => (b.trim(), true),
            None => (body, false),
        };
        if let Some(target) = body.strip_suffix(".*") {
            if is_static {
                h.imports.insert(target.to_string());
            } else {
                h.wildcards.insert(target.to_string());
            }
        } else if is_static {
            if let Some((ty, _member)) = body.rsplit_once('.') {
                h.imports.insert(ty.to_string());
            }
        } else {
            h.imports.insert(body.to_string());
        }
    }
    h
}

/// Whether `word` occurs in `text` as a whole Java identifier.
pub fn contains_word(text: &[u8], word: &[u8]) -> bool {
    let ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80;
    let n = word.len();
    if n == 0 || text.len() < n {
        return false;
    }
    text.windows(n).enumerate().any(|(i, w)| {
        w == word
            && (i == 0 || text.get(i.wrapping_sub(1)).is_some_and(|b| !ident(*b)))
            && text.get(i.saturating_add(n)).is_none_or(|b| !ident(*b))
    })
}

/// A widely used project type and the files that depend on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Widely {
    /// Its fully qualified name.
    pub fqn: String,
    /// Files importing it by name.
    pub importers: u64,
    /// The proxy fan-out: files that import it, or import its package on demand, or share its
    /// package, and name it. What a change to the type makes an index re-resolve.
    pub dependants: Vec<PathBuf>,
    /// Files naming it as a word anywhere (an upper bound: other types of the same simple name
    /// count too).
    pub mentions: u64,
}

struct Scanned {
    path: PathBuf,
    header: Header,
}

fn scan_headers(root: &Path) -> Vec<Scanned> {
    e1::list_java(root)
        .into_iter()
        .filter_map(|path| {
            let bytes = e1::read_capped(&path).ok()??;
            let header = header(&String::from_utf8_lossy(&bytes));
            Some(Scanned { path, header })
        })
        .collect()
}

fn split_fqn(fqn: &str) -> (&str, &str) {
    fqn.rsplit_once('.').unwrap_or(("", fqn))
}

fn names(path: &Path, simple: &str) -> bool {
    e1::read_capped(path)
        .ok()
        .flatten()
        .is_some_and(|b| contains_word(&b, simple.as_bytes()))
}

fn dependants_of(files: &[Scanned], fqn: &str) -> Vec<PathBuf> {
    let (package, simple) = split_fqn(fqn);
    let own = format!("{simple}.java");
    files
        .iter()
        .filter(|f| {
            if f.header.imports.contains(fqn) {
                return true;
            }
            let same_package = f.header.package.as_deref() == Some(package)
                && f.path.file_name().is_none_or(|n| n != own.as_str());
            (same_package || f.header.wildcards.contains(package)) && names(&f.path, simple)
        })
        .map(|f| f.path.clone())
        .collect()
}

/// The `count` project types imported by the most files under `root` (ties by name), with their
/// dependants. A project type is one whose `<package>/<Name>.java` is in the checkout, so a JDK or
/// library type is never picked.
pub fn widely_used(root: &Path, count: usize) -> Result<Vec<Widely>> {
    let files = scan_headers(root);
    let declared: BTreeSet<(String, String)> = files
        .iter()
        .filter_map(|f| {
            let stem = f.path.file_stem()?.to_string_lossy().into_owned();
            Some((f.header.package.clone().unwrap_or_default(), stem))
        })
        .collect();
    let mut importers: HashMap<&str, u64> = HashMap::new();
    for f in &files {
        for fqn in &f.header.imports {
            let (p, s) = split_fqn(fqn);
            if declared.contains(&(p.to_string(), s.to_string())) {
                let e = importers.entry(fqn).or_insert(0);
                *e = e.saturating_add(1);
            }
        }
    }
    let mut ranked: Vec<(&str, u64)> = importers.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    ranked
        .into_iter()
        .take(count)
        .map(|(fqn, n)| {
            let simple = split_fqn(fqn).1;
            let mentions = files.iter().filter(|f| names(&f.path, simple)).count() as u64;
            Ok(Widely {
                fqn: fqn.to_string(),
                importers: n,
                dependants: dependants_of(&files, fqn),
                mentions,
            })
        })
        .collect()
}

fn digest(name: &str) -> String {
    blake3::hash(name.as_bytes())
        .to_hex()
        .as_str()
        .get(..16)
        .unwrap_or_default()
        .to_string()
}

/// Run `measure` `warmup` times untimed, then `runs` times, and return the timed results.
fn repeat<T>(warmup: u32, runs: u32, mut measure: impl FnMut() -> Result<T>) -> Result<Vec<T>> {
    for _ in 0..warmup {
        measure()?;
    }
    (0..runs).map(|_| measure()).collect()
}

/// A metric name under the suite.
fn name(size: &str, case: &str, rest: &str) -> String {
    format!("incremental/{size}/{case}/proxy/{rest}")
}

fn edit_metrics(size: &str, e: &Edits, metrics: &mut Vec<Metric>) {
    let what = format!("edits of {} files", e.files);
    type Get = fn(&EditSample) -> f64;
    let stages: [(&str, Get); 4] = [
        ("rehash", |s| s.rehash_ms),
        ("reparse", |s| s.reparse_ms),
        ("decl_diff", |s| s.diff_ms),
        ("total", |s| s.rehash_ms + s.reparse_ms + s.diff_ms),
    ];
    for (stage, get) in stages {
        let values: Vec<f64> = e.samples.iter().map(get).collect();
        for (label, p) in [("p50", 0.5), ("p95", 0.95)] {
            metrics.push(Metric::percentile_over(
                name(size, "edit", &format!("{stage}_ms/{label}")),
                "ms",
                Better::Smaller,
                values.clone(),
                p,
                label,
                &what,
            ));
        }
    }
    metrics.push(Metric::single(
        name(size, "edit", "files"),
        "count",
        Better::Info,
        e.files as f64,
    ));
    metrics.push(Metric::single(
        name(size, "edit", "unexpected_diffs"),
        "count",
        Better::Info,
        e.unexpected as f64,
    ));
}

fn pair_metrics(
    root: &Path,
    size: &str,
    o: &Options,
    pair: &Pair,
    metrics: &mut Vec<Metric>,
) -> Result<()> {
    let changed = changed_java(root, &pair.commit)?;
    let role = pair.role.as_str();
    let diffs = repeat(o.warmup, o.runs, || {
        let t = Instant::now();
        black_box(changed_java(root, &pair.commit)?);
        Ok(ms_since(t))
    })?;
    let reparses = repeat(o.warmup, o.runs, || {
        Ok(e1::index_files(&changed.present, o.threads, Stage::Extract)?.work_ms)
    })?;
    let totals: Vec<f64> = diffs.iter().zip(&reparses).map(|(d, r)| d + r).collect();
    metrics.push(Metric::median_of(
        name(size, role, "diff_ms"),
        "ms",
        Better::Smaller,
        diffs,
    ));
    metrics.push(Metric::median_of(
        name(size, role, &format!("t{}/reparse_ms", o.threads)),
        "ms",
        Better::Smaller,
        reparses,
    ));
    metrics.push(Metric::median_of(
        name(size, role, &format!("t{}/total_ms", o.threads)),
        "ms",
        Better::Smaller,
        totals,
    ));
    for (field, v) in [
        ("changed_files", changed.present.len() as u64),
        ("deleted_files", changed.deleted),
    ] {
        metrics.push(Metric::single(
            name(size, role, field),
            "count",
            Better::Info,
            v as f64,
        ));
    }
    Ok(())
}

fn fanout_metrics(
    root: &Path,
    size: &str,
    o: &Options,
    metrics: &mut Vec<Metric>,
    slots: &mut Vec<QuerySlot>,
) -> Result<()> {
    let picked = widely_used(root, o.fanout)?;
    let files = scan_headers(root);
    for (i, w) in picked.iter().enumerate() {
        let slot = format!("k{i:02}");
        slots.push(QuerySlot {
            slot: slot.clone(),
            name_blake3: digest(&w.fqn),
        });
        let case = format!("fanout/{slot}");
        let scans = repeat(o.warmup, o.runs, || {
            let t = Instant::now();
            black_box(dependants_of(&files, &w.fqn));
            Ok(ms_since(t))
        })?;
        let reparses = repeat(o.warmup, o.runs, || {
            Ok(e1::index_files(&w.dependants, o.threads, Stage::Extract)?.work_ms)
        })?;
        metrics.push(Metric::median_of(
            name(size, &case, "scan_ms"),
            "ms",
            Better::Smaller,
            scans,
        ));
        metrics.push(Metric::median_of(
            name(size, &case, &format!("t{}/reparse_ms", o.threads)),
            "ms",
            Better::Smaller,
            reparses,
        ));
        for (field, v) in [
            ("dependants", w.dependants.len() as u64),
            ("importers", w.importers),
            ("mentions", w.mentions),
        ] {
            metrics.push(Metric::single(
                name(size, &case, field),
                "count",
                Better::Info,
                v as f64,
            ));
        }
    }
    Ok(())
}

/// A warm re-hash of every file: the walk plus read and blake3 on the pool, ms.
fn rehash(root: &Path, threads: usize) -> Result<f64> {
    let t = Instant::now();
    let files = e1::list_java(root);
    let list_ms = ms_since(t);
    Ok(list_ms + e1::index_files(&files, threads, Stage::Hash)?.work_ms)
}

fn reopen_metrics(
    root: &Path,
    size: &str,
    o: &Options,
    drop: &dyn CacheDrop,
    metrics: &mut Vec<Metric>,
    notes: &mut Vec<String>,
) -> Result<()> {
    let walks = repeat(o.warmup, o.runs, || {
        let t = Instant::now();
        black_box(e1::list_java(root));
        Ok(ms_since(t))
    })?;
    let stats = repeat(o.warmup, o.runs, || {
        let t = Instant::now();
        let seen = e1::list_java(root)
            .iter()
            .filter_map(|f| std::fs::metadata(f).ok())
            .fold(0u64, |acc, m| {
                let mtime = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                acc.wrapping_add(m.len()).wrapping_add(mtime)
            });
        black_box(seen);
        Ok(ms_since(t))
    })?;
    let rehashes = repeat(o.warmup, o.runs, || rehash(root, o.threads))?;
    metrics.push(Metric::median_of(
        name(size, "reopen", "walk_ms"),
        "ms",
        Better::Smaller,
        walks,
    ));
    metrics.push(Metric::median_of(
        name(size, "reopen", "stat_ms"),
        "ms",
        Better::Smaller,
        stats,
    ));
    metrics.push(Metric::median_of(
        name(size, "reopen", &format!("t{}/rehash_ms", o.threads)),
        "ms",
        Better::Smaller,
        rehashes,
    ));
    if o.cold_runs > 0 {
        let mut cold = Vec::new();
        for _ in 0..o.cold_runs {
            if let Err(e) = drop.drop_caches() {
                notes.push(format!("reopen cold runs skipped: {e}"));
                cold.clear();
                break;
            }
            cold.push(rehash(root, o.threads)?);
        }
        if !cold.is_empty() {
            metrics.push(Metric::median_of(
                name(size, "reopen", &format!("t{}/cold/rehash_ms", o.threads)),
                "ms",
                Better::Smaller,
                cold,
            ));
        }
    }
    Ok(())
}

/// Run every case on the checkout `root` of `size`, with `pairs` for the rebase and switch cases.
/// A role the suite asks for that `size` has no pair for is skipped with a note (the small corpus
/// has none).
pub fn measure(
    root: &Path,
    size: &str,
    o: &Options,
    pairs: &[Pair],
    drop: &dyn CacheDrop,
    notes: &mut Vec<String>,
) -> Result<(Vec<Metric>, Vec<QuerySlot>)> {
    let mut metrics = Vec::new();
    let mut slots = Vec::new();
    let e = edits(root, o.edits, o.seed, o.warmup, o.runs)?;
    if e.unexpected > 0 {
        notes.push(format!(
            "edit: {} of {} edited files did not diff as exactly one added declaration",
            e.unexpected, e.files
        ));
    }
    edit_metrics(size, &e, &mut metrics);
    for role in &o.roles {
        match pairs.iter().find(|p| &p.role == role) {
            Some(pair) => pair_metrics(root, size, o, pair, &mut metrics)?,
            None => notes.push(format!(
                "{role}: no pinned commit pair for {size} (scripts/bench-corpus.sh pairs); not measured"
            )),
        }
    }
    fanout_metrics(root, size, o, &mut metrics, &mut slots)?;
    reopen_metrics(root, size, o, drop, &mut metrics, notes)?;
    notes.push(
        "incremental: every series is a syntax-tier proxy (`/proxy/` in its name); the full metrics arrive with IDX-09, IDX-11, SEM-01 and SEM-02 (doc/bench/baseline.md)"
            .to_string(),
    );
    Ok((metrics, slots))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_insert_the_probe_before_the_last_brace_when_editing() {
        // given
        let src = b"class A {\n  int x;\n}\n";
        // when
        let edited = apply_edit(src).unwrap();
        // then
        let text = String::from_utf8(edited).unwrap();
        assert!(text.ends_with(&format!("{PROBE}}}\n")), "{text}");
        assert_eq!(apply_edit(b"no braces"), None);
    }

    #[test]
    fn should_count_one_addition_when_the_probe_is_parsed() {
        // given
        let mut parser = e1::java_parser().unwrap();
        let src = b"package p;\nclass A {\n  int x;\n  void m() {}\n}\n";
        let old = e1::parse_extract(&mut parser, src).unwrap();
        let new = e1::parse_extract(&mut parser, &apply_edit(src).unwrap()).unwrap();
        // when
        let diff = decl_diff(&old, &new);
        // then
        assert_eq!(diff, (1, 0));
        assert_eq!(decl_diff(&new, &old), (0, 1));
        assert_eq!(decl_diff(&old, &old), (0, 0));
    }

    #[test]
    fn should_read_package_imports_and_wildcards_when_scanning_a_header() {
        // given
        let src = "package a.b;\n\nimport x.y.Z;\nimport static x.y.Util.max;\nimport static x.y.Consts.*;\nimport q.r.*;\n\nclass C {}\n";
        // when
        let h = header(src);
        // then
        assert_eq!(h.package.as_deref(), Some("a.b"));
        let imports: Vec<&str> = h.imports.iter().map(String::as_str).collect();
        assert_eq!(imports, ["x.y.Consts", "x.y.Util", "x.y.Z"]);
        let wild: Vec<&str> = h.wildcards.iter().map(String::as_str).collect();
        assert_eq!(wild, ["q.r"]);
    }

    #[test]
    fn should_match_whole_identifiers_only_when_looking_for_a_word() {
        // given
        let text = b"Ledger x = new LedgerService(); MyLedger y; Ledger$1 z; (Ledger)";
        // when
        // then
        assert!(contains_word(text, b"Ledger"));
        assert!(!contains_word(b"LedgerService MyLedger Ledger_", b"Ledger"));
        assert!(contains_word(b"Ledger", b"Ledger"));
        assert!(!contains_word(b"", b"Ledger"));
    }

    #[test]
    fn should_accept_role_and_commit_lines_and_refuse_anything_else_when_reading_pairs() {
        // given
        let good = "rebase f0fbdf71ed16a021c46eb42712057a184040b5aa\nswitch a4566465f1f1d98bf52f5cc289b9e63b918bb9c3\n";
        // when
        let pairs = parse_pairs(good).unwrap();
        // then
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[1].role, "switch");
        assert!(parse_pairs("rebase --output=x\n").is_err());
        assert!(parse_pairs("rebase f0fb extra\n").is_err());
        assert_eq!(parse_pairs("").unwrap(), []);
    }
}
