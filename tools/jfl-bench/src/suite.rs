//! Suites as TOML: what to run, on which corpus size, how many times.
//!
//! A suite names an operation (`kind`) and its knobs; the corpus size is not in the file but on
//! the command line, so one suite runs on `small` for the inner loop and on `medium` for a
//! milestone. Unknown keys are refused, so a typo is an error and not a silently default knob.
//!
//! Pinned by: this module's tests (every built-in suite parses and validates).

use serde::Deserialize;

use crate::e1::Stage;
use crate::error::{BenchError, Context, Result};

/// The built-in suites, embedded so `jfl-bench run rg-floor` needs no working-directory layout.
pub const BUILTIN: &[(&str, &str)] = &[
    ("rg-floor", include_str!("../suites/rg-floor.toml")),
    ("e1", include_str!("../suites/e1.toml")),
];

/// What a suite does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// `rg` answering name queries, timed end to end through hyperfine.
    RgFloor,
    /// The E1 parse pipeline in process, stage by stage.
    E1,
}

/// Repetition knobs shared by every suite.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    /// Untimed runs before the timed ones.
    #[serde(default = "default_warmup")]
    pub warmup: u32,
    /// Timed runs; a metric is their median (rg-floor: their p50 and p95).
    #[serde(default = "default_runs")]
    pub runs: u32,
    /// Runs after a page-cache drop; skipped, with a note, where the cache cannot be dropped.
    #[serde(default = "default_cold_runs")]
    pub cold_runs: u32,
    /// Seed for picking queries from the corpus.
    #[serde(default = "default_seed")]
    pub seed: u64,
}

impl Default for Run {
    fn default() -> Self {
        Self {
            warmup: default_warmup(),
            runs: default_runs(),
            cold_runs: default_cold_runs(),
            seed: default_seed(),
        }
    }
}

fn default_warmup() -> u32 {
    3
}
fn default_runs() -> u32 {
    10
}
fn default_cold_runs() -> u32 {
    1
}
fn default_seed() -> u64 {
    1
}

/// How `rg-floor` picks its queries.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Queries {
    /// Words always queried (the E1 spike's `String` floor).
    #[serde(default)]
    pub fixed: Vec<String>,
    /// Type names picked from the corpus by seed, besides the fixed ones.
    #[serde(default)]
    pub picked: usize,
    /// Regexes run in the spike's own form (`rg -c -t java REGEX .` in the checkout, no `-w`),
    /// series `spikeNN`, so the floor of doc/bench-corpus.md is compared like for like.
    #[serde(default)]
    pub spike_form: Vec<String>,
}

/// The `e1` suite's knobs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct E1 {
    /// Thread counts to run the pipeline at.
    pub threads: Vec<usize>,
    /// Cumulative stages to time, in order; the last is the full index.
    pub stages: Vec<Stage>,
    /// Stages timed again after a cache drop.
    #[serde(default)]
    pub cold_stages: Vec<Stage>,
}

/// One suite.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    /// The suite's name, the prefix of its metrics.
    pub name: String,
    /// What it measures, one sentence.
    pub description: String,
    /// The operation.
    pub kind: Kind,
    /// Repetition knobs.
    #[serde(default)]
    pub run: Run,
    /// `rg-floor` only.
    pub queries: Option<Queries>,
    /// `e1` only.
    pub e1: Option<E1>,
}

impl Suite {
    /// Parse and validate a suite's TOML.
    pub fn parse(text: &str) -> Result<Self> {
        let suite: Self = toml::from_str(text).context("suite")?;
        suite.validate()?;
        Ok(suite)
    }

    fn validate(&self) -> Result<()> {
        if self.name.is_empty()
            || !self
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err(BenchError::new(format!(
                "suite name {:?} is not [A-Za-z0-9-]+",
                self.name
            )));
        }
        if self.run.runs == 0 {
            return Err(BenchError::new("suite: run.runs must be at least 1"));
        }
        match self.kind {
            Kind::RgFloor => {
                let q = self
                    .queries
                    .as_ref()
                    .context("suite: an rg-floor suite needs [queries]")?;
                if q.fixed.is_empty() && q.picked == 0 {
                    return Err(BenchError::new("suite: [queries] selects nothing"));
                }
                if self.e1.is_some() {
                    return Err(BenchError::new("suite: [e1] belongs to an e1 suite"));
                }
            }
            Kind::E1 => {
                let e = self.e1.as_ref().context("suite: an e1 suite needs [e1]")?;
                if e.threads.is_empty() || e.threads.contains(&0) || e.stages.is_empty() {
                    return Err(BenchError::new("suite: [e1] needs threads >= 1 and stages"));
                }
                if self.queries.is_some() {
                    return Err(BenchError::new(
                        "suite: [queries] belongs to an rg-floor suite",
                    ));
                }
            }
        }
        Ok(())
    }

    /// A built-in suite by name, or a suite file when `spec` names a `.toml` path.
    pub fn load(spec: &str) -> Result<Self> {
        if let Some((_, text)) = BUILTIN.iter().find(|(name, _)| *name == spec) {
            return Self::parse(text);
        }
        if spec.ends_with(".toml") {
            let text = std::fs::read_to_string(spec).context(format!("read {spec}"))?;
            return Self::parse(&text);
        }
        let known: Vec<&str> = BUILTIN.iter().map(|(n, _)| *n).collect();
        Err(BenchError::new(format!(
            "unknown suite {spec:?}; built in: {}; or pass a .toml path",
            known.join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_parse_and_validate_when_the_suite_is_built_in() {
        // given
        // when
        // then
        for (name, text) in BUILTIN {
            let suite = Suite::parse(text).unwrap();
            assert_eq!(suite.name, *name);
        }
    }

    #[test]
    fn should_refuse_when_a_key_is_unknown() {
        // given
        let text = "name = \"x\"\ndescription = \"d\"\nkind = \"e1\"\nrunz = 3\n";
        // when
        let err = Suite::parse(text).unwrap_err();
        // then
        assert!(err.to_string().contains("runz"), "{err}");
    }

    #[test]
    fn should_refuse_when_the_section_does_not_match_the_kind() {
        // given
        let text = "name = \"x\"\ndescription = \"d\"\nkind = \"rg-floor\"\n[e1]\nthreads = [1]\nstages = [\"read\"]\n";
        // when
        let err = Suite::parse(text).unwrap_err();
        // then
        assert!(err.to_string().contains("[queries]"), "{err}");
    }

    #[test]
    fn should_refuse_when_zero_runs_are_asked() {
        // given
        let text = "name = \"x\"\ndescription = \"d\"\nkind = \"e1\"\n[run]\nruns = 0\n[e1]\nthreads = [1]\nstages = [\"read\"]\n";
        // when
        let err = Suite::parse(text).unwrap_err();
        // then
        assert!(err.to_string().contains("runs"), "{err}");
    }

    #[test]
    fn should_name_the_built_ins_when_the_suite_is_unknown() {
        // given
        // when
        let err = Suite::load("nope").unwrap_err();
        // then
        assert!(err.to_string().contains("rg-floor"), "{err}");
    }
}
