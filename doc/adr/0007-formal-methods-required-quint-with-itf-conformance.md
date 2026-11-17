# 0007. Formal methods required: Quint models with ITF conformance against the real code

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17); amended 2026-11-16, 2026-11-17 (owner) and 2026-11-17 (FORMAL-02).

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

## Amended 2026-11-17 (owner): Quint models and property-based state-machine tests, both

The owner answered "both": the Quint models stay, and property-based state-machine tests on
[`proptest-state-machine`](https://crates.io/crates/proptest-state-machine) are required beside
them. Each covers what the other cannot:

- **Quint models** (FORMAL lane, ver §6) state the design: views and generations, the daemon
  lifecycle and diagnostics publication as protocols with invariants, explored over interleavings
  of several actors (a checkout during a reconcile, two proxies spawning at once, results arriving
  in reverse order) by simulation and Apalache's bounded verification, and replayed against the
  real code through ITF conformance drivers. They catch a wrong design and a real component that
  drifts from it.
- **`proptest-state-machine` tests** (ver §6.4) state the implementation: a reference state
  machine in Rust drives the real component with random command sequences, checks its
  postconditions after every command and shrinks a failure to a minimal sequence. They run in the
  ordinary test stages of every gate with no Node or JVM, reach the component's own data
  (generation bookkeeping, overlays, the diagnostics coordinator's versions, the lock and socket
  states, a pool's cap and LRU, caches) with far more steps than a trace budget allows, and
  cover stateful components no model describes.

Every component with a Quint driver (IDX-09, IDX-12, LSP-04) also gets a state-machine test, and
any other stateful component (the javac worker pool, the summary and library stores, the frame
decoder) gets one where its state outlives a call. Neither replaces the other: a model change
still comes first (model first), and a state-machine test never stands in for a missing driver.

## Amended 2026-11-17 (FORMAL-01): the toolchain as pinned, and Apalache without its server

The toolchain lands with Quint 0.33.1 (from npm, its whole dependency graph locked by mise),
Node 26.13.1 to run it, Apalache **0.62.4** rather than 0.62.1 (the version Quint 0.33.1 names as
its own, `DEFAULT_APALACHE_VERSION_TAG`; 0.63.0 is newer and not Quint's pairing) and the Temurin
JDK 21.0.13+9 to run Apalache, pinned through mise and `tools/versions.toml` (ADR 0035, which
superseded ADR 0030). The `formal-typecheck` and `formal-test` stages gate every push (the
`formal` job of `ci.yml`); `formal-verify` runs Apalache directly on Quint's JSON IR (`quint
compile --target json`, then `apalache-mc check`), not through `quint verify`, which would
download Apalache at run time and drive it as a server on a TCP port. Quint's tests and
simulations run on its TypeScript backend, since the default Rust backend downloads an evaluator
at run time. This ADR's second half, the `proptest-state-machine` tests, needs no toolchain: they
arrive with the components they test (ver §6.4).

## Amended 2026-11-17 (FORMAL-02): the harness is ours over `itf`; quint-connect is not taken

The 2026-11-16 amendment reused `quint-connect` 0.1 for the drivers and the generated runs. Read
at 0.1.2 (the latest, Apache-2.0, released 2026-05-25), it does not fit, for reasons the crate
cannot be configured around:

- `Step`, the value its `Driver::step` receives, can be built only inside the crate, and its
  replay loop is private: a committed regression trace cannot go through a quint-connect driver,
  so the drivers would serve generated traces only, and the committed replay would need a second
  driver interface.
- Its trace generation runs `quint run` without `--backend`, so Quint's default `rust` backend
  downloads an evaluator from GitHub at run time (fnd §8 rule 5; FORMAL-01 runs Quint on its
  TypeScript backend for this reason), and it spawns `quint` itself, while `jfl-conformance` holds
  no process grant.
- Its seed (`QUINT_SEED`) and verbosity (`QUINT_VERBOSE`) are read with `option_env!` at compile
  time, and its divergence report says "State invariant failed" with no trace, step, expected or
  actual state unless verbosity was compiled in.
- It depends on `anyhow`, which `deny.toml` wraps to binaries and tools (ARCH-anyhow-in-library),
  and on a build script.

Decision: `jfl-conformance` holds its own minimal harness over `itf` 0.4 (Apache-2.0, the
crate quint-connect itself uses): a `Driver` trait of quint-connect's shape (a step dispatched by
action name with its picks; a serde observation compared after every step, the initial state
included), the ITF reading (fail closed: anything but a trace with Quint's `--mbt` metadata is
`HARNESS_ERROR`), the replay and its divergence report (model, seed, trace, step, action,
expected, actual and a line diff by `similar`), the regression manifest and a binary. xtask, which
holds the process grant, generates traces with `quint run --backend typescript --mbt` at a printed
seed and runs the binary: the stages `formal-freshness` and `conformance-replay` (ver §6.2). Each
driver's negative control runs in its tests and again over every generated run, which fails
unless the control diverges. The cost: `itf` brings `dashu-int` and `rustversion`, whose build
scripts `deny.toml` admits by checksum. quint-connect stays the runner-up: should it expose `Step`
construction and a backend choice, the drivers can move to it with their shape unchanged.
