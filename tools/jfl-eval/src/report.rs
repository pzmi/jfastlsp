//! Runs into the E0 report: per arm and per task, means and spreads (ver §11).
//!
//! A report holds arm ids, task ids, fixed metric and kind names, and numbers; nothing else. The
//! types make that structural: every string in [`Report`] is an [`Id`] or a `&'static str`, so no
//! prompt, path or line of code read from a run can reach it. Pinned by:
//! `tests/report.rs::should_leave_out_every_planted_string_when_the_input_carries_them` and the
//! goldens of `tests/report.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use serde::Serialize;

use crate::harbor::HARBOR_VERSION;
use crate::ids::Id;
use crate::metrics::RunMetrics;
use crate::run::{Found, Run, Verdict};

/// The report's schema version.
pub const REPORT_SCHEMA: u32 = 1;

/// Mean, sample standard deviation and range of one metric over `n` runs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Stat {
    /// Runs with a value.
    pub n: u64,
    /// Mean; `None` (JSON `null`, Markdown `-`) when no run had a value, which is not 0.
    pub mean: Option<f64>,
    /// Sample standard deviation (0 for one value); `None` when no run had a value.
    pub sd: Option<f64>,
    /// Smallest value; `None` when no run had a value.
    pub min: Option<f64>,
    /// Largest value; `None` when no run had a value.
    pub max: Option<f64>,
}

impl Stat {
    /// The statistics of `values`; only `n = 0` for none.
    pub fn of(values: &[f64]) -> Self {
        let n = values.len();
        if n == 0 {
            return Self {
                n: 0,
                mean: None,
                sd: None,
                min: None,
                max: None,
            };
        }
        let count = n as f64;
        let mean = values.iter().sum::<f64>() / count;
        let sd = if n < 2 {
            0.0
        } else {
            (values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (count - 1.0)).sqrt()
        };
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        Self {
            n: n as u64,
            mean: Some(round4(mean)),
            sd: Some(round4(sd)),
            min: Some(round4(min)),
            max: Some(round4(max)),
        }
    }
}

/// Four decimals: enough for rates and token counts, and stable in a golden.
fn round4(x: f64) -> f64 {
    let r = (x * 10_000.0).round() / 10_000.0;
    if r == 0.0 { 0.0 } else { r }
}

/// Run counts and the success rate of a group of runs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Counts {
    /// Runs.
    pub runs: u64,
    /// Runs whose test command passed.
    pub successes: u64,
    /// Runs with no reward recorded: the trial errored before the verifier (an infrastructure
    /// fault, a timeout of the environment), so the test command never ran.
    pub no_verdict: u64,
    /// Successes over all runs: a run with no verdict counts as a failure here.
    pub success_rate: f64,
    /// Successes over the runs with a verdict, so infrastructure faults do not lower it;
    /// `None` when no run has one.
    pub success_rate_with_verdict: Option<f64>,
}

/// One task of one arm.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaskReport {
    /// The task.
    pub task: Id,
    /// Its counts.
    #[serde(flatten)]
    pub counts: Counts,
    /// Its scalar metrics.
    pub metrics: BTreeMap<&'static str, Stat>,
}

/// One arm.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ArmReport {
    /// The arm.
    pub arm: Id,
    /// Its counts.
    #[serde(flatten)]
    pub counts: Counts,
    /// Runs by transcript source (`none` when a run left no transcript).
    pub sources: BTreeMap<&'static str, u64>,
    /// Scalar metrics over every run.
    pub metrics: BTreeMap<&'static str, Stat>,
    /// Tool calls and tokens over successful runs only (plan §1.2, metric 2).
    pub per_successful_run: BTreeMap<&'static str, Stat>,
    /// Tool calls per run by kind.
    pub tool_calls_by_kind: BTreeMap<&'static str, Stat>,
    /// LSP calls per run by operation.
    pub lsp_operations: BTreeMap<&'static str, Stat>,
    /// The next action after an empty `findReferences`, summed over runs.
    pub next_action_after_empty_references: BTreeMap<&'static str, u64>,
    /// Per task.
    pub tasks: Vec<TaskReport>,
}

/// How well the input read, summed over runs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Quality {
    /// Lines that were not JSON objects.
    pub malformed_lines: u64,
    /// Lines over the line limit.
    pub oversized_lines: u64,
    /// Files read only up to their limit.
    pub cut_files: u64,
    /// Lines repeated by uuid.
    pub duplicate_lines: u64,
    /// Runs that left no transcript.
    pub runs_without_transcript: u64,
}

/// The whole report.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    /// [`REPORT_SCHEMA`].
    pub schema: u32,
    /// The Harbor version whose formats were read.
    pub harbor: &'static str,
    /// Runs read.
    pub runs: u64,
    /// Trials of tasks `jfl-eval harbor` did not write.
    pub skipped_foreign: u64,
    /// Trials that could not be read: `result.json` missing, a symlink, too large or not JSON,
    /// or a transcript read that failed (I/O, too many files).
    pub skipped_unreadable: u64,
    /// How well the input read.
    pub data_quality: Quality,
    /// Per arm, in id order.
    pub arms: Vec<ArmReport>,
}

/// The scalar metrics of a run, by name; `None` where the run cannot say (no LSP answer, or
/// diagnostics not observable in its transcript).
fn scalars(run: &Run) -> Vec<(&'static str, Option<f64>)> {
    let m: &RunMetrics = &run.metrics;
    let n = |v: u64| Some(v as f64);
    let diag = |v: u64| m.diagnostics_observable.then_some(v as f64);
    vec![
        (
            "success",
            Some(if run.verdict == Verdict::Success {
                1.0
            } else {
                0.0
            }),
        ),
        ("tool_calls", n(m.tool_calls)),
        ("tokens_input", n(m.tokens_input)),
        ("tokens_cached", n(m.tokens_cached)),
        ("tokens_output", n(m.tokens_output)),
        (
            "tokens_total",
            n(m.tokens_input.saturating_add(m.tokens_output)),
        ),
        ("edits", n(m.edits)),
        ("builds", n(m.builds)),
        (
            "edits_then_compile_failure",
            n(m.edits_then_compile_failure),
        ),
        (
            "compile_failures_with_diagnostic_before_fix",
            diag(m.compile_failures_with_diagnostic_before_fix),
        ),
        (
            "edits_with_error_diagnostic",
            diag(m.edits_with_error_diagnostic),
        ),
        ("diagnostics_events", diag(m.diagnostics_events)),
        ("lsp_calls", n(m.lsp_calls)),
        ("lsp_errors", n(m.lsp_errors)),
        ("empty_references", n(m.empty_references)),
        (
            "time_to_first_lsp_answer_ms",
            m.time_to_first_lsp_answer_ms.map(|v| v as f64),
        ),
    ]
}

fn scalar_stats(runs: &[&Run]) -> BTreeMap<&'static str, Stat> {
    let mut values: BTreeMap<&'static str, Vec<f64>> = BTreeMap::new();
    for run in runs {
        for (name, value) in scalars(run) {
            let slot = values.entry(name).or_default();
            if let Some(v) = value {
                slot.push(v);
            }
        }
    }
    values.into_iter().map(|(k, v)| (k, Stat::of(&v))).collect()
}

/// Per-run statistics of a histogram, a key missing from a run counting as 0.
fn histogram_stats(
    runs: &[&Run],
    pick: impl Fn(&RunMetrics) -> &BTreeMap<&'static str, u64>,
) -> BTreeMap<&'static str, Stat> {
    let keys: BTreeSet<&'static str> = runs
        .iter()
        .flat_map(|r| pick(&r.metrics).keys().copied())
        .collect();
    keys.into_iter()
        .map(|k| {
            let v: Vec<f64> = runs
                .iter()
                .map(|r| pick(&r.metrics).get(k).copied().unwrap_or(0) as f64)
                .collect();
            (k, Stat::of(&v))
        })
        .collect()
}

fn counts(runs: &[&Run]) -> Counts {
    let total = runs.len() as u64;
    let successes = runs
        .iter()
        .filter(|r| r.verdict == Verdict::Success)
        .count() as u64;
    let no_verdict = runs
        .iter()
        .filter(|r| r.verdict == Verdict::NoVerdict)
        .count() as u64;
    let success_rate = if total == 0 {
        0.0
    } else {
        round4(successes as f64 / total as f64)
    };
    let with_verdict = total.saturating_sub(no_verdict);
    let success_rate_with_verdict =
        (with_verdict > 0).then(|| round4(successes as f64 / with_verdict as f64));
    Counts {
        runs: total,
        successes,
        no_verdict,
        success_rate,
        success_rate_with_verdict,
    }
}

/// Aggregate what a walk found.
pub fn build(found: &Found) -> Report {
    let mut by_arm: BTreeMap<&Id, Vec<&Run>> = BTreeMap::new();
    let mut quality = Quality::default();
    for run in &found.runs {
        by_arm.entry(&run.arm).or_default().push(run);
        let q = &run.quality;
        quality.malformed_lines = quality.malformed_lines.saturating_add(q.malformed_lines);
        quality.oversized_lines = quality.oversized_lines.saturating_add(q.oversized_lines);
        quality.cut_files = quality.cut_files.saturating_add(q.cut_files);
        quality.duplicate_lines = quality.duplicate_lines.saturating_add(q.duplicate_lines);
        if run.source.is_none() {
            quality.runs_without_transcript = quality.runs_without_transcript.saturating_add(1);
        }
    }
    let arms = by_arm
        .into_iter()
        .map(|(arm, runs)| arm_report(arm, &runs))
        .collect();
    Report {
        schema: REPORT_SCHEMA,
        harbor: HARBOR_VERSION,
        runs: found.runs.len() as u64,
        skipped_foreign: found.skipped_foreign,
        skipped_unreadable: found.skipped_unreadable,
        data_quality: quality,
        arms,
    }
}

fn arm_report(arm: &Id, runs: &[&Run]) -> ArmReport {
    let mut sources: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut next: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut by_task: BTreeMap<&Id, Vec<&Run>> = BTreeMap::new();
    for run in runs {
        let source = run.source.map_or("none", |s| s.as_str());
        let slot = sources.entry(source).or_insert(0);
        *slot = slot.saturating_add(1);
        for (k, v) in &run.metrics.next_action_after_empty_references {
            let slot = next.entry(k).or_insert(0);
            *slot = slot.saturating_add(*v);
        }
        by_task.entry(&run.task).or_default().push(run);
    }
    let successful: Vec<&Run> = runs
        .iter()
        .copied()
        .filter(|r| r.verdict == Verdict::Success)
        .collect();
    let per_successful_run = scalar_stats(&successful)
        .into_iter()
        .filter(|(k, _)| matches!(*k, "tool_calls" | "tokens_total"))
        .collect();
    ArmReport {
        arm: arm.clone(),
        counts: counts(runs),
        sources,
        metrics: scalar_stats(runs),
        per_successful_run,
        tool_calls_by_kind: histogram_stats(runs, |m| &m.tool_calls_by_kind),
        lsp_operations: histogram_stats(runs, |m| &m.lsp_operations),
        next_action_after_empty_references: next,
        tasks: by_task
            .into_iter()
            .map(|(task, runs)| TaskReport {
                task: task.clone(),
                counts: counts(&runs),
                metrics: scalar_stats(&runs),
            })
            .collect(),
    }
}

/// The report as pretty JSON, with a final newline.
pub fn to_json(report: &Report) -> String {
    let mut s = serde_json::to_string_pretty(report).unwrap_or_default();
    s.push('\n');
    s
}

/// The report as Markdown.
pub fn to_markdown(r: &Report) -> String {
    let mut out = String::new();
    let q = &r.data_quality;
    let _ = write!(
        out,
        "# E0 report\n\n\
         Harbor {} · report schema {} · {} runs read · {} skipped (task not written by jfl-eval) · \
         {} unreadable\n\n\
         Data quality: {} malformed lines, {} oversized lines, {} cut files, {} duplicate lines, \
         {} runs without a transcript.\n",
        r.harbor,
        r.schema,
        r.runs,
        r.skipped_foreign,
        r.skipped_unreadable,
        q.malformed_lines,
        q.oversized_lines,
        q.cut_files,
        q.duplicate_lines,
        q.runs_without_transcript,
    );
    for arm in &r.arms {
        let c = &arm.counts;
        let sources: Vec<String> = arm
            .sources
            .iter()
            .map(|(k, v)| format!("{k} {v}"))
            .collect();
        let _ = write!(
            out,
            "\n## Arm `{}`\n\n\
             {} runs · {} successes · {} without a verdict · success rate {:.2} (a run without a \
             verdict counts as a failure; {} over the runs with one) · transcripts: {}\n",
            arm.arm,
            c.runs,
            c.successes,
            c.no_verdict,
            c.success_rate,
            fmt2(c.success_rate_with_verdict),
            sources.join(", "),
        );
        stat_table(&mut out, "Metric (per run)", &arm.metrics);
        stat_table(&mut out, "Per successful run", &arm.per_successful_run);
        stat_table(
            &mut out,
            "Tool calls by kind (per run)",
            &arm.tool_calls_by_kind,
        );
        stat_table(&mut out, "LSP operation (per run)", &arm.lsp_operations);
        out.push_str("\n| Next action after an empty findReferences | Count |\n|:--|--:|\n");
        if arm.next_action_after_empty_references.is_empty() {
            out.push_str("| (none) | 0 |\n");
        }
        for (k, v) in &arm.next_action_after_empty_references {
            let _ = writeln!(out, "| {k} | {v} |");
        }
        out.push_str(
            "\n| Task | Runs | Successes | Success rate | Tool calls | Tokens | \
             Edits then compile failure | LSP calls |\n\
             |:--|--:|--:|--:|--:|--:|--:|--:|\n",
        );
        for t in &arm.tasks {
            let mean_sd = |name: &str| match t.metrics.get(name) {
                Some(Stat {
                    mean: Some(mean),
                    sd: Some(sd),
                    ..
                }) => format!("{mean:.2} ± {sd:.2}"),
                _ => "-".to_string(),
            };
            let _ = writeln!(
                out,
                "| {} | {} | {} | {:.2} | {} | {} | {} | {} |",
                t.task,
                t.counts.runs,
                t.counts.successes,
                t.counts.success_rate,
                mean_sd("tool_calls"),
                mean_sd("tokens_total"),
                mean_sd("edits_then_compile_failure"),
                mean_sd("lsp_calls"),
            );
        }
    }
    out
}

fn stat_table(out: &mut String, title: &str, stats: &BTreeMap<&'static str, Stat>) {
    let _ = write!(
        out,
        "\n| {title} | n | Mean | SD | Min | Max |\n|:--|--:|--:|--:|--:|--:|\n"
    );
    if stats.is_empty() {
        out.push_str("| (none) | 0 | - | - | - | - |\n");
    }
    for (name, s) in stats {
        let _ = writeln!(
            out,
            "| {name} | {} | {} | {} | {} | {} |",
            s.n,
            fmt2(s.mean),
            fmt2(s.sd),
            fmt2(s.min),
            fmt2(s.max)
        );
    }
}

/// Two decimals, or `-` for a value no run had.
fn fmt2(v: Option<f64>) -> String {
    v.map_or_else(|| "-".to_string(), |x| format!("{x:.2}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_give_mean_sample_sd_and_range_when_values_differ() {
        // given
        let v = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        // when
        let s = Stat::of(&v);
        // then: sample variance 32/7
        assert_eq!(
            (s.n, s.mean, s.min, s.max),
            (8, Some(5.0), Some(2.0), Some(9.0))
        );
        assert_eq!(s.sd, Some(2.1381));
    }

    #[test]
    fn should_give_zero_spread_when_there_is_one_value() {
        // given
        // when
        let s = Stat::of(&[3.0]);
        // then
        assert_eq!((s.n, s.mean, s.sd), (1, Some(3.0), Some(0.0)));
    }

    #[test]
    fn should_leave_the_statistics_absent_when_no_run_has_a_value() {
        // given
        // when
        let s = Stat::of(&[]);
        // then: not 0, which would read as an instant LSP answer
        assert_eq!(
            (s.n, s.mean, s.sd, s.min, s.max),
            (0, None, None, None, None)
        );
        assert_eq!(fmt2(s.mean), "-");
        assert_eq!(
            serde_json::to_value(s).unwrap()["mean"],
            serde_json::Value::Null
        );
    }
}
