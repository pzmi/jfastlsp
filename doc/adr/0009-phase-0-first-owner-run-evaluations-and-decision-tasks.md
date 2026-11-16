# 0009. Phase 0 first: owner-run evaluations and a decision task per open choice

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-16).

## Context

The plan's go/no-go and several design choices (storage tier, resolver, diagnostics scope,
`referencesMode`) depend on measurements on a real Java repository and on real Claude Code
sessions (plan §9). Agents cannot choose the repository, may not be able to read it, and should
not spend the owner's Claude Code budget on evaluation runs unasked.

## Decision

- Phase 0 is modelled as tasks: harnesses and runbooks (EVAL lane) that agents build and test on
  synthetic fixtures, with `(owner)` acceptance items for the steps only the owner can take
  (choosing the target repository, running the E0 arms, running harnesses against the target).
- Each open choice is a **`DEC-nn` decision task** whose outcome is an ADR. DEC-01 chooses the
  target repository; DEC-02 is the go/no-go (⛳) with the kill criterion of plan §1.3; DEC-03..06
  record the storage tier, resolver, diagnostics scope and `referencesMode` from E1, E2, E3 and
  E0; DEC-07 decides the optional M6. Agents prepare the inputs and may draft the ADR as
  Proposed; only the owner decides.
- Product lanes (IDX onwards) start after DEC-02.

## Consequences

Phase 0's progress depends on the owner's time; the orchestrator batches the `(owner)` items into
checkpoint reports. Evaluation results from a private repository enter this repository only as
aggregates (ADR 0004).
