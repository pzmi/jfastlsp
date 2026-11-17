//! The command line: `run`, `compare`, and the building blocks `parse`, `queries`, `machine`
//! and `sample` that `run` drives and a person can call alone. See `doc/bench/README.md`.

use std::ffi::OsString;
use std::path::PathBuf;

use crate::corpus;
use crate::e1::{self, Stage};
use crate::error::{BenchError, Context, Result};
use crate::machine;
use crate::queries;
use crate::report::{self, Report};
use crate::runner;
use crate::sampler;
use crate::suite::Suite;

/// The noise band of `doc/bench/README.md`: the spread two runs of one suite show on the
/// reference VM class.
const DEFAULT_BAND_PCT: f64 = 20.0;

const USAGE: &str = "\
usage: jfl-bench <command>

  run <suite> [SIZE]     measure a suite on a corpus checkout (default SIZE: small)
      --corpus-path DIR    use this checkout instead of asking scripts/bench-corpus.sh
      --repo-root DIR      this repository (default: the current directory)
      --out DIR            where the JSON goes (default: target/bench)
      --runs N | --seed N  override the suite's repetitions or query seed
      --allow-debug        run a debug build anyway (refused otherwise; the report says so)
  compare A.json B.json  per-metric change between two reports [--band PCT, default 20] [--strict]
  parse ROOT             the E1 pipeline once, one JSON line [--threads N] [--upto read|hash|parse|extract]
  queries ROOT           queries picked by seed [--count N] [--seed N]
  machine                the machine facts as JSON [--corpus-path DIR] [--repo-root DIR]
  sample -- CMD ARGS...  run a command once and report its peak and steady RSS

Suites built in: rg-floor, e1; or a path to a .toml suite. Reports, never gates.";

/// Parsed `--flag value` pairs and positionals.
#[derive(Default)]
struct Parsed {
    positional: Vec<String>,
    flags: Vec<(String, String)>,
    switches: Vec<String>,
    after_dashes: Vec<OsString>,
}

fn parse_args(args: Vec<OsString>, switches: &[&str]) -> Result<Parsed> {
    let mut out = Parsed::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let s = arg.to_string_lossy().into_owned();
        if s == "--" {
            out.after_dashes = it.by_ref().collect();
            break;
        }
        if let Some(flag) = s.strip_prefix("--") {
            if switches.contains(&flag) {
                out.switches.push(flag.to_string());
            } else {
                let value = it
                    .next()
                    .ok_or_else(|| BenchError::new(format!("--{flag} needs a value")))?;
                out.flags
                    .push((flag.to_string(), value.to_string_lossy().into_owned()));
            }
        } else {
            out.positional.push(s);
        }
    }
    Ok(out)
}

impl Parsed {
    fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn number<T: std::str::FromStr>(&self, name: &str) -> Result<Option<T>> {
        self.flag(name)
            .map(|v| {
                v.parse::<T>()
                    .map_err(|_| BenchError::new(format!("--{name}: {v:?} is not a number")))
            })
            .transpose()
    }

    fn only(&self, allowed: &[&str]) -> Result<()> {
        match self
            .flags
            .iter()
            .find(|(n, _)| !allowed.contains(&n.as_str()))
        {
            Some((n, _)) => Err(BenchError::new(format!("unknown option --{n}\n\n{USAGE}"))),
            None => Ok(()),
        }
    }

    fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }
}

/// Run the command line (without the program name).
pub fn run(args: Vec<OsString>) -> Result<()> {
    let mut it = args.into_iter();
    let Some(command) = it.next() else {
        println!("{USAGE}");
        return Ok(());
    };
    let rest: Vec<OsString> = it.collect();
    match command.to_string_lossy().as_ref() {
        "run" => cmd_run(parse_args(rest, &["allow-debug"])?),
        "compare" => cmd_compare(parse_args(rest, &["strict"])?),
        "parse" => cmd_parse(parse_args(rest, &[])?),
        "queries" => cmd_queries(parse_args(rest, &[])?),
        "machine" => cmd_machine(parse_args(rest, &[])?),
        "sample" => cmd_sample(parse_args(rest, &[])?),
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(BenchError::new(format!(
            "unknown command {other:?}\n\n{USAGE}"
        ))),
    }
}

fn cmd_run(p: Parsed) -> Result<()> {
    p.only(&["corpus-path", "repo-root", "out", "runs", "seed"])?;
    if cfg!(debug_assertions) && !p.switch("allow-debug") {
        return Err(BenchError::new(
            "this is a debug build: its numbers are not measurements. Build with \
             `cargo build --profile bench-tool -p jfl-bench` (`just bench` does), or pass \
             --allow-debug to run anyway (the report is marked)",
        ));
    }
    let spec = p.positional.first().context("run needs a suite")?;
    let size = p.positional.get(1).map_or("small", String::as_str);
    let repo_root = PathBuf::from(p.flag("repo-root").unwrap_or("."));
    let mut suite = Suite::load(spec)?;
    if let Some(runs) = p.number::<u32>("runs")? {
        suite.run.runs = runs.max(1);
    }
    if let Some(seed) = p.number::<u64>("seed")? {
        suite.run.seed = seed;
    }
    let explicit = p.flag("corpus-path").map(PathBuf::from);
    let corpus_root = corpus::locate(&repo_root, size, explicit.as_deref())?;
    let out_dir = PathBuf::from(p.flag("out").unwrap_or("target/bench"));
    let scratch = std::env::temp_dir().join(format!("jfl-bench-{}", std::process::id()));
    let args = runner::Args {
        suite,
        size: size.to_string(),
        corpus_root,
        repo_root,
        exe: std::env::current_exe().context("locate this executable")?,
        scratch: scratch.clone(),
    };
    let result = runner::run(&args, &runner::SystemCacheDrop);
    let _ = std::fs::remove_dir_all(&scratch);
    let report = result?;
    let written = report::write(&report, &out_dir)?;
    print_summary(&report);
    println!("wrote {}", written.full.display());
    Ok(())
}

fn print_summary(report: &Report) {
    println!(
        "{} on {} ({} {}, {} .java files); {} ({} CPUs, {} MiB); {}",
        report.suite,
        report.corpus.size,
        report.corpus.repo,
        report
            .corpus
            .commit
            .get(..12)
            .unwrap_or(&report.corpus.commit),
        report.corpus.java_files,
        report.machine.cpu_model,
        report.machine.logical_cpus,
        report
            .machine
            .ram_mib
            .map_or("?".to_string(), |m| m.to_string()),
        report.machine.os,
    );
    for m in &report.metrics {
        if m.better != report::Better::Info {
            let spread = m
                .spread_pct
                .map_or(String::new(), |s| format!(", spread {s:.1}%"));
            println!(
                "  {:<58} {:>12.3} {} ({}{spread})",
                m.name, m.value, m.unit, m.stat
            );
        }
    }
    for note in &report.notes {
        println!("  note: {note}");
    }
}

fn read_report(path: &str) -> Result<Report> {
    let text = std::fs::read_to_string(path).context(format!("read {path}"))?;
    serde_json::from_str(&text).context(format!("parse {path}"))
}

fn cmd_compare(p: Parsed) -> Result<()> {
    p.only(&["band"])?;
    let (Some(a), Some(b)) = (p.positional.first(), p.positional.get(1)) else {
        return Err(BenchError::new("compare needs two report files"));
    };
    let band = p.number::<f64>("band")?.unwrap_or(DEFAULT_BAND_PCT);
    let (ra, rb) = (read_report(a)?, read_report(b)?);
    let deltas = report::compare(&ra, &rb, band);
    let outside = deltas.iter().filter(|d| !d.within_band).count();
    for d in &deltas {
        println!(
            "{:<58} {:>12.3} -> {:>12.3} {} {:+.1}% {}",
            d.name,
            d.a,
            d.b,
            d.unit,
            d.change_pct,
            if d.within_band { "ok" } else { "OUTSIDE" }
        );
    }
    println!(
        "{} metric(s) compared, {outside} outside the {band}% band",
        deltas.len()
    );
    if outside > 0 && p.switch("strict") {
        return Err(BenchError::new(format!(
            "{outside} metric(s) outside the band"
        )));
    }
    Ok(())
}

fn cmd_parse(p: Parsed) -> Result<()> {
    p.only(&["threads", "upto"])?;
    let root = p
        .positional
        .first()
        .context("parse needs a root directory")?;
    let upto = p
        .flag("upto")
        .map_or(Ok(Stage::Extract), Stage::from_name)?;
    let threads = p.number::<usize>("threads")?.unwrap_or(1).max(1);
    let result = e1::run(&e1::Options {
        root: PathBuf::from(root),
        threads,
        upto,
    })?;
    println!(
        "{}",
        serde_json::to_string(&result).context("encode the result")?
    );
    Ok(())
}

fn cmd_queries(p: Parsed) -> Result<()> {
    p.only(&["count", "seed"])?;
    let root = p
        .positional
        .first()
        .context("queries needs a root directory")?;
    let picked = queries::pick(
        &PathBuf::from(root),
        p.number::<usize>("count")?.unwrap_or(20),
        p.number::<u64>("seed")?.unwrap_or(1),
    )?;
    println!(
        "{}",
        serde_json::to_string_pretty(&picked).context("encode the queries")?
    );
    Ok(())
}

fn cmd_machine(p: Parsed) -> Result<()> {
    p.only(&["corpus-path", "repo-root"])?;
    let repo_root = PathBuf::from(p.flag("repo-root").unwrap_or("."));
    let corpus = PathBuf::from(p.flag("corpus-path").unwrap_or("."));
    let facts = machine::collect(&repo_root, &corpus, &["hyperfine", "rg", "git"]);
    println!(
        "{}",
        serde_json::to_string_pretty(&facts).context("encode the machine")?
    );
    Ok(())
}

fn cmd_sample(p: Parsed) -> Result<()> {
    p.only(&[])?;
    let out = std::env::temp_dir().join(format!("jfl-bench-sample-{}.out", std::process::id()));
    let sampled = sampler::run(&p.after_dashes, &out);
    let _ = std::fs::remove_file(&out);
    let s = sampled?;
    println!(
        "{}",
        serde_json::json!({
            "wall_ms": s.wall_ms,
            "exit_code": s.exit_code,
            "peak_kib": s.peak_kib,
            "steady_kib": s.steady_kib,
            "samples": s.samples,
            "stdout_bytes": s.stdout.len(),
        })
    );
    Ok(())
}
