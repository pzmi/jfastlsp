# 0002. Integrate on main: one signed-off commit per task, landed by the orchestrator

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-16); amended 2026-11-16 (accepted by the owner on 2026-11-17).

## Context

The plan is built by agents in parallel worktrees under one orchestrator, with the owner reviewing
at checkpoints, often from a phone. A pull request per task would add a merge step the owner must
take for every task without adding a review the orchestrator does not already run. What matters is
that every commit on `main` passed the full gate on the merged tree and that CI confirms it.

## Decision

Everything lands directly on `main` as **one DCO-signed-off commit per task**. Implementing agents
commit on their lane branch and never push. The orchestrator reviews each commit and lands it:

- until FND-03 lands, by hand: fetch, a clean `main` equal to `origin/main`, cherry-pick, the live
  gate on the merged tree in the foreground (the fallback gate until FND-02), push only on green;
- from FND-03 on, with `cargo xtask land <sha> --push origin main`, which does the same and
  resolves only `Cargo.lock` and the ADR index.

One commit lands per green CI run. A red `main` is fixed immediately by a new commit. There is no
pull-request flow and no branch protection during the plan; a later ADR may introduce either.

## Consequences

`main` is always the integration point and every commit on it is individually gated. History is
linear and never rewritten (no force-push). Outside contributions still arrive as pull requests
and are integrated as signed-off commits. Pinned by the orchestrate skill, `doc/sdlc.md` §5 and,
from FND-03, `land`'s refusals of a dirty or stale `main`.

## Alternatives considered

- Pull request per task, owner squash-merges: an owner action per task; slower; no stronger gate.
- Agents push directly: no review before landing, and interleaved pushes hide which commit broke CI.

## Amended 2026-11-16 (planner; accepted by the owner on 2026-11-17): `land` is a script

Only the tool names change; the decision stands, and `scripts/land.sh` may be used from FND-03
on whether or not this amendment has been accepted. The decision is unchanged: one signed-off
commit per task, gated on the merged tree, pushed only on green. Only the tool changes (ADR 0030): from FND-03 on, the orchestrator lands with
`scripts/land.sh <sha>` (about 150 lines of shell: fetch, a worktree at `origin/main`,
cherry-pick, `just check`, push, one retry on a non-fast-forward), not `cargo xtask land`. CI
adds a DCO check on every pushed commit.

Before `ci.yml` exists (FND-05), "one commit lands per green CI run" cannot hold: there is no CI
run. Until then the local gate on the merged tree is the only gate and the next commit may land as
soon as the previous one is pushed (BOOT-02).
