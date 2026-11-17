# Baseline: the comparison arms and the incremental suite (BENCH-03)

The first baseline with every comparison arm, measured before any product code exists, and the
syntax-tier proxies of plan §1.7's incremental rows. It feeds DEC-02 (plan §9.3 (d): "the BENCH
baseline shows the §1.7 targets are reachable on the corpus"). It **reports and never gates**
(ADR 0031); budgets are changed only by the owner, and nothing here loosens one. Every number names
its suite, machine and corpus; the raw reports are in [`results/bench-03/`](results/bench-03/)
(they hold no corpus names: query and type slots carry digests only).

## Machine and method

| | |
|:--|:--|
| Machine | 4-vCPU Intel Xeon **2.10 GHz** VM, 15.7 GiB RAM, Ubuntu 24.04, Linux 6.18, one virtio disk |
| Tools | `bench-tool` build of `jfl-bench`, hyperfine 2.0.0, ripgrep 15.2.0, git 2.43.0, OpenJDK 21.0.12, jdtls 1.61.0 (`scripts/jdtls-fetch.sh`), kmp-lsp 0.27.0 at `cd086c8fa363e442b88223ec8c23fa39ea41d609` (`scripts/kmp-lsp-fetch.sh`; built offline with rustc 1.97.0, binary sha256 `bc1f5774…` in the reports' `kmp-lsp-build`) |
| Tree | from a clean tree, in two rounds. Round 1 (17:56–18:15 UTC, `jfl-bench` on rustc 1.97.0, `machine.commit` `e5658ac`): `incremental`, `e1`, `rg-floor`. Round 2 (18:37–18:54 UTC, after the review fixes and the rebase onto rustc 1.99.1, `machine.commit` `f7165fc`): `kmp-lsp` and `jdtls`. Both commits are provisional local commits whose tree is this commit's code |
| Runs | `just bench <suite> <size>`, one suite at a time; `jdtls-fixture-heap256.json` is the jdtls suite with `heap_mib = 256` |
| Noise | **another agent's lane shared the VM** (load average 0.8–3.2 during the runs); each metric's `spread_pct` is in its report |

**These numbers are slower than BENCH-02's, by more than the machine explains.** BENCH-02's
baseline (README) ran on a 2.30 GHz Xeon of the same shape; this one is 2.10 GHz, a 9.5 % clock
difference. `e1` on spring-framework is +41 % at 4 threads warm (1,574 vs 1,115 ms), +54 % cold
(2,305 vs 1,497 ms) and +66 % at 1 thread warm (6,851 vs 4,118 ms): several times the clock
difference. The co-tenant lane is a confound that these runs cannot separate from the machine.
A sandbox VM is not a reference machine: the absolute targets are checked on the recorded
reference machine (ver §10) and the regression gate on one runner class (BENCH-04). The gaps
between arms (4.6× in time, 12.3× in memory for kmp-lsp against `e1`) are large enough that this
noise does not change them; every absolute number is this VM's under that load.

## Arms

| Arm | Ran here | On | Not run, and why |
|:--|:--|:--|:--|
| `rg` floor (`rg-floor`) | yes | medium, lombok (small: BENCH-02) | — |
| kmp-lsp 0.27.0 (`kmp-lsp`), CLI end to end | yes, every query returned results (`<op>/qNN/matches`) | small, medium, lombok | large (elasticsearch is not fetched in this sandbox; owner, reference machine) |
| jdtls behind `jfl-jdtls` (`jdtls`), over LSP | yes (heap cap 1 GiB, and again at 256 MiB) | `fixture`: the synthetic three-module project, Gradle and Maven imports off | **medium and large: owner-run** (`(owner)` item). A jdtls import of a corpus repository runs its Gradle build, which never happens in the sandbox; `jfl-bench run jdtls medium --trusted-build` is the command for a `bench.yml` dispatch job (BENCH-04) or the owner |
| Our product (`jfl`) | — | — | nothing to measure until IDX and LSP-06; the `e1` pipeline is the syntax tier's stand-in |

kmp-lsp is measured through its CLI (`kmp-lsp index`, `refs`, `find`): each `refs` or `find`
process loads its whole cache, so those numbers are a cold-process cost, not a warm server's
request latency. Its LSP mode was not driven in this task: the CLI covers its index, `refs` and
`find` without a build, and driving it over LSP (the harness's client already does this for
jdtls) is a follow-up for whoever extends the arm, recorded here rather than dropped. Its index covers the
Kotlin files of the tree too, and with no `fd` installed it walked with its built-in walker.

**jdtls memory is set by its configuration.** jfl-jdtls starts the JVM with `-Xms` equal to the
heap cap (up to 1 GiB) and `-Xmx` the cap, so its RSS reflects the configured heap, not what
jdtls needs. On the fixture with the suite's 1 GiB cap: RSS 1,044 MiB, heap committed 945 MiB,
heap used 438 MiB (`jcmd GC.heap_info`; with that much room ParallelGC barely collects). With a
256 MiB cap: RSS 653 MiB, heap used 87 MiB, and the same latencies within noise (definition
3.5 / 13.3 ms, hover 14.2 / 34.9 ms). So the JVM outside the heap (metaspace, code cache, JIT)
is about 400 MiB, and about 650 MiB is a floor for jdtls on 10 files; a real project's need is
the owner-run medium measurement.

## Against plan §1.7

"Recommendation" is a proposal for the owner at DEC-02, never a change made here.

| §1.7 row (corpus) | Budget | Measured here (2.10 GHz VM) | Arms | Recommendation |
|:--|:--|:--|:--|:--|
| Full syntax index, cold, 4 cores (medium) | ≤ 2.5 s | `e1` 2.31 s total cold (2.04 s work), 1.63 s warm; fineract 2.72 s cold (spread 35 %), 1.51 s warm | kmp-lsp `index`, empty cache, files in page cache: **7.47 s** (fineract 7.30 s, junit 1.42 s; round 1: 7.61, 5.44, 1.54 s); jdtls import: owner | **Confirm**, with a caveat: met on medium at 2.31 s of 2.5 s here (BENCH-02's VM: 1.50 s), on a loaded VM; whether it holds is a question for the reference machine, which should be named first. The lombok slot is over it cold here: reported, not a budget of its own |
| Full syntax index, cold (large) | ≤ 13 s | not run (elasticsearch not fetched) | — | Owner run on the reference machine |
| Parse and extract per file | p95 ≤ 5 ms, max ≤ 200 ms | medium p95 2.36 ms, max 63 ms; fineract p95 2.84 ms, max 98 ms | — | **Confirm** |
| Unchanged-file check: warm re-hash (large) | ≤ 0.2 s | medium 85 ms, fineract 79 ms (walk + blake3, 4 threads; `reopen/proxy/t4/rehash_ms`) | — | **Confirm**; large on the reference machine |
| Peak RSS of the syntax tier (large) | ≤ 250 MB | medium 47 MiB (49 MB), fineract 60 MiB (63 MB) (`e1`, 4 threads); large not run | kmp-lsp `index` 581 MiB (medium); jdtls on the 10-file fixture: RSS 1,044 MiB at a 1 GiB heap cap (configured, not needed: 438 MiB heap used), 653 MiB at a 256 MiB cap (87 MiB used) | **Confirm for medium and lombok** (a fifth of the budget); large on the reference machine. Note MiB here, MB in the budget |
| Marginal extra worktree (medium) | < 50 MB, < 2 s (*proposed*) | partial proxy for the time half: the `switch` pair (270 files, the spike's pair) diff 7.6 ms + re-parse 192 ms; it leaves out the walk, stat and re-hash of the new worktree (60–85 ms here, the `reopen` row). The memory half is not measured | jdtls: owner | **Confirm** the time half on this evidence; the memory half has none until LSP-06 measures both |
| `initialize`, warm daemon | < 200 ms (*v3*) | — | jdtls, fresh JVM on the fixture: `initialize` 3.59 s, `ServiceReady` 3.64 s (one session; round 1: 3.34 / 3.38 s) | **Confirm**; a warm-daemon jdtls `initialize` (second client) is a cheap follow-up of the arm |
| `def` / `hover`, warm (medium) | p50 < 20 ms, p95 < 50 ms (*proposed*) | — | jdtls on the fixture: definition 3.7 / 9.3 ms (6 of 6 sites answered), hover 14.2 / 29.0 ms (6 of 6); kmp-lsp `find` (process per query) 1,575 / 1,897 ms | **Confirm**: jdtls on a 10-file project uses well over half the hover p95 budget; the medium numbers are owner-run |
| `refs`, warm, widely used (medium) | p50 < 100 ms, p95 < 300 ms (*proposed*) | — | `rg` floor 55.9 / 60.6 ms (BENCH-02's VM: 43 / 48 ms); kmp-lsp `refs` 1,444 / 1,623 ms; jdtls on the fixture 14.0 / 35.1 ms (5 of 6 sites answered; the sixth is a declaration with no other use) | **Confirm** |
| `sym` prefix, warm (large) | p95 < 50 ms (*proposed*) | — | jdtls `workspace/symbol` on the fixture 1.7 / 7.1 ms | Confirm; nothing on large yet |
| First useful answer, fresh worktree (medium) | < 2 s (*proposed*) | proxy: the full syntax index alone is 1.6 s warm and 2.3 s cold here | jdtls `ServiceReady` on the 10-file fixture alone: 3.6 s | **Confirm, with the plan's reading**: the first answer must not wait for the full index (it says how complete it is), because the index alone is at or over 2 s cold on this VM |
| Warm reopen (large) | < 1 s (*proposed*) | proxy, medium: walk 58 ms, stat of every file 63 ms, full re-hash 85 ms warm, 487 ms cold | kmp-lsp from its cache: 1,432 ms (medium) | **Confirm**: on medium the floor is a tenth of the budget |
| Freshness lag (medium) | p95 < 100 ms to a published generation (*proposed*) | proxy `edit`: re-hash + re-parse + declaration diff p50 0.74 ms, p95 2.74 ms (fineract 0.12 / 2.39 ms) | — | **Confirm**: the syntax work is 3 % of the budget; the rest is the generation's publish (IDX-09) |
| Branch switch or rebase (medium; lombok) | < 2 s to a generation that answers (*proposed*) | proxies: medium rebase 75 ms (60 files), switch 201 ms (270); fineract rebase 290 ms (450), switch 933 ms (2,041 files: 1,752 re-parsed, 289 deleted) | jdtls re-import: owner | **Confirm for medium. Flag for lombok-sized switches**: the syntax work alone (diff and re-parse, median 933 ms) is 47 % of the budget, so re-resolution after a large switch has to be incremental too |
| Widely used type changed: fan-out (medium; lombok) | fan-out per case; p95 < 1 s until dependants answer (*proposed*) | proxy fan-out: medium k00 1,586 files (17 % of the tree), k01 683, k02 596; fineract 933, 880, 466; re-reading the dependants' syntax 511 ms (medium k00) | — | **Confirm**, on one condition for SEM: a type change must re-resolve the dependants from their stored summaries, never re-parse them (re-reading them is a median 511 ms on medium k00, about half the p95 budget) |
| Postings update after an edit (medium) | p95 < 50 ms per file (*proposed*) | no proxy: postings need IDX-11 (the edit's re-parse p95, 2.7 ms, is the syntax part) | — | Keep; IDX-11 measures it |

**DEC-02 in one line:** on the medium corpus every §1.7 row a syntax tier can be measured on is met
or has a syntax floor well under its budget, except that the medium cold full index uses 2.31 s
of its 2.5 s on this slower, shared VM, and a lombok-sized branch switch spends 47 % of its budget
on syntax work alone. kmp-lsp, the arm to beat for the full index, is 4.6× slower and 12.3× larger
than the `e1` pipeline on medium. jdtls takes 3.6 s to become ready on 10 files before any
import; its memory is whatever heap it is given (653 MiB RSS at a 256 MiB cap, with 87 MiB of
heap used), so the owner-run medium measurement decides that comparison.

## The incremental suite

Suite `incremental` (`tools/jfl-bench/suites/incremental.toml`: 1 warm-up, 5 runs, 3 cold runs,
seed 1, 4 threads, 20 edits, 3 types), on **medium** (spring-framework `4cb62d89`) and **lombok**
(fineract `9dfe8dfe`). Every row is a **proxy** (`/proxy/` in its series name); none is full yet.

### Commit pairs

Pinned in `scripts/bench-corpus.sh` and [`bench-corpus.md`](../bench-corpus.md#commit-pairs-the-incremental-suite)
(first-parent ancestors of the pin; the checkout moves from the pair to the pin):

| Size | Role | Commit | Changed `.java` (re-parsed + deleted) |
|:--|:--|:--|--:|
| medium | rebase | `f0fbdf71ed16a021c46eb42712057a184040b5aa` (2026-11-06, 36 commits) | 60 + 0 |
| medium | switch | `a4566465f1f1d98bf52f5cc289b9e63b918bb9c3` (2026-10-08, 240 commits; the E1 spike's pair) | 266 + 4 |
| lombok | rebase | `12e7559288b9106accbbdfc0225c2745fef9582f` (2026-11-10, 58 commits) | 441 + 9 |
| lombok | switch | `c5cc48a1371c14d2688be32bd99462c29c4b98e4` (2026-10-10, 724 commits) | 1,752 + 289 |

### Widely used types chosen

Picked by the harness, not by hand: the three project types imported by name by the most files
(`jfl-bench fanout ROOT --count 3` prints them). Both repositories are Apache-2.0; only the type
names are written here (ADR 0004, `bench-corpus.md`).

| Size | Slot | Type | Importers | Dependants (proxy fan-out) | Files naming it |
|:--|:--|:--|--:|--:|--:|
| medium | k00 | `org.springframework.util.Assert` | 1,546 | 1,586 | 1,643 |
| medium | k01 | `org.springframework.context.annotation.Configuration` | 601 | 683 | 824 |
| medium | k02 | `org.springframework.util.StringUtils` | 585 | 596 | 601 |
| lombok | k00 | `org.apache.fineract.infrastructure.core.api.JsonCommand` | 932 | 933 | 937 |
| lombok | k01 | `org.apache.fineract.infrastructure.core.data.CommandProcessingResult` | 878 | 880 | 919 |
| lombok | k02 | `org.apache.fineract.commands.handler.NewCommandSourceHandler` | 466 | 466 | 467 |

Dependants are the files that import the type by name, or import its package on demand or share
it, and name it as a word: a line scan of the headers, not resolution (`incremental::header`). The
"files naming it" column is an upper bound that includes other types of the same simple name.

### Results (proxy)

| Case | Series (`incremental/<size>/...`) | medium | lombok |
|:--|:--|--:|--:|
| One-file edit | `edit/proxy/total_ms` p50 / p95 (stat + read + blake3, re-parse from scratch, declaration diff; 20 files × 5 runs) | 0.74 / 2.74 ms | 0.12 / 2.39 ms |
| | `edit/proxy/reparse_ms` p50 / p95 | 0.71 / 2.68 ms | 0.11 / 2.32 ms |
| Rebase | `rebase/proxy/diff_ms` (`git diff --name-only`) | 5.8 ms | 7.9 ms |
| | `rebase/proxy/t4/reparse_ms` | 71 ms | 282 ms |
| | `rebase/proxy/t4/total_ms` | 75 ms | 290 ms |
| Branch switch | `switch/proxy/diff_ms` | 7.6 ms | 19.6 ms |
| | `switch/proxy/t4/reparse_ms` | 192 ms | 916 ms |
| | `switch/proxy/t4/total_ms` | 201 ms | 933 ms |
| Widely used type | `fanout/k00/proxy/dependants` | 1,586 | 933 |
| | `fanout/k00/proxy/scan_ms` (find the dependants with the headers in memory) | 4.6 ms | 1.0 ms |
| | `fanout/k00/proxy/t4/reparse_ms` (re-read the dependants' syntax) | 511 ms | 297 ms |
| Warm reopen | `reopen/proxy/walk_ms` / `stat_ms` | 58 / 63 ms | 71 / 65 ms |
| | `reopen/proxy/t4/rehash_ms` warm / `t4/cold/rehash_ms` | 85 / 487 ms | 79 / 459 ms |

Every edit diffed as exactly one added declaration (`edit/proxy/unexpected_diffs` 0). Spreads are
large on the short series (the 5–8 ms diffs up to 82 %; one fan-out scan 275 %): read them as
orders of magnitude, not as a regression baseline.

### From proxy to full

A proxy series keeps running after its full metric arrives, so the history shows how far the
product sits above its own syntax-tier floor. Each card below runs the suite and adds the full
series under the same case, without `proxy`:

| Case | Proxy now | Full metric (reserved name) | Card that makes it measurable |
|:--|:--|:--|:--|
| One-file edit (freshness lag) | `edit/proxy/*` | `incremental/<size>/edit/generation_ms` (p50, p95): disk edit to a published generation that answers | IDX-09 (views, generations, freshness) |
| Postings after an edit | — (none) | `incremental/<size>/edit/postings_ms` (p95 per edited file) | IDX-11 (`refs`, postings) |
| Rebase, branch switch | `{rebase,switch}/proxy/*` | `incremental/<size>/{rebase,switch}/generation_ms` | IDX-09, with IDX-05's changed files |
| Widely used type | `fanout/kNN/proxy/*` | `incremental/<size>/fanout/kNN/reresolved` (files) and `reresolve_ms` | SEM-01, SEM-02 (resolver) |
| Warm reopen | `reopen/proxy/*` | `incremental/<size>/reopen/generation_ms` | IDX-08 (store), IDX-09; through the daemon at IDX-12 |

## Owner-run

- jdtls behind the proxy on the medium corpus with its Gradle import, and the large corpus
  (`e1`, `rg-floor`, `kmp-lsp`) on the reference machine: `jfl-bench run jdtls medium
  --trusted-build` (or BENCH-04's dispatch job), `just bench e1 large`, `just bench kmp-lsp
  large`. Name the machine in the result.

## Reproduce

```bash
just bench incremental medium      # fetches the corpus and its commit pairs
just bench incremental lombok
just bench e1 medium; just bench e1 lombok
just bench rg-floor medium; just bench rg-floor lombok
just bench kmp-lsp small; just bench kmp-lsp medium; just bench kmp-lsp lombok   # builds kmp-lsp once
just bench jdtls fixture           # fetches the pinned jdtls; needs Java 21
```
