# 0030. Foundation from maintained tools: just, mise, cargo-deny, no_std and a thin test layer

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07); amended
2026-11-16 and 2026-11-17. Supersedes
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

## Amended 2026-11-16 (planner, BOOT-02): review findings before FND-01

- **Purity needs a no-std target.** `#![no_std]` refuses std in the crate's own code but not a
  std-only dependency. A `nostd` stage builds the pure crates for `x86_64-unknown-none` (in
  `rust-toolchain.toml`'s targets), and a negative fixture crate proves the first half; a
  `compile_fail` doctest cannot, since rustdoc links doctests as std binaries (fnd §2.3, §3.4).
- **Bootstrap.** mise and just are installed by `scripts/bootstrap-tools.sh` at pinned,
  checksum-verified versions (fnd §8); prebuilt `aqua:`/`github:` backends are preferred to
  `cargo:`.
- **Private names are hashed, not hidden.** The private-name grep needed a pattern file kept
  outside the repository, which no fresh session or fork has. Instead the repository commits
  salted SHA-256 hashes of the normalised names (`.private-names.sha256`); the check hashes the
  tokens of the changed text, needs no secret, and is a stage of `check` (fnd §7.4).
- **Bench gate on one runner class.** Wall times from a laptop, the sandbox and a shared runner
  are not comparable, so `bench-gate` runs only in `bench.yml` on a fixed runner class against
  that class's history (medians, reproduced once), gating on instruction counts where possible;
  `check` and `land.sh` never run it (fnd §9, ADR 0031).
- **Diff-based stages.** `changed-lines`, `mutants-diff` and `private-names` use
  `git merge-base HEAD origin/main` plus the working tree as their base; `mutants-diff` leaves
  `check` and runs in CI and before landing large tasks, with a time cap.
- **Seven FND tasks.** FND-02 is the gate and its tools; the architecture test is FND-07.

## Amended 2026-11-17 (orchestrator, FND-07): the architecture test's real size and its build scripts

Status of this amendment: Proposed (orchestrator; owner confirms at FND-05).

- **Size.** The architecture test is about 2,000 formatted lines of source plus about 1,500 of
  tests, not "about 200" (and the glue estimate in Context is low by the same amount). Most of
  it is what makes the test trustworthy rather than features: the policy parsed strictly
  (`serde` with `deny_unknown_fields`, every table a typed struct); the `ARCH-clippy-lists` model
  computed from the root list's sections independently of `scripts/clippy-lists.sh`, so a
  generator bug cannot agree with itself; and a small lexer that removes literals and comments
  and joins lines, so the source scan reads multi-line attributes and `cfg!` whole and never
  matches inside a string. It is still the reuse choice: `cargo_metadata` does the graph,
  cargo-deny, clippy and the compiler keep everything they can express, and no maintained tool
  checks a layer table, per-crate clippy lists against grants, or test naming (`cargo-bylaw` and
  `cargo-pup` remain too young, `dylint` needs real name resolution only for rules that bind by
  review). The ported checker this ADR rejected was about 16k lines.
- **Dependencies.** `cargo_metadata`, `toml` and `serde` (derive) for jfl-architecture, and
  `serde_json` for its tests, each in `architecture.toml`'s `[external.*]` with its reason.
- **Build scripts.** Their closure runs eight build scripts (camino, proc-macro2, quote, serde,
  serde_core, serde_json, thiserror, zmij), each a rustc or target probe that only sets cfgs.
  `deny.toml` admits them through `[bans.build] bypass`, each pinned by the SHA-256 of its
  `build.rs` as read, rather than by name in `allow-build-scripts`: a release whose script
  changes fails `bans` until it is read again (cargo-deny 0.21 supports this; a negative case in
  `tests/gate_negative_cases.rs` pins it). The bypass is global, by crate rather than by
  dependent: a production crate that later pulls serde reuses this review, and the task that
  does so says so (fnd §3.5).

