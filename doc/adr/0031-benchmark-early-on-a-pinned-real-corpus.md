# 0031. Benchmark early on a pinned real-repository corpus; regressions over 10 % block

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07); amended
2026-11-16. The owner's
direction (2026-11-16): "test on some Java projects and benchmark early; the point of this
project is speed".

## Context

BOOT-00 put benchmarks at M5 (REL-05) and tested only on small synthetic fixtures. Speed is the
product's primary value (plan §1.1, §1.7), so a regression found at M5 is found too late, and a
toy fixture hides the costs that matter on a large Gradle build (file counts, deep packages,
generated sources, many worktrees). Real open-source Gradle Java repositories exist at every size
and can be fetched at a fixed commit without committing their code.

## Decision

- **A pinned benchmark corpus** of public Gradle Java repositories at fixed commits, listed in
  [`bench-corpus.md`](../bench-corpus.md) with licence and size: junit-framework (small),
  spring-framework and kafka (medium), elasticsearch (large), gradle/gradle (permissive large
  fallback). A script fetches them into a cache directory outside the repository (BENCH-01); their
  code is never committed, and elasticsearch, which is not permissively licensed, is cloned at
  bench time only. An E1 spike on this corpus (2026-11-16) set the first budgets of plan §1.7.
- **A harness from day one** (`tools/jfl-bench`, tool only, independent, BENCH-02): hyperfine for
  the CLI end to end, divan or criterion for in-process benchmarks; results as JSON with machine
  facts. Metrics: cold and warm index time, files per second, RSS, the marginal cost of each extra
  worktree, query p50 and p95 per operation, freshness lag after an edit.
- **Comparison arms:** `rg` as the floor, jdtls behind our proxy (ADR 0028), kmp-lsp at a pinned
  commit (BENCH-03).
- **Real-corpus golden cases:** expectations keyed by repository, commit, file and position, with
  `scip-java` as the oracle, from the Apache-2.0 repositories only; the code stays in the fetched
  corpus (BENCH-05).
- **A budget table** in plan §1.7. From M1 on it is a blocking gate: a tracked metric that
  regresses by more than 10 % against the stored history fails, and milestone gates check the
  budget on the recorded reference machine.
- **History:** [github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark)
  (MIT, data in this repository's `gh-pages` branch, `alert-threshold: 110%`,
  `fail-on-alert`), chosen under the reuse rule (ADR 0019) because it needs no external account
  or token. BENCH-04 confirms its current version and pins it by SHA.
- **Every product task** runs on the corpus (small at least) and reports the metrics it changes.

## Consequences

The BENCH lane runs beside FND and phase 0. E1 (EVAL-05) and E4 (EVAL-06) reuse the harness and
corpus. Shared CI runners are noisy, so tracked CI metrics are medians of several runs, a
regression must reproduce once before it counts (ver §1), and absolute budgets are judged on the
reference machine. REL-05 keeps only the mutation lane.

## Alternatives considered

- [bencher.dev](https://bencher.dev): statistical thresholds and a dashboard, but an external
  service with an API token; the runner-up if the 10 % threshold proves too noisy.
- Benchmarks at M5 only: too late for a speed product. Committing corpus code: licences and size.

## Amended 2026-11-16 (planner, BOOT-02): where the 10 % gate runs

A local `just check` or `land.sh` would compare a sandbox run with a history recorded on GitHub
runners, a different machine, and github-action-benchmark compares single results, not medians.
So the blocking gate runs only in `bench.yml`, on one fixed runner class, against the history of
that class: `jfl-bench` runs each suite N times and writes the median, and a regression must
reproduce in a second run before the job fails. In-process suites gate on instruction counts
(`gungraun` or cachegrind) where available, which do not depend on a noisy neighbour, and report
wall time beside them. Local and sandbox runs report and never gate. BENCH-04 builds it; IDX-15
makes it blocking.
