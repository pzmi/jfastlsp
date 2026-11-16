# 0017. Bind views per request URI; exclude nested worktrees

Date: 2026-11-16

## Status

Proposed.

## Context

Agent worktrees land inside the main checkout (`.claude/worktrees/<name>/`), and it is unverified
whether a subagent gets its own language server. A view bound per connection would answer one
worktree's question from another's files; a naive scan of the main checkout would index every
agent's copy.

## Decision

Each request's file resolves to the innermost enclosing worktree root, cached per directory
prefix; a view is per worktree root, never per connection (plan §4.1). When scanning a worktree,
directories holding a `.git` file or directory are skipped, and `.claude/worktrees/` explicitly
(plan §4.2). Freshness is checked per request by a stat of the file, `HEAD` and the index (plan
§4.3), with watcher events as hints.

## Consequences

`workspace/symbol` returns one copy per symbol; a request from a nested worktree is answered from
it. `NoCrossView` in `views.qnt`, the nested-worktree fixture and the 10-worktree e2e test of
IDX-15 pin it.
