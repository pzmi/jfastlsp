# 0016. One daemon per user with thin per-client proxies

Date: 2026-11-16

## Status

Proposed.

## Context

The expensive state (library and JDK indexes, Gradle models, `javac` workers, syntax summaries
shared across worktrees) should exist once per machine, not once per agent. Agents and subagents
each start their own `jfl lsp` or `jfl` process.

## Decision

One daemon per state directory (one per user by default) owns all index state and serves every
repository and worktree; `jfl lsp` and the CLI subcommands are thin proxies over a Unix socket
(plan §5.1–§5.3). The daemon is the single owner by an exclusive lock held for its life; proxies
spawn it on demand and reconnect after a restart, re-sending their open documents; per-connection
state dies with its connection, so a restarted daemon holds no ghost buffers. The lifecycle is
modelled in `daemon.qnt` (ADR 0007). Repositories are identified by their Git common directory;
caches are keyed by content (`SyntaxKey`, `EnvironmentKey`, `SemanticKey`, plan §5.4).

## Consequences

The Nth worktree costs a view, not a copy of every index. A daemon crash affects all agents until
a proxy respawns it, which the reconnect path and REL-04's soak test cover. IDX-12 implements it.
