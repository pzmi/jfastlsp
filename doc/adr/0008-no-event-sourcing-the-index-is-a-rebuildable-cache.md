# 0008. No event sourcing: the index is a rebuildable cache

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07).

## Context

Event sourcing earns its cost where history is the business truth: auditability, replay,
reconstruction. jfastlsp's persistent state (syntax summaries, library indexes, project models)
is derived from files on disk and can always be recomputed from them.

## Decision

jfastlsp has no journal, event store, projections or migrations. Domain events, where useful, are
plain in-memory values. Persisted caches carry a format version and are discarded and rebuilt,
never migrated, when the version changes (fnd §6). Deleting the state directory loses time, never
correctness.

## Consequences

No event-sourcing rules, fixtures or checker rules are ported. A future need for durable history
(none is foreseen) would be a new ADR with its own model.
