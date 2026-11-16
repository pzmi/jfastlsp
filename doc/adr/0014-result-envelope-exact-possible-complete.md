# 0014. Result envelope: exact, possible, complete

Date: 2026-11-16

## Status

Proposed.

## Context

An empty reference list is the worst answer an agent can get: it either trusts it and breaks
callers, or falls back to grep and the tool earned nothing. Returning only proven answers makes
emptiness ambiguous; returning everything makes precision meaningless.

## Decision

Every navigation answer is one envelope (plan §2.1): `exact` (bound to the target with evidence),
`possible` (a sound superset of the remaining candidates, each with its reason), `complete`
(`true` only when every module that can see the target's module is indexed and fresh at the
pinned generation, so `exact ∪ possible` holds every reference), `incomplete_because`, the view
and generation, and freshness. No empty answer without a completeness statement. Definition uses
bounded ambiguity (all overloads of a proved owner type, never another type's member). Positions
follow plan §2.3. **`complete=true` is never wrong**: it is pinned by the `CompleteIsHonest`
invariant of `views.qnt`, the completeness oracle and the envelope-honesty mutation suite
(ver §7).

## Consequences

The envelope is a `jfl-domain` type with a `jfl-protocol` golden; IDX-02 defines it, IDX-11
computes completeness, SEM-03 adds resolver evidence. Tokens spent on long `possible` lists are
measured in E0; the CLI groups and caps them and offers `--exact-only`.
