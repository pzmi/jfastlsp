# 0032. Evaluation target: spring-framework at the bench pin

Date: 2026-11-17

## Status

Accepted (owner, 2026-11-17).

## Context

DEC-01 (plan §9.1; ADR 0009, which gives each open owner choice its own decision task) asks the
owner to choose the one large Gradle Java repository that the phase 0 experiments run on. E0 (the agent eval, plan §9.2) needs 15 to 20 tasks from its history,
each with one test command. E1 and E4 need a full worktree and worktrees at nearby commits. E2
needs a `scip-java` index as its oracle, and E3 needs a build with processors to compile.

`doc/eval/target-criteria.md` set out the must-have and should-have criteria (C1 to C8, S1 to S3)
and three options: A, a private repository; B, spring-framework (public, Apache-2.0) at its
bench pin; C, kafka at its bench pin. It ranked B first, on reproducibility, publishability and
the E1 and E4 numbers already on record. The owner chose B on 2026-11-17.

## Decision

The E0 to E4 evaluation target is **spring-framework**
(<https://github.com/spring-projects/spring-framework>) at commit
`4cb62d893e09e23791d661e8d9b35aedee7006d0`, the pin in `doc/bench-corpus.md`. It is the
medium-class entry of the bench corpus: 9,294 `.java` files at the pin, 22 Gradle modules, pure
Java, Apache-2.0.

Its task definitions (commit ids, prompts, test commands) and the aggregates of the eval may be
published. Golden cases taken from it are positions and symbol names only (`doc/bench-corpus.md`).
Any other commit needs a dated amendment to this record.

Why B:

- **Reproducible.** Anyone can check out the pin and build it with the Gradle wrapper (C1), so the
  tasks can be rerun, which the plan's eval depends on (plan §9.2, EVAL-02; ADR 0029 runs the
  tasks on Harbor).
- **Publishable.** Apache-2.0 and a public repository let the task set and results be cited
  (criteria §3, C8). Option A would have kept only aggregates.
- **Measured.** The E1 spike and the E4 worktree timings already exist at this pin
  (`doc/bench-corpus.md`), so E0 to E4 read against one target.
- **Pure Java.** No Scala beside the Java, so the Java arms and the coverage measure are not
  diluted (C3, E2).

This record does not decide the per-task time limit (set by the EVAL-01 smoke task on the owner's
machine, C5) or the E0 arms. It is compatible with ADR 0004: nothing from a private repository
enters this one.

## Consequences

- EVAL-02 takes the E0 task set from this repository's history, and that set and its results may
  be committed under `doc/eval/`. No private task text or path enters the repository.
- **Contamination.** Public history may already be in model training data, so E0 success rates
  can be inflated. The Multi-SWE-bench control (EVAL-12, ADR 0029) carries the same risk, so the
  two are read together and the owner reads the rates with it in mind (criteria §4, §6).
- **No Lombok or MapStruct.** The pin uses neither (`doc/bench-corpus.md`; criterion S1). Processor
  behaviour is therefore covered by the synthetic fixtures of EVAL-07 (Lombok and MapStruct, with
  generated sources present and missing), by the SEM-05 acceptance (zero false errors with
  generated sources missing), and by the E3 spike (EVAL-10), which must say whether the pin runs any
  source-generating processor (criterion S2). The E0 task set does not claim processor coverage.
- Tests and rules are unchanged: the eval harness (EVAL-01) and the report rules (no prompt, path
  or source line in a report) apply to the public target as to any other.

## Alternatives considered

- **A: a private repository meeting C1 to C8.** The most representative of the owner's work and
  the only option with Lombok or MapStruct in the real build. Rejected for now: its task sets and
  commit ids cannot be published, so nobody else can rerun the eval, and the E1 and E4 numbers
  would have no corpus baseline. A private target can be a later second target under a new record
  (criteria §6).
- **C: kafka at its bench pin.** Public and Apache-2.0, but about 68 projects from one root build
  with Scala beside Java: a heavier build for three arms times three runs, and E2's coverage
  would skip the Scala call sites. Ranked second in the criteria.
