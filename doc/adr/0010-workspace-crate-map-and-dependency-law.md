# 0010. Workspace crate map and dependency law in architecture.toml

Date: 2026-11-16

## Status

Proposed.

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
tool only, the last two independent oracles. The legal edges are plan §6.3's table and authority
is plan §6.5's table, both encoded as data in `architecture.toml` (schema in fnd §3.1) and
enforced by `cargo xtask architecture` with a negative fixture per rule. Plan §6.2 records why
the cut differs from one-adapter-per-technology in four places.

## Consequences

A new crate, edge, dependency or authority is a policy change in the same commit, reviewed and
justified. FND-01 creates the stubs and the policy tables; FND-04 and FND-05 enforce them. The
resolver's crate is added by SEM-01 per DEC-04.

## Alternatives considered

- One crate with modules: no compiler-enforced boundaries; `pub(crate)` leaks everything.
- A process-authority grant per adapter: three places to get argv, environment and process
  groups right instead of one.
