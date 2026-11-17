# 0012. Port xtask from mayfly

Date: 2026-11-16

## Status

Superseded by [0030](0030-foundation-from-maintained-tools.md) (2026-11-16); its direction restored by [0035](0035-port-mayfly-xtask-piece-by-piece.md)
(2026-11-17).

## Context

The engineering foundation needs a command surface where local and CI gates run the same code,
an architecture checker over the resolved Cargo graph and the source, ADR allocation, and a land
command that gates the merged tree. The public repository mayfly has all of these in
`tools/xtask` (itself ported from the public repository site-preview and hardened by several
review rounds), with data-driven selectors in `architecture.toml`, fixtures inside the xtask
crate, a checkout and environment preflight, and bootstrap-safe hooks.

## Decision

Port `tools/xtask` from mayfly, adapted to jfastlsp: `cli`, `process`, `repo` (preflight),
`tools` (doctor, install-tools, `tools/versions.toml`), `catalog`, `check`, `quick`, `test`,
`adr`, `land`, `action_pins`, `security`; the architecture checker's policy, inventory, graph,
external and semantic layers with the generic rules of fnd §3.3 and a curated set of negative
and positive fixtures renamed to jfastlsp's crates; the formal registry and conformance stages
later (FORMAL-01, FORMAL-02). Not ported: anything specific to event sourcing, control and data
planes, virtual machines or hardware lanes. Crate-name tables hardcoded in mayfly's semantic
rules move into `architecture.toml` selectors where practical. Fuzz, mutation, bench and release
stages come later from site-preview's designs (REL-03, REL-05, REL-06).

## Consequences

FND-02..FND-06 are a port, not a design from scratch, and each cites the mayfly module it
adapts. Until FND-02 lands the gate is the fallback list of cargo commands; until FND-03 lands,
`land` is done by hand (ADR 0002).
