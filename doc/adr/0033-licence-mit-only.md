# 0033. Licence: MIT only

Date: 2026-11-17

## Status

Accepted (owner, 2026-11-17). Supersedes
[0005](0005-dual-licence-mit-or-apache-2-0.md).

## Context

ADR 0005 proposed the Rust ecosystem's dual licence, `MIT OR Apache-2.0`, as an orchestrator
default for the FND checkpoint. The owner decided at that checkpoint (2026-11-17) that jfastlsp
is MIT only, not dual. The licence decision is also an input to reuse: code copied into the
repository must be compatible with MIT-only distribution (ADR 0036 is the first record that
depends on it).

## Decision

- jfastlsp is licensed under the **MIT** licence alone: `LICENSE-MIT`, "Copyright (c) 2026
  Patryk Żmigrodzki". `LICENSE-APACHE` is removed.
- Every crate's manifest says `license = "MIT"` (through `[workspace.package]`).
- Contributions are accepted under the MIT licence, signed off under the DCO (`CONTRIBUTING.md`).
- Dependencies stay limited to the permissive licences of `deny.toml`; an exception is per crate
  with its reason (fnd §7.4). `deny.toml` lists dependency licences only and does not change.

## Consequences

- A file copied from elsewhere keeps its own licence and notice and must be compatible with an
  MIT-licensed project: permissive sources (MIT, BSD, Apache-2.0 with its notice kept, as for the
  nvim-treesitter query IDX-07 starts from) may be copied with attribution; copyleft sources
  (GPL, even with the Classpath Exception) are read as reference only, never copied (ADR 0036).
- Runtime-optional pieces that are only run, never bundled or linked (jdtls, scip-java's plugin),
  keep their own licences.
- The javac worker under `java/` and the Quint models are under MIT too.

## Alternatives considered

- `MIT OR Apache-2.0` (ADR 0005): the ecosystem convention with an explicit patent grant; the
  owner chose the single, simpler licence.
