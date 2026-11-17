# 0005. Dual licence MIT OR Apache-2.0

Date: 2026-11-16

## Status

Superseded by [0033](0033-licence-mit-only.md) (2026-11-17, the owner's decision at the FND checkpoint).

## Context

jfastlsp is open source Rust. The Rust ecosystem's convention is the dual licence, which keeps
the code usable both by projects that need Apache-2.0's patent grant and by those that need
GPLv2 compatibility.

## Decision

jfastlsp is licensed `MIT OR Apache-2.0`, at the user's option: `LICENSE-MIT` and
`LICENSE-APACHE`, "Copyright (c) 2026 Patryk Żmigrodzki". Every crate's manifest says
`license = "MIT OR Apache-2.0"` (FND-01). Contributions are accepted under the same terms, signed
off under the DCO (`CONTRIBUTING.md`). Dependencies are limited to permissive licences by
`deny.toml`; an exception is per crate with its own ADR.

## Consequences

`cargo-deny`'s licence check (FND-02) refuses a copyleft dependency without an ADR. The javac
worker (Java source in `java/`) and the Quint models are under the same licence.
