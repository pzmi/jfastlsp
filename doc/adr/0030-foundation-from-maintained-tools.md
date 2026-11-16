# 0030. Foundation from maintained tools: just, mise, cargo-deny, no_std and a thin test layer

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07). Supersedes
[0011](0011-pinned-toolchains-rust-1-97-0-and-tools-versions-toml.md) and
[0012](0012-port-xtask-from-mayfly.md).

## Context

ADR 0012 ported mayfly's `tools/xtask`: 32,770 lines measured, of which about 16k are generic
(the architecture graph and policy, gate composition, doctor, ADR, land, coverage, action pins,
security, formal and conformance). A survey of 2026-11-16 found a maintained tool for nearly
each piece; what stays custom is about 700 to 900 lines of glue.

## Decision

- **Toolchain** (carried over from ADR 0011): `rust-toolchain.toml` pins Rust 1.97.0 with
  `rustfmt`, `clippy`, `llvm-tools-preview`; edition 2024, resolver 3; `Cargo.lock` committed and
  every cargo command `--locked`.
- **Tool pins: [mise](https://github.com/jdx/mise)** (`mise.toml` + `mise.lock`,
  `mise install --locked`, checksums and aqua verification) for every other tool: just,
  cargo-nextest, cargo-deny, cargo-llvm-cov, cargo-mutants, diff-cover, gitleaks, actionlint,
  shellcheck, zizmor, pinact, adrs, jq, Node with Quint, Apalache, a Temurin JDK, later
  cargo-fuzz with its nightly, dist and release-plz. Replaces `tools/versions.toml`, `doctor` and
  `install-tools`; CI uses `jdx/mise-action`.
- **Gate: [just](https://github.com/casey/just)** with nextest profiles: one recipe per stage;
  `just quick`, `just check` and `just check-only <stage>…`; CI jobs call the same recipes;
  `just --list` and `just --dump --dump-format json` are the catalog. A `_preflight` recipe
  refuses an environment that steers the build (`RUSTC_WRAPPER`, `RUSTFLAGS` beyond
  `-D warnings`, `CLIPPY_CONF_DIR`, `RUSTC_BOOTSTRAP`, `CARGO_UNSTABLE_*`).
- **Purity by the compiler:** pure crates are `#![no_std]` + `extern crate alloc`. Elsewhere,
  clippy `disallowed-methods`/`-types`/`-macros` (denied in `[workspace.lints]`): a root
  `clippy.toml` with the canonical list, and per-crate `clippy.toml` files that drop exactly the
  crate's grants (clippy has no inheritance and no module wildcards). The daemon launcher spawns
  through `jfl-process`, so no crate needs a module-level grant.
- **[cargo-deny](https://github.com/EmbarkStudios/cargo-deny)**: advisories, licences, sources,
  and bans with `wrappers` (each adapter-only external crate usable only by its adapter),
  `[[bans.features]]` and a build-script allowlist.
- **One test crate, `jfl-architecture`** (`tools/jfl-architecture`, test only, about 200 lines
  over `cargo_metadata`): every member classified, the layer matrix of `architecture.toml`,
  dev edges, the production closure, independent oracles, the external allowlist, lint config,
  the clippy lists in sync, a small source scan (test names and sections, `#[ignore]`, unbacked
  `#[allow]`, `cfg!(target_os…)`), and the CI lockstep. Table-driven cases over synthetic
  metadata plus a few negative fixture workspaces. **No `tools/xtask`.**
- **ADRs: [adrs](https://crates.io/crates/adrs)**, Nygard format kept (ADR 0001); `just
  adr-index` checks the index. **Land:** `scripts/land.sh` (about 150 lines) and a DCO check in
  CI (ADR 0002).
- **Coverage:** cargo-llvm-cov with [diff-cover](https://pypi.org/project/diff-cover/) for changed
  lines and a script of about 100 lines for per-crate floors. **Mutation:** cargo-mutants
  `--in-diff`.
- **Workflows and secrets:** zizmor (`unpinned-uses` hash pins), pinact `--check`, actionlint,
  Dependabot; gitleaks, GitHub secret scanning, and a private-name grep whose patterns live
  outside the repository.
- **Formal:** [quint-connect](https://github.com/informalsystems/quint-connect) and `itf` drive
  the models; about 200 lines replay committed traces (ADR 0007).
- **Release** (REL-06): dist and release-plz.

## Consequences

The FND lane is six smaller tasks. Rules the ported semantic checker would have enforced and no
tool covers (command and query isolation, observability isolation, hidden seams, log event names)
bind by review until a task proves a need for `dylint`. Risks: clippy lists are lints, not
compile errors; `cargo-deny` wrappers on path crates and quint-connect 0.1 are confirmed by a
test before relying on them.

## Alternatives considered

- Port xtask (ADR 0012): about 16k lines to own. `cargo-bylaw`, `cargo-pup`: too young.
  `cargo-make`: last release January 2025. A PR and merge-queue flow instead of `land.sh`: the
  owner chose trunk (ADR 0002), and merge queues need an organisation-owned repository.
