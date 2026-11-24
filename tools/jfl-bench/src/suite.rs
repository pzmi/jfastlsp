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
    ("incremental", include_str!("../suites/incremental.toml")),
    ("kmp-lsp", include_str!("../suites/kmp-lsp.toml")),
    ("jdtls", include_str!("../suites/jdtls.toml")),
    ("jfl-lsp", include_str!("../suites/jfl-lsp.toml")),
];

/// What a suite does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// `rg` answering name queries, timed end to end through hyperfine.
    RgFloor,
    /// The E1 parse pipeline in process, stage by stage.
    E1,
    /// The incremental proxies: an edit, a rebase and a switch, a widely used type, a reopen.
    Incremental,
    /// The kmp-lsp arm through hyperfine: its index, cached index, `refs` and `find`.
    KmpLsp,
    /// The jdtls arm: the pinned stock jdtls, over LSP.
    Jdtls,
    /// The built `jfl lsp` over LSP, from a fresh state directory (LSP-02).
    JflLsp,
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

/// The `incremental` suite's knobs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Incremental {
    /// Worker threads for re-parsing a changed set and re-hashing a worktree.
    pub threads: usize,
    /// Files the `edit` case edits, picked by seed.
    pub edits: usize,
    /// Widely used types the `fanout` case measures.
    pub fanout: usize,
    /// Commit-pair roles to measure (`rebase`, `switch`), from `scripts/bench-corpus.sh pairs`.
    #[serde(default)]
    pub roles: Vec<String>,
}

/// The `kmp-lsp` arm's knobs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Kmp {
    /// Names picked by seed for `refs` and `find` (the same picks as `rg-floor`'s).
    pub picked: usize,
    /// Runs of the empty-cache index (each a full index, so fewer than the other series).
    pub index_runs: u32,
}

/// The `jdtls` arm's knobs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jdtls {
    /// Types picked by seed; each is queried at a use in another file.
    pub sites: usize,
    /// jdtls's heap cap (`-Xmx`, MiB).
    pub heap_mib: u32,
    /// The whole session's deadline (`timeout`), seconds.
    pub deadline_secs: u64,
}

/// The `jfl-lsp` suite's knobs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JflLsp {
    /// Types picked by seed; each is queried at a use in another file.
    pub sites: usize,
    /// Each session's deadline (`timeout`), seconds.
    pub deadline_secs: u64,
    /// Files whose pushed diagnostics are timed (LSP-04): a breaking `didChange` to its
    /// `publishDiagnostics`, plus the corpus's largest file; 0 skips the phase.
    #[serde(default)]
    pub diagnostics: usize,
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
    /// `incremental` only.
    pub incremental: Option<Incremental>,
    /// `kmp-lsp` only.
    pub kmp: Option<Kmp>,
    /// `jdtls` only.
    pub jdtls: Option<Jdtls>,
    /// `jfl-lsp` only.
    pub jfl_lsp: Option<JflLsp>,
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
        let sections = [
            ("[queries]", self.queries.is_some(), Kind::RgFloor),
            ("[e1]", self.e1.is_some(), Kind::E1),
            (
                "[incremental]",
                self.incremental.is_some(),
                Kind::Incremental,
            ),
            ("[kmp]", self.kmp.is_some(), Kind::KmpLsp),
            ("[jdtls]", self.jdtls.is_some(), Kind::Jdtls),
            ("[jfl_lsp]", self.jfl_lsp.is_some(), Kind::JflLsp),
        ];
        if let Some((section, _, owner)) = sections
            .iter()
            .find(|(_, present, owner)| *present && *owner != self.kind)
        {
            return Err(BenchError::new(format!(
                "suite: {section} belongs to a {owner:?} suite, not {:?}",
                self.kind
            )));
        }
        if let Some((section, _, _)) = sections
            .iter()
            .find(|(_, present, owner)| !*present && *owner == self.kind)
        {
            return Err(BenchError::new(format!(
                "suite: a {:?} suite needs {section}",
                self.kind
            )));
        }
        match self.kind {
            Kind::RgFloor => {
                if let Some(q) = &self.queries
                    && q.fixed.is_empty()
                    && q.picked == 0
                {
                    return Err(BenchError::new("suite: [queries] selects nothing"));
                }
            }
            Kind::E1 => {
                if let Some(e) = &self.e1
                    && (e.threads.is_empty() || e.threads.contains(&0) || e.stages.is_empty())
                {
                    return Err(BenchError::new("suite: [e1] needs threads >= 1 and stages"));
                }
            }
            Kind::Incremental => {
                if let Some(i) = &self.incremental
                    && (i.threads == 0 || i.edits == 0)
                {
                    return Err(BenchError::new(
                        "suite: [incremental] needs threads >= 1 and edits >= 1",
                    ));
                }
            }
            Kind::KmpLsp => {
                if let Some(k) = &self.kmp
                    && k.picked == 0
                {
                    return Err(BenchError::new("suite: [kmp] picks no names"));
                }
            }
            Kind::Jdtls => {
                if let Some(j) = &self.jdtls
                    && (j.sites == 0 || j.deadline_secs == 0)
                {
                    return Err(BenchError::new(
                        "suite: [jdtls] needs sites >= 1 and a deadline",
                    ));
                }
            }
            Kind::JflLsp => {
                if let Some(j) = &self.jfl_lsp
                    && (j.sites == 0 || j.deadline_secs == 0)
                {
                    return Err(BenchError::new(
                        "suite: [jfl_lsp] needs sites >= 1 and a deadline",
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
        assert!(err.to_string().contains("[e1] belongs to"), "{err}");
    }

    #[test]
    fn should_name_the_missing_section_when_a_suite_lacks_its_own() {
        // given
        let text = "name = \"x\"\ndescription = \"d\"\nkind = \"incremental\"\n";
        // when
        let err = Suite::parse(text).unwrap_err();
        // then
        assert!(err.to_string().contains("needs [incremental]"), "{err}");
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
