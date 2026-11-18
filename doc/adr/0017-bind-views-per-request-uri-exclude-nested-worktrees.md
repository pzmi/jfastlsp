# 0017. Bind views per request URI; exclude nested worktrees

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17); amended 2026-11-18 (IDX-05; accepted by the owner 2026-11-18).

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

## Amended 2026-11-18 (IDX-05; accepted by the owner 2026-11-18): the root is cached, the walk to it is not

"Cached per directory prefix" is narrowed to "confirmed once per root". `jfl-adapter-git` walks up
from a request's directory with one `stat` per level on every call and caches only Git's
confirmation of a root (`rev-parse`, keyed by the `.git` entry's identity). The reason: a
`git init` or `git worktree add` inside a directory that was already resolved makes it a new
innermost root, and nothing announces it (the watcher leaves `.git` paths out of its hints), so a
cached directory-to-root answer would keep binding its requests to the parent, which skips the new
worktree when it scans. The walk costs about 20 µs a request on the medium corpus (`just bench-git`).
Pinned by `crates/jfl-adapter-git/tests/worktrees.rs::should_resolve_to_a_new_nested_repository_when_it_appears_after_a_resolve`;
plan §4.1 says the same.
