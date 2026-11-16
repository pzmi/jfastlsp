# 0024. Git: the git CLI on the reconcile path, stat-only freshness, not gitoxide

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07). Settles the open
question plan §14 gave to IDX-05; IDX-05 may amend it with measurements.

## Context

The daemon needs, per worktree: the innermost root, the common directory, the `HEAD` and index
files to stat on every request, and the files changed since a tree (plan §4.3, §4.6). Linked
worktrees under `.claude/worktrees/` are the normal case. Survey of 2026-11-16:
[gitoxide](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md) (`gix` 0.90,
monthly breaking releases) documents that "the interplay between `GIT_COMMON_DIR` and
`GIT_WORK_TREE` isn't implemented"; `git2` brings libgit2's C build and lags git on extensions.

## Decision

- **Hot path, no Git library:** the per-request freshness check stats `$GIT_DIR/HEAD` and
  `$GIT_DIR/index` and reads the few small files it needs directly (`.git` file's `gitdir:`,
  `commondir`, the first line of `HEAD` or a ref, `packed-refs`).
- **Reconcile path, the `git` CLI through `jfl-process`:** `git status -z --porcelain=v2`,
  `git diff --name-only -z <tree> <tree>`, `git worktree list --porcelain -z`,
  `git rev-parse --git-common-dir`; output parsed defensively and fuzzed. It is always right for
  the user's git version and configuration, at about 2–5 ms per spawn.
- Revisit `gix` (its `status`, `dirwalk` and `blob-diff` features, or the near-stable
  `gix-ref`/`gix-discover` sub-crates for the hot path) if measurements show spawn cost.

## Consequences

`jfl-adapter-git` depends on `jfl-process` (already legal, plan §6.3) and on no Git crate. Plan
§14's "git CLI or gix" row is closed. IDX-05 measures spawn cost on the fixtures.

## Alternatives considered

- `gix`: pure Rust and fast, but the linked-worktree gap hits our main case.
- `git2`: a C build for capability the CLI already gives.
