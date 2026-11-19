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
just bench-fs small [cold]         # jfl-adapter-fs's walk (paths, paths and stat; 1 and 4 threads; warm, and cold only when asked: it drops the page cache) and a watch's start (divan), after checking the listing is git ls-files' set
just bench-queries medium          # jfl-application's definition, hover and workspace symbols over the view service on the corpus (IDX-10)
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

### The walk (IDX-06, `just bench-fs`)

`jfl-adapter-fs`'s `list`, measured 2026-11-18 on the 4-vCPU VM above (divan medians of 100
samples warm; 3 samples cold, each after a page-cache drop; the `bench` profile; another build
lane was running, so read them as an upper bound). Each corpus's listing equals
`git ls-files '*.java'` before anything is timed.

| Corpus (files) | Paths, 1 thread | Paths, 4 threads | Paths and stat, 1 thread | Paths and stat, 4 threads | Cold, paths / and stat, 4 threads | Watch start (watched directories) |
|:--|--:|--:|--:|--:|--:|--:|
| small, junit-framework (1,813) | 9.0 ms | 5.5 ms | 10.8 ms | 6.1 ms | 25.6 / 25.0 ms | 29.4 ms (801) |
| medium, spring-framework (9,294) | 39.3 ms | 16.5 ms | 52.1 ms | 19.7 ms | 65.8 / 78.2 ms | 101.4 ms (2,802) |
| lombok, fineract (7,301) | 37.1 ms | 15.2 ms | 48.1 ms | 17.9 ms | 65.0 / 74.2 ms | 120.4 ms (2,861) |

Watch start is the IDX-06 review's design: the walk of the directories the exclusion rules accept
and one non-recursive inotify watch for each, every one a round trip to `notify`'s thread (about
35 µs a directory); `notify`'s recursive watch of the whole tree, `.git` included, took 9.6, 55
and 53 ms. The walk columns are the first run's; a rerun on 2026-11-18 gave the same within the
noise (medium, 4 threads: 19.0 ms paths, 22.4 ms with stat). Cold runs only with
`just bench-fs SIZE cold`.

For comparison, `e1`'s serial walk (`list_ms`, hidden directories skipped) took 41–83 ms on
medium and 54–71 ms on lombok, and the `incremental` suite's `reopen/proxy/walk_ms` 58 ms
(medium) and 71 ms (lombok).

### The git adapter (IDX-05, `just bench-git`)

`jfl-adapter-git`'s calls, measured 2026-11-18 on the 4-vCPU VM above (divan medians, the `bench`
profile). Each command runs through `jfl-process` with the adapter's hardening. Before anything is
timed, the adapter's tracked `.java` files must equal `git ls-files '*.java'`, a tracked file must
resolve to the checkout, and a status must leave the stat set unmoved. The diff compares two pinned
trees 100 files apart, made in a shared clone under cargo's scratch directory. The corpus
checkouts are detached, so their stat set is `HEAD` and the index.

| Corpus (files) | Stat set | Resolve, cached | Resolve, first | `rev-parse` | `diff --name-only` | `ls-files -z` | `status -z` | `changed_since` |
|:--|--:|--:|--:|--:|--:|--:|--:|--:|
| synthetic (2,000, an agent worktree) | 6.7 µs | 25 µs | 1.62 ms | 1.64 ms | 2.10 ms | 2.36 ms | 10.7 ms | 13.7 ms |
| small, junit-framework (1,813) | 2.9 µs | 19 µs | 1.97 ms | 1.72 ms | 2.18 ms | 3.13 ms | 10.9 ms | 14.3 ms |
| medium, spring-framework (9,294) | 3.1 µs | 23 µs | 1.76 ms | 1.83 ms | 2.97 ms | 9.30 ms | 40.9 ms | 44.0 ms |

`changed_since` is `rev-parse`, the `git config` listing of filter drivers (about 1.7 ms, so they
can be turned off) and `status`; the base is `HEAD`'s tree, so there is no diff. These are the
numbers after the IDX-05 review.
Measured by hand on medium, `--untracked-files=no` instead of `all` takes `status` from 30–40 ms to
10 ms. With `GIT_OPTIONAL_LOCKS=0`, `status` cannot refresh an index whose entries are racy, so it
rehashes them on every call: on the freshly made synthetic repository that is all of its 10 ms,
and it drops below 5 ms once a plain `git status` has refreshed the index.

### The syntax index (IDX-07, `just bench-syntax`)

`crates/jfl-composition/benches/syntax.rs` reads every tracked `.java` file of the corpus into memory, then indexes them all with `jfl-adapter-java`, using orchard.
- Each file goes through: the BOM, the bounds and the `\u` translation, then the parse, the queries' table, the summary and the diagnostics, all within one work budget.
- It runs on 1 and 4 threads, one parser per thread. Each thread count gets a warm-up and three runs; the median is reported.
- It lives in composition because timing and threads need grants the adapter does not hold.

Measured 2026-11-18 (IDX-07 round 2) on the 4 vCPU Xeon VM **while another lane was running (load 1.3 to 2.5)**.
- Each row sits beside E1's own extract stage (`jfl-bench parse --upto extract`, median of 3) run straight after on the same machine.
- **These ratios come from single runs on a loaded machine.** EVAL-05 and `bench.yml` confirm on a quiet runner.
- Plan §1.7's quiet-machine E1 numbers are faster than both columns.

| corpus | threads | wall ms | files/s | p50 / p95 / max ms | E1 wall ms, p95 | ratio |
|:--|--:|--:|--:|:--|:--|--:|
| medium spring-framework (9,294 files, 53.7 MB) | 1 | 8,120 | 1,145 | 0.43 / 3.02 / 62.8 | 6,434, 2.48 | 1.26× |
| | 4 | 2,003 | 4,639 | 0.42 / 3.07 / 65.0 | 1,700, 2.56 | 1.18× |
| lombok fineract (7,301 files, 50.5 MB) | 1 | 6,964 | 1,048 | 0.29 / 3.45 / 103.2 | 5,250, 2.64 | 1.33× |
| | 4 | 1,852 | 3,942 | 0.31 / 3.68 / 122.9 | 1,583, 2.92 | 1.17× |

Round 3 (the type-parameter map), one run later the same day under load 2 to 3: medium took
7,350 ms at 1 thread and 2,534 ms at 4, against E1's 5,977 and 2,051 (1.23× and 1.24×).

**Work budget headroom.** The bench prints the most of its work budget any file used. It was
2.8 % on medium (`AutowiredAnnotationBeanPostProcessorTests.java`) and 3.2 % on lombok
(`LoansApiResourceSwagger.java`), so a real file sits about 30 times below the refusal.

Earlier single runs (round 1, the same day) gave 1.06–1.25× on small, medium and lombok, and lombok at 4 threads 1.44×.

**Summary bytes per file** (`heap_bytes` plus the value):

| corpus | mean | p95 |
|:--|--:|--:|
| small | 5.2 KB | 18.4 KB |
| medium | 6.1 KB | 20.7 KB |
| lombok | 6.9 KB | 24.8 KB |

- A medium file has 14.9 declarations and 135 reference sites on average.
- No file of the three corpora has a syntax error or is refused, so none runs out of the work budget. E1 agrees on the syntax errors.
- Peak RSS with every summary held: 24 MB small, 88 MB medium, 109 MB lombok.

**Where the time goes.** Measured with callgrind over 300 junit-framework files: the adapter executes 1.54 times the parse's own instruction count, against about 1.15 for E1. Of the adapter's instructions:

| share | what |
|--:|:--|
| 65 % | the parse |
| 11 % | the cursor walk |
| 6 % | declarations: signatures, erasure, Javadoc |
| 5 % | reference sites: sort, deduplication, the locals lookup |
| about 4 % | interning and header text |
| rest | allocation and dropping the tree |

E1 extracts no reference sites, no locals and no signatures.

**Changes that moved the numbers:**
- Running the same queries through tree-sitter's query engine cost 2.0 times the parse's instructions. That is why the queries are now read as a table (ADR 0022 as amended 2026-11-18).
- Cloning the capture lists per file kept 4 threads from scaling. Removing the clones took medium from 2.6 s to 2.1 s, and p95 from 5.1 ms to 3.3 ms.

### The summary store (IDX-08, `just bench-store`)

`crates/jfl-composition/benches/store.rs` summarises every tracked `.java` file with `jfl-adapter-java` (4 threads), keys each by its blake3 `SyntaxKey`, then measures `jfl-adapter-store` in a scratch directory under cargo's `CARGO_TARGET_TMPDIR` (removed at the end).
- A full write into an empty store on 1 and 4 threads, then every entry read back and compared with what was put; the store's open (the scan its cap needs); the load of the whole view: every whole summary (the small and medium tier), and every file's declarations plus the name postings built from them (the large tier, plan §5.5).
- With `cold` (`just bench-store medium cold`), also loads after dropping the page cache (Linux, root; it disturbs the whole machine, so only when asked).
- The postings' size and lookup time over every name, a `refs`-shaped read of the three most used names' files, and both tiers' memory projected linearly to elasticsearch's 33,850 files.
- Medians of three runs after a warm-up; it lives in composition because timing and threads need grants the adapter does not hold.

Measured 2026-11-18 (IDX-08) on the 4 vCPU Xeon VM **while another lane was running (load 1.2 to 3.1)**; write times moved by up to 3× between runs, so read them as orders of magnitude.

| corpus | encoded B/file (mean, p95, max) | on disk | full write, 4 threads | open | whole view, warm | declarations + postings, warm | in memory: whole / declarations + postings |
|:--|:--|--:|--:|--:|--:|--:|:--|
| small junit-framework (1,813 files) | 2,961, 10,104, 90,637 | 5.1 MiB | 115–384 ms | 4 ms | 8 ms | 7 + 6 ms | 9.0 / 6.7 MiB |
| medium spring-framework (9,294 files) | 3,301, 11,104, 140,734 | 29.3 MiB | 177–285 ms | 17–19 ms | 44–63 ms | 36–40 + 51–54 ms | 53.6 / 38.1 MiB |
| lombok fineract (7,301 files) | 4,015, 14,275, 261,986 | 28.0 MiB | 154 ms | 17 ms | 44 ms | 31 + 50 ms | 47.9 / 33.9 MiB |

- **Real-corpus check**: every entry of all three corpora (18,408 files) reads back equal to what was put, whole and as declarations.
- **Encoded size**: an entry is 0.55–0.58 of the summary's bytes in memory (postcard varints, ranges as start and length, reference starts as deltas).
- **Cold** (medium): open 28 ms, whole view 85 ms, declarations and postings 119 ms. This VM's disk sits on the host's page cache, so a guest cache drop is not a cold disk; EVAL-05 measures cold on CI hardware.
- **Postings** (medium): 44.4k names, 310k postings (33 names per file), 2.3 MiB; a lookup takes about 180 ns. A `refs`-shaped read of `String` lists 61 % of the view's files and reads them whole in 31 ms warm.
- **Projected to elasticsearch** (linear in files, from medium; lombok in brackets): open 62 ms; whole view 160–228 ms; declarations and postings 317–343 ms (373 ms), against plan §1.7's warm reopen under 1 s with the walk and hash at about 0.1 s. Memory: whole summaries 195 MiB (222 MiB), declarations and postings 139 MiB (157 MiB; the postings scale as an upper bound, since names repeat across files), against the 250 MB syntax-tier RSS budget; the store on disk 107 MiB (130 MiB), a tenth of the default 1 GiB cap.
- **Why the postings are a sorted table, not `fst`** (a one-off measurement, not a bench arm): on medium, a version of the postings over an `fst` map took 101–126 ms in this bench's postings step against 50–54 ms for the table, for lookups no faster (211 against 190 ns). The arm was not kept. Details are in `crates/jfl-adapter-store/src/postings.rs` and plan §5.5.
- **What memory the large tier keeps**: declarations hold the whole string arena, reference names included, so they are about 0.7 of a whole summary; dropping the strings only reference sites use would cut that further (a later optimisation, if EVAL-05's RSS asks for it).

### The view registry (IDX-09, `just bench-views`)

`crates/jfl-composition/benches/views.rs` `lstat`s every tracked `.java` file and measures `jfl-application`'s view registry over those real stats (warm; no summary read or stored, no cookie file written: the store and the syntax index have their own suites, and the cookie's round trip through the watcher lands with the registry's async shell).
- The stat of every file and the open of a view from them (generation 1 published).
- One request's freshness path over 2,000 files spread over the view, five times: its file's `lstat` (the clock read first, the racy rule applied), then `request_start` (bind to the innermost view, the stat compared with the published entry), the cookie delivered back, the pin (`HEAD` and the index re-statted and compared, completeness computed) and `finish`; p50 and p95 of the registry's part and of the `lstat`.
- One changed file reconciled and published (its build beside the published generation), and a full rescan (`sync`: every file re-statted, nothing changed, published). Medians of five runs.

Measured 2026-11-18 (IDX-09) on the 4 vCPU Xeon VM, one other lane running:

| corpus | stat of every file | open | request, registry (p50, p95) | request, its `lstat` (p50, p95) | one change published | full rescan |
|:--|--:|--:|--:|--:|--:|--:|
| small junit-framework (1,813 files) | 2.1 ms | 0.19 ms | 0.69, 2.2 µs | 1.1, 2.4 µs | 0.21 ms | 2.5 ms |
| medium spring-framework (9,294 files) | 9.0 ms | 1.3 ms | 0.60, 1.9 µs | 1.0, 2.3 µs | 1.25 ms | 12.9 ms |

- **Per request** the registry's part does not grow with the view: a pin reads the generation's file count and latest read stamp, both counted at publish (a first version counted the files at every pin: 13.6 µs p50 on medium). Every request was complete (the corpus did not change while it ran).
- **A publish** copies the generation's index (a build sits beside the published one, which pinned requests keep): linear in the view's files, 1.25 ms on medium, about 4.5 ms projected to elasticsearch's 33,850 files. A persistent map would make it proportional to the change; not needed at these numbers.
- **The rescan takes the view's listing** since the review's round 2 (new files are read, the view certified only with one): one medium rerun gave 14.3 ms for the full rescan and an unchanged request path (p50 0.70 µs), within this VM's noise, so the table keeps the first run.
- **The cookie's round trips** (one, or two when a summary was read after the first), which plan §1.7's 100 ms freshness target bounds, are not in these numbers: they are the watcher's latency, measured by the view service's half below.

#### The view service (IDX-09, the second half of `just bench-views`)

`crates/jfl-composition/benches/view_service.rs` drives `jfl-application`'s view service over the real adapters: the fs tree (inotify), the git probe, the Java indexer and the on-disk summary store in a scratch directory. Every port runs its blocking work on one pool of four threads, as composition's bounded pool will (the bench has no runtime). The service keeps 16 files in flight (`ServiceConfig::parallelism`) and opens whole only views of up to 256 files (`ServiceConfig::open_whole`); `--parallelism N` and `--open-whole N` override both. The cookie's deadline polls the clock (no runtime timer).
- **Stages:** each step of a cold open alone, over the adapters' blocking calls, on 1 and 4 threads: the listing with stats, stat, read, blake3, parse and summarise, encode, encode and write into a fresh store (in the files' order and in key order), a store miss; and one hand-off to the pool and back.
- **View open, empty store, warm page cache:** a fresh service with an empty store, after the stages have read every file. *First answer*: the first request returns, its view opened with only its file's directory read. *Whole view published*: `settle` has read every other file and published the full syntax index. *Store written*: `write_back` has written every summary, after the publish. One run.
- **View open, page cache dropped** (`just bench-views SIZE cold`; Linux, root): only the open above, three times, each with an empty store after `echo 3 > /proc/sys/vm/drop_caches`, which is what plan §1.7's "cold" rows mean (E1's). Nothing else runs. `--pool N` sets the pool's threads.
- **View open, warm:** the same with the store filled (a daemon restart): every file read and hashed, its summary read back. Three runs.
- **Cookie round trip:** 200 requests on unchanged files spread over the view, each writing its cookie into the checkout's root and waiting for inotify to deliver it; p50 and p95, and the whole request's time.
- **Rescan with listing:** `sync`, five times: listing and `git ls-files`, every indexed file re-statted, nothing changed, published. Median.

Measured 2026-11-19 (IDX-09, slice 3, after its review) on the 4 vCPU Xeon @ 2.10 GHz VM (15 GB, ext4 mounted with `discard`). The other lane was idle; the load average was 0.5 to 2.9, mostly from the bench itself. Medium is the median of three runs (each run's value in brackets), small one run of each kind.

| corpus | page cache dropped, empty store: first answer / whole view published | warm page cache, empty store: first answer / whole view published | store written (warm cache, from start) | warm store: first answer / whole view | cookie round trip (p50, p95) | whole request (p50, p95) | rescan with listing | complete |
|:--|--:|--:|--:|--:|--:|--:|--:|--:|
| small junit-framework (1,813 files) | 133 ms (141, 133, 131) / 524 ms (525, 524, 497) | 62 / 399 ms | 1,184 ms | 86–101 / 152–180 ms | 0.094, 0.215 ms | 0.343, 0.502 ms | 25.4 ms | 200 of 200 |
| medium spring-framework (9,294 files) | **385 ms** (712, 385, 376) / **2,742 ms** (3,299, 2,742, 2,740) | 194 ms (205, 194, 191) / 2,207 ms (2,207, 2,295, 2,180) | 3.7–6.5 s | 251–358 / 592–754 ms | 0.091, 0.177 ms | 0.305, 0.505 ms | 100.0 ms | 200 of 200 |

Against plan §1.7 on medium, where cold means after a page-cache drop (E1's runs):
- **The first useful answer on a fresh worktree is under 2 s:** 385 ms with the page cache dropped, 194 ms warm. It is incomplete (`ModulePartlyIndexed`), as the row allows.
- **The full syntax index, cold, on 4 cores is over its 2.5 s:** 2,742 ms with the page cache dropped. With a warm page cache it is 2,207 ms.
  - The drop costs about 0.5 s, against E1's 0.38 s (1.42 s cold, 1.04 s warm). About 170 ms of it is before the first answer: the walk of 2,800 directories, the watch, `git` and the binary's own pages. About 300 ms is in `settle`'s 53.7 MB of reads.
  - A pool of 6 or 8 threads, so that reads waiting on the disk overlap the parse, gave 2,603–2,818 ms (`--pool 6`, `--pool 8`, 24 files in flight), within noise of four.
  - The rest of the gap is the parse itself: 1.90–1.97 s on four threads here, against E1's extract at about 1.04 s warm (the adapter's summary does more, `just bench-syntax`).
  - This VM's disk sits on the host's cache, so a guest drop may not be a cold disk (`just bench-store`'s note); EVAL-05 measures CI hardware.
- **Under a neighbour's build** (load 3 to 4) the parse alone took 2.5 s, so even the warm-cache index is over budget then.

**Where the cold open's time goes** (medium, the stages, three runs; 4 threads unless noted):

| step | time | in the open |
|:--|--:|:--|
| watch, `HEAD` stat set, listing with stats, `git ls-files`, the request's directory, cookie, pin | about 200 ms | the first answer (the watch alone is about 100 ms: `just bench-fs`) |
| listing with stats | 23–29 ms | once, at the open |
| stat / read / blake3 | 7–8 / 41–46 / 8–11 ms | each file in `settle` |
| store miss | 72–76 ms | skipped: an empty store is not asked |
| parse and summarise | **1,919–1,926 ms** (6,681–6,951 ms on 1 thread) | each file in `settle`: the floor |
| hand-offs to the pool | 66 µs a round trip, four per file, overlapped by 16 files in flight | about 150–250 ms over the floor |
| publish | a copy of the index, 1.3–2 ms (the registry's own numbers above) | once |
| encode | 71–78 ms | in `write_back`, after the publish |
| encode and write, files' order | 590–1,546 ms (2,202–2,320 ms on 1 thread) | in `write_back`, after the publish |
| encode and write, key order | 272–396 ms (449–474 ms on 1 thread) | the order `write_back` writes in |

So with a warm page cache the whole view publishes at about the parse's floor plus the first answer's open plus the hand-offs; a dropped cache adds about 0.5 s of reads. What changed, and what each was worth (medium, warm page cache, quiet, one run each unless noted):
- **Store writes off the publish path.** Before, each file was written to the store before the publish, from the bench's single thread: 1.5 to 3 s of a cold open. They now run after the publish (`ViewService::write_back`). The registry's atomic-publish guard is untouched: a generation holds its own summaries (slice 2), so the disk store is only the reopen cache.
- **Every port on the pool.** Before, the reads, hashes and store calls ran on the calling thread and only the parse was parallel (a thread per parse): 5.7 s. With every port on the four-thread pool, 4 files in flight published at 3,143 ms, 8 at 2,598 ms, 16 at 2,328 ms and 32 at 2,214 ms. Each step is a hand-off to a pool thread and back (66 µs here, a parked vCPU's wake-up), so the pool needs more files in flight than threads. The default is 16; 32 was within noise of it.
- **No store lookup when the store is empty.** A cold open's every lookup misses: 2,160–2,207 ms against 2,214–2,328 ms before (four runs).
- **The first answer does not wait for the open.** The view opens with the request's directory (7 files here) and answers `ModulePartlyIndexed` in about 200 ms (385 ms cold). `settle` then reads the rest; a request on a file still pending reads just that file.
- **Bytes in flight are capped** (`ServiceConfig::read_budget`, 32 MiB; a larger file is read alone), not only the count: 16 files of the 16 MiB maximum would otherwise be 256 MiB, the whole syntax-tier budget. The corpus's files never reach it (medium's mean is 6 KB), and the medium numbers did not move.
- **The store's writes** take 1.5 to 4.3 s after the publish on this VM, more than the stages' 0.3 to 1.5 s for the same writes. The spread follows what the disk did just before (each run deletes the previous stores, and the root filesystem discards freed blocks online) and the write order: one file per entry, created and renamed under the store's lock. Neither plan row includes them, but they hold the service (`&mut self`) while they run, so a daemon writes back in slices between requests (the bench writes 256 at a time). A cheaper write (an entry pack per batch, or a writer that does not take the service) is left to the store's owner (IDX-08, DEC-03's large tier).

Before slice 3 (2026-11-19, slice 2 after its review, another lane building at load about 4), the cold open read, parsed and stored every file before the first answer: 744 ms on small and 5,723 ms on medium (12,847 ms with one parse at a time, the slice's first run). The warm open (153–180 ms small, 620–737 ms medium) is unchanged within noise. The cookie round trip and the whole request now go through the pool (the file's stat, the Git stat set at the start and at the pin, and the cookie's write each hand off to it), so a request costs 0.3 ms p50 where the calling thread took 0.06 ms; that is composition's choice of where a stat runs (IDX-12), far inside plan §1.7's 100 ms freshness target.

- **The cookie** costs well under 0.1 ms through inotify, more than a thousand times inside plan §1.7's 100 ms freshness target; a request that needs a second cookie pays it twice. The 200 ms deadline (`ServiceConfig::new`) is never near.
- **The rescan** is the listing (the walk, `just bench-fs`) plus `git ls-files` plus the stats of every indexed file, 16 at a time on the pool: 96–120 ms on medium. The shell does not narrow a rescan with `git diff` and `status` (`changed_since`), which alone cost more (`just bench-git`: 39 ms for `status` on medium) than every stat does.

### The queries (IDX-10, `just bench-queries`)

`crates/jfl-composition/benches/queries.rs` opens and settles the corpus's view through the view service over the real adapters (as `view_service.rs` does: four pool threads, a scratch store), then measures each query over the published generation:
- **The first query of a generation** builds its name tables (`QueryCache`: every declaration by simple name, the types apart, each type's members by parent, the distinct names laid out for the symbol scan): what the first query after each publish pays.
- **Definition and hover** at 200 reference sites picked by a fixed seed from the view's summaries: the query alone (the request already pinned), and the request plus the query (stat, cookie through inotify, pin, query; plan §1.7's `jfl def` row adds the carrier's framing and IPC to this). The tally says what the definitions answered.
- **The syntax facts** a definition proves `exact` from (IDX-10 slice 2), over every file of the view with a reference site: the parse alone, the parse and one site's walk (what each definition pays, design (a), taken), and every site's walk beyond the parse, summed (what keeping the facts in the summary would add to indexing, design (b), not taken); one thread.
- **Workspace symbols** for 50 type names picked by the same seed: their first three letters, the whole names, their humps' initials, one letter; and the prefixes with a limit of one, which leaves the scan and a single file read (the rest of a default answer, 100 symbols, is the reads of their files' text, checked against the generation's keys).

Measured 2026-11-19 (IDX-10, as committed: resolved names gated to `possible`) on the 4 vCPU Xeon @ 2.10 GHz VM, the other lane idle, load 0.3 to 0.6; three runs of each corpus, the median run's numbers (the others within 0.3 ms on definition and 1.5 ms on symbols); the other symbol kinds were not rerun after the gate, which does not touch them (last run: limit 1 4.6, 6.5 / exact 3.7, 8.4 / humps 4.8, 11.0 / one letter 9.9, 13.3 ms on medium):

| corpus | name tables built | definition alone (p50, p95) | request and definition (p50, p95) | hover alone (p50, p95) | symbols, prefix (p50, p95) |
|:--|--:|--:|--:|--:|--:|
| small junit-framework (1,813 files) | 10.9 ms | 0.17, 0.60 ms | 0.51, 1.09 ms | 0.16, 0.64 ms | 3.9, 5.1 ms |
| medium spring-framework (9,294 files) | 79.2 ms (74–82) | 0.21, 2.30 ms | 0.55, 2.67 ms | 0.18, 2.23 ms | 9.0, 12.1 ms |

What the 200 definitions at reference sites answered (the same picks every run): small 0 exact, 125 possible only, 32 outside the index, 43 unresolved; medium 0, 123, 33, 44. No reference site is on the allowlist of `exact` (only a declaration's own name is), so none is `exact` by design until the syntax-tree resolver; the scope rules' candidate is among the `possible` hits. Before the gate the rules called 46 of the medium sites (66 of small) exact, and the first version 66 (80); two reviews with javac as the oracle showed the rules wrong on constructs they misread.

**Slice 2, the syntax-tree resolver** (2026-11-23, the same VM, three runs of each corpus, the median of each figure; symbols unchanged within noise): what the 200 definitions answered, small 37 exact, 88 possible only, 32 outside, 43 unresolved; medium 34, 89, 33, 44 (javac checks every `exact` over the fixture projects: `jfl-adapter-java`'s `tests/definitions/main.rs`). A definition now parses the request's file when its site's shape could be proved (not a qualified site behind anything but `this.`, not a method reference):

| corpus | definition alone (p50, p95) | request and definition (p50, p95) | hover alone (p50, p95) | a file's parse (p50, p95, max) | parse and one site's walk (p50, p95, max) | every site's walk, summed (design (b)) |
|:--|--:|--:|--:|--:|--:|--:|
| small | 0.83, 6.5 ms | 1.18, 6.9 ms | 0.75, 6.8 ms | 0.13, 1.05, 8.8 ms | 0.11, 0.99, 8.4 ms | 1.5 s beside 0.53 s of parses (1,790 files) |
| medium | 1.02, 7.5 ms | 1.35, 8.1 ms | 0.94, 8.1 ms | 0.18, 1.38, 36 ms | 0.16, 1.31, 33 ms | 14.2 s beside 3.5 s of parses (9,211 files) |

The slice's first version (one run each, 2026-11-19) took 3.1 and 15.4 ms alone on medium (small 2.2, 9.9 ms): it parsed for every site, and its walk out to the unit asked each node for its parent, which tree-sitter answers by descending from the root again (quadratic in depth). A definition picks files by site, so larger files weigh more than in the per-file columns; the largest medium file with sites (316 KB) parses in 34–55 ms, inside definition's 300 ms deadline. The slice 1 rows above are the floor without the parse; against them this is a regression past ADR 0031's 10 %, pending the owner's sign-off (the card).

Against plan §1.7 (the rows measured end to end through a carrier from M1; these are the in-process floor):
- **`jfl def` / `jfl hover`, warm (medium), p50 < 20 ms, p95 < 50 ms:** the request and the query take 1.35 ms p50 and 8.1 ms p95 with slice 2's parse (0.55 and 2.7 ms without it). The first query after a publish adds the tables' build, 74–82 ms on medium, which alone is over the p95: an incremental build from the previous generation is a named item on the card, due before the M1 freshness and definition rows are gated.
- **`jfl sym` prefix query, warm (large), p95 < 50 ms:** 12.1 ms p95 on medium for a default answer of 100 symbols, about half of it the reads of the listed symbols' files; the scan grows with the distinct names (96k identifiers on medium), so large (3.6 times the files) projects to about 25–35 ms. Not measured on large here (the corpus is not cached in this sandbox).
- **`fst` was weighed for the symbol scan** (ADR 0025) in a one-off measurement outside the repository on the medium corpus's 96,046 distinct identifiers: a set built in 91–122 ms (plus 7–16 ms to sort the names first), against prefix queries of 0.13 ms where a linear prefix-and-subsequence scan takes 3.6 ms. The names are rebuilt with every generation, so the build would land on the first query after each publish, and the camel-hump search the card asks for is not a prefix or a Levenshtein automaton: not taken (the card's `Decided`).

### The daemon's IPC (IDX-12, `just bench-daemon`)

`crates/jfl-composition/benches/daemon.rs` times the daemon as a client meets it: the test daemon binary (the `jfl daemon` entry) spawned detached through `jfl-process` by a real proxy, in a scratch state directory, release build.

- **Spawn to ready**: a proxy that finds no daemon until the answer to its first request: the spawn lock, the second check, the spawn, the notify handshake, connect, the version handshake and one `jfl/status`. Twenty spawns, each daemon stopped with `jfl/stop` after.
- **Connect and handshake** against a running daemon: a new connection, its handshake request and the daemon's answer, 500 times.
- **A trivial request's round trip**: `jfl/status` (no view in it) on one open connection, 5,000 times.

No corpus: none of it reads a Java file (the queries' costs are the views' and the resolver's suites). Measured 2026-11-19 (IDX-12) on the 4 vCPU Xeon VM (Intel Xeon @ 2.10GHz), one other lane building:

| | p50 | p95 |
|:--|--:|--:|
| spawn to ready | 1.94 ms | 2.29 ms |
| connect and handshake | 50.1 µs | 85.2 µs |
| `jfl/status` round trip | 36.0 µs | 59.6 µs |

The table is the review round's build (2026-11-19): the proxy waits with `poll(2)` instead of
socket timeouts, the notify wait polls the child's pidfd, the daemon writes through a writer task
per connection. The first build measured spawn to ready 1.51 / 1.79 ms, connect and handshake
52.5 / 94.4 µs, the round trip 48.1 / 59.4 µs: the round trip lost its two `setsockopt` calls per
read; spawn to ready gained about 0.4 ms (opening the pidfd, the writer task, and another lane's
build beside it: within this VM's noise for 20 samples).

- **A cold `jfl` call** against a running daemon pays the connect and handshake once (about 0.05 ms) and a round trip per request (about 0.04 ms): together under 1 % of `jfl def`'s proposed p50 of 20 ms (plan §1.7); the first call of a session that must spawn the daemon pays about 2 ms more, before any index exists.
- **The round trip** is the floor every query's answer adds to its own work: two frames through `jfl-protocol`'s decoder, one current-thread runtime hop and the writer task's.

## Adding a suite

1. A `suites/<name>.toml` with `name`, `description`, `kind` and the knobs of its kind
   (`suite.rs` documents them; a built-in is listed in `suite::BUILTIN`).
2. A new `kind` is a variant of `suite::Kind` and a function in `runner.rs` returning metrics.
   Name them `<suite>/<size>/...`, say which way each improves (`Better`), and leave out, with a
   `notes` line, what the platform cannot measure.
3. A test that needs no corpus, no `hyperfine` and no `rg`: `just check` must not depend on them.
