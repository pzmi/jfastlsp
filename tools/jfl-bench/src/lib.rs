//! The benchmark harness over the pinned corpus and the comparison arms; imports no production
//! crate. Role: tool only, independent (ADR 0031, BENCH-02).
//!
//! `jfl-bench run <suite>` measures a suite on a corpus checkout and writes JSON results with
//! the machine's facts; `doc/bench/README.md` is the guide. A suite is a TOML file (the built-in
//! ones live in `suites/`): the `rg-floor` suite times `rg` answering name queries end to end
//! through hyperfine, the `e1` suite runs the E1 parse pipeline in process and reports its
//! stages, throughput and RSS. BENCH-03 adds the comparison arms (`arms`: kmp-lsp through
//! hyperfine, jdtls behind `jfl-jdtls` over LSP) and the `incremental` suite of proxies for an
//! index kept fresh. Everything here reports and never gates; the CI gate is BENCH-04.

pub mod arms;
pub mod cli;
pub mod corpus;
pub mod e1;
pub mod error;
pub mod hyperfine;
pub mod incremental;
pub mod lsp;
pub mod machine;
pub mod procfs;
pub mod queries;
pub mod report;
pub mod rng;
pub mod runner;
pub mod sampler;
pub mod stats;
pub mod suite;
