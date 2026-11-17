# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The product and architecture plan, the engineering foundation, the verification strategy, the
  development process, the ADRs (0001–0031) and the task breakdown.
- `scripts/bootstrap-tools.sh` (pinned, checksum-verified mise and just) and the archived E1 parse
  spike (`spikes/e1-parse/`).
- `tools/jfl-bench` (BENCH-02): the benchmark harness. `jfl-bench run <suite> [size]` runs the `rg-floor`
  suite (hyperfine, queries picked by seed) and the `e1` parse suite (the archived spike, in process)
  on a fetched corpus checkout and writes JSON with the machine's facts, in the format
  github-action-benchmark reads; `compare` states the noise between two runs. `doc/bench/README.md`.
- Agent instructions (`CLAUDE.md`, `.claude/`), the MIT licence (ADR 0033) and repository hygiene files.
