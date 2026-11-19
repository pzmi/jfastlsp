# 0039. Timed waits: only deadlines and ticks, in the daemon's timer module

Date: 2026-11-24

## Status

Accepted (owner, 2026-11-24). Supersedes [0010](0010-workspace-crate-map-and-dependency-law.md) in
part: its authority rule on sleeping (production crates hold no `sleep` grant).

## Context

ADR 0010 and plan §6.5 grant no production crate a sleep, and fnd §2.4 counts every timed wait
under the `sleep` grant: a plain sleep, a socket timeout, a `Condvar` or channel wait with a
timeout, and Tokio's `sleep`, `timeout` and `interval`. The daemon of IDX-12 (plan §5.2) needs
time: an idle exit after a period with no connection, a tick that checks the lock path and the
socket path (a lost lock file, a swept socket), and deadlines on a new connection's handshake and
on each reply's write, so that a hung or slow peer cannot hold the daemon. The first build of
IDX-12 met that by granting `sleep` to `jfl-composition` and `jfl-adapter-ipc`, which also lifted
`thread::sleep` and allowed a polling retry; the review (H1) refused it. The proxy's waits were
then rebuilt on `poll(2)` with the time left, as `jfl-process` waits; only the daemon's Tokio
timers needed a decision.

## Decision

A timed wait is allowed only as a **deadline on an event** or a **periodic tick**, and only in
`jfl-composition`'s `daemon/timer.rs`, by review: Tokio's `timeout`/`timeout_at` and
`interval`/`interval_at`, under a new `timer` grant that `architecture.toml` gives to
`jfl-composition` alone (fnd §2.4; the root `clippy.toml`'s `# grant: timer` section). Plain sleeps
(`thread::sleep`, Tokio's `sleep`, `park_timeout`) and polling loops stay banned everywhere in
production: the `sleep` grant remains `jfl-testkit`'s (its `poll` helper) and no production
crate's. Waits on descriptors (`jfl-process`, the IPC client, the notify handshake) are `poll(2)`
bounded by the time the caller has left, which needs no grant.

## Consequences

- `jfl-composition`'s clippy list lifts only the four timer entries; `thread::sleep`, socket
  timeouts and timed channel waits stay denied there. A new timer outside `daemon/timer.rs` is a
  review finding (the grant is per crate, the module rule by review).
- `jfl-architecture`'s grant list (`policy::GRANTS`) and `scripts/clippy-lists.sh` know the
  `timer` grant; plan §6.5's Sleeping row and fnd §2.4 say the same.
- Pinned by the `clippy` stage (each crate's list against the root list and `[grants]`,
  `ARCH-clippy-lists`) and `tools/jfl-architecture/tests/rules.rs::should_match_fnd_2_4_when_the_grants_are_read`.

## Alternatives considered

- Keep `sleep` for `jfl-composition` with the module restriction stated: it would lift
  `thread::sleep` and every timed wait for the whole crate, the loosening H1 refused.
- No timer: hand-rolled deadlines on `poll(2)` inside the Tokio loop would duplicate the runtime's
  timer wheel for no gain, and an idle exit needs a tick in any case.
