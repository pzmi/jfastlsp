# 0034. Two carriers over one query engine: LSP first, CLI second

Date: 2026-11-17

## Status

Accepted (owner, 2026-11-17). Supersedes
[0013](0013-two-carriers-over-one-query-engine-cli-first.md).

## Context

ADR 0013 proposed one query engine with two carriers, the `jfl` JSON CLI first (M1) and
`jfl lsp` second (M2), because the CLI carries the full envelope and works in cloud sessions
where Claude Code starts no plugin language server. At the FND checkpoint (2026-11-17) the owner
reversed the order: the LSP carrier comes first, the CLI second. The two-carrier design itself
is kept.

## Decision

- One query engine in the daemon serves two carriers, both thin proxies speaking `jfl-protocol`;
  `jfl-adapter-lsp` depends only on `jfl-protocol` and `jfl-domain`, so the carriers cannot
  compute different answers (unchanged from ADR 0013).
- **`jfl lsp` is built first (M1)**: the engine (IDX-01..IDX-12) is carried to agents through
  plain LSP 3.17, with references ordered `exact` first and hover's status line as the
  completeness channel (plan §2.5). M1's gate and the blocking bench gate move to LSP-06.
- **The `jfl` JSON CLI comes second (M2)**: the full envelope with `--json`, the CLI plugin with
  `bin/` and the skill, `AGENTS.md` and the cloud-session setup (IDX-13..IDX-15; the task ids are
  kept so citations stay valid). M2's gate is IDX-15.
- Two plugin variants still ship: the LSP plugin from M1, the CLI plugin from M2 (plan §2.6).

## Consequences

Until M2 an agent sees completeness only through hover's status line and the ordering of
references, and cloud sessions (no plugin language server) are served only from M2. The plan's
milestones (plan §10.1, §10.2), carriers (plan §2.4, §2.5) and the lanes of `doc/tasks.md` are
reordered in the same commit: lane IDX is the shared engine, lane LSP is M1, lane CLI is M2.
DEC-06 still decides `referencesMode` and the order of operations within each carrier.
