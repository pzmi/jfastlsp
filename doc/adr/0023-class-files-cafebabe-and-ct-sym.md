# 0023. Class files: cafebabe and zip; the JDK API from ct.sym

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07).

## Context

The library index summarises class files from JARs and the JDK, shared by checksum (plan §5.8).
Class files and JARs are untrusted input (plan §11), so the parser must be bounded and free of
`unsafe` in our crates. Survey of 2026-11-16, checked against the crates' published sources.

## Decision

- **[`cafebabe`](https://github.com/staktrace/cafebabe) 0.9.0** (0BSD): `#![forbid(unsafe_code)]`,
  about 5k lines, zero-copy over `&[u8]`, parses Record, PermittedSubclasses, Module and the raw
  Signature attribute. Read-only, which is all we need.
- **Our own generic-signature parser** (JVMS §4.7.9.1, about 300 lines, fuzzed) and **our own
  multi-release selection** (about 50 lines: `Multi-Release: true`, then the highest
  `META-INF/versions/N/` entry not above the environment's `--release`); no crate does either.
- **[`zip`](https://github.com/zip-rs/zip2) 9.6** (MIT, `default-features = false` plus
  `deflate`) for JARs, `jmods/*.jmod` (after the 4-byte `JM` header) and
  **`$JAVA_HOME/lib/ct.sym`**, the zip of per-release API stubs that `javac --release N` compiles
  against: the `--release` API of a JDK without starting a JVM.
- `ristretto_jimage` (Apache-2.0 OR MIT), the only pure-Rust reader of `lib/modules`, is added
  only if a JDK image has neither `ct.sym` nor `jmods`. Its 0.34.0 needs Rust 1.97.1 and 0.35.0
  needs 1.99.1, so adopting it is a toolchain bump in its own commit first.
- kmp-lsp's JAR indexer and ktlsp's `jar.rs` (both MIT) are read for layout, not linked.

## Consequences

`deny.toml` allows 0BSD. PROJ-03 and PROJ-04 implement this; the class-file and zip fuzz targets
(ver §8) cover our parser and cafebabe on hostile input.

## Alternatives considered

- `ristretto_classfile` 0.35: full fidelity and Java 25, but about 62k lines, monthly API churn
  and an MSRV above our pin; the runner-up (0.34 pinned) if fidelity gaps appear.
- `noak` (36 `unsafe` sites), `jclassfile` (small, young), `classfile-parser` (no Signature,
  Record or Module), `jimage` (Windows FFI): rejected.
