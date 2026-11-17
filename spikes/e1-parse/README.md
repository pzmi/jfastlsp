# E1 parse spike (throwaway evidence)

The spike of 2026-11-16 that measured how fast a Rust harness parses the pinned benchmark corpus
with tree-sitter: a `.gitignore`-aware walk, then per file a read, a blake3 hash, a parse with
`tree-sitter-java-orchard` (and upstream `tree-sitter-java` for comparison) and a declaration
walk. Its numbers are the "measured" column of plan §1.7 and the storage-tier argument of
plan §5.5; [`doc/bench-corpus.md`](../../doc/bench-corpus.md) summarises them.

**This is evidence, not product code.** It is kept so the numbers can be re-derived and so
BENCH-02 can start from it; BENCH-02 rewrites what it needs inside `tools/jfl-bench` under the
workspace's rules and then this directory stays as the record. It is excluded from the Cargo
workspace (`exclude = ["spikes"]`, FND-01), is not built or tested by the gate, and follows none
of the workspace's lints or rules.

| File | What it is |
|:--|:--|
| `RESULTS.md` | the full write-up: machine, corpus, method, every table, the conclusions |
| `tables.md` | the tables `summarize.py` produced from the raw log |
| `parse-spike/` | the Rust harness (`cargo build --release --locked` inside it; its own lockfile) |
| `bench.sh` | the parse driver: cold runs after `drop_caches`, warm runs three times per stage |
| `floors.sh` | the floors: `rg`, `git status`, `git ls-files`, cold and warm |
| `oid-diff.sh` | files that differ between two worktrees, by index blob id |
| `summarize.py` | turns the raw log into the tables |

The drivers expect the corpus cloned under `repos/<name>` next to them (now: `just bench-corpus`,
BENCH-01) and write `results.log` beside themselves; `drop_caches` needs root. The raw logs and
clones were not kept: they are large, and the corpus code is never committed (ADR 0031).
