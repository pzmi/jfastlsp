# 0029. E0 on Harbor, with a Multi-SWE-bench Java control

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07).

## Context

E0 (plan §9.2) runs 15 to 20 target-repository tasks, several arms, three runs each, on a large
Gradle build. BOOT-00 planned a harness of our own. Survey of 2026-11-16:
[`claude plugin eval`](https://code.claude.com/docs/en/plugin-evals) compares only "with plugin"
against "no plugin", runs each case in a temporary HOME with a scaffold of at most 120 s, a run of
at most 3600 s and 200 turns, allows no custom-code graders, and does not document whether
`claude -p` starts plugin language servers.
[Harbor](https://github.com/harbor-framework/harbor) (Apache-2.0, the Terminal-Bench 2 harness)
runs Docker task environments with a built-in Claude Code agent and full trajectories.
[Multi-SWE-bench](https://github.com/multi-swe-bench/multi-swe-bench) has 128 Java instances
with Docker images and test oracles, Gradle projects among them.

## Decision

- **Wrap Harbor for E0:** one image per arm (grep only; the stock `jdtls-lsp` plugin; jdtls
  behind our proxy, ADR 0028; kmp-lsp at a pinned commit; later `jfl`) with a pre-warmed Gradle
  cache baked in, and the tasks in Harbor's format. `jfl-eval` keeps what Harbor does not give:
  transcript metrics (LSP operation histogram, reaction to empty references, broken-build edits
  caught by diagnostics) and the aggregate-only report.
- **An external control:** 10 to 20 Gradle instances from Multi-SWE-bench's Java set, run in the
  same arms, so E0 is not fitted to one repository. Being public, the control set is committed.
- **First a smoke task** proving that language servers start under `claude -p` in a Harbor
  container. If they do not, E0 uses a thin `claude -p --output-format stream-json` runner of our
  own (about 1–2 days) with the same task format.
- **`claude plugin eval`** only for small skill regressions (did the agent call `jfl refs` before
  changing a signature), once its LSP behaviour is known.

## Consequences

EVAL-01 shrinks to a Harbor adapter plus the metrics; EVAL-12 builds the control set. Harbor and
Docker run on the owner's machine (dev-time only, never in the gate). A private target's tasks
stay outside this repository (ADR 0004); its images are built owner-side.

## Alternatives considered

- Our own harness only: rebuilds what Harbor has. `plugin eval` as the main harness: two arms
  only and limits a large Gradle repository breaks. SWE-bench-java-verified, SWE-PolyBench: older
  or smaller optional controls.
