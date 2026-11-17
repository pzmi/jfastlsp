//! The comparison arms beside the `rg` floor (BENCH-03, ADR 0031, plan §1.3 and §1.7): kmp-lsp
//! at a pinned commit, timed end to end through hyperfine, and the pinned stock jdtls (ADR 0028,
//! amended 2026-11-17), driven over LSP. Neither is linked: each is a separate program the
//! harness starts, so the harness stays independent of what it measures.
//!
//! - **kmp-lsp** (`scripts/kmp-lsp-fetch.sh` builds it): `kmp-lsp index` with an empty cache (a
//!   full syntax index, the §1.7 row's arm) and with its cache (its warm reopen), and its `refs`
//!   and `find` commands for the same seed-picked names as `rg-floor`. Its cache directory is a
//!   scratch `XDG_CACHE_HOME`, emptied by hyperfine's `--prepare` for the cold-cache series. It
//!   needs no build of the corpus, so it runs on every size.
//! - **jdtls** (started by [`crate::jdtls`]: a fresh `-data` per run, a heap cap, under a
//!   deadline): `initialize`, the time until jdtls reports `ServiceReady`, then
//!   `definition`, `references`, `hover` and `workspace/symbol` at usage sites of seed-picked
//!   types, and the backend JVM's resident set. A jdtls import runs the project's build, so in
//!   the sandbox it runs only on the synthetic multi-module fixture with the Gradle and Maven
//!   imports off and the fixture's source roots given as `java.project.sourcePaths`; on a corpus
//!   checkout it is refused unless `--trusted-build` (a `bench.yml` dispatch job or an owner
//!   run), which leaves the Gradle import on.
//!
//! Pinned by: `tests/lsp_arm.rs` (usage sites on the fixture), this module's unit tests (the
//! kmp-lsp command lines, the jdtls initialization options, heap parsing, kmp-lsp exit codes),
//! `jdtls`'s tests (the jdtls command line and its refusals) and
//! `runner::tests::should_refuse_a_corpus_checkout_when_the_jdtls_arm_is_not_trusted` (the
//! sandbox refusal).

use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Instant;

use serde_json::{Value, json};

use crate::e1;
use crate::error::{BenchError, Context, Result};
use crate::hyperfine;
use crate::jdtls;
use crate::lsp::{self, Client};
use crate::procfs;
use crate::queries::{self, Query};
use crate::report::{Better, Metric, QuerySlot};
use crate::suite::{Jdtls, Kmp};

/// The kmp-lsp release `scripts/kmp-lsp-fetch.sh` builds.
pub const KMP_LSP_VERSION: &str = "0.27.0";

/// Where `scripts/kmp-lsp-fetch.sh` puts the binary by default.
pub fn default_kmp_lsp() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join(format!(
        "jfastlsp/arms/kmp-lsp-{KMP_LSP_VERSION}/bin/kmp-lsp"
    )))
}

/// A query position at a use of a picked type, in LSP terms (zero-based line, UTF-16 column).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Site {
    /// The type's simple name (looked up at bench time, never written into a report).
    pub name: String,
    /// The file holding the use.
    pub file: PathBuf,
    /// Zero-based line.
    pub line: u32,
    /// Zero-based UTF-16 column of the name's first character.
    pub character: u32,
    /// Whether no use was found and this is the declaration itself.
    pub at_declaration: bool,
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// The byte offset of `word` as a whole identifier in `line`, if any.
fn word_at(line: &str, word: &str) -> Option<usize> {
    let mut from = 0usize;
    while let Some(i) = line.get(from..)?.find(word) {
        let at = from.saturating_add(i);
        let end = at.saturating_add(word.len());
        let before = line.get(..at)?.chars().next_back();
        let after = line.get(end..)?.chars().next();
        if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) {
            return Some(at);
        }
        from = end;
    }
    None
}

fn utf16_len(s: &str) -> u32 {
    u32::try_from(s.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// The first use of `name` outside its declaring file: the first file in sorted order, the first
/// code line naming it (package, import and comment lines skipped).
fn first_use(files: &[PathBuf], declaring: &Path, name: &str) -> Option<(PathBuf, u32, u32)> {
    for f in files.iter().filter(|f| f.as_path() != declaring) {
        let Ok(Some(bytes)) = e1::read_capped(f) else {
            continue;
        };
        let text = String::from_utf8_lossy(&bytes);
        for (n, line) in text.lines().enumerate() {
            let t = line.trim_start();
            let skip = ["package ", "import ", "//", "*", "/*"]
                .iter()
                .any(|p| t.starts_with(p));
            if skip {
                continue;
            }
            if let Some(at) = word_at(line, name) {
                let col = utf16_len(line.get(..at).unwrap_or(""));
                return Some((f.clone(), u32::try_from(n).unwrap_or(u32::MAX), col));
            }
        }
    }
    None
}

/// A usage site for each query: where an agent would ask "what is this?". A type used nowhere
/// else is queried at its declaration, and the site says so.
pub fn usage_sites(root: &Path, picked: &[Query]) -> Vec<Site> {
    let files = e1::list_java(root);
    picked
        .iter()
        .map(|q| {
            let declaring = root.join(&q.file);
            match first_use(&files, &declaring, &q.name) {
                Some((file, line, character)) => Site {
                    name: q.name.clone(),
                    file,
                    line,
                    character,
                    at_declaration: false,
                },
                None => Site {
                    name: q.name.clone(),
                    file: declaring,
                    line: q.line,
                    character: q.column,
                    at_declaration: true,
                },
            }
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

/// The kmp-lsp command line for `op` (`index`, or `refs`/`find` with a name).
pub fn kmp_argv(kmp: &Path, root: &Path, op: &str, name: Option<&str>) -> Vec<String> {
    let mut argv = vec![kmp.display().to_string(), op.to_string()];
    argv.extend(name.map(str::to_string));
    argv.push("--root".to_string());
    argv.push(root.display().to_string());
    argv
}

/// Lines a `refs` or `find` run prints (one per result), from one untimed run. Exit 0 with
/// output, or exit 1 with none (kmp-lsp's "nothing found"), is a result; anything else, exit 1
/// with output or text on stderr included, fails the run, so an error is never timed as a query.
fn kmp_matches(argv: &[String], env: &[(String, String)]) -> Result<u64> {
    let (program, rest) = argv.split_first().context("empty kmp-lsp command")?;
    let out = Command::new(program)
        .args(rest)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .output()
        .context(format!("run {program}"))?;
    let lines = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count() as u64;
    classify_kmp(
        out.status.code(),
        lines,
        &String::from_utf8_lossy(&out.stderr),
    )
}

/// See [`kmp_matches`]: kmp-lsp's own log lines (`INFO`) on stderr are not errors.
pub fn classify_kmp(code: Option<i32>, lines: u64, stderr: &str) -> Result<u64> {
    let errors: Vec<&str> = stderr
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.contains(" INFO ") && !l.contains(" WARN "))
        .collect();
    match code {
        Some(0) if errors.is_empty() => Ok(lines),
        Some(1) if lines == 0 && errors.iter().all(|l| l.starts_with("No ")) => Ok(0),
        _ => Err(BenchError::new(format!(
            "kmp-lsp exited with {code:?}, {lines} result lines: {}",
            errors.join(" | ")
        ))),
    }
}

/// What the kmp-lsp arm needs besides the suite.
pub struct KmpRun<'a> {
    /// The corpus size name.
    pub size: &'a str,
    /// The corpus checkout.
    pub root: &'a Path,
    /// The kmp-lsp binary.
    pub kmp: &'a Path,
    /// Scratch space: hyperfine's export and kmp-lsp's cache.
    pub scratch: &'a Path,
    /// Untimed and timed repetitions, and the query seed.
    pub warmup: u32,
    /// Timed repetitions.
    pub runs: u32,
    /// Seed for the picked names.
    pub seed: u64,
}

fn kib_to_mib(kib: f64) -> f64 {
    kib / 1024.0
}

/// The kmp-lsp arm: index with an empty and with a warm cache, then `refs` and `find` per name.
pub fn kmp_suite(
    r: &KmpRun<'_>,
    k: &Kmp,
    notes: &mut Vec<String>,
) -> Result<(Vec<Metric>, Vec<QuerySlot>)> {
    if !r.kmp.is_file() {
        return Err(BenchError::new(format!(
            "no kmp-lsp at {}: run `bash scripts/kmp-lsp-fetch.sh` or pass --kmp-lsp PATH",
            r.kmp.display()
        )));
    }
    let cache = r.scratch.join("kmp-cache");
    std::fs::create_dir_all(&cache).context(format!("create {}", cache.display()))?;
    let env = vec![("XDG_CACHE_HOME".to_string(), cache.display().to_string())];
    let size = r.size;
    // `refs` and `find` exit 1 when nothing matches; `index` must succeed.
    let spec = |argv: Vec<String>, warmup: u32, runs: u32, prepare: Option<String>| {
        let ok_exit_codes = if argv.get(1).is_some_and(|op| op == "index") {
            vec![0]
        } else {
            vec![0, 1]
        };
        hyperfine::Spec {
            argv,
            cwd: None,
            warmup,
            runs,
            ok_exit_codes,
            prepare,
            env: env.clone(),
        }
    };
    let mut metrics = Vec::new();
    let empty_cache = format!(
        "rm -rf {}",
        hyperfine::quote(&cache.join("kmp-lsp").display().to_string())
    );
    let cold = hyperfine::run(
        &spec(
            kmp_argv(r.kmp, r.root, "index", None),
            0,
            k.index_runs.max(1),
            Some(empty_cache),
        ),
        r.scratch,
    )?;
    metrics.push(Metric::median_of(
        format!("kmp-lsp/{size}/index_ms"),
        "ms",
        Better::Smaller,
        cold.times_ms,
    ));
    let rss: Vec<f64> = cold.peak_rss_kib.iter().map(|k| kib_to_mib(*k)).collect();
    if !rss.is_empty() {
        metrics.push(Metric::max_of(
            format!("kmp-lsp/{size}/index_rss_peak"),
            "MiB",
            Better::Smaller,
            rss,
        ));
    }
    let warm = hyperfine::run(
        &spec(kmp_argv(r.kmp, r.root, "index", None), 1, r.runs, None),
        r.scratch,
    )?;
    metrics.push(Metric::median_of(
        format!("kmp-lsp/{size}/index_cached_ms"),
        "ms",
        Better::Smaller,
        warm.times_ms,
    ));
    let mut slots = Vec::new();
    let picked = queries::pick(r.root, k.picked, r.seed)?;
    for op in ["refs", "find"] {
        let (mut p50s, mut p95s, mut rss) = (Vec::new(), Vec::new(), Vec::new());
        for (i, q) in picked.iter().enumerate() {
            if op == "refs" {
                slots.push(QuerySlot {
                    slot: format!("q{i:02}"),
                    name_blake3: digest(&q.name),
                });
            }
            let argv = kmp_argv(r.kmp, r.root, op, Some(&q.name));
            let matches = kmp_matches(&argv, &env)?;
            metrics.push(Metric::single(
                format!("kmp-lsp/{size}/{op}/q{i:02}/matches"),
                "count",
                Better::Info,
                matches as f64,
            ));
            if matches == 0 {
                notes.push(format!(
                    "kmp-lsp {op} q{i:02}: no results (exit 1, empty output)"
                ));
            }
            let t = hyperfine::run(&spec(argv, r.warmup, r.runs, None), r.scratch)?;
            p50s.push(crate::stats::median(&t.times_ms));
            p95s.push(crate::stats::percentile(&t.times_ms, 0.95));
            rss.extend(t.peak_rss_kib.iter().map(|k| kib_to_mib(*k)));
        }
        metrics.push(Metric::median_over(
            format!("kmp-lsp/{size}/{op}/p50"),
            "ms",
            Better::Smaller,
            p50s,
            "queries",
        ));
        metrics.push(Metric::median_over(
            format!("kmp-lsp/{size}/{op}/p95"),
            "ms",
            Better::Smaller,
            p95s,
            "queries",
        ));
        if !rss.is_empty() {
            metrics.push(Metric::max_of(
                format!("kmp-lsp/{size}/{op}/rss_peak"),
                "MiB",
                Better::Smaller,
                rss,
            ));
        }
    }
    notes.push(format!(
        "kmp-lsp {KMP_LSP_VERSION} (scripts/kmp-lsp-fetch.sh): its index also covers Kotlin and other sources it finds, and it walks with its own rules (fd if present, else a walk), so its file set is not exactly e1's"
    ));
    if crate::machine::find_on_path("fd").is_none()
        && crate::machine::find_on_path("fdfind").is_none()
    {
        notes.push("kmp-lsp: no fd on PATH; it listed files with its built-in walk".to_string());
    }
    Ok((metrics, slots))
}

/// The `java.project.sourcePaths` of a checkout without a build file: every `src/*/java`
/// directory, relative to the root.
pub fn source_paths(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = e1::list_java(root)
        .iter()
        .filter_map(|f| {
            let rel = f.strip_prefix(root).ok()?;
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let i = parts.windows(3).position(|w| {
                w.first().is_some_and(|a| a == "src") && w.get(2).is_some_and(|c| c == "java")
            })?;
            Some(parts.get(..i.saturating_add(3))?.join("/"))
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// What the jdtls arm needs besides the suite.
pub struct JdtlsRun<'a> {
    /// The corpus size name, or `fixture`.
    pub size: &'a str,
    /// The checkout jdtls opens (a scratch copy of the fixture).
    pub root: &'a Path,
    /// The verified jdtls install ([`jdtls::launcher`] checks it).
    pub jdtls_home: &'a Path,
    /// The `java` that runs it.
    pub java: &'a Path,
    /// Scratch space: jdtls's fresh `-data` and configuration, and its stderr.
    pub scratch: &'a Path,
    /// Whether jdtls may import the project's build (a corpus checkout, owner or dispatch only).
    pub trusted_build: bool,
    /// Untimed and timed rounds over the sites.
    pub warmup: u32,
    /// Timed rounds.
    pub runs: u32,
    /// Seed for the picked types.
    pub seed: u64,
}

/// `initializationOptions` for the arm: in the sandbox the imports are off and the source roots
/// are given; a trusted run leaves jdtls's import to its defaults.
pub fn jdtls_init_options(r: &JdtlsRun<'_>) -> Value {
    if r.trusted_build {
        return json!({});
    }
    json!({"settings": {"java": {
        "import": {"gradle": {"enabled": false}, "maven": {"enabled": false}},
        "project": {"sourcePaths": source_paths(r.root)}
    }}})
}

/// The `timeout` that runs jdtls. Ended normally by [`Session::wait`] once jdtls has exited; if
/// the arm unwinds first, `Drop` sends `timeout` a SIGTERM, which it passes on to jdtls (a SIGKILL
/// would end `timeout` alone and orphan the JVM), then waits, bounded by `--kill-after`.
struct Session {
    child: Option<Child>,
}

impl Session {
    fn wait(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .status();
            let _ = child.wait();
        }
    }
}

/// How long a helper call (`jcmd`) may take before `timeout` ends it.
const HELPER_DEADLINE: &str = "30s";

/// Run `program args` under `timeout`, so a helper that hangs cannot hang the harness; its
/// stdout when it exits 0.
fn bounded(program: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("timeout")
        .args(["--kill-after=5s", HELPER_DEADLINE])
        .arg(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(out.stdout)
}

/// The Java heap from `jcmd <pid> GC.heap_info`: (used, committed) KiB summed over the heap's
/// generations (the lines with `total NK, used NK`; Metaspace and class space are not heap).
pub fn parse_heap_info(text: &str) -> Option<(u64, u64)> {
    let kib = |s: &str| s.trim().strip_suffix('K')?.trim().parse::<u64>().ok();
    let mut seen = false;
    let (mut used, mut total) = (0u64, 0u64);
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(" total ") else {
            continue;
        };
        let Some((t, after)) = rest.split_once(", used ") else {
            continue;
        };
        let u = after.split_whitespace().next()?;
        total = total.saturating_add(kib(t)?);
        used = used.saturating_add(kib(u)?);
        seen = true;
    }
    seen.then_some((used, total))
}

fn position(site: &Site) -> Value {
    json!({
        "textDocument": {"uri": lsp::file_uri(&site.file)},
        "position": {"line": site.line, "character": site.character}
    })
}

/// The four operations at a site: method and params.
fn operations(site: &Site) -> [(&'static str, &'static str, Value); 4] {
    let mut refs = position(site);
    if let Some(m) = refs.as_object_mut() {
        m.insert("context".into(), json!({"includeDeclaration": false}));
    }
    [
        ("definition", "textDocument/definition", position(site)),
        ("references", "textDocument/references", refs),
        ("hover", "textDocument/hover", position(site)),
        ("symbol", "workspace/symbol", json!({"query": site.name})),
    ]
}

/// The jdtls arm: one session of stock jdtls from `initialize` to `shutdown` and `exit`.
pub fn jdtls_suite(
    r: &JdtlsRun<'_>,
    j: &Jdtls,
    notes: &mut Vec<String>,
) -> Result<(Vec<Metric>, Vec<QuerySlot>)> {
    let launcher = jdtls::launcher(r.jdtls_home)?;
    let picked = queries::pick(r.root, j.sites, r.seed)?;
    let sites = usage_sites(r.root, &picked);
    let slots: Vec<QuerySlot> = sites
        .iter()
        .enumerate()
        .map(|(i, s)| QuerySlot {
            slot: format!("s{i:02}"),
            name_blake3: digest(&s.name),
        })
        .collect();
    let at_decl = sites.iter().filter(|s| s.at_declaration).count();
    if at_decl > 0 {
        notes.push(format!(
            "jdtls: {at_decl} of {} sites are declarations (no use found elsewhere)",
            sites.len()
        ));
    }
    let data = r.scratch.join("jdtls-data");
    let configuration = r.scratch.join("jdtls-configuration");
    jdtls::fresh_dir(&data)?;
    jdtls::fresh_dir(&configuration)?;
    let launch = jdtls::Launch {
        java: r.java,
        home: r.jdtls_home,
        launcher: &launcher,
        data: &data,
        configuration: &configuration,
        heap_mib: j.heap_mib,
        deadline_secs: j.deadline_secs,
    };
    let stderr = std::fs::File::create(r.scratch.join("jdtls.stderr"))
        .context("create jdtls's stderr file")?;
    let t0 = Instant::now();
    let mut child = jdtls::command(&launch, r.root)?
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()
        .context("start jdtls under timeout (coreutils `timeout` must be on PATH)")?;
    let timeout_pid = child.id();
    let stdin = child.stdin.take().context("jdtls's stdin")?;
    let stdout = child.stdout.take().context("jdtls's stdout")?;
    let session = Session { child: Some(child) };
    let mut client = Client::new(BufReader::new(stdout), stdin);
    let result = drive(r, j, &sites, &mut client, t0, timeout_pid, notes);
    let _ = client.request("shutdown", Value::Null);
    let _ = client.notify("exit", Value::Null);
    // jdtls exits on `exit` (or at its stdin's end); wait for it, bounded by `timeout`.
    drop(client);
    session.wait();
    Ok((result?, slots))
}

fn drive<R: std::io::BufRead, W: std::io::Write>(
    r: &JdtlsRun<'_>,
    j: &Jdtls,
    sites: &[Site],
    client: &mut Client<R, W>,
    t0: Instant,
    timeout_pid: u32,
    notes: &mut Vec<String>,
) -> Result<Vec<Metric>> {
    let size = r.size;
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
    let root_uri = lsp::file_uri(r.root);
    let (caps, _) = client.request(
        "initialize",
        json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            "rootPath": r.root.to_string_lossy(),
            "workspaceFolders": [{
                "uri": root_uri,
                "name": r.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            }],
            "capabilities": {},
            "initializationOptions": jdtls_init_options(r),
        }),
    )?;
    let initialize_ms = ms(t0);
    if caps.get("capabilities").is_none() {
        return Err(BenchError::new("initialize answered without capabilities"));
    }
    client.notify("initialized", json!({}))?;
    let mut opened: Vec<&Path> = Vec::new();
    for site in sites {
        if opened.contains(&site.file.as_path()) {
            continue;
        }
        let text = e1::read_capped(&site.file)
            .context(format!("read {}", site.file.display()))?
            .context(format!("{} is over the size cap", site.file.display()))?;
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": lsp::file_uri(&site.file),
                "languageId": "java",
                "version": 1,
                "text": String::from_utf8_lossy(&text),
            }}),
        )?;
        opened.push(&site.file);
    }
    client.wait_notification(|m, p| {
        m == "language/status" && p.get("type").and_then(Value::as_str) == Some("ServiceReady")
    })?;
    let ready_ms = ms(t0);
    let mut metrics = vec![
        Metric::single(
            format!("jdtls/{size}/initialize_ms"),
            "ms",
            Better::Smaller,
            initialize_ms,
        ),
        Metric::single(
            format!("jdtls/{size}/ready_ms"),
            "ms",
            Better::Smaller,
            ready_ms,
        ),
    ];
    let names = ["definition", "references", "hover", "symbol"];
    let mut times: Vec<Vec<f64>> = vec![Vec::new(); names.len()];
    let mut found = vec![0u64; names.len()];
    for round in 0..r.warmup.saturating_add(r.runs) {
        for site in sites {
            for (k, (_, method, params)) in operations(site).into_iter().enumerate() {
                let (result, elapsed) = client.request(method, params)?;
                if round == r.warmup
                    && lsp::is_found(&result)
                    && let Some(f) = found.get_mut(k)
                {
                    *f = f.saturating_add(1);
                }
                if round >= r.warmup
                    && let Some(t) = times.get_mut(k)
                {
                    t.push(elapsed);
                }
            }
        }
    }
    let what = format!("requests ({} sites x {} rounds)", sites.len(), r.runs);
    for (k, op) in names.iter().enumerate() {
        let values = times.get(k).cloned().unwrap_or_default();
        for (label, p) in [("p50", 0.5), ("p95", 0.95)] {
            metrics.push(Metric::percentile_over(
                format!("jdtls/{size}/{op}/{label}"),
                "ms",
                Better::Smaller,
                values.clone(),
                p,
                label,
                &what,
            ));
        }
        metrics.push(Metric::single(
            format!("jdtls/{size}/{op}/found"),
            "count",
            Better::Info,
            found.get(k).copied().unwrap_or(0) as f64,
        ));
    }
    // The JVM is the one child of the `timeout` that runs it (Linux `/proc`; elsewhere neither
    // its heap nor its RSS is measured, and the notes say so).
    let pid = crate::procfs::children(timeout_pid).and_then(|c| c.first().copied());
    // RSS alone overstates what jdtls needs: the JVM starts with its initial heap (-Xms =
    // min(heap cap, 1024 MiB)) resident or not. The heap actually used is the number to compare
    // with an index's memory.
    let heap = crate::machine::find_on_path("jcmd").and_then(|jcmd| {
        let text = bounded(&jcmd, &[&pid?.to_string(), "GC.heap_info"])?;
        parse_heap_info(&String::from_utf8_lossy(&text))
    });
    match heap {
        Some((used, committed)) => {
            metrics.push(Metric::single(
                format!("jdtls/{size}/backend_heap_used"),
                "MiB",
                Better::Smaller,
                used as f64 / 1024.0,
            ));
            metrics.push(Metric::single(
                format!("jdtls/{size}/backend_heap_committed"),
                "MiB",
                Better::Smaller,
                committed as f64 / 1024.0,
            ));
        }
        None => notes.push(
            "jdtls: no jcmd GC.heap_info (jcmd absent or refused); heap use not measured"
                .to_string(),
        ),
    }
    match pid.and_then(|pid| procfs::read_status(Some(pid))) {
        Some(mem) => {
            if let Some(peak) = mem.peak_kib {
                metrics.push(Metric::single(
                    format!("jdtls/{size}/backend_rss_peak"),
                    "MiB",
                    Better::Smaller,
                    peak as f64 / 1024.0,
                ));
            }
            if let Some(now) = mem.rss_kib {
                metrics.push(Metric::single(
                    format!("jdtls/{size}/backend_rss"),
                    "MiB",
                    Better::Smaller,
                    now as f64 / 1024.0,
                ));
            }
        }
        None => notes.push(
            "jdtls: the backend's pid or /proc status was not available; its RSS is not measured"
                .to_string(),
        ),
    }
    notes.push(format!(
        "stock jdtls {} with a fresh -data, heap -Xms{} -Xmx{} MiB (its RSS is bounded by that configuration, not by need: compare backend_heap_used), {}; one session, so initialize and ready are single measurements",
        jdtls::JDTLS_VERSION,
        j.heap_mib.min(1024),
        j.heap_mib,
        if r.trusted_build {
            "Gradle import on (trusted build)"
        } else {
            "Gradle and Maven imports off, the fixture's source roots as java.project.sourcePaths"
        }
    ));
    Ok(metrics)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_find_whole_words_only_when_locating_a_use() {
        // given
        let line = "    LedgerService service = new DefaultLedgerService(Ledger.x);";
        // when
        // then
        assert_eq!(word_at(line, "LedgerService"), Some(4));
        assert_eq!(word_at(line, "Ledger"), Some(53));
        assert_eq!(word_at("MyLedger", "Ledger"), None);
        assert_eq!(utf16_len("ż😀a"), 4);
    }

    #[test]
    fn should_pass_the_name_and_root_when_building_kmp_commands() {
        // given
        let kmp = Path::new("/c/kmp-lsp");
        let root = Path::new("/corpus/x");
        // when
        let refs = kmp_argv(kmp, root, "refs", Some("Widget"));
        let index = kmp_argv(kmp, root, "index", None);
        // then
        assert_eq!(
            refs,
            ["/c/kmp-lsp", "refs", "Widget", "--root", "/corpus/x"]
        );
        assert_eq!(index, ["/c/kmp-lsp", "index", "--root", "/corpus/x"]);
    }

    #[test]
    fn should_sum_the_heap_generations_and_skip_metaspace_when_parsing_heap_info() {
        // given: jcmd GC.heap_info of a ParallelGC JVM
        let text = "4242:\n PSYoungGen      total 305664K, used 104857K [0x00000000eab00000, 0x0000000100000000)\n  eden space 262144K, 40% used [0x1,0x2,0x3)\n ParOldGen       total 699392K, used 20480K [0x00000000c0000000, 0x00000000eab00000)\n  object space 699392K, 2% used [0x1,0x2,0x3)\n Metaspace       used 61440K, committed 62000K, reserved 1114112K\n  class space    used 7000K, committed 7500K, reserved 1048576K\n";
        // when
        let heap = parse_heap_info(text);
        // then
        assert_eq!(heap, Some((104_857 + 20_480, 305_664 + 699_392)));
        assert_eq!(parse_heap_info("4242:\nnothing here\n"), None);
        assert_eq!(parse_heap_info(" PSYoungGen total 12, used 3K\n"), None);
    }

    #[test]
    fn should_count_results_and_refuse_errors_when_classifying_a_kmp_run() {
        // given
        let log = "[2026-11-17T17:41:41Z INFO  kmp_lsp::indexer::cache] Cache loaded\n";
        // when
        // then
        assert_eq!(classify_kmp(Some(0), 12, log).unwrap(), 12);
        assert_eq!(
            classify_kmp(Some(1), 0, "No declarations found for 'X'\n").unwrap(),
            0
        );
        assert!(classify_kmp(Some(1), 3, "").is_err());
        assert!(classify_kmp(Some(1), 0, "error: index corrupt\n").is_err());
        assert!(classify_kmp(Some(2), 0, "").is_err());
        assert!(classify_kmp(None, 0, "").is_err());
    }

    fn run<'a>(trusted: bool, root: &'a Path) -> JdtlsRun<'a> {
        JdtlsRun {
            size: "fixture",
            root,
            jdtls_home: Path::new("/c/jdtls"),
            java: Path::new("/jdk/bin/java"),
            scratch: Path::new("/s"),
            trusted_build: trusted,
            warmup: 1,
            runs: 1,
            seed: 1,
        }
    }

    #[test]
    fn should_turn_the_build_import_off_unless_trusted_when_starting_jdtls() {
        // given
        let root = Path::new("/nonexistent-root");
        // when
        let sandbox = jdtls_init_options(&run(false, root));
        let trusted = jdtls_init_options(&run(true, root));
        // then
        let import = &sandbox["settings"]["java"]["import"];
        assert_eq!(import["gradle"]["enabled"], false);
        assert_eq!(import["maven"]["enabled"], false);
        assert_eq!(trusted, json!({}));
    }
}
