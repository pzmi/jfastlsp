//! The hyperfine runner: a command timed end to end, many times, by the tool built for it.
//!
//! hyperfine (pinned by mise, fnd §8: 2.0.0) does the warm-up, the repetitions, `-N` (no shell),
//! `--prepare` (the cache drop of cold runs) and, per run, wall time, exit code and peak resident
//! set (`memory_peak_resident`, from the kernel's rusage of that child). A bespoke timer and a
//! bespoke RSS sampler were rejected (ADR 0019). The harness passes the command as a list of
//! arguments, quotes each one for hyperfine's own splitter, reads `--export-json` back and keeps
//! the raw per-run values so a metric can be a median or a percentile and a run's noise stated.
//!
//! The export is **schema version 2** (`results[].measurements[]`, each with `time_wall_clock`,
//! `exit_code`, `memory_peak_resident`). Any other `schema_version` is refused, so a hyperfine
//! release that changes the format fails loudly instead of being half-read.
//!
//! Pinned by: this module's tests (argv, quoting, parsing, refusals) and the golden export
//! `tests/fixtures/hyperfine-2.0.0-export.json`, a real 2.0.0 run (command path anonymised).
//! Running the real tool is the `just bench` recipe's job: the gate must not need it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::error::{BenchError, Context, Result};

/// The export format this module reads.
pub const SCHEMA_VERSION: u32 = 2;

/// What to time.
#[derive(Debug, Clone)]
pub struct Spec {
    /// The program and its arguments, run without a shell.
    pub argv: Vec<String>,
    /// The directory to run in (hyperfine inherits it), when the command's paths are relative.
    pub cwd: Option<PathBuf>,
    /// Untimed runs first.
    pub warmup: u32,
    /// Timed runs.
    pub runs: u32,
    /// Non-zero exit codes that are not a failure (`rg` exits 1 for no match).
    pub ok_exit_codes: Vec<i32>,
    /// A shell command run before every run (hyperfine's `--prepare`): the cache drop.
    pub prepare: Option<String>,
}

/// What the runs produced.
#[derive(Debug, Clone, PartialEq)]
pub struct Timings {
    /// Wall time of each run, milliseconds.
    pub times_ms: Vec<f64>,
    /// Peak resident set of each run, KiB; empty when hyperfine did not report it.
    pub peak_rss_kib: Vec<f64>,
}

/// Quote one argument for hyperfine's command splitter (POSIX-shell-like).
pub fn quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c));
    if plain {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', "'\\''"))
}

/// hyperfine's arguments for `spec`, exporting to `json`.
pub fn args(spec: &Spec, json: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![
        "-N".into(),
        "--style".into(),
        "none".into(),
        "--warmup".into(),
        spec.warmup.to_string().into(),
        "--runs".into(),
        spec.runs.to_string().into(),
    ];
    let tolerated: Vec<String> = spec
        .ok_exit_codes
        .iter()
        .filter(|c| **c != 0)
        .map(ToString::to_string)
        .collect();
    if !tolerated.is_empty() {
        a.push(format!("--ignore-failure={}", tolerated.join(",")).into());
    }
    if let Some(prepare) = &spec.prepare {
        a.push("--prepare".into());
        a.push(prepare.into());
    }
    a.push("--export-json".into());
    a.push(json.as_os_str().to_os_string());
    a.push("--".into());
    a.push(
        spec.argv
            .iter()
            .map(|s| quote(s))
            .collect::<Vec<_>>()
            .join(" ")
            .into(),
    );
    a
}

#[derive(Debug, Deserialize)]
struct Header {
    #[serde(default)]
    schema_version: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct Export {
    results: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    measurements: Vec<Measurement>,
}

#[derive(Debug, Deserialize)]
struct Measurement {
    time_wall_clock: Quantity,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    memory_peak_resident: Option<Quantity>,
}

#[derive(Debug, Deserialize)]
struct Quantity {
    value: f64,
    unit: String,
}

fn convert(q: &Quantity, expected: &str, per_unit: f64, what: &str) -> Result<f64> {
    if q.unit != expected {
        return Err(BenchError::new(format!(
            "hyperfine export: {what} is in {:?}, expected {expected:?}",
            q.unit
        )));
    }
    Ok(q.value * per_unit)
}

/// The per-run times (ms) and peak RSS (KiB) of the single command in a hyperfine export.
/// Fails on another `schema_version`, a unit it does not know, or a run whose exit code is not
/// 0 or in `ok_exit_codes`: a timing of a failing command times the failure.
pub fn parse_export(json: &str, ok_exit_codes: &[i32]) -> Result<Timings> {
    let header: Header = serde_json::from_str(json).context("hyperfine export")?;
    if header.schema_version != Some(SCHEMA_VERSION) {
        return Err(BenchError::new(format!(
            "hyperfine export has schema_version {:?}, this harness reads {SCHEMA_VERSION} \
             (hyperfine 2.x): pin the tool as mise.toml says",
            header.schema_version
        )));
    }
    let export: Export = serde_json::from_str(json).context("hyperfine export")?;
    let [entry] = export.results.as_slice() else {
        return Err(BenchError::new(format!(
            "hyperfine export: expected one command, found {}",
            export.results.len()
        )));
    };
    let mut out = Timings {
        times_ms: Vec::new(),
        peak_rss_kib: Vec::new(),
    };
    for m in &entry.measurements {
        let code = m.exit_code;
        if !code.is_some_and(|c| c == 0 || ok_exit_codes.contains(&c)) {
            return Err(BenchError::new(format!(
                "the timed command exited with {code:?}, not 0 or one of {ok_exit_codes:?}"
            )));
        }
        out.times_ms.push(convert(
            &m.time_wall_clock,
            "second",
            1e3,
            "time_wall_clock",
        )?);
        if let Some(rss) = &m.memory_peak_resident {
            out.peak_rss_kib
                .push(convert(rss, "byte", 1.0 / 1024.0, "memory_peak_resident")?);
        }
    }
    if out.times_ms.is_empty() {
        return Err(BenchError::new("hyperfine export: no measurements"));
    }
    Ok(out)
}

/// Run hyperfine on `spec`, using `scratch` for its export.
pub fn run(spec: &Spec, scratch: &Path) -> Result<Timings> {
    let json = scratch.join("hyperfine.json");
    let _ = std::fs::remove_file(&json);
    let mut cmd = Command::new("hyperfine");
    cmd.args(args(spec, &json))
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    let status = cmd
        .status()
        .context("run hyperfine (is it on PATH? `mise install --locked` pins it)")?;
    if !status.success() {
        return Err(BenchError::new(format!("hyperfine exited with {status}")));
    }
    let text = std::fs::read_to_string(&json).context(format!("read {}", json.display()))?;
    parse_export(&text, &spec.ok_exit_codes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOLDEN: &str = include_str!("../tests/fixtures/hyperfine-2.0.0-export.json");

    fn spec() -> Spec {
        Spec {
            argv: vec!["rg".into(), "-c".into(), "Foo Bar".into()],
            cwd: None,
            warmup: 3,
            runs: 20,
            ok_exit_codes: vec![0, 1],
            prepare: Some("sync".into()),
        }
    }

    #[test]
    fn should_leave_plain_words_alone_and_quote_the_rest_when_building_a_command() {
        // given
        let plain = ["-w", "/home/a/b-c_d.java"];
        // when
        let quoted: Vec<String> = plain.iter().map(|s| quote(s)).collect();
        // then
        assert_eq!(quoted, ["-w", "/home/a/b-c_d.java"]);
        assert_eq!(quote("a b"), "'a b'");
        assert_eq!(quote("it's"), "'it'\\''s'");
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn should_run_without_a_shell_and_tolerate_only_the_listed_codes_when_building_the_arguments() {
        // given
        let spec = spec();
        // when
        let a: Vec<String> = args(&spec, Path::new("/s/h.json"))
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        // then
        assert_eq!(a.first().map(String::as_str), Some("-N"));
        assert!(a.windows(2).any(|w| w == ["--warmup", "3"]));
        assert!(a.windows(2).any(|w| w == ["--runs", "20"]));
        assert!(a.windows(2).any(|w| w == ["--prepare", "sync"]));
        assert!(a.contains(&"--ignore-failure=1".to_string()));
        assert_eq!(a.last().map(String::as_str), Some("rg -c 'Foo Bar'"));
    }

    #[test]
    fn should_read_times_and_peak_rss_when_the_export_is_a_real_hyperfine_2_run() {
        // given: tests/fixtures/hyperfine-2.0.0-export.json, 20 runs of rg on a real checkout
        // when
        let t = parse_export(GOLDEN, &[0, 1]).unwrap();
        // then
        assert_eq!(t.times_ms.len(), 20);
        assert_eq!(t.peak_rss_kib.len(), 20);
        assert!((t.times_ms[0] - 93.541277).abs() < 1e-6);
        assert!((t.peak_rss_kib[0] - 6_238_208.0 / 1024.0).abs() < 1e-9);
    }

    #[test]
    fn should_refuse_when_the_schema_version_is_not_2() {
        // given: the 1.x layout, which a hyperfine downgrade would produce
        let json = r#"{"results":[{"times":[0.01],"exit_codes":[0]}]}"#;
        let v3 = GOLDEN.replace("\"schema_version\": 2", "\"schema_version\": 3");
        // when
        let old = parse_export(json, &[0]).unwrap_err();
        let new = parse_export(&v3, &[0, 1]).unwrap_err();
        // then
        assert!(old.to_string().contains("schema_version"), "{old}");
        assert!(new.to_string().contains("schema_version Some(3)"), "{new}");
    }

    #[test]
    fn should_fail_when_a_run_exited_with_an_unexpected_code() {
        // given
        let json = GOLDEN.replacen("\"exit_code\": 0", "\"exit_code\": 2", 1);
        // when
        let err = parse_export(&json, &[0, 1]).unwrap_err();
        // then
        assert!(err.to_string().contains("exited with"), "{err}");
    }

    #[test]
    fn should_fail_when_a_unit_is_not_the_expected_one() {
        // given
        let json = GOLDEN.replacen("\"unit\": \"second\"", "\"unit\": \"millisecond\"", 1);
        // when
        let err = parse_export(&json, &[0, 1]).unwrap_err();
        // then
        assert!(err.to_string().contains("millisecond"), "{err}");
    }

    #[test]
    fn should_fail_when_the_export_has_no_single_command() {
        // given
        let none = r#"{"schema_version":2,"results":[]}"#;
        // when
        let empty = parse_export(none, &[0]);
        let junk = parse_export("not json", &[0]);
        // then
        assert!(empty.is_err());
        assert!(junk.is_err());
    }
}
