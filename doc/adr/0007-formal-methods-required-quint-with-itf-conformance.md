# 0007. Formal methods required: Quint models with ITF conformance against the real code

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07); amended 2026-11-16.

## Context

The bugs that would hurt agents most are concurrency and ordering bugs: an answer mixing two
index generations, a stale answer after a shell edit, two daemons on one state directory, a
buffer surviving a daemon restart, diagnostics for an old version published over a new one. Tests
written by the implementer sample the interleavings the implementer thought of.

## Decision

Concurrency and lifecycle behaviour is **model first**: a Quint model with stimuli, invariants,
named scenarios and a broken-model control comes before the code, and a conformance driver
replays committed and freshly generated ITF traces against the **real Rust code**, comparing the
observed state with the model after every step, with a negative control that swaps in a faulty
component. jfastlsp's models (plan §7): `views.qnt` (pinned generations, atomic publish, no
mixed-generation answers, freshness after shell edits), `daemon.qnt` (single owner by lock, spawn
on demand and reconnect, no ghost buffers after restart) and `diagnostics.qnt` (results for an
older document version never published over a newer one). Apalache bounded verification runs
weekly. Quint and Apalache are development-time tools only.

## Consequences

The FORMAL lane (FORMAL-01..FORMAL-05) builds the toolchain, the harness and the models; the
drivers land with the code they drive (IDX-09, IDX-12, LSP-04). A change to views, the daemon
lifecycle or diagnostics publication without the model change fails review; a removed invariant
is HIGH. The `formal-*` and `conformance-replay` stages gate every push.

## Amended 2026-11-16 (planner): the harness is reused

The drivers implement `quint-connect`'s `Driver` trait and decode traces with `itf`; only the
replay of committed regression traces is our own code (about 200 lines in `jfl-conformance`).
Quint 0.33.1 and Apalache 0.62.1 are pinned through mise (ADR 0030).
