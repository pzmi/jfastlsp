# E1 spike: Java syntax-parse speed on real repos (throwaway)

Date 2026-11-16. Archived in `spikes/e1-parse/` by BOOT-02; the raw logs (`results.log`, `floors.log`, `errors-*.txt`) and the clones (`repos/`) were not kept.

## Machine

- CPU: Intel(R) Xeon(R) Processor @ 2.30GHz, 4 vCPUs (cloud VM); 15 GB RAM; virtio disk (`/dev/vda`)
- Kernel: Linux 6.18.44-fc-v470; Rust 1.97.0; git 2.43.0; ripgrep (system)
- Cold cache = `sync; echo 3 > /proc/sys/vm/drop_caches` before the run (works in this VM). Warm = median of 3 runs after a warm-up read.

## Corpus

Shallow clones (`git clone --depth 1`), 1.5 GB on disk in total.

| repo | url | sha | .java files | .java MB | .java LOC | build.gradle(.kts) files | real modules (note) | processors / Lombok | licence |
|---|---|---|---|---|---|---|---|---|---|
| junit-framework | https://github.com/junit-team/junit-framework | e5198f9b098c550e66d378650f78bac8883e2a72 | 1,813 | 7.9 | 229,712 | 18 | ~30 `include`s in settings.gradle.kts | own config processor (`junitPlatformConfigurationProcessor`); no Lombok | EPL-2.0 (weak copyleft) |
| kafka | https://github.com/apache/kafka | 7e0ebdf74f30cab1ded2a462cc9d45cefdf86c2a | 6,307 | 74.0 | 1,618,636 | 5 | ~68 projects declared in settings.gradle, configured from one root build.gradle; also Scala | JMH generator only; no Lombok | Apache-2.0 |
| spring-framework | https://github.com/spring-projects/spring-framework | 4cb62d893e09e23791d661e8d9b35aedee7006d0 | 9,294 | 53.7 | 1,528,956 | 2 | 22 modules, each `spring-x/spring-x.gradle` | none; no Lombok | Apache-2.0 |
| gradle | https://github.com/gradle/gradle | db675891c2bcf8e3c9a7c66ac9de429170a8a9cd | 11,750 | 37.9 | 991,847 | 2,737 | most build files are integration-test fixtures/samples; also much Groovy/Kotlin | internal instrumentation processor; Room in samples; no Lombok | Apache-2.0 |
| elasticsearch | https://github.com/elastic/elasticsearch | 157ebe1325f24f34bf871997e880f0c0293b7279 | 33,850 | 333.9 | 7,499,129 | 567 | ~336 outside qa/test fixtures | log4j plugin processor, own `:benchmarks:processor`, ESQL codegen (`gen`); no Lombok | **SSPL / Elastic License 2.0 / AGPL-3.0 triple — not permissive** |

None of the candidates uses Lombok. (`.java` counts: `git ls-files '*.java'`; the parser's gitignore-aware walk finds the same set ±8.)

## Method

`parse-spike/` (Rust, release, thin LTO): gitignore-aware walk (`ignore`) for `*.java` → rayon pool of N threads, one `tree_sitter::Parser` per thread →
per file: (a) `fs::read`, (b) `blake3::hash`, (c) `parser.parse` with tree-sitter 0.27.2 and `tree-sitter-java-orchard` 0.5.25 (MIT; fallback grammar `tree-sitter-java` 0.23.5 for comparison), (d) a full cursor walk keeping package, imports, class/interface/enum/record/@interface/method/constructor/field names with start/end (row, col) as `Box<str>` in a `Vec` per file (trees dropped, symbol lists kept, so RSS includes a naive symbol table).
`--upto read|hash|parse|extract` stops after the stage, so the stage columns below are cumulative wall times. Per-file ms = read→extract for that file. Peak RSS from `/usr/bin/time -f %M`. Parse error = `root_node().has_error()` (any ERROR or MISSING). Drivers: `bench.sh` (parse), `floors.sh` (rg, git), `oid-diff.sh` (worktree), `summarize.py` (tables). Raw: `results.log`, `floors.log`, `errors-*.txt`.

## Parse speed (E1)

### Warm cache, cumulative stages (median of 3; work = after listing; total includes the gitignore-aware walk)

| repo | thr | read ms | +hash ms | +parse ms | +extract ms | total ms | files/s | MB/s | p50 ms | p95 ms | max ms | peak RSS MB |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| junit-framework | 1 | 8 | 9 | 508 | 579 | 592 | 3131 | 13.7 | 0.135 | 1.143 | 9.2 | 11 |
| junit-framework | 4 | 2 | 2 | 126 | 147 | 157 | 12368 | 54.0 | 0.134 | 1.178 | 9.1 | 15 |
| kafka | 1 | 26 | 43 | 5030 | 5944 | 5975 | 1061 | 12.5 | 0.323 | 3.289 | 135.9 | 45 |
| kafka | 4 | 9 | 13 | 1299 | 1555 | 1594 | 4056 | 47.6 | 0.320 | 3.378 | 137.7 | 84 |
| spring-framework | 1 | 42 | 53 | 3444 | 3954 | 4001 | 2350 | 13.6 | 0.196 | 1.512 | 37.2 | 35 |
| spring-framework | 4 | 14 | 16 | 877 | 997 | 1042 | 9326 | 53.9 | 0.198 | 1.535 | 40.4 | 46 |
| gradle | 1 | 41 | 57 | 2210 | 2460 | 2685 | 4774 | 15.4 | 0.086 | 0.798 | 8.3 | 30 |
| gradle | 4 | 14 | 19 | 580 | 660 | 909 | 17788 | 57.3 | 0.087 | 0.801 | 8.4 | 33 |
| elasticsearch | 1 | 134 | 212 | 23940 | 26925 | 27149 | 1257 | 12.4 | 0.389 | 2.684 | 57.8 | 138 |
| elasticsearch | 4 | 41 | 65 | 5993 | 6975 | 7192 | 4853 | 47.9 | 0.394 | 2.737 | 63.1 | 153 |

### Cold cache (echo 3 > drop_caches before each run; single run)

| repo | thr | list ms | read-only work ms | read MB/s | full extract work ms | full total ms | files/s | MB/s | peak RSS MB |
|---|---|---|---|---|---|---|---|---|---|
| junit-framework | 1 | 99 | 94 | 83.9 | 777 | 876 | 2333 | 10.2 | 11 |
| junit-framework | 4 | 54 | 30 | 262.3 | 193 | 246 | 9417 | 41.1 | 15 |
| kafka | 1 | 142 | 324 | 228.3 | 6626 | 6768 | 952 | 11.2 | 44 |
| kafka | 4 | 109 | 131 | 563.0 | 1652 | 1761 | 3817 | 44.8 | 71 |
| spring-framework | 1 | 169 | 450 | 119.5 | 4866 | 5036 | 1910 | 11.0 | 34 |
| spring-framework | 4 | 191 | 180 | 298.3 | 1232 | 1423 | 7543 | 43.6 | 47 |
| gradle | 1 | 1026 | 605 | 62.5 | 3389 | 4415 | 3465 | 11.2 | 30 |
| gradle | 4 | 1128 | 201 | 187.8 | 885 | 2014 | 13266 | 42.7 | 33 |
| elasticsearch | 1 | 829 | 1675 | 199.4 | 28933 | 29762 | 1170 | 11.5 | 138 |
| elasticsearch | 4 | 954 | 601 | 555.4 | 7740 | 8694 | 4373 | 43.1 | 156 |

### Parse-error rate (files whose tree has ERROR or MISSING), warm, 4 threads

| repo | files | decls+imports extracted | orchard err files | upstream (tree-sitter-java 0.23.5) err files | orchard extract ms | upstream extract ms |
|---|---|---|---|---|---|---|
| junit-framework | 1813 | 42198 | 0 (0.00%) | 6 (0.33%) | 148 | 157 |
| kafka | 6307 | 225048 | 3 (0.05%) | 3 (0.05%) | 1593 | 1526 |
| spring-framework | 9294 | 237869 | 0 (0.00%) | 58 (0.62%) | 983 | 1003 |
| gradle | 11743 | 204414 | 11 (0.09%) | 12 (0.10%) | 641 | 628 |
| elasticsearch | 33850 | 1203767 | 0 (0.00%) | 16 (0.05%) | 7022 | 6881 |

Notes:
- **Throughput is ~12–15 MB/s per core and ~48–57 MB/s on 4 cores (3.7–3.9× scaling)**, regardless of repo. Read + blake3 together are ≤1% of the warm run (elasticsearch: 65 ms of 6,975 ms on 4 threads) and ~8% cold.
- The declaration walk adds ~12–15% over bare parsing.
- Max per-file time is set by giant test files: kafka's 1.6 MB `GroupMetadataManagerTest.java` takes ~137 ms; elasticsearch's 650 KB test files ~60 ms. p95 stays under 3.5 ms everywhere.
- Cold listing hurts only repos with large non-Java trees (gradle 1.0 s, elasticsearch 0.8–1.0 s), where it is 12–55% of the total time.
- RSS: 150 MB for elasticsearch with 1.2 M declarations and imports kept naively, about 120 B each. Parsing itself needs little memory.
- **Grammar:** orchard parses every file in junit, spring-framework and elasticsearch. Its 14 error files in kafka and gradle are real non-Java: Maven archetype and performance-test templates with `${...}` placeholders, plus gradle's deliberate `BrokenClass.java`. Upstream 0.23.5 also fails on Java 25 syntax: `import module` and compact source files in junit, and 58 spring-framework files (JSpecify type annotations on varargs, `Object @Nullable ... args`, e.g. `BeanFactory.java:218`). Orchard costs the same as upstream to within ±3%.

## Floors

`rg -c -t java '\bString\b' .` (default threads = 4, and `-j1`); `git status --porcelain`; `git ls-files`. Wall seconds.

| repo | rg cold | rg warm | rg warm -j1 | matches | git status cold | git status warm | `git ls-files '*.java' \| wc -l` | `git ls-files -s` | parse+extract warm 4 thr (from above) |
|---|---|---|---|---|---|---|---|---|---|
| junit-framework | 0.08 | 0.01 | 0.02 | 6,062 | 0.19 | 0.00 | 0.00 | 0.00 | 0.16 |
| kafka | 0.21 | 0.03 | 0.10 | 62,929 | 0.40 | 0.02 | 0.00 | 0.00 | 1.59 |
| spring-framework | 0.25 | 0.04 | 0.11 | 50,167 | 0.90 | 0.03 | 0.00 | 0.00 | 1.04 |
| gradle | 0.63 | 0.11 | 0.37 | 34,959 | 1.70 | 0.19 | 0.01 | 0.01 | 0.91 |
| elasticsearch | 1.06 | 0.18 | 0.55 | 233,349 | 0.86 | 0.15 | 0.02 | 0.02 | 7.19 |

A full syntax index costs about 40× a warm rg scan on elasticsearch (7.2 s against 0.18 s) and about 25× on spring-framework.

## Worktree marginal cost (spring-framework)

Deepened the clone to 300 commits and ran `git worktree add --detach ../spring-framework-wt2 HEAD~150` (2026-10-09; HEAD is 2026-11-14). The two trees have 9,294 and 9,286 `.java` files, and 270 of them differ.

| operation | wall |
|---|---|
| `git worktree add` (checkout of 96 MB) | 0.40 s |
| `git status` in the new worktree, cold / warm | 0.87 s / 0.03 s |
| diff by blob OID: `git ls-files -s -- '*.java'` in both + sort/join (`oid-diff.sh`) | 0.02–0.03 s (the two `ls-files` take 20 ms) |
| `git diff --name-only <old> <head> -- '*.java'` (tree diff, for reference) | 0.01 s (269 files) |
| blake3 re-hash of all 9,286 files in wt2, 4 thr, cold / warm (incl. walk) | 0.38 s / 0.05–0.08 s (hash work alone: 174 ms / 16 ms) |
| blake3 re-hash, 1 thread, warm | 0.10 s |
| full re-parse + extract of wt2, 4 thr (from the spring-framework row) | ~1.0 s warm / 1.4 s cold |
| re-parse of only the 270 changed files (estimate: 270 × p50 0.2 ms ÷ 4) | ~15–55 ms |

What this shows: the OID diff is about 2× cheaper than a warm re-hash and 15× cheaper than a cold one. **But index OIDs describe the index, not a dirty working tree.** You have to know the tree is clean, and a cold `git status` (0.87 s) costs more than a cold blake3 re-hash (0.38 s). Re-hashing is cheap enough to be the source of truth: about 3 GB/s warm and about 300 MB/s cold on 4 threads. Blob OIDs remain useful as a free first pass to find candidates, and as a content-addressed share key between worktrees, so the second worktree's marginal cost is about 270 parses instead of 9,300.

## Recommendations

**Corpus**
- **Small:** junit-framework (1.8k files, 7.9 MB, EPL-2.0). It is fast enough for the inner loop and exercises Java 25 syntax (`import module`, compact source files).
- **Medium:** spring-framework (9.3k files, 54 MB, Apache-2.0, pure Java, 22 modules, JSpecify `@Nullable` type-use annotations everywhere). Use kafka (6.3k files, 74 MB, Apache-2.0) as the "huge single files + Scala mix + one-root-build" variant. Its 1.6 MB test file is a good worst-case for per-file latency.
- **Large:** elasticsearch (33.9k files, 334 MB, 7.5 M LOC, ~340 Gradle modules, codegen annotation processors). It is the only candidate above 30k. Its licence is SSPL/ELv2/AGPL, so clone it at bench time only: never vendor it, and never commit fixtures or snippets from it. No permissive candidate reaches 30k; gradle/gradle (11.7k, Apache-2.0) is the fallback large-ish permissive repo and the many-build-files stress case.
- Pin the SHAs above. Get the bench harness's numbers with `drop_caches` cold runs and median-of-3 warm runs.

**Plan targets (syntax tier only, 4 cores of this VM)**
- Full re-parse + declaration extract of the large repo: **7.2 s warm and 8.7 s cold** on 4 cores; 27–30 s on 1 core. Medium repo: 1.0–1.6 s warm and 1.4–1.8 s cold. Small repo: 0.16–0.25 s.
- Unchanged-file detection: walk + blake3 of the whole large repo takes 0.1 s warm and ~1.5 s cold (1 s of that is the gitignore walk). That is 50–100× cheaper than re-parsing.
- What this means for storage: an in-memory-only index (re-parse at every start) is fine for small and medium repos (≤ ~2 s cold). It does not meet a sub-second start on the large repo (≥ 7 s). The large tier needs **persisted per-file syntax summaries keyed by content hash (blake3, or git blob OID as a pre-filter)**, so a restart costs walk + hash (~0.1–1.5 s) plus re-parse of changed files only. I could not read plan v3 §5 from here; map "persisted, content-addressed summary store" onto its storage tier. The threshold is around 10–15k files or 50 MB of Java on 4 cores.
- Suggested budgets to pin as benchmarks (≈1.5× headroom over this VM): large cold full index ≤ 13 s on 4 cores; medium ≤ 2.5 s; single-file re-parse p95 ≤ 5 ms and max ≤ 200 ms (1.6 MB file); re-hash of large repo warm ≤ 0.2 s. Peak RSS of the syntax tier ≤ 250 MB for the large repo.
- Grammar: use tree-sitter-java-orchard. It costs the same as upstream and has no false errors on Java 25 syntax or annotated varargs, while upstream 0.23.5 marks 0.3–0.6% of files in junit and spring as broken.
