# 0010. Workspace crate map and dependency law in architecture.toml

Date: 2026-11-16

## Status

Proposed; amended 2026-11-16 (twice).

## Context

Many agents will change this codebase in parallel. Boundaries that live only in prose drift. The
product has clear technology seams (tree-sitter, class files, Git, Unix sockets, JSON-RPC, JVM
workers, Gradle) and a core that must stay pure and testable: positions, keys, the envelope and
the completeness computation.

## Decision

A multi-crate Cargo workspace with the crate map of plan §6.1: pure `jfl-domain` and
`jfl-protocol`; `jfl-application` (use cases, view registry, ports); one adapter per technology;
`jfl-process` as the single support crate that spawns children; `jfl-composition` and `jfl-cli`
as roots; `jfl-testkit` and `jfl-conformance` test only; `xtask`, `jfl-oracle` and `jfl-eval`
tool only, the last two independent oracles (as amended below: no `xtask`; `jfl-architecture`
test only; `jfl-jdtls` and `jfl-bench` tool only and independent). The legal edges are plan §6.3's table and authority
is plan §6.5's table, both encoded as data in `architecture.toml` (schema in fnd §3.1) and
enforced by `cargo xtask architecture` with a negative fixture per rule. Plan §6.2 records why
the cut differs from one-adapter-per-technology in four places.

## Consequences

A new crate, edge, dependency or authority is a policy change in the same commit, reviewed and
justified. FND-01 creates the stubs and the policy tables; FND-02 and FND-07 enforce them (as
amended below). The
resolver's crate is added by SEM-01 per DEC-04.

## Alternatives considered

- One crate with modules: no compiler-enforced boundaries; `pub(crate)` leaks everything.
- A process-authority grant per adapter: three places to get argv, environment and process
  groups right instead of one.

## Amended 2026-11-16 (planner): enforcement by tools, not a ported checker

ADR 0030 replaces `cargo xtask architecture`. `architecture.toml` keeps the crate categories,
the layer matrix and the external allowlist; they are enforced by the compiler (`#![no_std]` on
pure crates), `cargo-deny` bans with `wrappers`, per-crate clippy `disallowed-*` lists and one
test crate, `jfl-architecture` (`tools/jfl-architecture`, test only, about 200 lines over
`cargo_metadata`), which replaces the `xtask` crate in the map. FND-02 lands all of it. Plan §6.1
also gains `jfl-jdtls` (phase 0 baseline, ADR 0028) and two JVM helpers outside the workspace
(`java/javac-worker/`, `java/gradle-model/`), both started through `jfl-process`.

## Amended 2026-11-16 (planner, BOOT-02): authority for tool and test crates; clock, sleep, env

Plan §6.5 granted authority only to production crates, yet the tool and test crates must spawn
processes (`git`, `cargo metadata`, jdtls, hyperfine, a JVM), read and write files, time runs and,
for `jfl-jdtls`, run a daemon on a Unix socket; and `jfl-process` needs `Instant` for deadlines
and the environment for its allowlist. Without a grant every one of their tasks would have to
break the canonical clippy list or invent policy. The grants are now `process`, `socket`, `fs`,
`spawn`, `sleep`, `clock` and `env`, and every crate's row is in `architecture.toml` (fnd §2.4,
§3.1): production crates keep the tight set of plan §6.5 (`jfl-process`: process, clock, env;
composition: fs, spawn, clock, env; the CLI: env); `jfl-testkit` gets process, fs, sleep and clock
(sleep and clock inside `poll` only, by review); `jfl-conformance` fs and env; `jfl-architecture`
process, fs and env; `jfl-bench` and `jfl-eval` process, fs, clock and env; `jfl-jdtls` process,
fs, socket, spawn, clock and env; `jfl-oracle` fs. TCP stays denied everywhere. These crates are
never reached from production (`ARCH-production-closure`), so their authority cannot leak into
`jfl`. The architecture test moves to its own task, FND-07, after the gate (FND-02).
