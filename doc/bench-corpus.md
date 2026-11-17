# Benchmark corpus

The repositories jfastlsp is measured on (ADR 0031, plan §1.7). Each is public, built with
Gradle and pinned to one commit. `just bench-corpus [size]` (BENCH-01) fetches them into a cache
directory outside this repository; **none of their code is ever committed here**. Golden cases
taken from the corpus (BENCH-05) are positions and symbol names only, and come from the Apache-2.0
repositories only (fineract included). fineract is also the second E0 and E3 evaluation target
(ADR 0032, amended 2026-11-17); spring-framework stays the primary.

## Repositories

| Size | Repository | Commit | `.java` files | `.java` MB | `.java` lines | Licence | Why |
|:--|:--|:--|--:|--:|--:|:--|:--|
| small | [junit-team/junit-framework](https://github.com/junit-team/junit-framework) | `e5198f9b098c550e66d378650f78bac8883e2a72` | 1,813 | 7.9 | 229,712 | EPL-2.0 (weak copyleft) | fast inner loop; Java 25 syntax (`import module`, compact source files); its own configuration processor |
| medium | [spring-projects/spring-framework](https://github.com/spring-projects/spring-framework) | `4cb62d893e09e23791d661e8d9b35aedee7006d0` | 9,294 | 53.7 | 1,528,956 | Apache-2.0 | pure Java, 22 modules, JSpecify type-use `@Nullable` throughout |
| lombok | [apache/fineract](https://github.com/apache/fineract) | `9dfe8dfe5e162afe09f102a9aa57d00a70c770f2` | 7,301 | 50.5 | 968,738 | Apache-2.0 | the Lombok and MapStruct slot: 2,874 files import `lombok.*` (constructor, builder, accessor, logger and `@Jacksonized` annotations), MapStruct in 24 modules (179 `*Mapper*.java`), 39 Gradle includes, Spring processors; no generated or delombok source in the tree |
| medium (huge files) | [apache/kafka](https://github.com/apache/kafka) | `7e0ebdf74f30cab1ded2a462cc9d45cefdf86c2a` | 6,307 | 74.0 | 1,618,636 | Apache-2.0 | about 68 projects from one root build; Scala beside Java; a 1.6 MB test file as the per-file worst case |
| large | [elastic/elasticsearch](https://github.com/elastic/elasticsearch) | `157ebe1325f24f34bf871997e880f0c0293b7279` | 33,850 | 333.9 | 7,499,129 | SSPL, Elastic License 2.0 or AGPL-3.0: **not permissive** | the only candidate above 30k files; about 340 Gradle modules; code-generating annotation processors |
| large (permissive fallback) | [gradle/gradle](https://github.com/gradle/gradle) | `db675891c2bcf8e3c9a7c66ac9de429170a8a9cd` | 11,750 | 37.9 | 991,847 | Apache-2.0 | 2,737 build files (mostly test fixtures and samples): the many-build-files stress case |

`.java` counts are `git ls-files '*.java'` at the commit; a `.gitignore`-aware walk finds the same
set to within 8 files. Only **fineract** uses Lombok and MapStruct (found by the survey in
[`spikes/lombok/`](../spikes/lombok/RESULTS.md), 2026-11-17: 0 parse errors in 7,301 files, 1.07 s
warm and 1.35 s cold on 4 threads, p95 2.0 ms, max 71 ms, 58 MB peak RSS, with the same harness as
the E1 spike; runner-up openrewrite/rewrite, rejected for a mixed licence and almost no MapStruct).
The rest of the corpus is covered for processors by the synthetic fixtures (ver §4), which stay
the oracle's offline cases. The tree-sitter index of fineract lacks the generated members
(getters, builders, constructors, MapStruct implementations) by construction: that is the
semantic layer's job, and the reason it is in the corpus. Its build-time generated sources are
not in the tree and were not checked, because no Gradle run happens in the sandbox.

**elasticsearch** is cloned at bench time only. Never vendor it, and never commit fixtures,
snippets, positions or expectations derived from it; it contributes timings and memory numbers
only.

## Fetching

`scripts/bench-corpus.sh [fetch] [small|medium|lombok|huge-files|large|large-permissive|all]`
clones each repository shallow at its pin into `${XDG_CACHE_HOME:-$HOME/.cache}/jfastlsp/bench-corpus` (override
with `--cache DIR`; a directory inside this repository is refused),
verifies the commit, the `.java` count of the table above and a clean tree, and is a no-op on a
cached checkout. `scripts/bench-corpus.sh worktrees SIZE [N]` adds N (default 10) linked worktrees
under the checkout's `.claude/worktrees/`; `clean-worktrees SIZE` removes them; `check-doc` fails
when the script's pins and this table differ. `scripts/test-bench-corpus.sh` tests the logic against
a local stand-in repository.

## E1 spike, measured 2026-11-16

Measured on a 4 vCPU Xeon 2.3 GHz VM (15 GB RAM, virtio disk, Linux, Rust 1.97.0) with a
throwaway Rust harness (archived in [`spikes/e1-parse/`](../spikes/e1-parse/README.md)): a `.gitignore`-aware walk, then per file a read, a blake3 hash, a parse
with tree-sitter 0.27.2 and tree-sitter-java-orchard 0.5.25, and a walk that keeps package,
imports and declarations. Cold runs follow a page-cache drop; warm runs are the median of three.
The fineract row was measured the next day with the same harness and machine class. The harness
seeded `tools/jfl-bench` (BENCH-02, which reproduces the junit-framework row within
its noise band: [`doc/bench/README.md`](bench/README.md)); EVAL-05 re-runs it on the `bench.yml` runner.

| Repository | Full index, 4 threads, warm | Cold | Files/s (4 thr, warm) | Per-file p95 | Per-file max | Peak RSS |
|:--|--:|--:|--:|--:|--:|--:|
| junit-framework | 0.16 s | 0.25 s | 12,368 | 1.2 ms | 9 ms | 15 MB |
| spring-framework | 1.04 s | 1.42 s | 9,326 | 1.5 ms | 40 ms | 46 MB |
| kafka | 1.59 s | 1.76 s | 4,056 | 3.4 ms | 138 ms | 84 MB |
| fineract | 1.07 s | 1.35 s | 7,160 | 2.0 ms | 71 ms | 58 MB |
| gradle | 0.91 s | 2.01 s | 17,788 | 0.8 ms | 8 ms | 33 MB |
| elasticsearch | 7.19 s | 8.69 s | 4,853 | 2.7 ms | 63 ms | 153 MB |

- Throughput is about 12–15 MB/s per core and 48–57 MB/s on 4 cores. Reading and hashing are
  under 1 % of a warm run and about 8 % of a cold one.
- Floors: `rg -c -t java '\bString\b'` takes 0.04 s warm on spring-framework and 0.18 s on
  elasticsearch; `git ls-files` takes at most 20 ms on any of them.
- A second worktree of spring-framework 150 commits away: a diff by blob id takes 20–30 ms,
  a warm blake3 re-hash 0.05–0.08 s, and about 270 of 9.3k files differ.
- Grammar: tree-sitter-java-orchard has no false errors (ADR 0022); upstream 0.23.5 fails
  0.3–0.6 % of files on Java 25 syntax and annotated varargs.

Plan §1.7 holds the budgets derived from these numbers; plan §5.5 the storage-tier consequence.
