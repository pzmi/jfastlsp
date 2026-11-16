# 0018. Agent sandbox and fail-closed tools

Date: 2026-11-16

## Status

Proposed.

## Context

Agents run with broad shell access in a shared sandbox of 4 CPUs and about 15 GB RAM, several at
once. A force-push, a pattern kill or a gate left running in the background can destroy another
lane's work; a missing tool silently skipped makes a red tree look green.

## Decision

- `.claude/settings.json` allows the gate's cargo commands, the `jfl` binary under `target/`,
  read-only and committing git commands, and denies force-push in every form (including
  `+refspec` and `git -C`), pushing tags or deletions, `--no-verify`, `git reset --hard` outside
  the orchestrator's `-C` worktree form, `git add -A`/`.`, `rm -rf /*`, `docker`, `pkill` and
  `killall`.
- Hooks are bootstrap-safe (each a no-op until its prerequisite exists): SessionStart installs the
  pinned toolchain and reports tools; every edited `*.rs` is formatted; Stop runs the fast gate
  when code, manifests, the policy, a model or a fixture changed.
- Every gate stage declares its tools and fails closed with an install hint (fnd §8).
- At most two lanes, each with its own `CARGO_TARGET_DIR` on disk, `CARGO_BUILD_JOBS=2` and
  `CARGO_INCREMENTAL=0`; processes are stopped by PID only; nothing runs in the background when a
  turn ends.

## Consequences

`CLAUDE.md` and `.claude/` belong to the orchestrator; a task agent reports a needed change.
Changing the deny list is an ADR amendment.
