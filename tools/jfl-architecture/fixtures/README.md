# Negative fixtures of the gate

Each directory is its own Cargo workspace (path dependencies only, so it resolves offline),
driven by `tests/gate_negative_cases.rs`; none is a member of the main workspace. FND-02 holds
the negative cases of the tool-enforced rules; FND-07 adds the architecture test's own (fnd §3.4).

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

The `_preflight` cases (steering variables, cargo config files, `RUSTC_WRAPPER`) and the
`check-only` cases need no fixture: they run the justfile with a cleared environment.
