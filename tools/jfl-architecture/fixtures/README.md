# Negative fixtures of the gate

Each directory is its own Cargo workspace (path dependencies only, so it resolves offline),
driven by `tests/gate_negative_cases.rs` (the tool-enforced rules, FND-02) or
`tests/fixture_workspaces.rs` (the architecture test's own end-to-end cases, FND-07; fnd §3.4);
none is a member of the main workspace, and the source scan does not read them. Every other
negative case of the architecture test is table-driven over synthetic `cargo metadata` JSON in
`tests/rules.rs`, whose meta-test fails when a rule fnd §3.3 assigns to FND-07 has no case.

| Fixture | Rule | Fails |
|:--|:--|:--|
| `process-without-grant` | `ARCH-process-authority` | clippy with the root `clippy.toml` |
| `sleep-in-test` | `ARCH-test-sleep` | clippy with the root `clippy.toml` |
| `pure-depends-on-std` | `ARCH-domain-purity` (the closure half) | the `nostd` stage; the host build passes |
| `wrapper-wrong-parent` | `wrappers` on workspace path crates (fnd §2.5) | `cargo deny check bans` with `deny.toml`; `deny-control.toml` passes |
| `authority-gaps` | spawn, sleep, clock and env paths beyond the obvious ones (fnd §2.4) | clippy with the root `clippy.toml` |
| `typo-in-list` | a list entry naming no path (only Tokio and crossbeam carry `allow-invalid`) | the `clippy` stage |
| `flaky-test` | `retries = 0` (ver §2 rule 4), against the fixture's own `retries = 3` | the `unit-tests` stage; plain nextest passes it as flaky |
| `build-script-in-workspace` | the build-script allowlist, workspace crates included | `cargo deny check bans` with the repository's `deny.toml` |
| `domain-depends-on-application` | `ARCH-forbidden-edge` | the architecture test's graph checks (`tests/fixture_workspaces.rs`) |
| `adapter-depends-on-adapter` | `ARCH-layer-isolation` (and `ARCH-forbidden-edge`) | the graph checks |
| `testkit-in-production-closure` | `ARCH-production-closure` (`app -> helper -> testkit`, though the table allows the edge) | the graph checks |
| `oracle-imports-domain` | `ARCH-independent-oracle` (though the table allows the edge) | the graph checks |
| `unlisted-external` | `ARCH-unlisted-external` (a path crate outside the workspace stands in for a registry crate) | the graph checks |
| `pure-uses-std` | `ARCH-domain-purity` (the crate's own code) | `cargo build`: a `#![no_std]` crate naming `std::fs` does not compile |

The `_preflight` cases (steering variables, cargo config files, `RUSTC_WRAPPER`) and the
`check-only` cases need no fixture: they run the justfile with a cleared environment.
