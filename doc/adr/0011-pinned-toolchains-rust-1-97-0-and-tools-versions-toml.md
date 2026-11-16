# 0011. Pinned toolchains: Rust 1.97.0, edition 2024; JDK and tools in tools/versions.toml

Date: 2026-11-16

## Status

Superseded by [0030](0030-foundation-from-maintained-tools.md) (2026-11-16).

## Context

Local and CI gates must agree, and a re-run must not fetch a different tool. jfastlsp also needs
a JDK at development time (the javac worker, the Java fixture corpus, Apalache) and Node for
Quint, none of which the shipped binary may require.

## Decision

- `rust-toolchain.toml` pins Rust **1.97.0** with `rustfmt`, `clippy` and `llvm-tools-preview`;
  edition 2024, resolver 3, `rust-version = "1.97"`; `Cargo.lock` committed and every gate
  command `--locked`. A dated nightly is pinned only for `cargo-fuzz`.
- `tools/versions.toml` pins every other tool a stage needs (cargo-nextest, cargo-deny,
  cargo-llvm-cov, gitleaks, actionlint, shellcheck, Node major, Quint through
  `formal/package-lock.json`, Apalache with SHA-256, the JDK major, later cargo-mutants,
  cargo-fuzz, cargo-cyclonedx), each with a probe; `cargo xtask doctor` checks them and every
  stage fails closed on a missing or wrong tool (fnd §8).
- The installed `jfl` needs no JVM, Node or Python; a JDK is needed only where a user enables
  semantic diagnostics or the Gradle model, and `jfl status` says so.

## Consequences

Toolchain bumps are their own commits with the gate green. FND-01 writes `rust-toolchain.toml`;
FND-02 adds `tools/versions.toml`; FORMAL-01 adds Node, Quint, Apalache and the JDK.
