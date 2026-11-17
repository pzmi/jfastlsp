# Benchmarks

How jfastlsp is measured (ADR 0031, plan §1.7, ver §10). Speed is the product's primary value, so
every claim about latency, memory or throughput names the suite that measured it, the machine, the
corpus and the numbers. The harness is `tools/jfl-bench`; it **reports and never gates**. The gate
is `bench.yml`'s `bench-gate` (BENCH-04), which reads what this harness writes.

## Run it

```bash
just bench-corpus small            # fetch and verify the pinned corpus (BENCH-01), once
just bench rg-floor small          # the ripgrep floor on junit-framework
just bench e1 medium               # the E1 parse pipeline on spring-framework
just bench incremental lombok      # the incremental proxies on fineract (fetches the commit pairs)
just bench kmp-lsp medium          # the kmp-lsp arm (builds the pinned kmp-lsp once)
just bench jdtls fixture           # the stock jdtls arm, on the synthetic fixture
just bench path/to/suite.toml small
jfl-bench compare a/e1-small.json b/e1-small.json     # two runs of one suite (default band 20 %)
just bench-domain small            # jfl-domain's position conversions in process (divan), after a corpus round-trip check
just bench-domain-envelope         # jfl-domain's envelope assembly, completeness, summary build and API digests (divan, synthetic)
just bench-protocol                # jfl-protocol's encode, frame decode and envelope decode of a 1k- and 10k-location response (divan, synthetic)
just bench-lsp                     # jfl-adapter-lsp's round trip of one request through a session in memory, and a 1k- and 10k-location answer on the typed path and through a JSON value (divan, synthetic)
```

`just bench [suite] [corpus] [out]` builds `jfl-bench` with the `bench-tool` profile (thin LTO, one
codegen unit: the E1 spike's settings, without which the 4-thread full index is 6–11 % slower) and runs the
suite. **`jfl-bench run` refuses a debug build** (`--allow-debug` overrides, and the report says so in
`notes`); every report records the profile, `debug_assertions` and the toolchain it was built with
(`build`). Results go to `target/bench/` (or `out`) as `<suite>-<corpus>.json` plus the history files
below. `hyperfine` and `rg` are pinned by mise; the recipe puts the real binaries first on `PATH`,
because a mise shim in front of either adds its launcher's start-up time and memory to every run
(the report says so in `notes` when it happens anyway). `jfl-bench` alone also runs `parse`,
`queries`, `machine` and `sample` (a debugging aid, see below); `jfl-bench` with no arguments
prints them.

Nothing from the corpus is ever committed. Run on a quiet machine: close other builds, and read
the report's `spread_pct`. Needs hyperfine 2.0.0 (its export `schema_version` 2; any other is
refused) and ripgrep 15.2.0, as `mise.toml` pins.

## Suites

A suite is a TOML file (`tools/jfl-bench/suites/`, embedded in the binary; or any path ending in
`.toml`). The corpus size is on the command line, not in the file. Unknown keys are errors.

| Suite | Measures | Tool | Metrics (`<suite>/<size>/...`) |
|:--|:--|:--|:--|
| `rg-floor` | `rg -c -w -t java --no-config --no-ignore-global --no-ignore-parent --no-ignore-exclude NAME ROOT`: what a name query costs with no index | hyperfine 2.0.0 (`-N`, 3 warm-ups, 20 runs) | per query slot `qNN`: `p50`, `p95` (ms), `rss_peak` (MiB), `matches`; the spike's own form `spike00` (`rg -c -t java '\bString\b' .` run in the checkout); over all slots: `p50`, `p95`, `rss_peak`; `cold_ms` |
| `incremental` | syntax-tier **proxies** for an index kept fresh (BENCH-03): one edit, a rebase and a switch between the pinned commit pairs of `doc/bench-corpus.md`, the fan-out of the most-imported types, a warm reopen | `jfl-bench`, `git` | `<case>/proxy/...`: `edit/proxy/{rehash,reparse,decl_diff,total}_ms/{p50,p95}` (over edits of 20 seed-picked files), `{rebase,switch}/proxy/diff_ms`, `t4/reparse_ms`, `t4/total_ms`, `changed_files`, `deleted_files`; `fanout/kNN/proxy/{dependants,importers,mentions,scan_ms,t4/reparse_ms}`; `reopen/proxy/{walk_ms,stat_ms,t4/rehash_ms,t4/cold/rehash_ms}` |
| `kmp-lsp` | the kmp-lsp arm at its pinned release (`scripts/kmp-lsp-fetch.sh`): `kmp-lsp index` with an empty cache and with its cache, `refs` and `find` for the first 10 of `rg-floor`'s picks | hyperfine 2.0.0 | `index_ms`, `index_rss_peak`, `index_cached_ms`; `refs/p50`, `refs/p95`, `find/p50`, `find/p95` (median over queries), `refs/rss_peak`, `find/rss_peak`; `refs/qNN/matches`, `find/qNN/matches` (result lines of one untimed run; an exit that is neither a result nor kmp-lsp's "nothing found" fails the run) |
| `jdtls` | the pinned stock jdtls (`scripts/jdtls-fetch.sh`, checksum-verified; started by `src/jdtls.rs` with a fresh `-data` per run, nothing of ours in between) over LSP: `initialize`, time to `ServiceReady`, then `definition`, `references`, `hover`, `workspace/symbol` at usage sites of seed-picked types; only `fixture` in the sandbox | `jfl-bench` (an LSP client) | `initialize_ms`, `ready_ms` (one session); per operation `p50`, `p95` (over all requests), `found`; `backend_rss_peak`, `backend_rss` (the JVM, found as the `timeout`'s child in `/proc`, bounded by the configured heap: `-Xms` is the cap up to 1 GiB), `backend_heap_used`, `backend_heap_committed` (`jcmd GC.heap_info`) |
| `e1` | the E1 pipeline in process, stage by stage (`jfl-bench parse`, one process per measurement) | `jfl-bench` | per thread count `tN` and stage `read`, `hash`, `parse`, `extract` (cumulative): `work_ms`, `total_ms`; for the full index also `list_ms`, `files_per_s`, `mb_per_s`, `file_p50_ms`, `file_p95_ms`, `file_max_ms`, `rss_peak_mib`, `rss_steady_mib`, and the counts `files`, `err_files`, `failed_files`, `decls`; `cold/<stage>/work_ms` and `total_ms` |

**Queries are picked from the corpus by seed**, not by hand, and **series are keyed by slot**
(`q00` is the first fixed word, `q01`... the picked type names in pick order), never by name: a
history key names nothing of the corpus, and a change of pin or seed does not rename series. The
same seed on the same pinned commit gives the same names on every machine, because the walk
reads only the checkout's own ignore files (below); `jfl-bench queries ROOT --count 20 --seed 1`
prints names with file, line and column, and the report carries only a digest per slot
(`queries[].name_blake3`). `tests/queries.rs` freezes the slot order for a fixed tree. Names are
looked up at bench time and never written into this repository.

**The file set is the corpus's, whatever the machine.** The walk (`ignore`) and `rg` honour the
`.gitignore`, `.ignore` and `.rgignore` files inside the checkout and nothing else: not the
user's global gitignore, not `.git/info/exclude`, not parent directories' ignore files, not
`RIPGREP_CONFIG_PATH` (`tests/cli.rs` pins it with a global ignore and a parent ignore). Hidden
directories, so nested `.claude/worktrees/`, are skipped. A file over 16 MiB is counted as
failed (`failed_files`) and never read whole.

**What `e1` does** (rewritten from `spikes/e1-parse/`, doc/bench-corpus.md): the walk, then per
file on a rayon pool with one tree-sitter parser per worker: read, blake3 hash, parse with
tree-sitter-java-orchard, and a walk that keeps package, imports and every declaration (kept in
memory until the run ends, as an index would). `work_ms` is the pool's time after listing,
`total_ms` includes the walk. The per-file percentiles are of the per-file wall time inside the
pool. `checksum` (in `jfl-bench parse` output) is an order-independent digest of what was
extracted: equal across runs and thread counts, and a note in the report names any run where
files, declarations or checksum differ.

**Cold runs** drop the page cache (`sync; echo 3 > /proc/sys/vm/drop_caches`, Linux and root):
for `rg-floor` through hyperfine's `--prepare` before each of 3 runs, so warm and cold share one
method; for `e1` before each of 3 runs. Where the drop is not allowed the cold series are left
out and `notes` says why. **RSS:** `rg-floor` takes each run's `memory_peak_resident` from
hyperfine (the kernel's rusage of that child) and reports the maximum; `e1` reads its own
`VmHWM` and `VmRSS` at the end of the run (the steady figure is the resident set with the index
still held; Linux). Without them the series are absent and `notes` says so; they are never
approximated. `jfl-bench sample -- CMD` (a `/proc` spin loop) is for looking at a process by
hand and is on no measured path: it competes for a core.

## Results

Three files per run, so the history tool needs no adapter:

| File | Holds |
|:--|:--|
| `<suite>-<corpus>.json` | everything: suite, corpus (repository, commit, `.java` count), **machine facts** (CPU model, logical CPUs, RAM, OS, kernel, architecture, the corpus disk, `hyperfine`/`rg`/`git` versions, the commit measured and whether the tree was dirty; never a host name), how the binary was built (`build`), the query slots, the repetition knobs, every metric with its `stat` (in words), `runs`, `value` and `spread_pct`, and `notes` |
| `<suite>-<corpus>.smaller.json` | the metrics that improve downward (times, memory) as github-action-benchmark's `customSmallerIsBetter` array: `{name, unit, value, range, extra}` |
| `<suite>-<corpus>.larger.json` | the throughput metrics as its `customBiggerIsBetter` array |

Counts (`files`, `decls`) are in the full report only: they have no better direction. Every metric
says what its value is in `stat`: `median of 3 runs`, `p95 of 20 runs`, `median over 20 queries`,
`1 run`, `count`; `spread_pct` is `(max - min) / median` of the repetitions and is absent where
there are none (one run, or an aggregate over queries), so a single measurement is never shown as
a median and the history tool's `extra` carries the same words. Metric names never change once a
series exists (BENCH-04 keys its charts on them). `jfl-bench compare A B [--band PCT] [--strict]` prints
the change per metric and flags those outside the band; `--strict` fails on one, which is for a
person or a later gate to choose, never `check`.

## The noise band, and the E1 spike's row

Measured on a 4-vCPU Intel Xeon 2.3 GHz VM (15 GB RAM, Ubuntu 24.04, Linux 6.18, one virtio
disk) with ripgrep 15.2.0, hyperfine 2.0.0 and the `bench-tool` build (rustc 1.97.0), from a
clean tree; the raw reports are in [`results/`](results/) (small run 1 and medium, the tree of
commit `544e88f`; they hold no corpus names). `e1` figures are the median of 3 runs after one
warm-up; 4 threads, warm unless noted.

**Honest status of acceptance item 3 ("two runs on the same machine agree within the noise
band").** The 20 % band was **chosen after** looking at these runs, not before; it is a
description of what this VM did, not a derived bound. Four runs of each small suite were
compared pairwise (6 pairs each, `jfl-bench compare`, band 20 %). Every pair agrees on the
headline series: `e1` stage times for parse and extract at 1 and 4 threads, files/s, per-file
p95, peak RSS (all pairs; e.g. 4-thread full index 156, 166, 157, 171 ms), and the `rg-floor`
aggregates `p50`, `p95`, `rss_peak` (13.5, 14.9, 13.9, 13.4 ms). The band is **not** met, in
some pairs, by: `e1` per-file `file_max_ms` (+21..26 %), the cold read stages (+20..48 %, the
worst in the single-thread cold read), the 3 ms 4-thread `read` stage, and per-query `rg-floor`
`p50`/`p95` (20 slots; 22 outside-band values over the 6 pairs, up to 29 %). Read those as
untrusted at the 20 % level; track the headline series, and treat one run as a measurement, not a
verdict (ver §1: a regression must reproduce). An earlier series on the same VM during a busy
period (before this fix round; not kept) had every 4-thread stage 55 % slower in one run, so the band also depends on
the neighbours; `spread_pct` of each metric is in the report.

| junit-framework (`e5198f9b`, 1,813 files), 4 threads | Spike (2026-11-16) | `just bench e1 small`, run 1 | run 3 |
|:--|--:|--:|--:|
| Full index, warm (`work_ms` / `total_ms`) | 0.16 s | 156 / 167 ms | 157 / 167 ms |
| Files per second, warm | 12,368 | 11,604 | 11,543 |
| Per-file p95 / max | 1.2 / 9 ms | 1.22 / 10.6 ms | 1.20 / 10.2 ms |
| Peak RSS | 15 MB | 15.9 MiB | 16.2 MiB |
| Cold (`work_ms` / `total_ms`) | 0.25 s | 208 / 262 ms | 209 / 271 ms |

The junit `e1` row reproduces: time within 3 %, files/s 6 % below, memory +5 %, the rest inside
the band. (The spike's table does not say whether its 0.16 s is `work_ms` or `total_ms`; both fit.)

**The `rg` floor row did not reproduce, and the reason is not established.** The spike's table
says `rg -c -t java '\bString\b'` takes 0.04 s warm on **spring-framework**, measured once with
GNU `time` (10 ms resolution) in the checkout. The harness now runs that exact command form
(`spike00`: `\bString\b`, no `-w`, `.` in the checkout, plus the `--no-config` and
`--no-ignore-*` flags, which change nothing on this corpus) through hyperfine 2.0.0: 59.5 ms on
spring-framework (median of 20, spread 40 %), against 62.6 ms for the `-w` form (`q00`), so `-w`
is about 5 % and not the cause. The remaining 1.5x is unexplained: `e1` on the same VM reproduces
the spike to 6 %, which does not suggest a uniformly slower CPU, but rg is syscall-heavy (130 ms
of system time across its threads here), and the filesystem state, the host or the one-shot GNU
`time` method could each differ. On junit-framework `spike00` is 17.0–19.1 ms and `q00` 17.3–21.4
ms. Treat the spike's 0.04 s as a different method's number, not as a target.

### First baseline (2026-11-17, this VM, clean tree)

`e1` and `rg-floor` on junit-framework (small, run 1) and spring-framework (medium, `4cb62d89`,
9,294 files); 4 threads, warm unless noted. Peak RSS: `e1` `VmHWM`, `rg-floor` hyperfine's
`memory_peak_resident`.

| Suite and metric | small | medium |
|:--|--:|--:|
| `e1` full index, 1 thread, warm (`work_ms`) | 636 ms (2,852 files/s) | 4,118 ms (2,257 files/s) |
| `e1` full index, 4 threads, warm (`work_ms`) | 156 ms (11,604 files/s, 51 MB/s) | 1,115 ms (8,332 files/s, 48 MB/s) |
| `e1` per-file p95 / max | 1.22 / 10.6 ms | 1.63 / 42.5 ms |
| `e1` peak RSS, 4 threads | 15.9 MiB | 46.3 MiB |
| `e1` full index, 4 threads, cold (`work_ms` / `total_ms`, median of 3) | 208 / 262 ms | 1,308 / 1,497 ms |
| `rg-floor` p50 / p95 (median over 20 queries) | 13.5 / 15.2 ms | 43.2 / 47.8 ms |
| `rg-floor` `spike00` (the spike's form) p50 | 17.8 ms | 59.5 ms |
| `rg-floor` peak RSS | 5.8 MiB | 6.1 MiB |
| `rg-floor` cold (first query, median of 3) | 72 ms | 268 ms |

The spike's spring-framework `e1` row (1.04 s, 9,326 files/s, 46 MB) is 7 % faster in time, 11 %
in files/s, and within 5 % on memory.

## Comparison arms and the incremental suite (BENCH-03)

The first baseline with every arm, the comparison against plan §1.7 and the recommendations for
DEC-02 are in [`baseline.md`](baseline.md). In short:

- **Arms.** The `rg` floor (`rg-floor`), **kmp-lsp** at a pinned release (`kmp-lsp`; its own
  index and cache, timed end to end) and **stock jdtls** (`jdtls`, an LSP client in the
  harness over jdtls's stdio; no thread and no timed wait: the session runs under `timeout(1)`,
  which ends a server that stops answering). The launcher (`src/jdtls.rs`) refuses an install
  whose `.verified` is not the pinned digest, gives every run a fresh `-data` and configuration,
  caps the heap and clears the environment; `--jdtls-home DIR` points at another verified
  install. Before FND-12 the arm ran jdtls behind the since-removed `jfl-jdtls` proxy, with the
  same JVM options (plus a shared index location, unused with one workspace). jdtls's import runs the project's build, so the sandbox runs it
  only on `fixture`, the synthetic three-module project in `tools/jfl-bench/fixtures/multi-module`,
  with the Gradle and Maven imports off and its source roots given as `java.project.sourcePaths`;
  a corpus run needs `--trusted-build` and is a `bench.yml` dispatch job (BENCH-04) or an owner
  run. The fixture is opened as a scratch copy, because jdtls writes Eclipse metadata into it.
- **Incremental suite.** Before IDX exists every series is a **proxy** and says so in its name
  (`incremental/<size>/<case>/proxy/...`). The full metric of a case is written by the card that
  makes it measurable, under the same case without `proxy` (`incremental/<size>/edit/generation_ms`
  from IDX-09); the proxy series keeps running beside it, so the history shows how far the
  product is above its own syntax-tier floor. `baseline.md` maps each proxy to its full metric and
  card. The edit is applied in memory, so the corpus stays clean; the pairs are fetched into the
  pinned checkout (`scripts/bench-corpus.sh fetch-pairs SIZE`, which `just bench incremental`
  runs), which never moves.

## In-process benchmarks (the convention for product crates)

The harness times a process from outside; a hot function is timed from inside, in the crate that
owns it. The convention, for the first product task with a hot path (IDX-07 onwards):

- A `benches/<name>.rs` target in the crate, `harness = false`, with **divan** (a statistical
  micro-benchmark with `#[divan::bench]`, generic over inputs, allocation counts); **criterion**
  only where a statistical comparison of two implementations is the point (it keeps a baseline and
  reports a change with a confidence interval).
- Inputs are synthetic or generated in the benchmark, never the corpus (nothing from it enters
  this repository), unless the benchmark takes a corpus path from an argument, as `jfl-bench`
  does, or the corpus files on stdin from a recipe, as `jfl-domain`'s `positions` does (a pure
  crate holds no filesystem grant; `just bench-domain`). A benchmark function is named for what it measures (`parse_declarations_of_a_large_file`),
  not `should_...`: benchmarks are not tests, and `check` never runs them.
- Tracked in-process suites gate on instruction counts where available (`gungraun` or cachegrind,
  BENCH-04), which do not depend on a noisy neighbour, with wall time reported beside them.
- A product crate that adds `benches/` adds divan or criterion with `cargo add --dev`, lists it
  in `architecture.toml`'s `[external.dev]`, and its card says so (a benchmark with no subject is
  not added early: BENCH-02 writes the convention only).

What needs a product arrives with it: freshness lag after an edit and `initialize` against a warm
daemon with LSP-06 (the M1 gate) for `jfl`; the marginal cost of 1, 3 and 10 worktrees (the
corpus helper `scripts/bench-corpus.sh worktrees` creates them) with LSP-06 for `jfl` and with E4 (EVAL-06) for
jdtls; BENCH-03 added the comparison arms beside `rg`.

## Adding a suite

1. A `suites/<name>.toml` with `name`, `description`, `kind` and the knobs of its kind
   (`suite.rs` documents them; a built-in is listed in `suite::BUILTIN`).
2. A new `kind` is a variant of `suite::Kind` and a function in `runner.rs` returning metrics.
   Name them `<suite>/<size>/...`, say which way each improves (`Better`), and leave out, with a
   `notes` line, what the platform cannot measure.
3. A test that needs no corpus, no `hyperfine` and no `rg`: `just check` must not depend on them.
