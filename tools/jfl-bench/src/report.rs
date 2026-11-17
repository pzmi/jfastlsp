//! Results as JSON: the full report with the machine's facts, and the files
//! github-action-benchmark reads.
//!
//! The history tool (ADR 0031, BENCH-04) takes an array of `{name, unit, value, range?, extra?}`
//! in one of two tools: `customSmallerIsBetter` (times, memory) and `customBiggerIsBetter`
//! (throughput). A report therefore writes three files, `<stem>.json` (everything, with the
//! machine), `<stem>.smaller.json` and `<stem>.larger.json` (the tool's arrays); counts that
//! have no better direction (files, declarations) are in the full report only. A metric's value
//! is the median of its runs (the `runs` list), so one noisy run does not move a series; its
//! `spread_pct` states how far apart the runs were, which is what the noise band of
//! `doc/bench/README.md` is derived from.
//!
//! Pinned by: this module's tests, which parse the arrays with the history tool's own schema
//! (required `name`, `unit`, `value`; optional `range`, `extra`; nothing else).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::corpus::CorpusFacts;
use crate::error::{Context, Result};
use crate::machine::{Build, Machine};
use crate::stats;

/// Which way a metric improves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Better {
    /// Smaller is better: times, memory.
    Smaller,
    /// Bigger is better: throughput.
    Larger,
    /// A count or a fact: reported, never compared.
    Info,
}

/// One tracked number.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    /// `<suite>/<corpus>/<...>`, stable across runs: the series name. Never a corpus identifier:
    /// per-query series are keyed by slot (`q00`...), so a history file names nothing of the corpus.
    pub name: String,
    /// `ms`, `MiB`, `files/s`, `MB/s`, `count`.
    pub unit: String,
    /// Which way it improves.
    pub better: Better,
    /// What `value` is, in words: `median of 3 runs`, `p95 of 20 runs`, `median over 20 queries`,
    /// `1 run`. A single measurement is never presented as a median.
    pub stat: String,
    /// The statistic of `runs`.
    pub value: f64,
    /// Each measurement behind `value`, in the order taken.
    pub runs: Vec<f64>,
    /// `(max - min) / median` of `runs` in percent; `None` when `runs` are not repetitions of one
    /// measurement (one run, or an aggregate over queries).
    pub spread_pct: Option<f64>,
}

impl Metric {
    /// The median of `runs`, repetitions of one measurement.
    pub fn median_of(name: String, unit: &str, better: Better, runs: Vec<f64>) -> Self {
        let n = runs.len();
        Self {
            name,
            unit: unit.to_string(),
            better,
            stat: if n == 1 {
                "1 run".to_string()
            } else {
                format!("median of {n} runs")
            },
            value: stats::median(&runs),
            spread_pct: (n > 1).then(|| stats::spread_pct(&runs)),
            runs,
        }
    }

    /// The `p`-th percentile (`label` `p95`) of `runs`, repetitions of one measurement.
    pub fn percentile_of(
        name: String,
        unit: &str,
        better: Better,
        runs: Vec<f64>,
        p: f64,
        label: &str,
    ) -> Self {
        let n = runs.len();
        Self {
            name,
            unit: unit.to_string(),
            better,
            stat: format!("{label} of {n} runs"),
            value: stats::percentile(&runs, p),
            spread_pct: (n > 1).then(|| stats::spread_pct(&runs)),
            runs,
        }
    }

    /// The maximum of `runs`, repetitions of one measurement (a peak is a maximum, not a typical).
    pub fn max_of(name: String, unit: &str, better: Better, runs: Vec<f64>) -> Self {
        let n = runs.len();
        Self {
            name,
            unit: unit.to_string(),
            better,
            stat: format!("max of {n} runs"),
            value: stats::percentile(&runs, 1.0),
            spread_pct: (n > 1).then(|| stats::spread_pct(&runs)),
            runs,
        }
    }

    /// The median over `values` that are different measurements (one per query): no spread.
    pub fn median_over(
        name: String,
        unit: &str,
        better: Better,
        values: Vec<f64>,
        what: &str,
    ) -> Self {
        Self {
            name,
            unit: unit.to_string(),
            better,
            stat: format!("median over {} {what}", values.len()),
            value: stats::median(&values),
            spread_pct: None,
            runs: values,
        }
    }

    /// A metric with a single measurement, or a count.
    pub fn single(name: String, unit: &str, better: Better, value: f64) -> Self {
        Self {
            name,
            unit: unit.to_string(),
            better,
            stat: if better == Better::Info {
                "count".to_string()
            } else {
                "1 run".to_string()
            },
            value,
            runs: vec![value],
            spread_pct: None,
        }
    }
}

/// A query's slot, with a digest of its name instead of the name: the report is committed and
/// published, and nothing of the corpus is (doc/bench-corpus.md). `jfl-bench queries` prints the
/// names; the digest lets anyone check that two runs picked the same ones.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuerySlot {
    /// `q00`, `q01`, ...
    pub slot: String,
    /// The first 16 hex digits of the blake3 digest of the name (or word).
    pub name_blake3: String,
}

/// How the run was configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunFacts {
    /// Timed runs per metric.
    pub runs: u32,
    /// Warm-up runs.
    pub warmup: u32,
    /// The query seed.
    pub seed: u64,
}

/// A suite's full result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// The report format's version.
    pub schema: u32,
    /// The suite.
    pub suite: String,
    /// The corpus measured.
    pub corpus: CorpusFacts,
    /// The machine measured on.
    pub machine: Machine,
    /// How the measuring binary was built.
    pub build: Build,
    /// The query slots of the series that are keyed by slot.
    pub queries: Vec<QuerySlot>,
    /// The repetition knobs.
    pub config: RunFacts,
    /// Every metric.
    pub metrics: Vec<Metric>,
    /// What was skipped or could not be measured, and why.
    pub notes: Vec<String>,
}

/// An entry of github-action-benchmark's `custom*IsBetter` input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    /// The series name.
    pub name: String,
    /// The unit shown on the chart.
    pub unit: String,
    /// The value.
    pub value: f64,
    /// A spread shown beside the value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// Free text shown in the tooltip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<String>,
}

/// The history tool's entries for the metrics that improve in direction `better`.
pub fn history_entries(report: &Report, better: Better) -> Vec<HistoryEntry> {
    report
        .metrics
        .iter()
        .filter(|m| m.better == better)
        .map(|m| HistoryEntry {
            name: m.name.clone(),
            unit: m.unit.clone(),
            value: m.value,
            range: m.spread_pct.map(|s| format!("± {:.1}%", s / 2.0)),
            extra: Some(format!(
                "{}; {}; {}; commit {}",
                m.stat,
                report.machine.cpu_model,
                report.corpus.repo,
                report
                    .machine
                    .commit
                    .get(..12)
                    .unwrap_or(&report.machine.commit)
            )),
        })
        .collect()
}

/// The files `write` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    /// The full report.
    pub full: PathBuf,
    /// The `customSmallerIsBetter` array.
    pub smaller: PathBuf,
    /// The `customBiggerIsBetter` array, when any metric improves upward.
    pub larger: Option<PathBuf>,
}

/// Write `<dir>/<suite>-<corpus>.json` and its history arrays.
pub fn write(report: &Report, dir: &Path) -> Result<Written> {
    std::fs::create_dir_all(dir).context(format!("create {}", dir.display()))?;
    let stem = format!("{}-{}", report.suite, report.corpus.size);
    let put = |path: &Path, text: String| {
        std::fs::write(path, text + "\n").context(format!("write {}", path.display()))
    };
    let full = dir.join(format!("{stem}.json"));
    put(
        &full,
        serde_json::to_string_pretty(report).context("encode the report")?,
    )?;
    let smaller = dir.join(format!("{stem}.smaller.json"));
    put(
        &smaller,
        serde_json::to_string_pretty(&history_entries(report, Better::Smaller))
            .context("encode the history array")?,
    )?;
    let larger_entries = history_entries(report, Better::Larger);
    let larger = if larger_entries.is_empty() {
        None
    } else {
        let path = dir.join(format!("{stem}.larger.json"));
        put(
            &path,
            serde_json::to_string_pretty(&larger_entries).context("encode the array")?,
        )?;
        Some(path)
    };
    Ok(Written {
        full,
        smaller,
        larger,
    })
}

/// One metric present in both reports of a comparison.
#[derive(Debug, Clone, PartialEq)]
pub struct Delta {
    /// The series name.
    pub name: String,
    /// The unit.
    pub unit: String,
    /// The first report's value.
    pub a: f64,
    /// The second report's value.
    pub b: f64,
    /// `(b - a) / a` in percent; 0 when `a` is 0.
    pub change_pct: f64,
    /// Whether `|change_pct|` is within the band.
    pub within_band: bool,
}

/// Compare two reports metric by metric (`Info` metrics are skipped; a metric in one report
/// only is skipped, and the caller can see it from the lengths).
pub fn compare(a: &Report, b: &Report, band_pct: f64) -> Vec<Delta> {
    a.metrics
        .iter()
        .filter(|m| m.better != Better::Info)
        .filter_map(|ma| {
            let mb = b.metrics.iter().find(|m| m.name == ma.name)?;
            let change_pct = if ma.value == 0.0 {
                0.0
            } else {
                (mb.value - ma.value) / ma.value.abs() * 100.0
            };
            Some(Delta {
                name: ma.name.clone(),
                unit: ma.unit.clone(),
                a: ma.value,
                b: mb.value,
                change_pct,
                within_band: change_pct.abs() <= band_pct,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn report(metrics: Vec<Metric>) -> Report {
        Report {
            schema: 1,
            suite: "rg-floor".to_string(),
            corpus: CorpusFacts {
                size: "small".to_string(),
                repo: "junit-framework".to_string(),
                commit: "e5198f9b098c550e66d378650f78bac8883e2a72".to_string(),
                java_files: 1813,
            },
            machine: Machine {
                cpu_model: "Test CPU".to_string(),
                logical_cpus: 4,
                ram_mib: Some(16000),
                os: "Test OS".to_string(),
                kernel: "Linux 6".to_string(),
                arch: "x86_64".to_string(),
                disk: None,
                tools: BTreeMap::new(),
                commit: "0123456789abcdef0123".to_string(),
                dirty: false,
            },
            build: Build {
                profile: "bench-tool".to_string(),
                debug_assertions: false,
                toolchain: "1.97.0".to_string(),
            },
            queries: vec![QuerySlot {
                slot: "q00".to_string(),
                name_blake3: "0123456789abcdef".to_string(),
            }],
            config: RunFacts {
                runs: 3,
                warmup: 1,
                seed: 1,
            },
            metrics,
            notes: vec![],
        }
    }

    fn sample() -> Report {
        report(vec![
            Metric::median_of(
                "rg-floor/small/String/p50".into(),
                "ms",
                Better::Smaller,
                vec![40.0, 41.0, 43.0],
            ),
            Metric::median_of(
                "e1/small/t4/extract/files_per_s".into(),
                "files/s",
                Better::Larger,
                vec![12000.0, 12400.0, 12300.0],
            ),
            Metric::single("e1/small/files".into(), "count", Better::Info, 1813.0),
        ])
    }

    #[test]
    fn should_take_the_median_and_state_the_spread_when_a_metric_has_runs() {
        // given
        // when
        let m = Metric::median_of("x".into(), "ms", Better::Smaller, vec![90.0, 100.0, 110.0]);
        // then
        assert_eq!(m.value, 100.0);
        assert!((m.spread_pct.unwrap() - 20.0).abs() < 1e-9);
        assert_eq!(m.stat, "median of 3 runs");
    }

    #[test]
    fn should_not_call_a_single_run_a_median_or_invent_a_range_when_there_is_one_measurement() {
        // given
        let report = report(vec![
            Metric::median_of("a".into(), "ms", Better::Smaller, vec![5.0]),
            Metric::percentile_of(
                "b".into(),
                "ms",
                Better::Smaller,
                (1..=20).map(f64::from).collect(),
                0.95,
                "p95",
            ),
        ]);
        // when
        let entries = history_entries(&report, Better::Smaller);
        // then
        assert!(entries[0].extra.as_deref().unwrap().starts_with("1 run;"));
        assert_eq!(entries[0].range, None);
        assert!(
            entries[1]
                .extra
                .as_deref()
                .unwrap()
                .starts_with("p95 of 20 runs;")
        );
        assert!(entries[1].range.is_some());
        assert_eq!(report.metrics[1].value, 19.0);
    }

    #[test]
    fn should_label_a_peak_as_the_max_of_its_runs_when_built_from_repetitions() {
        // given
        let runs = vec![5.9, 6.2, 6.0];
        // when
        let m = Metric::max_of("rss".into(), "MiB", Better::Smaller, runs);
        // then
        assert_eq!(m.stat, "max of 3 runs");
        assert_eq!(m.value, 6.2);
    }

    #[test]
    fn should_parse_with_the_history_tools_schema_when_the_smaller_array_is_written() {
        // given
        let report = sample();
        // when
        let text = serde_json::to_string(&history_entries(&report, Better::Smaller)).unwrap();
        let parsed: Vec<HistoryEntry> = serde_json::from_str(&text).unwrap();
        // then: name, unit and a numeric value on every entry; no extra keys (deny_unknown_fields)
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "rg-floor/small/String/p50");
        assert_eq!(parsed[0].unit, "ms");
        assert_eq!(parsed[0].value, 41.0);
        let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(raw[0]["value"].is_number() && raw[0]["name"].is_string());
    }

    #[test]
    fn should_accept_the_history_tools_own_example_when_parsed_as_an_entry_array() {
        // given: the example of github-action-benchmark's README for customSmallerIsBetter
        let example = r#"[
            {"name": "My Custom Smaller Is Better Benchmark - CPU Load", "unit": "Percent", "value": 50},
            {"name": "My Custom Smaller Is Better Benchmark - Memory Used", "unit": "Megabytes",
             "value": 100, "range": "3", "extra": "Value for Memory usage"}
        ]"#;
        // when
        let parsed: Vec<HistoryEntry> = serde_json::from_str(example).unwrap();
        // then
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].range.as_deref(), Some("3"));
    }

    #[test]
    fn should_keep_info_metrics_out_of_the_history_arrays_when_directions_are_split() {
        // given
        let report = sample();
        // when
        let smaller = history_entries(&report, Better::Smaller);
        let larger = history_entries(&report, Better::Larger);
        // then
        assert_eq!(smaller.len(), 1);
        assert_eq!(larger.len(), 1);
        assert_eq!(larger[0].name, "e1/small/t4/extract/files_per_s");
    }

    #[test]
    fn should_write_three_files_when_the_report_has_both_directions() {
        // given
        let dir = std::env::temp_dir().join(format!("jfl-bench-report-{}", std::process::id()));
        let report = sample();
        // when
        let written = write(&report, &dir).unwrap();
        // then
        let full: Report =
            serde_json::from_str(&std::fs::read_to_string(&written.full).unwrap()).unwrap();
        assert_eq!(full, report);
        assert!(written.smaller.ends_with("rg-floor-small.smaller.json"));
        assert!(written.larger.is_some_and(|p| p.is_file()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn should_flag_the_metric_outside_the_band_when_two_reports_differ() {
        // given
        let a = report(vec![
            Metric::single("a".into(), "ms", Better::Smaller, 100.0),
            Metric::single("b".into(), "ms", Better::Smaller, 100.0),
            Metric::single("n".into(), "count", Better::Info, 5.0),
        ]);
        let b = report(vec![
            Metric::single("a".into(), "ms", Better::Smaller, 104.0),
            Metric::single("b".into(), "ms", Better::Smaller, 130.0),
            Metric::single("n".into(), "count", Better::Info, 9.0),
        ]);
        // when
        let deltas = compare(&a, &b, 10.0);
        // then
        assert_eq!(deltas.len(), 2);
        assert!(deltas[0].within_band);
        assert!(!deltas[1].within_band);
        assert!((deltas[1].change_pct - 30.0).abs() < 1e-9);
    }
}
