# 0025. Storage crates: rusqlite, fst, postcard and blake3

Date: 2026-11-16

## Status

Retired 2026-11-24 (owner): detail lives in plan §5.5 and §16

## Context

The caches are content-addressed and rebuildable (ADR 0008): per-file summaries keyed by
`SyntaxKey`, library summaries by checksum, manifests and, at higher tiers, postings and
per-commit baselines. Survey of 2026-11-16: `bincode` is unmaintained
([RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141)); the other options are
maintained.

## Decision

- **`blake3` 1.8** (CC0-1.0 OR Apache-2.0) for every content key; grammar, schema and crate
  versions are hashed in.
- **`postcard` 1.1** (MIT OR Apache-2.0; a stable wire specification since 1.0) for per-file
  summary blobs, in a **content-addressed blob directory of our own** (about 150 lines,
  `objects/ab/cdef…`, write to a temporary file then `rename`, LRU size cap; cacache's layout,
  not the crate, which is quiet and SHA-based).
- **`fst` 0.4** (Unlicense OR MIT; finished, not abandoned) for name lookup: prefix and fuzzy
  `workspace/symbol` over immutable segments, per `SyntaxKey` batch or JAR checksum.
- **`rusqlite` 0.43** (MIT, `bundled`, WAL) for metadata, manifests and postings, in one database
  per state directory, once DEC-03's tier needs more than the blob directory.
- Every format carries a version and is discarded, never migrated (fnd §6).

## Consequences

`deny.toml` allows CC0-1.0 and Unlicense, and `rusqlite`'s bundled SQLite build script by name.
All four crates are wrapped to `jfl-adapter-store` (and `blake3` to the crates that derive keys
from digests they compute). IDX-08 and PROJ-04 implement this; E1 sizes it.

## Alternatives considered

- `redb` 4.6 (pure Rust, MVCC snapshots map onto pinned generations): the runner-up if SQLite
  write contention or WAL checkpoints show up in E1.
- `rkyv`: a measured optimisation only, never adopted for its own sake. `bitcode`: no format
  stability. `bincode`: unmaintained. `tantivy`: overkill for symbol names. `salsa` 0.28:
  deferred; its persistence is experimental and its database per process.
