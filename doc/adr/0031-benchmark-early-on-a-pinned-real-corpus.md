# 0031. Benchmark early on a pinned real-repository corpus; regressions over 10 % block

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17); amended
2026-11-16 and 2026-11-17. The owner's
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

## Amended 2026-11-17 (BENCH-02): what the harness links, and what it is built with

`tools/jfl-bench` landed with these consequences for the policy files; each is the smallest
entry that lets it build, and each says here why (fnd §3.5).

- **The E1 pipeline is in the harness, not borrowed from the product.** The `e1` suite runs the
  archived spike's walk, read, blake3, tree-sitter parse and declaration walk in process, so
  `jfl-bench` links `tree-sitter` and `tree-sitter-java-orchard`, and `deny.toml` names it among
  their `wrappers`. The harness imports no production crate (fnd §2.1: a judge that shares code
  with the judged cannot catch its mistakes); the product's own indexer (IDX-07) is measured
  beside the spike's numbers, not through them.
- **`foldhash` (Zlib) is admitted by a per-crate licence exception.** tree-sitter 0.27.2 declares
  `serde_json` with `preserve_order` (indexmap, hashbrown, foldhash) in its own
  `[build-dependencies]`, unconditionally; the crate is a host build dependency and is never
  linked, but `cargo deny` walks it. Zlib is permissive; the exception names the one crate, not
  the licence, so a second Zlib crate still fails `licenses` until someone reads it.
- **Eight build scripts are admitted by checksum** (`[bans.build] bypass`), each read at the
  pinned version: blake3, tree-sitter and tree-sitter-java-orchard compile their vendored C or
  assembly with the `cc` crate and set cfgs; tree-sitter-language emits wasm header paths only
  for a wasm target; crossbeam-deque, crossbeam-epoch and crossbeam-utils set sanitizer and
  no-atomic cfgs; rayon-core only declares `links`. The task that adds `tree-sitter` to
  `jfl-adapter-java` (IDX-07) reuses these reviews and says so.
- **Tools:** hyperfine 2.0.0 (times a command end to end, and reports each run's peak RSS) and ripgrep 15.2.0 (the floor arm) are
  pinned in `mise.toml` and `mise.lock`; a release that changes either changes the baseline, so
  a bump is its own commit with a re-run.
- **A build profile, `bench-tool`** (`inherits = "release"`, thin LTO, one codegen unit): the
  spike's release settings, without which the 4-thread full index is 6–11 % slower
  (doc/bench/README.md). Only `just bench` builds with it.
- **Peak RSS comes from the tools, not a sampler on the measured path:** hyperfine 2.0.0 reports
  each run's `memory_peak_resident` (the export is read as `schema_version` 2 and any other is
  refused), and the in-process suite reads its own `VmHWM` at exit (Linux). `jfl-bench sample`,
  a `/proc` spin loop, remains a debugging command only: it competes for a core. Cold runs use
  hyperfine's `--prepare` (a page-cache drop) so warm and cold share one method.
- **Series are keyed by query slot** (`q00`...): a history key never names a corpus identifier, and
  a change of pin or seed does not rename series; the report carries only a digest per slot. The
  walk and `rg` ignore every ignore file outside the checkout, so the file set is the pinned
  corpus's on any machine.
- **divan and criterion are not added yet.** The card asks for the in-process convention, not a
  benchmark with no subject; `doc/bench/README.md` writes the convention and the first product
  task with a hot path adds the dependency and the first `benches/` target.
