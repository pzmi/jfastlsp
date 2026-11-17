//! Running a suite: what each kind measures, and the metric names it writes.
//!
//! Metric names are `<suite>/<corpus size>/<...>` and never change once a series exists, because
//! the history tool keys its chart on them (BENCH-04): `rg-floor/small/<query>/p50`,
//! `e1/small/t4/extract/work_ms`. A measurement that cannot be taken here (no page-cache drop
//! without root, no `/proc` on macOS) is left out and named in the report's `notes`; an absent
//! series is honest, a made-up one is not.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::arms;
use crate::corpus;
use crate::e1::{RunResult, Stage};
use crate::error::{BenchError, Context, Result};
use crate::hyperfine;
use crate::incremental;
use crate::machine;
use crate::queries;
use crate::report::{Better, Metric, QuerySlot, Report, RunFacts};
use crate::stats;
use crate::suite::{E1, Kind, Queries, Suite};

/// Everything a run needs besides the suite.
#[derive(Debug, Clone)]
pub struct Args {
    /// The suite.
    pub suite: Suite,
    /// The corpus size name.
    pub size: String,
    /// The corpus checkout.
    pub corpus_root: PathBuf,
    /// This repository, for the commit and the facts.
    pub repo_root: PathBuf,
    /// This binary, which the e1 suite runs once per measurement.
    pub exe: PathBuf,
    /// A scratch directory for hyperfine's export and captured output.
    pub scratch: PathBuf,
    /// The kmp-lsp binary for the kmp-lsp arm (default: where `scripts/kmp-lsp-fetch.sh` puts it).
    pub kmp_lsp: Option<PathBuf>,
    /// The verified jdtls install for the jdtls arm (default: where `scripts/jdtls-fetch.sh`
    /// puts it, [`crate::jdtls::default_home`]).
    pub jdtls_home: Option<PathBuf>,
    /// Let the jdtls arm import a corpus checkout's build (runs its Gradle build: a dispatch job
    /// or the owner, never the sandbox).
    pub trusted_build: bool,
}

/// The size name of the synthetic multi-module fixture (`tools/jfl-bench/fixtures/multi-module`),
/// the only checkout the jdtls arm opens without `--trusted-build`.
///
/// Pinned by: `runner::tests::should_refuse_a_corpus_checkout_when_the_jdtls_arm_is_not_trusted`.
pub const FIXTURE_SIZE: &str = "fixture";

/// Run the suite and return its report.
pub fn run(args: &Args, drop: &dyn CacheDrop) -> Result<Report> {
    std::fs::create_dir_all(&args.scratch).context(format!("create {}", args.scratch.display()))?;
    let tools: &[&str] = match args.suite.kind {
        Kind::Jdtls => &["hyperfine", "rg", "git", "java"],
        _ => &["hyperfine", "rg", "git"],
    };
    let mut machine = machine::collect(&args.repo_root, &args.corpus_root, tools);
    let corpus = corpus::describe(&args.corpus_root, &args.size)?;
    let mut notes = Vec::new();
    let driven: &[&str] = match args.suite.kind {
        Kind::RgFloor => &["rg", "hyperfine"],
        Kind::KmpLsp => &["hyperfine"],
        Kind::E1 | Kind::Incremental | Kind::Jdtls => &[],
    };
    for tool in driven {
        if machine::find_on_path(tool).is_some_and(|p| machine::is_shim(&p)) {
            notes.push(format!(
                "{tool} resolves to a version-manager shim: its launcher's startup time and memory are in every number; put the real binary first on PATH (`just bench` does)"
            ));
        }
    }
    let build = machine::Build::current();
    if build.debug_assertions {
        notes.push(
            "DEBUG BUILD: these numbers are not measurements; build with the bench-tool profile"
                .to_string(),
        );
    }
    let mut query_slots = Vec::new();
    let metrics = match args.suite.kind {
        Kind::RgFloor => {
            let q = args
                .suite
                .queries
                .as_ref()
                .context("rg-floor suite without [queries]")?;
            let (m, slots) = rg_floor(args, q, drop, &mut notes)?;
            query_slots = slots;
            m
        }
        Kind::E1 => {
            let e = args.suite.e1.as_ref().context("e1 suite without [e1]")?;
            e1_suite(args, e, drop, &mut notes)?
        }
        Kind::Incremental => {
            let i = args
                .suite
                .incremental
                .as_ref()
                .context("incremental suite without [incremental]")?;
            let (m, slots) = incremental_suite(args, i, drop, &mut notes)?;
            query_slots = slots;
            m
        }
        Kind::KmpLsp => {
            let k = args
                .suite
                .kmp
                .as_ref()
                .context("kmp-lsp suite without [kmp]")?;
            let kmp = args
                .kmp_lsp
                .clone()
                .or_else(arms::default_kmp_lsp)
                .context("no kmp-lsp path: pass --kmp-lsp PATH")?;
            machine
                .tools
                .insert("kmp-lsp".to_string(), machine::version_of(&kmp));
            // scripts/kmp-lsp-fetch.sh's record: the commit, toolchain and sha256 of the binary.
            if let Some(verified) = kmp
                .parent()
                .and_then(Path::parent)
                .and_then(|d| std::fs::read_to_string(d.join(".verified")).ok())
            {
                let build: Vec<&str> = verified.lines().map(str::trim).collect();
                machine
                    .tools
                    .insert("kmp-lsp-build".to_string(), build.join("; "));
            }
            let (m, slots) = arms::kmp_suite(
                &arms::KmpRun {
                    size: &args.size,
                    root: &args.corpus_root,
                    kmp: &kmp,
                    scratch: &args.scratch,
                    warmup: args.suite.run.warmup,
                    runs: args.suite.run.runs,
                    seed: args.suite.run.seed,
                },
                k,
                &mut notes,
            )?;
            query_slots = slots;
            m
        }
        Kind::Jdtls => {
            let j = args
                .suite
                .jdtls
                .as_ref()
                .context("jdtls suite without [jdtls]")?;
            let (m, slots) = jdtls_suite(args, j, &mut notes)?;
            query_slots = slots;
            m
        }
    };
    Ok(Report {
        schema: 1,
        suite: args.suite.name.clone(),
        corpus,
        machine,
        build,
        queries: query_slots,
        config: RunFacts {
            runs: args.suite.run.runs,
            warmup: args.suite.run.warmup,
            seed: args.suite.run.seed,
        },
        metrics,
        notes,
    })
}

fn incremental_suite(
    args: &Args,
    i: &crate::suite::Incremental,
    drop: &dyn CacheDrop,
    notes: &mut Vec<String>,
) -> Result<(Vec<Metric>, Vec<QuerySlot>)> {
    let pairs = if i.roles.is_empty() {
        Vec::new()
    } else {
        match incremental::read_pairs(&args.repo_root, &args.size) {
            Ok(p) => p,
            Err(e) => {
                notes.push(format!("pairs: {e}"));
                Vec::new()
            }
        }
    };
    let run = &args.suite.run;
    let options = incremental::Options {
        threads: i.threads,
        warmup: run.warmup,
        runs: run.runs,
        cold_runs: run.cold_runs,
        seed: run.seed,
        edits: i.edits,
        fanout: i.fanout,
        roles: i.roles.clone(),
    };
    incremental::measure(&args.corpus_root, &args.size, &options, &pairs, drop, notes)
}

/// Copy the tree at `from` to `to` (files and directories only).
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).context(format!("create {}", to.display()))?;
    for entry in std::fs::read_dir(from).context(format!("read {}", from.display()))? {
        let entry = entry.context("read a directory entry")?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type().context("read a file type")?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)
                .context(format!("copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn jdtls_suite(
    args: &Args,
    j: &crate::suite::Jdtls,
    notes: &mut Vec<String>,
) -> Result<(Vec<Metric>, Vec<QuerySlot>)> {
    let fixture = args.size == FIXTURE_SIZE;
    if !fixture && !args.trusted_build {
        return Err(BenchError::new(format!(
            "the jdtls arm on the {} corpus imports its Gradle build, which never runs in the sandbox: \
             run `jfl-bench run jdtls fixture` here, and the corpus run as a bench.yml dispatch job or \
             an owner run with --trusted-build (doc/bench/baseline.md)",
            args.size
        )));
    }
    // jdtls writes Eclipse metadata into the project it opens: the fixture is opened as a scratch
    // copy (a git root, as an LSP client's workspace usually is), never in this repository.
    let root = if fixture {
        let copy = args.scratch.join("fixture");
        copy_tree(&args.corpus_root, &copy)?;
        std::fs::create_dir_all(copy.join(".git")).context("mark the fixture copy a git root")?;
        copy.canonicalize().context("resolve the fixture copy")?
    } else {
        notes.push(
            "jdtls: trusted build: jdtls imported the checkout's Gradle build and wrote Eclipse metadata into it"
                .to_string(),
        );
        args.corpus_root.clone()
    };
    let jdtls_home = args
        .jdtls_home
        .clone()
        .or_else(crate::jdtls::default_home)
        .context("no --jdtls-home and no HOME to find scripts/jdtls-fetch.sh's install under")?;
    let java = crate::jdtls::java()
        .context("no java for the jdtls arm: set JAVA_HOME or put java (21 or newer) on PATH")?;
    arms::jdtls_suite(
        &arms::JdtlsRun {
            size: &args.size,
            root: &root,
            jdtls_home: &jdtls_home,
            java: &java,
            scratch: &args.scratch,
            trusted_build: args.trusted_build,
            warmup: args.suite.run.warmup,
            runs: args.suite.run.runs,
            seed: args.suite.run.seed,
        },
        j,
        notes,
    )
}

/// The shell command hyperfine runs before every cold run (`--prepare`).
pub const DROP_CACHES_COMMAND: &str = "sync; echo 3 > /proc/sys/vm/drop_caches";

/// Something that can empty the page cache. A trait so a failed drop is testable: the real one
/// needs Linux and root, a test's does not.
pub trait CacheDrop {
    /// Empty the page cache, or say why it cannot be.
    fn drop_caches(&self) -> Result<()>;
}

/// Empty the page cache for real (Linux, root): `sync`, then write 3 to `drop_caches`.
#[derive(Debug, Clone, Copy)]
pub struct SystemCacheDrop;

impl CacheDrop for SystemCacheDrop {
    fn drop_caches(&self) -> Result<()> {
        let synced = Command::new("sync").status().context("run sync")?;
        if !synced.success() {
            return Err(BenchError::new("sync failed"));
        }
        std::fs::write("/proc/sys/vm/drop_caches", "3").context("write /proc/sys/vm/drop_caches")
    }
}

/// The ripgrep command of a query. `--no-config` and the three `--no-ignore-*` flags keep the
/// file set the corpus's own ignore files give, whatever `RIPGREP_CONFIG_PATH` or the user's
/// global gitignore say (`e1::list_java` applies the same rule to the walk).
fn rg_argv(pattern: &str, whole_word: bool, target: &str) -> Vec<String> {
    let mut argv: Vec<String> = ["rg", "-c"].iter().map(|s| (*s).to_string()).collect();
    if whole_word {
        argv.push("-w".to_string());
    }
    for flag in [
        "-t",
        "java",
        "--no-messages",
        "--no-config",
        "--no-ignore-global",
        "--no-ignore-parent",
        "--no-ignore-exclude",
    ] {
        argv.push(flag.to_string());
    }
    argv.push(pattern.to_string());
    argv.push(target.to_string());
    argv
}

/// The sum of `rg -c`'s `path:count` lines.
fn rg_match_total(stdout: &str) -> u64 {
    stdout
        .lines()
        .filter_map(|l| l.rsplit_once(':')?.1.trim().parse::<u64>().ok())
        .fold(0, u64::saturating_add)
}

fn kib_to_mib(kib: f64) -> f64 {
    kib / 1024.0
}

fn mib(kib: u64) -> f64 {
    kib as f64 / 1024.0
}

/// A query slot's series prefix.
fn slot(i: usize) -> String {
    format!("q{i:02}")
}

fn name_digest(name: &str) -> String {
    blake3::hash(name.as_bytes())
        .to_hex()
        .as_str()
        .get(..16)
        .unwrap_or_default()
        .to_string()
}

fn match_count(argv: &[String], cwd: &Path) -> Result<u64> {
    let (program, rest) = argv.split_first().context("empty rg command")?;
    let out = Command::new(program)
        .args(rest)
        .current_dir(cwd)
        .output()
        .context(format!("run {program}"))?;
    if !matches!(out.status.code(), Some(0 | 1)) {
        return Err(BenchError::new(format!(
            "{program} exited with {:?}",
            out.status.code()
        )));
    }
    Ok(rg_match_total(&String::from_utf8_lossy(&out.stdout)))
}

fn rg_floor(
    args: &Args,
    q: &Queries,
    drop: &dyn CacheDrop,
    notes: &mut Vec<String>,
) -> Result<(Vec<Metric>, Vec<QuerySlot>)> {
    let run = &args.suite.run;
    let size = &args.size;
    let root = args.corpus_root.display().to_string();
    let mut names: Vec<String> = q.fixed.clone();
    for picked in queries::pick(&args.corpus_root, q.picked, run.seed)? {
        if !names.contains(&picked.name) {
            names.push(picked.name);
        }
    }
    let mut metrics = Vec::new();
    let mut slots = Vec::new();
    let (mut p50s, mut p95s, mut rss) = (Vec::new(), Vec::new(), Vec::new());
    let series = |base: String,
                  argv: Vec<String>,
                  cwd: Option<PathBuf>,
                  metrics: &mut Vec<Metric>|
     -> Result<(f64, f64, Option<f64>)> {
        let count_dir = cwd.clone().unwrap_or_else(|| args.corpus_root.clone());
        metrics.push(Metric::single(
            format!("{base}/matches"),
            "count",
            Better::Info,
            match_count(&argv, &count_dir)? as f64,
        ));
        let t = hyperfine::run(
            &hyperfine::Spec {
                argv,
                cwd,
                warmup: run.warmup,
                runs: run.runs,
                ok_exit_codes: vec![0, 1],
                prepare: None,
                env: Vec::new(),
            },
            &args.scratch,
        )?;
        let rss_mib: Vec<f64> = t.peak_rss_kib.iter().map(|k| kib_to_mib(*k)).collect();
        let peak = (!rss_mib.is_empty()).then(|| stats::percentile(&rss_mib, 1.0));
        if !rss_mib.is_empty() {
            metrics.push(Metric::max_of(
                format!("{base}/rss_peak"),
                "MiB",
                Better::Smaller,
                rss_mib,
            ));
        }
        let p50 = Metric::median_of(
            format!("{base}/p50"),
            "ms",
            Better::Smaller,
            t.times_ms.clone(),
        );
        let p95 = Metric::percentile_of(
            format!("{base}/p95"),
            "ms",
            Better::Smaller,
            t.times_ms,
            0.95,
            "p95",
        );
        let out = (p50.value, p95.value, peak);
        metrics.push(p50);
        metrics.push(p95);
        Ok(out)
    };
    for (i, name) in names.iter().enumerate() {
        slots.push(QuerySlot {
            slot: slot(i),
            name_blake3: name_digest(name),
        });
        let base = format!("rg-floor/{size}/{}", slot(i));
        let (p50, p95, peak) = series(base, rg_argv(name, true, &root), None, &mut metrics)?;
        p50s.push(p50);
        p95s.push(p95);
        rss.extend(peak);
    }
    // The spike's own command form, `rg -c -t java '\bString\b' .` run in the checkout, so the
    // comparison with doc/bench-corpus.md's floor is made with the same arguments.
    for (i, pattern) in q.spike_form.iter().enumerate() {
        let base = format!("rg-floor/{size}/spike{i:02}");
        series(
            base,
            rg_argv(pattern, false, "."),
            Some(args.corpus_root.clone()),
            &mut metrics,
        )?;
    }
    let n = names.len();
    metrics.push(Metric::median_over(
        format!("rg-floor/{size}/p50"),
        "ms",
        Better::Smaller,
        p50s,
        "queries",
    ));
    metrics.push(Metric::median_over(
        format!("rg-floor/{size}/p95"),
        "ms",
        Better::Smaller,
        p95s,
        "queries",
    ));
    if rss.is_empty() {
        notes.push(
            "rss: hyperfine reported no memory_peak_resident; peak RSS not measured".to_string(),
        );
    } else {
        metrics.push(Metric::median_over(
            format!("rg-floor/{size}/rss_peak"),
            "MiB",
            Better::Smaller,
            rss,
            "queries",
        ));
    }
    if let (Some(first), true) = (names.first(), run.cold_runs > 0) {
        match drop.drop_caches() {
            Err(e) => notes.push(format!("cold runs skipped: {e}")),
            Ok(()) => {
                let t = hyperfine::run(
                    &hyperfine::Spec {
                        argv: rg_argv(first, true, &root),
                        cwd: None,
                        warmup: 0,
                        runs: run.cold_runs,
                        ok_exit_codes: vec![0, 1],
                        prepare: Some(DROP_CACHES_COMMAND.to_string()),
                        env: Vec::new(),
                    },
                    &args.scratch,
                )?;
                metrics.push(Metric::median_of(
                    format!("rg-floor/{size}/cold_ms"),
                    "ms",
                    Better::Smaller,
                    t.times_ms,
                ));
            }
        }
    }
    let _ = n;
    Ok((metrics, slots))
}

/// `n` measurements, each after a page-cache drop. The first failed drop ends it with its error
/// and nothing is measured.
fn cold_runs<T>(
    drop: &dyn CacheDrop,
    n: u32,
    mut measure: impl FnMut() -> Result<T>,
) -> Result<Vec<T>> {
    let mut out = Vec::new();
    for _ in 0..n {
        drop.drop_caches()?;
        out.push(measure()?);
    }
    Ok(out)
}

fn run_parse(exe: &Path, root: &Path, threads: usize, upto: Stage) -> Result<RunResult> {
    let out = Command::new(exe)
        .arg("parse")
        .arg(root)
        .args(["--threads", &threads.to_string(), "--upto", upto.as_str()])
        .output()
        .context(format!("run {} parse", exe.display()))?;
    if !out.status.success() {
        return Err(BenchError::new(format!(
            "parse --upto {} failed: {}",
            upto.as_str(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    serde_json::from_slice(&out.stdout).context("parse the run's result line")
}

fn series(rs: &[RunResult], f: impl Fn(&RunResult) -> f64) -> Vec<f64> {
    rs.iter().map(f).collect()
}

fn e1_suite(
    args: &Args,
    e: &E1,
    drop: &dyn CacheDrop,
    notes: &mut Vec<String>,
) -> Result<Vec<Metric>> {
    let run = &args.suite.run;
    let size = &args.size;
    let last = e.stages.last().copied().unwrap_or(Stage::Extract);
    let mut metrics = Vec::new();
    let mut rss_seen = false;
    for &threads in &e.threads {
        let base = format!("e1/{size}/t{threads}");
        for _ in 0..run.warmup {
            run_parse(&args.exe, &args.corpus_root, threads, last)?;
        }
        for &stage in &e.stages {
            let rs: Vec<RunResult> = (0..run.runs)
                .map(|_| run_parse(&args.exe, &args.corpus_root, threads, stage))
                .collect::<Result<_>>()?;
            let name = |f: &str| format!("{base}/{}/{f}", stage.as_str());
            let add = |metrics: &mut Vec<Metric>, f: &str, unit: &str, better, vals| {
                metrics.push(Metric::median_of(name(f), unit, better, vals));
            };
            add(
                &mut metrics,
                "work_ms",
                "ms",
                Better::Smaller,
                series(&rs, |r| r.work_ms),
            );
            add(
                &mut metrics,
                "total_ms",
                "ms",
                Better::Smaller,
                series(&rs, |r| r.total_ms),
            );
            if let Some(first) = rs.first()
                && rs.iter().any(|r| {
                    (r.files, r.decls, r.checksum) != (first.files, first.decls, first.checksum)
                })
            {
                notes.push(format!(
                    "t{threads} {}: files, declarations or checksum differ between runs",
                    stage.as_str()
                ));
            }
            if stage != last {
                continue;
            }
            add(
                &mut metrics,
                "list_ms",
                "ms",
                Better::Smaller,
                series(&rs, |r| r.list_ms),
            );
            add(
                &mut metrics,
                "files_per_s",
                "files/s",
                Better::Larger,
                series(&rs, |r| r.files_per_s),
            );
            add(
                &mut metrics,
                "mb_per_s",
                "MB/s",
                Better::Larger,
                series(&rs, |r| r.mb_per_s),
            );
            add(
                &mut metrics,
                "file_p50_ms",
                "ms",
                Better::Smaller,
                series(&rs, |r| r.p50_ms),
            );
            add(
                &mut metrics,
                "file_p95_ms",
                "ms",
                Better::Smaller,
                series(&rs, |r| r.p95_ms),
            );
            add(
                &mut metrics,
                "file_max_ms",
                "ms",
                Better::Smaller,
                series(&rs, |r| r.max_ms),
            );
            let peaks: Vec<f64> = rs.iter().filter_map(|r| r.rss_peak_kib).map(mib).collect();
            let ends: Vec<f64> = rs.iter().filter_map(|r| r.rss_end_kib).map(mib).collect();
            if !peaks.is_empty() {
                rss_seen = true;
                add(&mut metrics, "rss_peak_mib", "MiB", Better::Smaller, peaks);
                add(&mut metrics, "rss_steady_mib", "MiB", Better::Smaller, ends);
            }
            if let Some(f) = rs.first() {
                for (field, v) in [
                    ("files", f.files),
                    ("err_files", f.err_files),
                    ("failed_files", f.failed_files),
                    ("decls", f.decls),
                ] {
                    metrics.push(Metric::single(name(field), "count", Better::Info, v as f64));
                }
            }
        }
    }
    if !rss_seen {
        notes.push("rss: no /proc on this platform; peak RSS not measured".to_string());
    }
    if run.cold_runs > 0 && !e.cold_stages.is_empty() {
        match e1_cold(args, e, drop) {
            Ok(cold) => metrics.extend(cold),
            Err(err) => notes.push(format!("cold runs skipped: {err}")),
        }
    }
    Ok(metrics)
}

fn e1_cold(args: &Args, e: &E1, drop: &dyn CacheDrop) -> Result<Vec<Metric>> {
    let size = &args.size;
    let mut metrics = Vec::new();
    for &threads in &e.threads {
        for &stage in &e.cold_stages {
            let rs = cold_runs(drop, args.suite.run.cold_runs, || {
                run_parse(&args.exe, &args.corpus_root, threads, stage)
            })?;
            let base = format!("e1/{size}/t{threads}/cold/{}", stage.as_str());
            metrics.push(Metric::median_of(
                format!("{base}/work_ms"),
                "ms",
                Better::Smaller,
                series(&rs, |r| r.work_ms),
            ));
            metrics.push(Metric::median_of(
                format!("{base}/total_ms"),
                "ms",
                Better::Smaller,
                series(&rs, |r| r.total_ms),
            ));
        }
    }
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jdtls_args(size: &str, scratch: &Path) -> Args {
        Args {
            suite: Suite::load("jdtls").unwrap(),
            size: size.to_string(),
            corpus_root: scratch.join("corpus"),
            repo_root: PathBuf::from("."),
            exe: scratch.join("jfl-bench"),
            scratch: scratch.join("scratch"),
            kmp_lsp: None,
            jdtls_home: Some(scratch.join("no-such-jdtls")),
            trusted_build: false,
        }
    }

    #[test]
    fn should_refuse_a_corpus_checkout_when_the_jdtls_arm_is_not_trusted() {
        // given: a corpus size, no --trusted-build, and no jdtls that could be started
        let scratch = std::env::temp_dir().join(format!("jfl-bench-refuse-{}", std::process::id()));
        let args = jdtls_args("medium", &scratch);
        let j = args.suite.jdtls.clone().unwrap();
        let mut notes = Vec::new();
        // when
        let err = jdtls_suite(&args, &j, &mut notes).unwrap_err();
        // then: refused before anything is copied or started
        assert!(
            err.to_string().contains("never runs in the sandbox"),
            "{err}"
        );
        assert!(err.to_string().contains("--trusted-build"), "{err}");
        assert!(!args.scratch.exists());
        assert!(notes.is_empty());
    }

    #[test]
    fn should_sum_the_counts_when_rg_prints_one_line_per_file() {
        // given
        let out = "/c/a/A.java:3\n/c/b/B.java:12\n/c/odd:name/C.java:1\n";
        // when
        let total = rg_match_total(out);
        // then
        assert_eq!(total, 16);
        assert_eq!(rg_match_total(""), 0);
    }

    struct FailingDrop;

    impl CacheDrop for FailingDrop {
        fn drop_caches(&self) -> Result<()> {
            Err(BenchError::new("not permitted"))
        }
    }

    struct CountingDrop(std::cell::Cell<u32>);

    impl CacheDrop for CountingDrop {
        fn drop_caches(&self) -> Result<()> {
            self.0.set(self.0.get().saturating_add(1));
            Ok(())
        }
    }

    #[test]
    fn should_search_whole_words_in_java_files_without_config_or_global_ignores_when_building_the_rg_command()
     {
        // given
        // when
        let argv = rg_argv("Widget", true, "/corpus/junit");
        // then
        assert_eq!(
            argv,
            [
                "rg",
                "-c",
                "-w",
                "-t",
                "java",
                "--no-messages",
                "--no-config",
                "--no-ignore-global",
                "--no-ignore-parent",
                "--no-ignore-exclude",
                "Widget",
                "/corpus/junit"
            ]
        );
        assert!(!rg_argv("\\bString\\b", false, ".").contains(&"-w".to_string()));
    }

    #[test]
    fn should_name_slots_and_hide_names_when_keying_series() {
        // given
        // when
        // then
        assert_eq!(slot(0), "q00");
        assert_eq!(slot(19), "q19");
        assert_eq!(name_digest("String").len(), 16);
        assert_ne!(name_digest("String"), name_digest("Widget"));
    }

    #[test]
    fn should_measure_nothing_and_return_the_error_when_the_cache_cannot_be_dropped() {
        // given
        let mut measured = 0;
        // when
        let result = cold_runs(&FailingDrop, 3, || {
            measured += 1;
            Ok(1.0)
        });
        // then
        assert!(result.unwrap_err().to_string().contains("not permitted"));
        assert_eq!(measured, 0);
    }

    #[test]
    fn should_drop_before_every_measurement_when_the_cache_can_be_dropped() {
        // given
        let drop = CountingDrop(std::cell::Cell::new(0));
        // when
        let result = cold_runs(&drop, 3, || Ok(2.0)).unwrap();
        // then
        assert_eq!(result, [2.0, 2.0, 2.0]);
        assert_eq!(drop.0.get(), 3);
    }
}
