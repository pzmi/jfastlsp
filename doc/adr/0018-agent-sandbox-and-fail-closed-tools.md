# 0018. Agent sandbox and fail-closed tools

Date: 2026-11-16

## Status

Proposed; amended 2026-11-16 (twice).

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

## Amended 2026-11-16 (planner): the gate's commands changed

With ADR 0030 the allow list names `just`, `mise`, `adrs` and `scripts/land.sh` instead of
`cargo xtask`; SessionStart runs `mise install --locked` once `mise.toml` exists, and Stop runs
`just quick` once the justfile has it. The deny list is unchanged.

## Amended 2026-11-16 (planner, BOOT-02): the deny list, the bootstrap and the Stop hook

- **Deny list.** `rm -rf /*` also matched every `rm -rf` of an absolute path, including the lane
  target directories and worktrees the process requires deleting; it is narrowed to `rm -rf /`
  and `rm -rf / *`, and `rm -rf` is allowed under the session scratchpad. Deleting a remote ref by
  refspec (`git push origin :main`) and the `git -C <dir> push` forms of deletion and `+refspec`
  were not denied, although this record said "in every form"; they are now. The commands the
  process mandates (`cargo add`, `git cat-file`, `setpriv`, `shellcheck`, `bash scripts/*.sh`,
  Quint, `rustup target add`, `mkdir -p .work/*`) are allowed, since a background agent cannot
  answer a permission prompt.
- **Bootstrap.** SessionStart now installs mise and just themselves (pinned, checksum-verified,
  `scripts/bootstrap-tools.sh`) before `mise install --locked`; it reports and never blocks the
  session.
- **Stop hook.** It fails when the justfile has `quick` and `just` is missing, instead of quietly
  running the cargo-only fallback; it builds with `CARGO_BUILD_JOBS=2`. It guards the checkout the
  session started in, which for the orchestrator is the main checkout; lane worktrees are guarded
  by the work skill's ship station (the gate on the exact tree, its exit code recorded) and by the
  gate on the merged tree when landing.
