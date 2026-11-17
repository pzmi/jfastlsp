//! The agent-evaluation harness around Harbor (ADR 0029, EVAL-01); imports no production crate.
//! Role: tool only, independent (fnd §2.7).
//!
//! `jfl-eval harbor` turns a task set ([`taskset`]) and arms ([`arm`]) into Harbor tasks
//! ([`harbor`]); `jfl-eval smoke` writes the smoke task that shows whether language servers start
//! under `claude -p` in a Harbor container; `jfl-eval report` reads the trials Harbor leaves
//! ([`run`]), extracts per-run metrics from Claude Code transcripts or Harbor trajectories
//! ([`transcript`], [`metrics`]) and aggregates them per arm and per task ([`report`]) into a
//! report of ids and numbers only. Harbor, Docker and Claude Code run only on the owner's machine;
//! everything here is tested on synthetic fixtures. `doc/eval/harness.md` is the runbook.

pub mod arm;
pub mod bounded;
pub mod cli;
pub mod error;
pub mod harbor;
pub mod ids;
pub mod metrics;
pub mod report;
pub mod run;
pub mod taskset;
pub mod transcript;
