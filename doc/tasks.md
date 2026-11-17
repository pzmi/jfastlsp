# jfastlsp: task breakdown

This file cuts the build of [`plan.md`](plan.md) (product and architecture),
[`foundation.md`](foundation.md) (engineering rules) and [`verification.md`](verification.md)
(test levels, oracles, formal models) into small tasks in dependency order. The three documents
own *what*; a citation "plan §5.2", "fnd §3" or "ver §4" points to a section of one of them. This
file owns *order* and *size*. Each task is one focused commit that a fresh agent can finish in one
session after reading only what the card names.

**Live gate:** `just check` (and `just quick` while iterating), with tools from
`mise install --locked`; mise and just come from `scripts/bootstrap-tools.sh` (fnd §8), which the
SessionStart hook runs in cloud sessions. The orchestrator lands commits with `scripts/land.sh`
([`sdlc.md`](sdlc.md) §2), and CI (`ci.yml`, [`required-checks.md`](required-checks.md)) runs
the same stages on every push to `main`.

**Review checkpoints (⛳):** after a ⛳ task lands green, the orchestrator runs `/review` plus an
adversarial second review over the range since the previous `checkpoint-*` tag, lands the fixes,
tags `checkpoint-<ID>` locally and reports to the owner with the open owner decisions.
Checkpoints: **FND-05** (gate, dependency law and CI; the owner confirms the Proposed defaults,
DEC-07), **FORMAL-05** (models and harness), **DEC-02** (phase 0 go/no-go and the speed
targets), **LSP-06** (M1, the LSP carrier, and the blocking bench gate), **IDX-15** (M2, the
CLI carrier), **SEM-07** (M4),
**REL-06** (release preview).

## Overview

| ID | Title | Depends on | Model | Size | ⛳ | Parallel with |
|---|---|---|---|---|---|---|
| FND-01 | Workspace, toolchain, crate stubs, mise | — | standard | M |  | DEC-01 |
| FND-02 | `just` gate, nextest, cargo-deny, clippy authority lists, `nostd` stage | FND-01 | deep | M |  | DEC-01, BENCH-01 |
| FND-07 | Architecture test (`jfl-architecture`) and its negative fixtures | FND-02 | deep | M |  | EVAL-01, EVAL-11 |
| FND-03 | ADR tooling, `scripts/land.sh`, DCO check | FND-07 | standard | M |  | EVAL-01, BENCH-02, EVAL-11 |
| FND-04 | Security and supply chain: gitleaks, zizmor, pinact, actionlint, private names | FND-03 | standard | S |  | EVAL-11 |
| FND-05 | CI workflows and required checks | FND-04 | standard | M | ⛳ | EVAL-02, EVAL-06 |
| FND-08 | Checkpoint fixes: close the gate bypasses found at the FND-05 ⛳ | FND-05 | deep | L |  | FND-06, BENCH-03 |
| FND-09 | Close the gate gaps left after FND-08: tool configs, secrets in trees, mise before its check | FND-08 | deep | M |  | FND-06, BENCH-03 |
| FND-06 | Coverage, changed lines, mutation in the diff | FND-05 | standard | M |  | EVAL-03, EVAL-10, BENCH-03 |
| DEC-07 | (owner) Confirm the Proposed defaults | FND-05 | light | S |  | DEC-08 |
| DEC-08 | (owner) Shared skills plugin or repo-local skills | FND-05 | light | S |  | DEC-07 |
| FND-10 | Toolchain 1.99.1 | FND-08 | standard | S |  | XT-01 |
| FND-11 | Remove the private-names hashes and check | FND-04, XT-02 | standard | S |  | EVAL lane |
| FND-12 | Delete the jdtls proxy; keep stock jdtls as a bench arm | FND-11, BENCH-03 | standard | M |  | EVAL lane |
| XT-01 | Scaffold `tools/xtask` and port the gate composition | FND-08, FND-09 | deep | M |  | FND-10, EVAL lane |
| XT-02 | Tool pins: `tools/versions.toml`, `cargo xtask doctor`, `install-tools` | XT-01 | standard | M |  | EVAL lane |
| XT-03 | Security and workflow stages into xtask | XT-02 | standard | M |  | EVAL lane |
| XT-04 | Clippy lists, ADR index and DCO into xtask | XT-03 | standard | S |  | EVAL lane |
| XT-05 | `cargo xtask land` replaces `scripts/land.sh` | XT-04 | deep | M |  | EVAL lane |
| XT-06 | The architecture checks move into `cargo xtask architecture` | XT-05 | deep | M |  | EVAL lane |
| XT-07 | Bench and fetch scripts into xtask | XT-06 | standard | S |  | EVAL lane |
| XT-08 | Remove just and mise | XT-07 | standard | M |  | EVAL lane |
| BENCH-01 | Benchmark corpus: pinned real repositories and the fetch script | FND-01 | standard | S |  | FND-02 |
| BENCH-02 | Bench harness: `jfl-bench`, hyperfine, divan, the `rg` floor | BENCH-01, FND-02 | standard | M |  | FND-03 |
| BENCH-03 | Comparison arms and the first baseline | BENCH-02, EVAL-11 | standard | M |  | FND-06 |
| BENCH-04 | Results history and the regression gate on one runner class | BENCH-03, FND-05 | standard | S |  | FORMAL-01 |
| BENCH-05 | Real-corpus golden cases | BENCH-04, EVAL-08 | deep | M |  | EVAL-10 |
| BENCH-06 | Lombok repository in the corpus and as a second evaluation target | BENCH-01 | standard | S |  | BENCH-05 |
| FORMAL-01 | Formal toolchain: Quint, Apalache, JDK pins, `MODELS`, formal stages | FND-05 | standard | M |  | EVAL-12, BENCH-04 |
| FORMAL-02 | Conformance harness: quint-connect drivers, committed-trace replay, negative controls | FORMAL-01 | deep | M |  | EVAL-07, EVAL-05 |
| FORMAL-03 | `views.qnt`: generations, freshness, completeness | FORMAL-02, DEC-02; started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton | deep | M |  | IDX-01, IDX-04 |
| FORMAL-04 | `daemon.qnt`: single owner, spawn, reconnect, no ghost buffers | FORMAL-03 | deep | M |  | IDX-02 |
| FORMAL-05 | `diagnostics.qnt`: staleness | FORMAL-04 | deep | M | ⛳ | IDX-03 |
| DEC-01 | (owner) Choose the target Java repository | — | light | S |  | FND-01, FND-02 |
| EVAL-01 | Evaluation harness: Harbor adapter, arms, transcript metrics | FND-02 | standard | M |  | FND-07, FND-03 |
| EVAL-11 | jdtls behind our multiplexing proxy (`jfl-jdtls`; later removed by FND-12) | FND-02 | deep | M |  | FND-07, FND-03, FND-04 |
| EVAL-02 | E0 task set from the target repository's history | DEC-01, EVAL-01 | standard | M |  | FND-05 |
| EVAL-03 | E0 arms, Harbor images and runbook | EVAL-01, EVAL-11 | standard | M |  | FND-06 |
| EVAL-12 | E0 control set from Multi-SWE-bench Java | EVAL-01 | standard | S |  | FORMAL-01 |
| EVAL-04 | (owner) Run E0 and report | EVAL-02, EVAL-03, EVAL-12 | standard | M |  | — |
| EVAL-05 | E1 in the harness: parse and summarise the corpus on the bench runner and the target | BENCH-04 | standard | S |  | FORMAL-02 |
| EVAL-06 | E4: stock jdtls at 1, 3 and 10 worktrees | FND-12, BENCH-02 | standard | M |  | FND-05, EVAL-08 |
| EVAL-07 | Golden resolution corpus and `java-fixtures` stage | FORMAL-01 | deep | L |  | FORMAL-02 |
| EVAL-08 | SCIP oracle (`jfl-oracle`) | EVAL-07 | deep | M |  | EVAL-06 |
| EVAL-09 | E2 resolver bake-off | EVAL-07, EVAL-08, EVAL-10, BENCH-05 | deep | L |  | DEC-03 |
| EVAL-10 | E3 `javac` worker spike: our own worker with a warm javac context | EVAL-01 | standard | M |  | FND-06, BENCH-05 |
| DEC-03 | Storage tier (from E1) | EVAL-05 | deep | S |  | DEC-04, DEC-05, DEC-06, EVAL-09 |
| DEC-04 | Resolver and the SCIP baseline (from E2) | EVAL-09 | deep | S |  | DEC-03, DEC-05, DEC-06 |
| DEC-05 | Semantic diagnostics in v1 (from E3) | EVAL-10 | deep | S |  | DEC-03, DEC-04, DEC-06 |
| DEC-06 | `referencesMode` default and operation priority (from E0) | EVAL-04 | deep | S |  | DEC-03, DEC-04, DEC-05 |
| DEC-02 | Go/no-go memo, the kill criterion and the speed targets | DEC-03, DEC-04, DEC-05, DEC-06, DEC-07, EVAL-06, BENCH-03 | deep | M | ⛳ | — |
| IDX-01 | Domain: paths, positions and encodings | DEC-02; started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton | standard | M |  | IDX-04, FORMAL-03 |
| IDX-02 | Domain: summaries, symbols, cache keys, the envelope | IDX-01; started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton | deep | M |  | IDX-04, FORMAL-04 |
| IDX-03 | Protocol v1: messages, versions, frame decoder, goldens | IDX-02; started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton | deep | M |  | IDX-05, IDX-06, FORMAL-05 |
| IDX-04 | `jfl-process`: argv-only child runner | DEC-02; started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton | deep | M |  | IDX-01, IDX-02, FORMAL-03 |
| IDX-05 | Git adapter: roots, identity, freshness stat, changed files | IDX-01, IDX-04 | standard | M |  | IDX-03, IDX-06 |
| IDX-06 | FS adapter: scanning, nested-worktree exclusion, watcher hints | IDX-01 | standard | M |  | IDX-03, IDX-05 |
| IDX-07 | Syntax adapter: tree-sitter summaries and the golden Java corpus | IDX-02 | deep | L |  | IDX-08 |
| IDX-08 | Summary store at the DEC-03 tier | IDX-02, DEC-03 | standard | M |  | IDX-07 |
| IDX-09 | View registry, generations and freshness (views driver) | IDX-05, IDX-06, IDX-07, IDX-08, FORMAL-03 | deep | L |  | LSP-01 |
| IDX-10 | Queries: `sym`, `def`, `hover` | IDX-09 | deep | L |  | LSP-01 |
| IDX-11 | Queries: `refs`, the `possible` superset, completeness | IDX-10 | deep | L |  | IDX-12 |
| IDX-12 | Daemon and IPC: lock, socket, spawn, reconnect (daemon driver) | IDX-03, IDX-09, FORMAL-04 | deep | L |  | IDX-11 |
| LSP-01 | LSP adapter on `lsp-server` and the LSP conformance harness | IDX-03; started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton | deep | L |  | IDX-09, IDX-10 |
| LSP-02 | `jfl lsp`: lifecycle, encodings, document sync, deadlines | LSP-01, IDX-12 | deep | L |  | IDX-11 |
| LSP-03 | Navigation methods | LSP-02, IDX-11, DEC-06 | standard | L |  | LSP-04 |
| LSP-04 | Syntax diagnostics and the coordinator (diagnostics driver) | LSP-02, FORMAL-05 | deep | L |  | LSP-03, LSP-05 |
| LSP-05 | LSP plugin and `.lsp.json` | LSP-03 | standard | M |  | LSP-04 |
| LSP-06 | M1 gate suite and the blocking bench gate | LSP-03, LSP-04, LSP-05 | deep | M | ⛳ | — |
| IDX-13 | `jfl` CLI: subcommands, output, `--json`, exit codes, `jfl impls` | LSP-06 | standard | M |  | — |
| IDX-14 | CLI plugin, skill, `AGENTS.md` and cloud setup | IDX-13 | standard | M |  | — |
| IDX-15 | M2 gate: the CLI carrier | IDX-14 | standard | M | ⛳ | — |
| PROJ-01 | Gradle model: wrap build-server-for-gradle, opt-in, fingerprint cache | IDX-15 | deep | L |  | PROJ-03 |
| PROJ-02 | Modules, visibility and `EnvironmentKey` in the envelope | PROJ-01 | deep | M |  | PROJ-04 |
| PROJ-03 | Class-file, JAR and `lib/modules` reader on cafebabe and ristretto_jimage | IDX-15, FND-10 | deep | L |  | PROJ-01 |
| PROJ-04 | JDK API index from `lib/modules` and `ct.sym`, the shared library index | PROJ-03 | standard | M |  | PROJ-02 |
| PROJ-05 | Library types in hover and definition; M3 gate | PROJ-02, PROJ-04 | standard | M |  | — |
| DEP-01 | Switch cafebabe from the git pin to the first release with the 2026-11-13 hostile-input fix | PROJ-03 and a cafebabe release with the fix | light | S |  | any |
| SEM-01 | Resolver core per DEC-04 | PROJ-05, DEC-04 | deep | L |  | SEM-04 |
| SEM-02 | Resolver: inferred receivers and generated members | SEM-01 | deep | L |  | SEM-05 |
| SEM-03 | Exact answers in the envelope; envelope-honesty mutants | SEM-02 | deep | M |  | SEM-05 |
| SEM-04 | `javac` worker (our own, scip-java plugin) and pool | PROJ-02, DEC-05 | deep | L |  | SEM-01 |
| SEM-05 | Semantic diagnostics, processors and `jfl diag` | SEM-04, LSP-04 | deep | L |  | SEM-02, SEM-03, SEM-08 |
| SEM-06 | SCIP differential gates | SEM-03, EVAL-08 | deep | M |  | — |
| SEM-08 | SCIP baseline import (M6 folded in) | SEM-06 | deep | M |  | SEM-05 |
| SEM-07 | M4 gate | SEM-05, SEM-06, SEM-08 | standard | M | ⛳ | — |
| REL-01 | Call hierarchy and `jfl calls` | SEM-07 | standard | L |  | REL-03 |
| REL-02 | Resource limits and watcher recovery | SEM-07 | deep | L |  | REL-03 |
| REL-03 | Fuzz lane | SEM-07 | standard | M |  | REL-01, REL-02 |
| REL-04 | Crash and restart soak | REL-02 | deep | M |  | REL-05 |
| REL-05 | Full mutation lane and budget ratchet | REL-02 | standard | M |  | REL-04 |
| REL-06 | Release workflow and release preview | REL-01, REL-03, REL-04, REL-05 | standard | M | ⛳ | — |

## Tasks

### Lane FND: foundation

Seven small tasks assemble the foundation from maintained tools (ADR 0030): mise pins, a `just`
gate, the compiler, clippy and cargo-deny for the dependency law (FND-02), one architecture test
(FND-07), adrs and a land script, the security scanners, CI, coverage and mutation in the diff. The FND checkpoint (FND-05)
puts the Proposed defaults and the skills question to the owner (DEC-07, DEC-08, both decided on
2026-11-17); FND-08 lands the checkpoint reviews' fixes and FND-09 the gaps its re-verification
found; FND-10 moves the toolchain to 1.99.1. Lane XT then ports mayfly's `xtask` over this
foundation piece by piece (ADR 0035).

#### FND-01: Workspace, toolchain, crate stubs, mise
- **Depends on:** none. **Model:** standard. **Size:** M. **Parallel with:** DEC-01.
- **Cites:** plan §6.1, §6.3, §6.5, §1.6, §16; fnd §2.1, §2.3, §2.4, §4.1, §4.2, §8; ADR 0006, ADR 0010, ADR 0030, ADR 0033.
- Create the Cargo workspace with every production, test-only and tool-only crate of plan §6.1 as
  a stub: `Cargo.toml` (explicit `members`, `exclude = ["spikes"]` so the archived E1 spike stays
  outside, resolver 3, `[workspace.package]` with `version = "0.1.0"`, `edition = "2024"`,
  `rust-version = "1.97"`, `license = "MIT"` (ADR 0033; FND-01 wrote ADR 0005's dual licence),
  repository, readme;
  `[workspace.lints]` exactly as fnd §4.2), each crate's `Cargo.toml` (`publish = false`,
  `[lints] workspace = true`, only the path dependencies plan §6.3 allows) and `src/lib.rs` with
  a `//!` naming its single responsibility and role; the pure crates `#![no_std]` with
  `extern crate alloc`; `jfl-cli` with `[[bin]] name = "jfl"` whose `main` prints the version;
  `tools/jfl-architecture` as an empty test crate; `tools/jfl-oracle`, `tools/jfl-eval`,
  `tools/jfl-jdtls` and `tools/jfl-bench` as binaries printing usage. `rust-toolchain.toml`
  (1.97.0, `rustfmt`, `clippy`, `llvm-tools-preview`, `targets = ["x86_64-unknown-none"]` for the
  `nostd` stage, fnd §2.3); `mise.toml` and `mise.lock` pinning `just` (at the version
  `scripts/bootstrap-tools.sh` pins) and `cargo-nextest` at their current releases (checked,
  never invented; prebuilt backends, fnd §8), every lock entry with a checksum or a recorded
  reason; a first `justfile` with `doctor`, which also fails when `just --version` differs from
  the pin; `deny.toml` skeleton per fnd §7.4 with `[graph] targets`; `architecture.toml` with
  `schema = 1` and the `[workspace]`, `[roles]`, `[grants]` (every crate's row of fnd §2.4,
  production and tool and test crates alike) and `[dependencies.normal]` tables of fnd §3.1 and
  plan §6.3 filled, empty `[external.*]` and `[exceptions]`; `.editorconfig`, `.gitattributes`
  (`* text=auto eol=lf`); `Cargo.lock`. No third-party dependency.
- **Reuse considered:** mise for every non-Rust tool (replaces `tools/versions.toml`, `doctor` and
  `install-tools`), bootstrapped by `scripts/bootstrap-tools.sh` (BOOT-02); `#![no_std]` plus a
  no-std target build instead of a purity checker; `cargo-run-bin` rejected (Rust tools only).
- **Accept:**
  - [x] the fallback gate is green; `cargo build --locked --workspace` succeeds
  - [x] `cargo metadata --format-version 1 --no-deps` lists exactly the crates of plan §6.1 (minus `jfl-fuzz` and `jfl-resolve`), and nothing under `spikes/`
  - [x] `grep -L 'publish = false' crates/*/Cargo.toml tools/*/Cargo.toml` prints nothing; every member has `[lints] workspace = true`
  - [x] `target/debug/jfl --version` prints `jfl 0.1.0`; `rustc --version` inside the repository reports 1.97.0
  - [x] `bash scripts/bootstrap-tools.sh` succeeds in the sandbox, then `mise install --locked` succeeds from an empty mise data directory and `just doctor` exits 0
  - [x] each pure crate's `lib.rs` starts with `#![no_std]` (a grep in the commit body; FND-07 makes it a rule) and `cargo build --locked -p jfl-domain -p jfl-protocol --target x86_64-unknown-none` succeeds
- **Impact:** dependency graph (initial).

#### FND-02: `just` gate, nextest, cargo-deny, clippy authority lists, `nostd` stage
- **Depends on:** FND-01. **Model:** deep. **Size:** M. **Parallel with:** DEC-01, BENCH-01.
- **Cites:** plan §6.3, §6.5; fnd §2.3, §2.4, §2.5, §3.3, §3.6, §9; ver §2, §3; ADR 0010, ADR 0018, ADR 0030.
- The gate and the tool-enforced half of the dependency law (fnd §3). `justfile`: `_preflight`
  (fnd §3.6), one recipe per stage (`fmt-check`, `clippy`, `nostd`, `deny`, `unit-tests`,
  `integration-tests`, `doctests`; `architecture` arrives with FND-07), `quick`, `check` and
  `check-only <stage>…`. `.config/nextest.toml` (`default`, `ci` with JUnit and no fail-fast,
  `serial`; `retries = 0`; slow-timeout 30 s × 4; a `daemon-or-jvm` test group bounded to 2);
  `jfl-testkit::poll` (`wait_until(deadline, reason, condition)`). `cargo-deny` pinned by mise;
  `deny.toml` with advisories, the licence allowlist of plan §16.2, crates.io only, and `bans`
  (wrappers for the adapter-only crates of fnd §2.5, features, the build-script allowlist). The
  root `clippy.toml` canonical list grouped by grant and every crate's own list per fnd §2.4's
  table; `disallowed_*` denied in `[workspace.lints.clippy]`. The negative cases of this card's
  rules as small fixture crates under `tools/jfl-architecture/fixtures/` driven by one test file
  there (FND-07 builds the rest of that crate around them).
- **Reuse considered:** `cargo-deny` bans with `wrappers`, clippy `disallowed-*`, `#![no_std]`
  with a no-std target build, and nextest profiles replace mayfly's checker; `cargo-bylaw` and
  `cargo-pup` (0.1, too young) and `dylint` (only for a rule clippy cannot express) were weighed;
  `cargo-make` is stale.
- **Accept:**
  - [x] `just quick` and `just check` exit 0 on the tree; `just check-only nope` exits non-zero naming the valid recipes
  - [x] a path crate wrapped to the wrong parent fails `cargo deny check bans` (confirms `wrappers` on workspace crates)
  - [x] `std::process::Command` in a crate without the `process` grant, and `std::thread::sleep` in a test outside `poll`, each fail clippy; `poll` times out with its reason in the message
  - [x] a pure fixture crate with a path dependency on a std crate builds on the host and fails the `nostd` stage
  - [x] `_preflight` refuses `RUSTC_WRAPPER` (a test that sets it on the child process, not an inline variable in a script)
  - [x] `tasks.md`'s live gate line still holds: the fallback until this commit, `just check` from it
- **Impact:** none (tooling); the tool-enforced dependency law and authority now in force.

#### FND-07: Architecture test (`jfl-architecture`) and its negative fixtures
- **Depends on:** FND-02. **Model:** deep. **Size:** M. **Parallel with:** EVAL-01, EVAL-11.
- **Cites:** plan §6.3, §6.5; fnd §2.1, §2.2, §2.3, §2.4, §2.5, §3.1, §3.2, §3.3, §3.4; ADR 0010, ADR 0030.
- `tools/jfl-architecture`: the graph checks, the clippy-list sync against `[grants]` and the
  source scan of fnd §3.2, as table-driven cases over synthetic `cargo metadata` JSON plus the
  negative fixture workspaces of fnd §3.4, `pure-uses-std` among them; the `architecture` stage
  joins `quick` and `check`; a meta-test fails when a rule of fnd §3.3 has no negative case.
- **Reuse considered:** `cargo_metadata` and `toml` with `deny_unknown_fields`; the ported
  `syn` checker rejected (ADR 0030).
- **Accept:**
  - [x] each rule fnd §3.3 assigns to FND-07 fails its negative case with its id and path; a rule without a negative case fails the meta-test
  - [x] `pure-uses-std` (a `#![no_std]` crate naming `std::fs`) is built by a test and fails to compile
  - [x] a crate's `clippy.toml` that drops a lint its grants do not cover fails `ARCH-clippy-lists`
  - [x] `just quick` runs the stage; the tree is green
- **Impact:** none (tooling); the dependency law now enforced.

#### FND-03: ADR tooling, `scripts/land.sh`, DCO check
- **Depends on:** FND-07. **Model:** standard. **Size:** M. **Parallel with:** EVAL-01, BENCH-02, EVAL-11.
- **Cites:** fnd §7.4, §9, §10.2, §10.3; ADR 0001, ADR 0002, ADR 0030.
- `adrs` pinned by mise and configured for the Nygard format with `doc/adr/template.md`;
  `just adr-index` checks the index rows against the files (`adrs generate toc` compared with the
  table, or a check of about 50 lines if `adrs` does not model our status annotations; the commit
  says which) and joins `check` as the `docs` stage. `scripts/land.sh` (about 150 lines,
  shellcheck-clean): refuse a dirty checkout, fetch, create a temporary worktree at
  `origin/main`, cherry-pick each commit, resolve only `Cargo.lock` (`cargo update -w`) and the
  ADR index, run `just check` on the merged tree (once FND-04 lands `private-names`, with the
  hash file required, fnd §7.4), push, retry once after a fetch on a non-fast-forward,
  `--no-push` for a dry run. `scripts/dco-check.sh`: every
  commit in a range carries a `Signed-off-by` matching its author. Tests in `jfl-architecture`
  drive both scripts against scratch repositories in tempdirs.
- **Reuse considered:** `adrs` (Nygard or MADR 4, index, lint) replaces about 1,090 lines;
  `adr-tools` is unmaintained, `log4brains` a Node site generator. `land`: a merge queue needs an
  organisation-owned repository and a PR flow is not ADR 0002's choice. DCO: the DCO app
  (cncf/dco2) or an inline step; `committed` for message shape where its options fit.
- **Accept:**
  - [x] `just adr-index` is clean over ADRs 0001–0031 as committed (fix the index or the check, never the records' decisions)
  - [x] `adrs new "Test"` in a scratch copy creates the next record from the template and the index check passes after the row is added
  - [x] `land.sh --no-push` on a scratch repository lands a clean commit, refuses a dirty checkout, aborts on a non-lockfile conflict naming the files, and moves nothing when `just check` fails
  - [x] `dco-check.sh` fails on a commit without a matching sign-off; the `docs` stage fails when an index row and a file disagree
- **Impact:** none (tooling).

#### FND-04: Security and supply chain: gitleaks, zizmor, pinact, actionlint, private names
- **Depends on:** FND-03. **Model:** standard. **Size:** S. **Parallel with:** EVAL-11.
- **Cites:** fnd §7.3, §7.4, §8; ADR 0004, ADR 0005, ADR 0030.
- gitleaks, actionlint, shellcheck, zizmor and pinact pinned by mise at their current releases.
  Recipes: `secrets` (gitleaks over the full history; `.gitleaks.toml` with `useDefault` and no
  allowlist), `workflows` (actionlint with shellcheck; zizmor offline audits with an
  `unpinned-uses` hash-pin policy; `pinact run --check --verify-comment`), `private-names`
  (`scripts/private-names-check.sh`: the hashed check of fnd §7.4 over the diff since
  `git merge-base HEAD origin/main`, the working tree and the commit messages in that range,
  against `.private-names.sha256` and `scripts/private-name-add.sh`, both committed in BOOT-02;
  `SECURITY.md` says how the owner adds a name, without naming any). `deny` completed with
  advisories (yanked denied). Fixtures: an unpinned action, a tag-pinned action, a fake secret
  built at run time, a test hash file with its own salt and a planted name.
- **Reuse considered:** zizmor, pinact and actionlint replace mayfly's `action_pins.rs` (about
  860 lines); the gitleaks binary rather than `gitleaks-action` (a licence key for organisation
  accounts); trufflehog (AGPL), `cargo-audit` (redundant with cargo-deny) and `cargo-vet` (heavy
  for one maintainer; optional later) were weighed.
- **Later removed (FND-11, 2026-11-17, the owner's order):** the `private-names` stage, the hash
  file and its scripts; the card stays as the record of what was built.
- **Accept:**
  - [x] the stages, `private-names` included, are in `check` and green on the tree
  - [x] each fixture fails its stage with the offending file named
  - [x] `just private-names` finds the planted name written as `Foo Bar`, `foo-bar` and `FooBar` in a changed line and in a commit message, naming the path and line but never printing the name; a missing hash file fails under `CI=true` and in `land.sh` and only warns otherwise
  - [x] `just doctor` lists the five tools
  - [ ] (owner) further private names added with `scripts/private-name-add.sh` and committed (only hashes) (withdrawn: FND-11 removed the list)
- **Impact:** none (tooling).

#### FND-05: ⛳ CI workflows and required checks
- **Depends on:** FND-04. **Model:** standard. **Size:** M. **Parallel with:** EVAL-02, EVAL-06.
- **Cites:** fnd §7.1, §7.2, §7.3; ADR 0002, ADR 0006, ADR 0030; [`required-checks.md`](required-checks.md).
- `.github/workflows/ci.yml` with the jobs of `required-checks.md` that exist now
  (`architecture`, `static`, `tests`, `linux-arm`, `macos`, `security`, `dco`, `required`): each
  installs tools with `jdx/mise-action` (pinned by SHA) from `mise.lock` and runs `just
  check-only <stage>…`; a `platform-tests` recipe for the native jobs; `Swatinem/rust-cache`
  keyed by toolchain, `Cargo.lock` and `mise.lock`, saved only on `main`. `security.yml`,
  `codeql.yml`, `scorecard.yml`, `.github/dependabot.yml` (cargo, github-actions with a
  cooldown), `.github/CODEOWNERS`, `.github/ISSUE_TEMPLATE/`, `.github/pull_request_template.md`
  (for outside contributors). `ARCH-ci-lockstep` in `jfl-architecture` (fnd §7.2). Rewrite
  `required-checks.md` from the real workflow. This task is the FND checkpoint: its report puts
  DEC-07 and DEC-08 to the owner.
- **Reuse considered:** `jdx/mise-action` rather than `taiki-e/install-action` (one source of
  truth with local runs); `Swatinem/rust-cache`; the OpenSSF Scorecard action; Dependabot rather
  than Renovate (no extra app).
- **Accept:**
  - [x] every workflow green on `main` for this commit; `Required` green
  - [x] zizmor, pinact and actionlint clean; every `uses:` pinned by SHA
  - [x] the lockstep test fails on a fixture `ci.yml` with a stage in two jobs, with `continue-on-error`, and with `|| true`
  - [ ] (owner) GitHub secret scanning and push protection on
  - [ ] (owner) a branch ruleset on `main` requiring only `Required`, if the owner wants one
- **Impact:** CI topology.

#### FND-08: Checkpoint fixes: close the gate bypasses found at the FND-05 ⛳
- **Depends on:** FND-05. **Model:** deep. **Size:** L. **Parallel with:** FND-06, BENCH-03.
- **Cites:** fnd §2.4, §2.5, §3, §3.2, §3.3, §3.6, §7.4, §9; plan §5.3; ADR 0028, ADR 0030; the
  FND-05 checkpoint review and its adversarial second review.
- The fixes of the two FND-05 checkpoint reviews, each written as an allowlist or a pinned shape
  (never a list of bad patterns: fnd §3's standing principle, added here, after FND-02, FND-07
  and FND-05 each needed two or three review rounds for denylist checks), each with a negative
  case that reproduces the reviewer's bypass:
  - **N1** (HIGH): no cargo configuration file inside the repository, at any depth; outside it a
    strict plain shape (`_preflight`), and the same in `CARGO_HOME`; the unicode-escaped runner
    and alias refused.
  - **N3** and the first review's "`check:` not pinned": `ARCH-gate-config` pins the gate's own
    recipes (`check`, `quick`, `check-only`, `ci-parity`, `platform-tests`, every stage and their
    dependencies, the justfile's top-level lines) and the scripts the stages run by SHA-256 in
    `.config/gate.sha256`; `.config/nextest.toml`, `mise.toml` and `rust-toolchain.toml` keep
    allowlisted shapes; the lockstep reads `ci.yml`'s `on:`.
  - **N2**: the source scan's lexer reads every literal prefix (`b`, `c`, `r`, `br`, `cr`) and
    fails closed (`ARCH-hidden-source`) on what it cannot lex; a corpus test over every pair of
    tricky literals and every truncation.
  - **N4**: dependencies matched by source: a member's own path or crates.io, for direct
    dependencies and every package of the graph; no `[patch]`, `[replace]`, `workspace.exclude`
    other than `spikes`, or profile other than `bench-tool`.
  - **N5**: `.gitleaks.toml` is gitleaks' default configuration without its global path
    allowlist, generated from the pinned binary (`scripts/gitleaks-config.sh`) and compared byte
    for byte.
  - **N6**: `mise.toml` is only `[tools]`; no other mise configuration may load; the stages run the
    tools from their install directories, not through shims; the Stop hook and `land.sh` resolve
    `just` the same way.
  - **N7**: the sleep grant covers every timed wait (`recv_timeout`, `Condvar::wait_timeout*`,
    socket read and write timeouts, Tokio's `timeout` and `interval`); `jfl-jdtls` holds it.
  - **N8**, **N9** (LOW): the private-names limits documented; the Stop hook also triggers on
    `.config/`, `.cargo/`, `scripts/`, `.github/`, `.gitleaks.toml` and `mise.*`.
  - Docs: the pre-FND-02/03/05 conditionals of `CLAUDE.md`, `sdlc.md`, `README.md` and the skills
    replaced by the current state; the Accept boxes of the landed tasks ticked; ADR 0028's
    amendment in the `## Amended` form and plan §5.3 stating the socket-path policy that was built
    (Proposed for the product daemon, IDX-12).
- **Reuse considered:** `sha2` (RustCrypto) for the pins, the `toml` crate already in the checker,
  gitleaks' own embedded default configuration rather than a hand-kept copy, `mise bin-paths` and
  `mise config ls` rather than re-implementing mise's search; Python's `tomllib` in `_preflight`
  rejected (not a pinned tool), so the shell half is a strict line subset that fails closed.
- **Accept:**
  - [ ] each finding above has its negative case, which fails before the fix and passes after
  - [ ] `just check` and `just ci-parity` are green on the tree
  - [ ] fnd §3 states the allowlist principle; fnd §3.3 has the `ARCH-gate-config` row and the meta-test covers it
- **Impact:** none (tooling); the gate's configuration is pinned.

#### FND-09: Close the gate gaps left after FND-08
- **Depends on:** FND-08. **Model:** deep. **Size:** M. **Parallel with:** FND-06, BENCH-03.
- **Cites:** fnd §3, §3.3, §3.6, §7.3, §7.4, §8; ADR 0030; the re-verification of FND-08 (the
  FND-05 checkpoint's adversarial review, findings F1 to F4).
- Each fix is an allowlist or a pinned shape (fnd §3) with a negative case that reproduces the
  reviewer's bypass:
  - **F1** (MEDIUM): the linters of the `workflows` stage read no implicit configuration:
    shellcheck runs with `--norc`, actionlint with `-config-file /dev/null`, zizmor with the
    pinned `.github/zizmor.yml`; the entries allowed at the repository root (its dotfiles and
    the `mise*` names), under `.github/` and under `.config/` are allowlists, and no
    `rustfmt.toml` may exist (`scripts/tool-config-check.sh` before any tool, `ARCH-gate-config`
    after).
  - **F2** (MEDIUM): the `secrets` stage also scans every blob reachable from any ref as a file
    (`gitleaks dir`), so a NUL byte, a `-diff` attribute or a merge resolution hides nothing;
    every `.gitattributes` in the history holds only allowlisted attributes; the policy's
    global self-allowlist is gone.
  - **F3** (MEDIUM): no mise runs on a tree before its configuration passes a non-mise shape
    check (`mise.toml` only `[tools]` with allowlisted backends and plain versions, `mise.lock`
    only checksummed GitHub release assets, no other mise file at the root): the justfile's
    top-level lines, `_preflight`, `land.sh` (with the landing checkout's own copy of the check,
    and no longer trusting the merged tree), `scripts/bootstrap-tools.sh` (which no longer runs
    `mise trust`) and the Stop hook. The reviewer's payload (`[env] _.source` and an `exec`
    template, with the tree trusted) runs nothing.
  - **F4** (LOW): `.config/gate.sha256` also pins `scripts/test-*.sh` (by glob), the new check,
    `.github/zizmor.yml`, `deny.toml`, `mise.toml` and `mise.lock`; fnd §3.6 says why
    `architecture.toml`, the `clippy.toml` files and the checker's own sources are not pinned.
  - The review's G1 to G6: blobs are scanned as text and the pass fails on any file gitleaks
    skips as binary or any archive (G1); `land.sh` checks each cherry-picked tree, cargo and
    toolchain configuration included, before any cargo or mise runs, and fnd §3.6 says the gate
    is not a sandbox (G2); inline shellcheck and zizmor suppressions are an allowlist (G3); no
    `.gitleaks.toml` may exist but the root one, which needs no allowlist of its own (G4); `land.sh`,
    `dco-check.sh` and the two hooks are pinned (G5); commit and tag messages are scanned (G6).
  - The secret-scanning alert: the generated policy holds no secret-shaped literal (every text
    its own rules match is rewritten into the same regex with one character in a class), scans
    clean with itself and has no self-allowlist; no token-shaped text is committed whole.
- **Reuse considered:** gitleaks' own `dir` mode over the history's blobs rather than a second
  scanner; a policy that matches nothing of itself rather than any allowlist of its path; the
  linters' own flags (`--norc`, `-config-file`) rather than wrappers; mise's untrusted-config
  behaviour (it reads `[tools]`, refuses `[env]`) rather than trusting the tree.
- **Accept:**
  - [ ] each finding above has its negative case, which fails before the fix and passes after
  - [ ] the reviewer's mise payload runs nothing through `just`, `land.sh` or the bootstrap
  - [ ] `just check` and `just ci-parity` are green on the tree
- **Impact:** none (tooling); the gate reads no implicit tool configuration.

#### FND-06: Coverage, changed lines, mutation in the diff
- **Depends on:** FND-05. **Model:** standard. **Size:** M. **Parallel with:** EVAL-03, EVAL-10, BENCH-03.
- **Cites:** ver §2, §9; fnd §8, §9; ADR 0030.
- cargo-llvm-cov, diff-cover and cargo-mutants pinned by mise. Recipes: `coverage`
  (`cargo llvm-cov nextest` with LCOV and JSON output; `scripts/coverage-floors.sh`, about 100
  lines of jq, compares each crate with `coverage-floor.toml`, floors set to the measured values
  rounded down and only ever raised), `changed-lines` (diff-cover against
  `git merge-base HEAD origin/main` plus the working tree, `--fail-under` 90), `mutants-diff`
  (`cargo mutants --in-diff` over the same diff with a 20-minute cap, outside `check` (fnd §9); a
  missed mutant in `jfl-domain`, `jfl-protocol` or `jfl-application` fails, elsewhere it is
  reported; a run cut by the cap reports what it did not reach and is not green); the `coverage`
  CI job (all three) in `Required`; the orchestrate skill already runs `mutants-diff` before
  landing a task sized `L` once the recipe exists.
- **Reuse considered:** diff-cover replaces the changed-line half of mayfly's `coverage.rs` (about
  900 lines); Codecov's patch status is an external service; cargo-llvm-cov has one global floor
  per run, so per-crate floors are a thin script.
- **Accept:**
  - [ ] the coverage job is green and in `Required`
  - [ ] an untested changed line fails `changed-lines` on a fixture; a planted surviving mutant in the diff fails `mutants-diff`; the base is the merge-base for an uncommitted tree, a commit and a cherry-pick alike (a test over a scratch repository)
  - [ ] `coverage-floor.toml` holds the measured floors, each with the date it was measured
- **Impact:** none (tooling).

#### DEC-07: (owner) Confirm the Proposed defaults
- **Depends on:** FND-05. **Model:** light. **Size:** S. **Parallel with:** DEC-08.
- **Cites:** [`adr/README.md`](adr/README.md); plan §14, §16; ADR 0009, ADR 0019.
- Only ADRs 0001–0004 and 0032 are the owner's decisions; every other record is an orchestrator or planner
  default. In the FND-05 checkpoint report an agent lists each Proposed record (0005–0010,
  0013–0031) in one line with its evidence link, and the two proposed amendments (ADR 0001: the
  `adrs` tool, and optionally MADR 4; ADR 0002: `scripts/land.sh`). The owner accepts, overrides
  or rejects each.
- **Accept:**
  - [x] the list is in the checkpoint report
  - [x] (owner) each record decided; statuses read `Accepted (owner, date)` or `Rejected (owner, date)`; the ADR index and the Decisions table updated
- **Decided:** 2026-11-17, recorded by BOOT-02. Accepted as written: 0006, 0008–0010, 0014–0021,
  0024–0026, 0029, 0031, and the amendments of 0001 and 0002. Accepted with the owner's
  amendment: 0007 (Quint models and `proptest-state-machine` tests, both), 0022 (orchard
  pinned exactly, the switch-back boundary built from the start), 0023 (`ristretto_jimage`, Rust
  1.99.1: FND-10), 0028 (jdtls for testing and comparison only). Superseded: 0005 by 0033 (MIT
  only), 0013 by 0034 (LSP first, CLI second), 0027 by 0036 (our own javac worker), 0030 by 0035
  (mayfly's `xtask`, lane XT). ADR 0001 is amended with what deserves an ADR at all; 0010 and 0018
  were more than needed and no more of that kind are written. Agents are denied `git push`
  (ADR 0018, amended).
- **Impact:** none (decision).

#### DEC-08: (owner) Shared skills plugin or repo-local skills
- **Depends on:** FND-05. **Model:** light. **Size:** S. **Parallel with:** DEC-07.
- **Cites:** plan §16.4; ADR 0018.
- An agent writes `doc/skills-plugin.md`: what a personal marketplace (`pzmi/claude-plugins`)
  with one `sdlc` plugin would hold (`work`, `orchestrate`, `review`, parameterised by each
  repository's `CLAUDE.md` and `doc/`), how a repository adds it (`claude plugin marketplace add
  … --scope project`, pinned by `ref` or `sha`), what stays repository-local, `claude plugin
  eval` cases for the behaviours that matter, the migration steps and their cost. The owner
  decides.
- **Reuse considered:** `superpowers` and the official `feature-dev` and `pr-review-toolkit`
  plugins (generic; they encode none of this repository's rules: DCO, `land`, ⛳, the dependency
  law, Quint conformance).
- **Accept:**
  - [ ] `doc/skills-plugin.md` committed (not needed: the owner decided without it)
  - [x] (owner) decided; if "shared", a follow-up task is added here (repo-local: none)
- **Decided:** 2026-11-17: the skills stay repo-local in `.claude/skills/` for now; the shared
  `pzmi/claude-plugins` plugin is deferred. No ADR (ADR 0001, amended): this line is the record.
- **Impact:** none (decision).

#### FND-10: Toolchain 1.99.1
- **Depends on:** FND-08. **Model:** standard. **Size:** S. **Parallel with:** XT-01.
- **Cites:** fnd §3.6, §4.1, §7.2, §8; ADR 0023 (amended 2026-11-17), ADR 0035.
- Move Rust from 1.97.0 to 1.99.1, which `ristretto_jimage` 0.35.0 needs (ADR 0023, amended):
  `rust-toolchain.toml`; `[workspace.package] rust-version = "1.99"`; the pin recorded in ADR 0035 (which carries ADR
  0011's toolchain pin; 0011 itself is superseded and stays as written) by a dated amendment, and
  `rust-toolchain.toml`'s comment citing ADR 0035 instead of 0030 and 0011; CI (`ci.yml`'s toolchain and the
  `Swatinem/rust-cache` key); the gate's pins (`.config/gate.sha256` and `rust-toolchain.toml`'s
  allowlisted shape); mise (any pinned tool built against the toolchain, and its lock entries);
  fnd §4.1 and the docs that name 1.97 as current (landed cards stay as history). New warnings
  from clippy 1.98 and 1.99 are fixed in the code, never allowed to get green. `ristretto_jimage`
  itself arrives with PROJ-03, not here.
- **Reuse considered:** none needed: a pin change.
- **Accept:**
  - [ ] `rustc --version` inside the repository reports 1.99.1; `just check` and `just ci-parity` are green
  - [ ] every `ci.yml` job is green on the new toolchain, the native macOS and Linux arm64 jobs included
  - [ ] `grep -rn '1\.97'` finds only history (landed cards, superseded ADRs, the E1 spike)
- **Impact:** toolchain.

#### FND-11: Remove the private-names hashes and check
- **Depends on:** FND-04, XT-02. **Model:** standard. **Size:** S. **Parallel with:** the EVAL lane.
- **Cites:** fnd §7.4, §9, §10.2; ADR 0004 (amended 2026-11-17), ADR 0035; [`required-checks.md`](required-checks.md).
- The owner's order of 2026-11-17: the salted hashes of `.private-names.sha256` are reversed by
  guessing short names in seconds, so the list confirmed what it meant to hide. Removed whole:
  the hash file, `scripts/private-names-check.sh`, `scripts/private-name-add.sh`,
  `scripts/private-names-fold.pl` and `scripts/test-private-names.sh`; the `private-names` stage
  from xtask's catalog (and the frozen catalog test), its recipe, the justfile's `check_stages`
  (read only by `land.sh`'s hook), `ci.yml`'s security step and its `required-checks.md` row;
  `land.sh`'s hook and its three `land_scripts.rs` cases; the pins and `GATE_FILES` entries, and
  the hash file's place in the root allowlists of `tool-config` and `ARCH-gate-config`. The rule
  of ADR 0004 stays; review enforces it (ADR 0004, amended). Removing a gate stage is allowed
  here only because the owner ordered it; every other negative test stays.
- **Reuse considered:** none needed: a removal.
- **Accept:**
  - [x] no file, recipe, stage, CI step, pin or doc of the mechanism is left (`git grep -i private-name` finds only history: landed cards, the ADR amendment, superseded ADR 0030)
  - [x] `just check` and `just ci-parity` are green; `ARCH-ci-lockstep` and the frozen catalog test agree on the stages without `private-names`
- **Impact:** gate (one stage removed, on the owner's order); CI (the security job's step).

#### FND-12: Delete the jdtls proxy; keep stock jdtls as a bench arm
- **Depends on:** FND-11, BENCH-03. **Model:** standard. **Size:** M. **Parallel with:** the EVAL lane.
- **Cites:** plan §1.3, §6.1, §9.2, §16; fnd §2.1, §2.4, §2.7; ADR 0010, ADR 0028 (amended 2026-11-17), ADR 0031.
- The owner's order of 2026-11-17: jdtls is for testing and comparison only and is never
  included, so the multiplexing proxy built for it (`tools/jfl-jdtls`, EVAL-11: pooling,
  per-worktree workspaces, a daemon and its lifecycle), which only made sense for shipping it,
  is deleted with its tests, its workspace member, its `architecture.toml` row and grants (the
  only tool crate that held `socket`, `spawn` and `sleep`), its `deny.toml` and `clippy.toml`
  entries, the `jdtls-it` recipe and `doc/eval/jfl-jdtls.md`. `jfl-bench`'s jdtls arm keeps
  measuring the same thing with a small launcher of its own (`src/jdtls.rs`): the pinned stock
  jdtls, refused unless `scripts/jdtls-fetch.sh` verified it (the install's `.verified` holds the
  pinned SHA-256), a fresh `-data` and configuration per run, the heap cap, argv only with a
  cleared environment, under `timeout(1)`'s deadline, over the existing LSP client (which now
  answers `workspace/configuration` itself). The JVM's pid comes from `/proc` (the `timeout`'s
  child), so `jcmd GC.heap_info` and the RSS are kept. `--trusted-build` still gates a corpus
  run. `scripts/jdtls-fetch.sh` stays (minimal, pinned checksum), installing beside kmp-lsp.
- **Reuse considered:** jdtls's own `bin/jdtls` launcher (Python, and it keys `-data` on the
  working directory unless told otherwise; the arm needs the heap cap, the deadline and the
  cleared environment, so it builds the same JVM command line itself).
- **Accept:**
  - [x] `tools/jfl-jdtls` and every reference to it outside history are gone; `architecture.toml`, `deny.toml`, the clippy lists and `Cargo.lock` agree
  - [x] `jfl-bench run jdtls fixture` runs stock jdtls with a fresh `-data` and reports the same metric names, `backend_heap_used` included (sandbox run, 2026-11-17)
  - [x] `runner::tests::should_refuse_a_corpus_checkout_when_the_jdtls_arm_is_not_trusted` passes; an unverified install is refused (`jdtls::tests`)
  - [x] `just check` and `just ci-parity` are green
- **Impact:** dependency graph (a tool crate removed); bench (the jdtls arm's launcher).

### Lane XT: port mayfly's `xtask`, piece by piece

ADR 0035 (the owner, 2026-11-17) supersedes ADR 0030: `tools/xtask` with a `cargo xtask` surface
replaces the justfile, mise and the shell scripts **one piece per card**. External tools stay and
xtask runs them: cargo-deny, gitleaks, zizmor, actionlint, shellcheck, hyperfine, cargo-nextest
and Quint, and the others the gate uses today until a card decides. Tool versions go in
`tools/versions.toml` with `cargo xtask doctor` (ADR 0011). Rules for every XT card:

- **Nothing stops running.** Each card lists the stages, scripts and tests it moves; every
  negative test, every `scripts/test-*.sh` case and FND-09's negative tests for what it moves
  are its acceptance criteria, ported as Rust tests that fail before the fix they pin. A stage is
  never dropped and re-added later.
- **The gate stays pinned** (fnd §3.6): when a script's logic moves into xtask, its line in
  `.config/gate.sha256` moves to the xtask source that replaces it, in the same commit.
- **Cite the mayfly module** each piece starts from, and say what was adapted. Reuse considered:
  mayfly's `tools/xtask` (itself ported from site-preview and hardened by review rounds).
- `.claude/` (hooks, settings) belongs to the orchestrator: a card that changes what a hook or
  the allow list must call reports the change rather than making it.
- Later, not scheduled: the parts shared with mayfly and site-preview move into a separate
  project that the three repositories depend on.

#### XT-01: Scaffold `tools/xtask` and port the gate composition
- **Depends on:** FND-08, FND-09. **Model:** deep. **Size:** M. **Parallel with:** FND-10, the EVAL lane.
- **Cites:** fnd §2.4, §3.3, §3.6, §9; ADR 0035; mayfly `tools/xtask` (`cli`, `process`, `repo` preflight, `catalog`, `check`, `quick`).
- `tools/xtask` as a tool-only crate (`members`, its `architecture.toml` row and grants, `deny.toml`
  and `clippy.toml` entries in the same commit). One `.cargo/config.toml` holding exactly
  `[alias] xtask = "run --locked -p xtask --"`, admitted by FND-08's N1 rule as a pinned shape (the
  only cargo configuration file allowed; any other key or file still fails). `cargo xtask check`,
  `quick`, `check-only <stage>…` and `list [--json]`: the stage catalog, its order and the
  `_preflight` refusals in Rust, each stage still running the same tool command its `just` recipe
  runs today. `just check`, `quick` and `check-only` become one-line delegations to `cargo xtask`,
  so CI, `land.sh` and the Stop hook keep working unchanged. `ARCH-gate-config` pins the xtask
  composition sources; `ARCH-ci-lockstep` reads the catalog from `cargo xtask list --json`. The
  gate's shape checks move in with the composition, FND-09's `scripts/tool-config-check.sh` (and
  its self-test `scripts/test-tool-config.sh`) among them.
- **Accept:**
  - [ ] `cargo xtask list --json` names exactly the stages `just check` ran before this card, in the same order (a test against the catalog frozen in this commit)
  - [ ] every `_preflight` negative case (`RUSTC_WRAPPER`, `RUSTFLAGS` beyond `-D warnings`, `CLIPPY_CONF_DIR`, `RUSTC_BOOTSTRAP`, `CARGO_UNSTABLE_*`, a cargo configuration file other than the alias, N1's unicode-escaped forms) and FND-09's negative tests pass against `cargo xtask`
  - [ ] an edited xtask composition source without its `.config/gate.sha256` line fails `ARCH-gate-config`
  - [ ] FND-09's tool-configuration shape check (`scripts/tool-config-check.sh`) runs as an xtask stage; every negative case of `scripts/test-tool-config.sh` is a Rust test that fails before the check and passes after, and both scripts are removed with their pins moved
  - [ ] `cargo xtask check` and `just check` are green; `ci.yml` is unchanged and green
- **Deviation (orchestrator, 2026-11-17): alias deferred: needs the G2 rule to admit one pinned
  shape, landed by hand per sdlc step 5.** FND-09's G2 rule refuses any cargo configuration file
  at any depth (cargo configuration is a code-execution vector), and `land.sh` runs main's own
  copy of that check, so a commit adding `.cargo/config.toml` cannot land through it. XT-01 adds
  none: the gate recipes delegate to `bash scripts/xtask.sh <args>`, which builds xtask from
  outside the tree (where the tree's cargo configuration does not apply, a `runner` included) and
  runs it; `cargo run --locked -q -p xtask -- <args>` stands for `cargo xtask` meanwhile. The
  preflight case "a cargo configuration file other than the alias" is today "any cargo
  configuration file". The alias itself is a follow-up on XT-08's card.
- **Impact:** none (tooling); dependency graph (a new tool crate).

#### XT-02: Tool pins: `tools/versions.toml`, `cargo xtask doctor`, `install-tools`
- **Depends on:** XT-01. **Model:** standard. **Size:** M. **Parallel with:** the EVAL lane.
- **Cites:** fnd §8; ADR 0011, ADR 0035; mayfly `tools/xtask` (`tools`: doctor, install-tools, versions).
- `tools/versions.toml`: every tool `mise.toml` pins today, with its version and, per platform,
  the release URL and SHA-256 (the values `mise.lock` holds); `cargo xtask install-tools` downloads
  into a cache directory outside the repository and refuses a checksum mismatch; `cargo xtask
  doctor` fails closed on a missing or wrong tool, naming it and the fix; the stages run tools from
  that directory. mise stays installed and in CI until XT-08; a test fails when `versions.toml`
  and `mise.lock` disagree.
- **Accept:**
  - [ ] `doctor` names a missing tool and a wrong version (fixtures); `install-tools` refuses a tampered archive
  - [ ] FND-08's N6 cases (no other mise configuration, tools run from their install directories) hold for both paths
  - [ ] the network facts of fnd §8 hold: every download is a GitHub release asset or a crates.io file, never `api.github.com`
  - [ ] the XT-01 review's LOWs, carried here:
    - **PATH:** `scripts/xtask.sh` builds xtask with the pinned toolchain's own `cargo` (and finds rustup from the pinned install, not from the caller's PATH), so a `cargo` or `rustup` earlier on PATH cannot build or replace it; a planted one fails a test
    - **a shared target directory:** the landing check builds xtask into a target directory the gate never shares, or verifies the binary it runs against cargo's own `deps/xtask-<hash>` artifact, so code a gated tree runs cannot leave a binary the next land's trusted `check_tree` executes; a planted binary fails a test
- **Impact:** none (tooling).

#### XT-03: Security and workflow stages into xtask
- **Depends on:** XT-02. **Model:** standard. **Size:** M. **Parallel with:** the EVAL lane.
- **Cites:** fnd §7.3, §7.4; ADR 0004, ADR 0035; mayfly `tools/xtask` (`security`, `action_pins`).
- `secrets`, the generated `.gitleaks.toml` check and `workflows` become xtask stages and
  subcommands that run gitleaks, actionlint, shellcheck, zizmor and pinact;
  `scripts/secrets-check.sh`, `scripts/gitleaks-config.sh` and `scripts/workflows-check.sh` are
  removed with their pins moved. `SECURITY.md` names the new command. (`private-names` is not
  ported: FND-11 removed it on the owner's order.)
- **Accept:**
  - [ ] every case of `scripts/test-secrets.sh` and `scripts/test-workflows.sh` is a Rust test and passes; FND-04's fixtures and FND-08's N5 case still fail their stages
  - [ ] FND-09's negative tests for these stages pass against xtask
  - [ ] the XT-02 review's findings, carried here (these stages are the first to run more tools from xtask's cache):
    - **Z1:** a tool on the stages' PATH is the pinned one: either the preflight refuses a non-default `MISE_DATA_DIR`, `XDG_DATA_HOME` or `XDG_CACHE_HOME`, or xtask verifies each cached tool's SHA-256 against `tools/versions.toml` before it puts the cache on the stages' PATH; a planted fake `cargo-nextest` in the cache (which made three stages pass in the review) fails a test
    - **Z2:** the `land-xtask` cache (`$XDG_CACHE_HOME/jfastlsp/land-xtask`) stays writable by code the gate runs as the same user: XT-02 keeps it apart from the gate's target directory, which does not close that hole, and this card does not claim it closed (XT-05, which moves landing into xtask, takes it up)
    - **Z3:** `install-tools` re-verifies an existing install by its digest instead of trusting that it is there; a tampered installed binary is reinstalled or refused (test)
  - [ ] the LOWs of FND-09's adversarial review, carried into the port:
    - **M1:** the suppression allowlist has one source: one data file that both the workflows stage and `ARCH-workflow-yaml` read, or a single `allowed_suppressions` definition whose opener is enforced to occur exactly once; a decoy list fails (test)
    - **L1:** the gitleaks policy generator's regex-rewrite structural check has committed tests (the implementer's probes plus a rewrite inside `\Q…\E`, inside `(?…)` and inside `\p`/`\x` escape arguments), and the `\Q…\E` refusal is kept
    - **L2:** the binary-skip detection reads gitleaks' structured output, or compares the files scanned with the files written, never console text; a planted skip fails (test)
    - **L3:** a heredoc or string line containing `# shellcheck ` is still refused (it fails closed today), and the message tells the author to reword it; a test pins both
    - composite actions (`.github/actions/*/action.yml`), whose `run:` scripts actionlint does not shellcheck, have their decoded `run:` scripts shellchecked by xtask; a planted SC2086 in one fails
    - **H3:** the docs and the stage's output say that secrets in encodings other than the listed archive and compression formats (zlib, lz4, brotli and the like) are out of scope
- **Impact:** none (tooling).

#### XT-04: Clippy lists, ADR index and DCO into xtask
- **Depends on:** XT-03. **Model:** standard. **Size:** S. **Parallel with:** the EVAL lane.
- **Cites:** fnd §2.4, §10.3; ADR 0001, ADR 0035; mayfly `tools/xtask` (`adr`).
- `cargo xtask clippy-lists`, `adr new <title>`, `adr index [sort]` and `dco <range>` replace
  `scripts/clippy-lists.sh`, `adrs`, `scripts/adr-index.sh` and `scripts/dco-check.sh`; the `docs`
  stage and `ci.yml`'s `dco` job call them.
- **Accept:**
  - [ ] every case of `scripts/test-clippy-lists.sh` and the `adr_index` and DCO cases of `land_scripts.rs` pass against xtask
  - [ ] `cargo xtask adr new "Test"` in a scratch copy writes the next Nygard record with the index row
- **Impact:** none (tooling).

#### XT-05: `cargo xtask land` replaces `scripts/land.sh`
- **Depends on:** XT-04. **Model:** deep. **Size:** M. **Parallel with:** the EVAL lane.
- **Cites:** fnd §10.2; ADR 0002, ADR 0018 (amended 2026-11-17), ADR 0035; mayfly `tools/xtask` (`land`).
- `cargo xtask land <sha>… [--no-push]` does what `land.sh` does today: refuse a dirty checkout,
  fetch, a worktree at `origin/main`, cherry-pick, sign-off and signature checks, resolve only
  `Cargo.lock` and the ADR index, the gate on the merged tree, push only on green, one retry after
  a non-fast-forward. It stays the only pusher: the report asks the orchestrator to allow
  `cargo xtask land` in `.claude/settings.json`, where the `git push` deny stays.
- **Accept:**
  - [ ] every `land_scripts.rs` case (clean land, dirty checkout, non-lockfile conflict naming the files, a red gate moves nothing, the retry, the signature and sign-off refusals) passes against `cargo xtask land`
  - [ ] `scripts/land.sh` removed; `sdlc.md` and fnd §10.2 name the new command
- **Impact:** none (tooling).

#### XT-06: The architecture checks move into `cargo xtask architecture`
- **Depends on:** XT-05. **Model:** deep. **Size:** M. **Parallel with:** the EVAL lane.
- **Cites:** fnd §3.1–§3.6; ADR 0010, ADR 0035; mayfly `tools/xtask` (the architecture checker's policy, inventory, graph and external layers).
- `jfl-architecture`'s rules, its negative fixture workspaces, its meta-test (every rule of fnd
  §3.3 has a negative case), the clippy-list model, the source scan and lexer, the lockstep and
  `ARCH-gate-config` move into xtask as `cargo xtask architecture`; `tools/jfl-architecture` is
  removed; fnd §3 names the new home.
- **Accept:**
  - [ ] every rule id keeps its negative case and fails it with its id and path; the meta-test passes
  - [ ] the `architecture` stage runs `cargo xtask architecture`; the tree is green
- **Impact:** none (tooling).

#### XT-07: Bench and fetch scripts into xtask
- **Depends on:** XT-06. **Model:** standard. **Size:** S. **Parallel with:** the EVAL lane.
- **Cites:** fnd §9; ADR 0031, ADR 0035.
- `scripts/bench-corpus.sh` and `scripts/jdtls-fetch.sh` (the bench's jdtls arm's pinned fetch)
  become `cargo xtask bench-corpus` and `cargo xtask jdtls-fetch`; hyperfine stays the timing tool `jfl-bench` drives.
- **Accept:**
  - [ ] every case of `scripts/test-bench-corpus.sh` and `scripts/test-jdtls-fetch.sh` passes against xtask
  - [ ] `doc/bench/README.md` and `doc/bench-corpus.md` name the new commands
- **Impact:** none (tooling).

#### XT-08: Remove just and mise
- **Depends on:** XT-07. **Model:** standard. **Size:** M. **Parallel with:** the EVAL lane.
- **Cites:** fnd §7.2, §8, §9; ADR 0018, ADR 0035; [`required-checks.md`](required-checks.md).
- The `cargo xtask` alias deferred from XT-01: alias deferred: needs the G2 rule to admit one
  pinned shape, landed by hand per sdlc step 5 (one `.cargo/config.toml` holding exactly
  `[alias] xtask = "run --locked -p xtask --"`, admitted by `tool-config`, the preflight and
  `ARCH-gate-config` as a pinned shape; the launcher `scripts/xtask.sh` stays the way the gate
  builds xtask, since `cargo run` under the tree's configuration would honour a `runner`). Until
  then the commands below read `bash scripts/xtask.sh`.
- Every `ci.yml` job calls `cargo xtask check-only <stage>…` and installs tools with
  `cargo xtask install-tools` (cached), not `jdx/mise-action`; the `justfile`, `mise.toml`,
  `mise.lock` and `scripts/bootstrap-tools.sh` are removed; `ARCH-ci-lockstep` and
  `ARCH-gate-config` read only xtask. `CLAUDE.md`, `README.md`, `CONTRIBUTING.md`, fnd §8 and §9,
  `sdlc.md` and `required-checks.md` name `cargo xtask`; the report lists the hook and allow-list
  changes the orchestrator makes in `.claude/` (SessionStart, Stop, `Bash(just *)`, `Bash(mise *)`).
- **Quint without mise (added by FORMAL-01, the review's W2):** `install-tools` installs Quint
  (`kind = "npm"`) itself, from the locked graph in `.mise/locks/` (or its successor), with
  lifecycle scripts disabled (`protobufjs` declares a `postinstall`), and a test shows Quint's
  `typecheck`, `test` and `run` work without them; each job installs only the tools it runs, the
  JDK and Apalache only in `formal.yml`, as `install_args` does now.
- **Accept:**
  - [ ] every stage of the catalog frozen in XT-01 still runs in `cargo xtask check` and in CI (the catalog test passes)
  - [ ] no workflow, script or doc command table names `just` or `mise`
  - [ ] `cargo xtask check` and every CI job green; `Required` green
- **Impact:** CI topology; tooling.

### Lane BENCH: benchmarks from the start

Speed is the product's primary value (plan §1.1), so measurement starts with the workspace, on
real public Gradle Java repositories pinned at fixed commits, beside the rest of FND and phase 0
(ADR 0031). From M1 on, a tracked metric that regresses by more than 10 % fails the
`bench-gate` job of `bench.yml`, run on one fixed runner class (fnd §9). No Gradle build and no
jdtls import of a corpus repository runs in the cloud sandbox: those runs are `bench.yml`
dispatch jobs or `(owner)` items.

#### BENCH-01: Benchmark corpus: pinned real repositories and the fetch script
- **Depends on:** FND-01. **Model:** standard. **Size:** S. **Parallel with:** FND-02.
- **Cites:** plan §1.7, §8; ver §4, §10; ADR 0004, ADR 0031.
- `doc/bench-corpus.md` (written in BOOT-01 from the E1 spike) lists the corpus: small
  junit-framework, medium spring-framework with kafka as the huge-file variant, large
  elasticsearch, and gradle/gradle as the permissive large fallback, each at a fixed commit.
  `just bench-corpus [size]` (`scripts/bench-corpus.sh`): fetch each repository at its commit
  (shallow) into a cache directory outside the repository (`$XDG_CACHE_HOME/jfastlsp/bench-corpus`
  by default, a recipe parameter to override), verify the commit and the `.java` file count, a
  no-op when cached; and a helper that adds N linked worktrees under the checkout's
  `.claude/worktrees/` for multi-worktree runs. Nothing from the corpus is committed;
  elasticsearch (SSPL, Elastic License 2.0 or AGPL-3.0) is cloned at bench time only and no
  fixture, snippet or position file is ever derived from it.
- **Reuse considered:** `git fetch --depth 1 <sha>`; Multi-SWE-bench images (built for
  evaluation, not benchmarks); git-annex or DVC (nothing to version but pins).
- **Accept:**
  - [x] `just bench-corpus small` fetches and verifies into an empty cache, and is a no-op on the second run; a wrong commit fails verification
  - [x] the file counts match `doc/bench-corpus.md`; `git status` is clean after a fetch
  - [x] the helper creates 10 linked worktrees of the small repository
- **Impact:** none (tooling).

#### BENCH-02: Bench harness: `jfl-bench`, hyperfine, divan, the `rg` floor
- **Depends on:** BENCH-01, FND-02. **Model:** standard. **Size:** M. **Parallel with:** FND-03.
- **Cites:** plan §1.7; ver §10; fnd §2.7, §8, §9; ADR 0031.
- `tools/jfl-bench` (tool only, independent; grants per fnd §2.4), seeded from the archived E1
  spike in `spikes/e1-parse/` (its `parse-spike/` harness: an `ignore` walk, a rayon pool with one
  tree-sitter parser per thread, read, blake3, parse and declaration-extract stages timed
  separately, cold runs after dropping the page cache, warm runs as the median of three; its
  `bench.sh`, `floors.sh` and `summarize.py` drivers), rewritten under the workspace's rules.
  Only what has a subject now: suites as TOML (corpus size, operation, query positions picked by
  seed from the corpus); a hyperfine runner (hyperfine and ripgrep pinned by mise) for commands
  end to end; RSS sampling (peak and steady); the `rg-floor` suite (`rg` answering name queries
  over the corpus) and the `e1` parse suite; JSON results with machine facts (CPU, RAM, OS, disk,
  toolchain, tool versions, commit) in the format github-action-benchmark reads
  (`customSmallerIsBetter`), each metric the median of N runs; `just bench [suite] [corpus]`,
  which reports and never gates; the in-process convention for product crates
  (`crates/*/benches/` with divan, criterion where a statistical comparison matters) written in
  `doc/bench/README.md`. Freshness lag and the 1/3/10-worktree marginal cost need a product:
  they arrive with LSP-06 (M1) for `jfl` and with E4 (EVAL-06) for jdtls.
- **Reuse considered:** hyperfine, divan, criterion, github-action-benchmark's JSON; a bespoke
  timer was rejected.
- **Accept:**
  - [x] `just bench rg-floor small` writes JSON with machine facts and p50 and p95 per query
  - [x] `just bench e1 small` reproduces the spike's junit-framework row of `doc/bench-corpus.md` within the noise band the report states (same machine class), or the commit body says why not
  - [x] two runs on the same machine agree within that noise band
  - [x] the JSON is accepted by the history tool's format (a test against its schema or examples)
  - [x] `jfl-bench` imports no production crate
- **Impact:** none (tooling).

#### BENCH-03: Comparison arms and the first baseline
- **Depends on:** BENCH-02, EVAL-11. **Model:** standard. **Size:** M. **Parallel with:** FND-06.
- **Cites:** plan §1.3, §1.7, §9.2; ver §10; ADR 0028, ADR 0031.
- Arms in `jfl-bench`: jdtls behind our proxy (`jfl-jdtls`, EVAL-11; since FND-12 stock jdtls, started by the arm itself) driven over LSP
  (definition, references, workspace symbol, hover, `initialize`), kmp-lsp at a pinned commit, and
  the `rg` floor. In the sandbox the jdtls arm runs only on the synthetic multi-module fixture with
  `java.import.gradle.enabled=false` and `java.import.maven.enabled=false` (a jdtls import runs the
  project's build); kmp-lsp and `rg` run on the small and medium corpus where they need no build.
  The jdtls arm on the corpus is a `bench.yml` dispatch job (BENCH-04 adds it) or an `(owner)`
  run. `doc/bench/baseline.md`: the numbers with machine facts and, per §1.7 target, a
  recommendation to confirm or correct it.
- **The incremental suite** (the owner asked whether incremental indexing is feasible; speed is
  the point of the project), on medium (spring-framework) and lombok (fineract):
  - the time from one disk edit until a published generation answers;
  - a branch switch and a rebase between pinned commits;
  - a change to a widely used type: how many files re-resolve (the fan-out) and the time;
  - the cost of updating the references postings;
  - a warm reopen.

  Until IDX exists the suite measures the syntax-tier proxies (re-hash, `git diff` between the
  pinned commits, re-parse of the changed set) and the report says so per row; each full metric
  turns on with the task that makes it measurable (IDX-09 freshness and generations, IDX-11
  postings, SEM-01/SEM-02 re-resolution fan-out) and those tasks run it. The results feed DEC-02,
  against the incremental rows of plan §1.7 (*proposed*).
- **Reuse considered:** kmp-lsp as shipped; jdtls's own sharing options (`sharedIndexLocation`,
  the syntax server).
- **Accept:**
  - [x] the baseline report committed; an arm that cannot run is recorded with the reason, never dropped
  - [x] the incremental suite runs on medium and lombok, each row marked "proxy" or "full", with the pinned commit pairs (branch switch, rebase) and the widely used types chosen named in `doc/bench/baseline.md`; its proxy numbers go into DEC-02's inputs
  - [ ] (owner) the jdtls arm on the medium corpus (or BENCH-04's dispatch job) and the large corpus run on the reference machine, named in the report
- **Impact:** none.

#### BENCH-04: Results history and the regression gate on one runner class
- **Depends on:** BENCH-03, FND-05. **Model:** standard. **Size:** S. **Parallel with:** FORMAL-01.
- **Cites:** plan §1.7; ver §10; fnd §7.1, §9; ADR 0031.
- `bench.yml` (push to `main`, dispatch) on **one fixed runner class** (named in the workflow and
  the results; a change of class starts a new series): the small-corpus suites (medium where the
  runner holds it), each metric the median of N runs written by `jfl-bench`, results stored per
  runner class in the `gh-pages` branch by github-action-benchmark (current release checked,
  pinned by SHA; `alert-threshold: 110%`; the job alone gets `contents: write`, with the reason
  beside it). The `bench-gate` recipe compares a run with that class's history and, on an alert,
  reruns the suite once and fails only if the regression reproduces; in-process suites gate on
  instruction counts (`gungraun` or cachegrind, pinned) where available, with wall time
  reported. `bench-gate` is never a stage of `check` and never runs in `land.sh` (fnd §9); it
  reports here and becomes blocking at LSP-06 (the M1 gate). Dispatch jobs for corpus work the sandbox must not
  do: the jdtls arm of BENCH-03 on the medium corpus (with its Gradle import), and a hook for
  BENCH-05's `scip-java` indexes. A short guide to reading the history in `doc/bench/README.md`.
- **Reuse considered:** github-action-benchmark (no account or token) over bencher.dev (the
  runner-up: statistical thresholds, but a SaaS and a token); own history storage rejected.
- **Accept:**
  - [ ] a dispatch run stores its results in the history
  - [ ] a planted 20 % slowdown in a test suite raises the alert and fails `bench-gate` after the rerun; a one-off slow run that does not reproduce passes
  - [ ] `just check` and `land.sh` do not run `bench-gate` (a test over the `justfile` dump)
  - [ ] zizmor and actionlint clean on `bench.yml`
  - [ ] (owner) the `gh-pages` branch allowed for GitHub Pages, if the owner wants the charts published
- **Impact:** CI topology.

#### BENCH-05: Real-corpus golden cases
- **Depends on:** BENCH-04, EVAL-08. **Model:** deep. **Size:** M. **Parallel with:** EVAL-10.
- **Cites:** plan §5.6, §8; ver §4, §7.1; ADR 0004, ADR 0031.
- `fixtures/corpus/<repository>/`: expectation files only (repository, commit, file, position,
  construct, expected symbol or `possible` reason), picked by construct with ast-grep (pinned by
  mise) from the **Apache-2.0 repositories only** (spring-framework, fineract, kafka,
  gradle; never elasticsearch, and not junit-framework's EPL-2.0 code), with expected answers from a `scip-java`
  index of the repository at its commit. Indexes are built outside the gate and outside the
  sandbox (a `bench.yml` dispatch job this task adds, which runs the Gradle build on the runner,
  or the owner's machine) and kept as artefacts, never committed. `jfl-oracle corpus-check`
  loads the cases. Only positions and symbol strings are committed, never code.
- **Reuse considered:** `scip-java` as the oracle, ast-grep for picking sites by construct, the
  `scip` crate; copying corpus code into fixtures (licences, size) was rejected.
- **Accept:**
  - [ ] at least 300 cases over at least 8 constructs from at least 2 Apache-2.0 repositories, each validated against its SCIP index (a test)
  - [ ] the fixtures hold positions and symbols only (a schema test)
  - [ ] (owner) the `scip-java` indexes for the medium corpus, if the sandbox cannot build them
- **Impact:** none (fixtures).

#### BENCH-06: Lombok repository in the corpus and as a second evaluation target
- **Depends on:** BENCH-01. **Model:** standard. **Size:** S. **Parallel with:** BENCH-05.
- **Cites:** ADR 0032, `doc/bench-corpus.md`; plan §3, §9.1, §9.2.
- The owner's decision of 2026-11-17 (ADR 0032, amended): apache/fineract (Apache-2.0) at
  `9dfe8dfe5e162afe09f102a9aa57d00a70c770f2` becomes the `lombok` size of the corpus and a
  second E0 and E3 target beside spring-framework, which stays primary. `scripts/bench-corpus.sh`
  gains the size (7,301 `.java` files, checked at the pin) and `doc/bench-corpus.md` the row, with
  the survey of `spikes/lombok/RESULTS.md` as evidence. The cards that leaned on synthetic-only
  processor coverage (EVAL-02, EVAL-07, EVAL-10, SEM-05) name fineract where a real Lombok or
  MapStruct build helps. No Gradle run or build of fineract in the sandbox.
- **Reuse considered:** the existing fetch script (one more pin line); openrewrite/rewrite
  (mixed licence, little MapStruct) and datahub (mixed-language tree) rejected in the survey.
- **Accept:**
  - [x] `just bench-corpus lombok` fetches and verifies fineract at the pin (7,301 `.java` files); `check-doc` and `scripts/test-bench-corpus.sh` pass
  - [x] ADR 0032 carries the dated amendment, Accepted (owner, 2026-11-17), and its index row (unchanged: `adr-index` ties the title to the file's) stays `Accepted (owner, 2026-11-17)`
  - [x] EVAL-02, EVAL-07, EVAL-10, SEM-05 and DEC-01's note name fineract where it applies
- **Impact:** none (tooling and docs). `tools/jfl-bench` takes the size name as it takes the others.

### Lane FORMAL: formal models and conformance

The toolchain and the conformance harness (FORMAL-01, FORMAL-02) come right after CI, beside
phase 0; the three models (FORMAL-03 to FORMAL-05) describe a product that is built only after
the go decision, so they follow DEC-02 and still precede the tasks that implement what they model
(model first, ADR 0007): `views.qnt` before IDX-09, `daemon.qnt` before IDX-12, `diagnostics.qnt`
before LSP-04.

#### FORMAL-01: Formal toolchain: Quint, Apalache, JDK pins, `MODELS`, formal stages
- **Depends on:** FND-05. **Model:** standard. **Size:** M. **Parallel with:** EVAL-12, BENCH-04.
- **Cites:** ver §6.1, §6.3; fnd §8, §9; ADR 0007, ADR 0035 (which superseded ADR 0030).
- Node with Quint 0.33.1 (`npm:@informalsystems/quint`), Apalache 0.62.1 (checksum in
  `mise.lock`) and a Temurin JDK 21 pinned by mise (current releases re-checked); the `MODELS`
  registry as `formal/models.toml` (file, main module, steps budget), read by the recipes and by a
  test in `jfl-conformance`; recipes `formal-typecheck` (every registered `.qnt`; an unregistered
  `.qnt` fails) and `formal-test` (Quint tests; zero tests fails); `just formal-verify [model]`;
  `formal.yml` (weekly and dispatch, one job per registered model, a test keeps the matrix equal
  to the registry); the `formal` CI job; a trivial `formal/smoke.qnt`.
- **Adapted 2026-11-17 (FORMAL-01), the card predating XT-01, XT-02 and ADR 0035:**
  - **The gate runner is xtask.** `formal-typecheck` and `formal-test` are rows of xtask's stage
    catalog (`tools/xtask/src/formal.rs`), stages of `check` after `script-tests` (fnd §9's
    order), each with its one-line just delegation; `formal-verify [MODEL]` is an xtask command
    and a recipe, not a stage. The frozen catalog test and ARCH-ci-lockstep's case that used
    `formal-typecheck` as a non-stage name were updated; no stage was removed. The registry is
    read by xtask in a pinned line shape and again with a TOML parser in
    `crates/jfl-conformance/tests/models.rs`, which also checks `formal.yml`'s matrix.
  - **Pins in both places.** mise.toml and mise.lock pin `node` 26.13.1, `npm:@informalsystems/
    quint` 0.33.1 (its npm graph locked in `.mise/locks/`, digest in mise.lock), `github:apalache-
    mc/apalache` 0.62.4 and `java` `temurin-21.0.13+9.0.LTS`; `tools/versions.toml` mirrors them
    (`kind = "tree"` for the three archives, `kind = "npm"` for Quint, `prints` for the JDK) and
    `doctor` checks all four. `install-tools` installs Node, the JDK and Apalache; Quint stays
    mise's until XT-08. The shapes of `tool-config` and ARCH-gate-config admit exactly these
    backends, Node's `nodejs.org/dist` archives, Temurin's `%2B` and the `.mise/locks/` files,
    which are pinned in `.config/gate.sha256`.
  - **Versions re-checked:** Apalache 0.62.4, not 0.62.1 (Quint 0.33.1's own pairing; 0.63.0 is
    newer); Node 26.13.1 (Active LTS); Temurin 21.0.13+9.
  - **ADR 0007 as amended needs Quint models and `proptest-state-machine` tests.** This card sets
    up the Quint side only; the state-machine tests need no toolchain and arrive with the
    components they test (ver §6.4).
  - **Each job installs what it runs (review round 2, W1).** mise-action's `install_args` is
    `--locked` followed by tool names from ARCH-ci-lockstep's `CI_TOOLS` (no flag, no bare
    `--locked`, never the JDK or Apalache in ci.yml): the gate's eleven tools in every job, plus
    Node and Quint in `formal`, which also runs `doctor` now (the only job with every tool
    `doctor` requires); `formal.yml` installs just, Node, Quint, the JDK and Apalache. The
    bootstrap installs the JDK and Apalache only with `--formal-verify`; `doctor` reports them
    as optional (`only = "formal-verify"` in versions.toml), and `formal-verify` probes all four
    at their pinned versions before it runs, in a fresh private scratch directory (W3).
  - **What `just check` runs now:** fmt-check, architecture, clippy, nostd, docs, unit-tests,
    integration-tests, doctests, script-tests, formal-typecheck, formal-test, deny, secrets,
    workflows. `formal-verify` (Apalache, a JVM per model) is outside it: the `formal.yml`
    workflow, weekly and by hand, one job per model; the `formal` job of ci.yml runs the two
    stages.
- **Reuse considered:** mise's npm, java and github backends for Node, Quint, the JDK and Apalache
  (replaces mayfly's `formal.rs` and `apalache.rs` downloads and digests); Quint's TypeScript
  backend, never a run-time download.
- **Accept:**
  - [x] the stages are green with `smoke.qnt` and fail closed (with the install command) when Node or Quint is missing
  - [x] an unregistered `.qnt` fails `formal-typecheck`
  - [x] `just formal-verify smoke` passes locally with the pinned Apalache
  - [x] `just doctor` reports Node, Quint, Apalache and the JDK
- **Impact:** none (tooling).

#### FORMAL-02: Conformance harness: quint-connect drivers, committed-trace replay, negative controls
- **Depends on:** FORMAL-01. **Model:** deep. **Size:** M. **Parallel with:** EVAL-07, EVAL-05.
- **Cites:** ver §6.2; plan §7; ADR 0007, ADR 0030.
- `jfl-conformance` (test only) on `quint-connect` 0.1 and `itf` 0.4: drivers implement
  `quint-connect`'s `Driver` (`step` dispatches actions with `switch!`; the state is compared
  after every step through `itf`'s serde types, initial state included); generated traces through
  `#[quint_run]`, named Quint tests through `#[quint_test]`; our own code is the committed-trace
  replay (about 200 lines: the regression manifest reader, replay through the same driver, the
  divergence report with model, seed, trace, step, action, expected, actual and a diff). Recipes
  `formal-freshness` (regression traces regenerate byte for byte) and `conformance-replay`
  (seeded runs plus the committed traces; the seed printed; `just conformance [model] [seed]
  [traces] [steps]` repeats a run). Prove the harness on `smoke.qnt` with a test-only driver over
  a counter, and a negative control where the counter skips a value and the replay fails at the
  named step.
- **Reuse considered:** `quint-connect` and `itf` replace mayfly's `conformance.rs` (trace
  generation, seeds and replay, about 2,670 lines with the formal stages); `stateright` (models in
  Rust lose Quint and Apalache) was rejected; `proptest-state-machine` serves components without a
  model.
- **Accept:**
  - [ ] `check` runs both stages green; the same seed reproduces a run
  - [ ] the negative control fails at the expected step with expected and actual state in the report
  - [ ] a stale regression trace fails `formal-freshness`
  - [ ] a malformed ITF file is `HARNESS_ERROR`, never a pass
- **Impact:** none (tooling).

#### FORMAL-03: `views.qnt`: generations, freshness, completeness
- **Depends on:** FORMAL-02, DEC-02. **Model:** deep. **Size:** M. **Parallel with:** IDX-01, IDX-04.
- **Ordering:** started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton.
- **Cites:** plan §4, §7.1, §2.1; ver §6.1; ADR 0007, ADR 0014, ADR 0017.
- The model of plan §7.1: worktrees with files and versions, `HEAD` changes touching many files,
  watcher hints that can be lost, duplicated or reordered, reconcile steps, atomic publish,
  requests that stat, pin, read and finish, `sync`. Invariants `NoMixedGeneration`,
  `GenerationsIncrease`, `AtomicPublish`, `FreshAfterStat`, `CompleteIsHonest`, `NoCrossView`,
  `allInvariants`. Named scenarios: a checkout during a reconcile; a lost watcher event caught by
  the stat; a request in a nested worktree; `sync` after a burst of writes. Broken-model controls:
  publish before every summary is stored (violates `AtomicPublish`), answer from the newest
  generation mid-request (violates `NoMixedGeneration`), skip the stat (violates
  `FreshAfterStat`). Regression traces for each scenario, registered. Document the observation
  the IDX-09 driver must produce in `doc/formal-models.md` (created here).
- **Accept:**
  - [ ] typecheck, Quint tests and simulation of `allInvariants` green at the registered budget
  - [ ] each broken-model control violates its named invariant within the bound (a Quint test)
  - [ ] regression traces committed and fresh
  - [ ] `doc/formal-models.md` states stimuli, observation and invariants for `views`
- **Impact:** none (model).

#### FORMAL-04: `daemon.qnt`: single owner, spawn, reconnect, no ghost buffers
- **Depends on:** FORMAL-03. **Model:** deep. **Size:** M. **Parallel with:** IDX-02.
- **Cites:** plan §5.2, §7.2; ver §6.1; ADR 0007, ADR 0016.
- The model of plan §7.2: proxies (connect, spawn racing, reconnect, open and close documents,
  drop), daemons (start, lock acquire or fail, crash, idle exit, SIGTERM). Invariants
  `AtMostOneOwner`, `ServedOnlyByOwner`, `NoGhostBuffers`, `ShutdownReleases`; temporal
  `EventuallyServed` under fairness, checked in the Apalache lane. Scenarios: two proxies spawning
  at once; a crash with open documents and a reconnect; idle exit racing a new connection.
  Broken-model controls: release the lock before removing the socket; keep overlays across a
  restart. Regression traces, registered; `doc/formal-models.md` section for `daemon`.
- **Accept:**
  - [ ] typecheck, tests and simulation green; each control violates its named invariant
  - [ ] `just formal-verify daemon` passes at the documented bound, or the report names the tooling blocker (never a fabricated pass)
  - [ ] regression traces committed and fresh
- **Impact:** none (model).

#### FORMAL-05: ⛳ `diagnostics.qnt`: staleness
- **Depends on:** FORMAL-04. **Model:** deep. **Size:** M. **Parallel with:** IDX-03.
- **Cites:** plan §3.2, §7.3; ver §6.1; ADR 0007.
- The model of plan §7.3: documents with versions, compile jobs started per version, results
  arriving in any order, worker crash and restart, close. Invariants `NeverOlderOverNewer`,
  `PublishedMatchesVersion`, `ClosedPublishesNothing`. Scenarios: two quick edits with results in
  reverse order; a crash mid-compile; close while a compile runs. Broken-model control: publish
  every result as it arrives. Regression traces, registered; `doc/formal-models.md` section.
  This task is the FORMAL checkpoint: the review covers all three models and the harness.
- **Accept:**
  - [ ] typecheck, tests and simulation green; the control violates `NeverOlderOverNewer`
  - [ ] regression traces committed and fresh
  - [ ] `doc/formal-models.md` covers the three models with their drivers' tasks (IDX-09, IDX-12, LSP-04)
- **Impact:** none (model).

### Lane EVAL: phase 0, measure before building

Phase 0 runs beside the foundation (plan §9). Harnesses are built and tested here on synthetic
fixtures; runs against the target repository and Claude Code sessions are `(owner)` items. If the
target repository is private, only aggregated reports under `doc/eval/` are committed (ADR 0004,
`sdlc.md` §7).

#### DEC-01: (owner) Choose the target Java repository
- **Depends on:** none. **Model:** light. **Size:** S. **Parallel with:** FND-01, FND-02.
- **Cites:** plan §9.1, §9.2; ADR 0004, ADR 0009.
- An agent writes `doc/eval/target-criteria.md`: what E0–E4 need from the target (a large Gradle
  Java build, at least several modules, Lombok or MapStruct if possible, a history of agent-sized
  changes with tests, a build that runs on the owner's machine, the licence or permission to run
  tools on it), what stays private and how results return as aggregates. The owner chooses.
- **Accept:**
  - [x] `doc/eval/target-criteria.md` committed
  - [x] (owner) the target repository chosen; an ADR records the choice, naming it only if it is public (otherwise "a private repository meeting the criteria", with its size class) and the commit to pin
- **Decided:** 2026-11-17, option B. [ADR 0032](adr/0032-evaluation-target-spring-framework-at-the-bench-pin.md) (Accepted, owner) names spring-framework (public, Apache-2.0) at the bench pin `4cb62d893e09e23791d661e8d9b35aedee7006d0`. Lombok and MapStruct stay with the synthetic fixtures (EVAL-07, SEM-05, EVAL-10), and, amended the same day (BENCH-06), apache/fineract is a second target for them.
- **Impact:** none (decision).

#### EVAL-01: Evaluation harness: Harbor adapter, arms, transcript metrics
- **Depends on:** FND-02. **Model:** standard. **Size:** M. **Parallel with:** FND-07, FND-03.
- **Cites:** plan §1.2, §9.2; ver §11; fnd §2.7; ADR 0029.
- `tools/jfl-eval` (tool only, independent) around Harbor (ADR 0029): a task-set format (TOML: id,
  starting commit, prompt, test command, known-good diff reference, category) converted to Harbor
  tasks, an arm format (plugins, settings, environment, setup commands, the Docker image with a
  pre-warmed Gradle cache), a smoke task that shows whether language servers start under `claude -p`
  in a Harbor container (if not, a thin `claude -p --output-format stream-json` runner takes
  Harbor's place), and a reader for Harbor trajectories and Claude Code transcripts (JSONL) that
  extracts per run: success (from the recorded test-command result), tool calls by kind, tokens,
  edits followed by a compile failure and whether a pushed diagnostic preceded the fix, the
  histogram of LSP operations, empty-reference responses and the next action, time to first LSP
  answer; and `report`, which aggregates runs per arm and per task with means and spreads and writes
  a report with no task text, paths or code (only ids and numbers). Synthetic transcript fixtures;
  Harbor itself runs only on the owner's machine.
- **Reuse considered:** Harbor (Apache-2.0, the Terminal-Bench 2 harness, Docker tasks, a built-in
  Claude Code agent) instead of our own runner; `claude plugin eval` only for skill regressions (two
  arms, a 120 s scaffold, no custom graders).
- **Accept:**
  - [x] `cargo run --locked -p jfl-eval -- report <fixture dir>` produces the expected aggregate report (a golden)
  - [x] the transcript reader extracts each metric from the synthetic fixtures (one test per metric)
  - [x] a report never contains a prompt, path or source line from the input (a test plants one and asserts its absence)
  - [x] `jfl-eval` imports no production crate (`architecture.toml` lists it independent)
- **Impact:** none (tooling). Matched to Harbor 0.26.0 (commit `3cb6273`); runbook
  `doc/eval/harness.md`.

#### EVAL-11: jdtls behind our multiplexing proxy (`jfl-jdtls`)
- **Depends on:** FND-02. **Model:** deep. **Size:** M. **Parallel with:** FND-07, FND-03, FND-04.
- **Cites:** plan §1.3, §9.2; fnd §2.7; ADR 0020, ADR 0028.
- `tools/jfl-jdtls` (tool only, independent): `jfl-jdtls lsp`, a stdio proxy, and a per-user
  daemon running one jdtls per worktree root (the innermost `.git`; a nested worktree binds to
  itself), with `-data` under the state directory keyed on a digest of the full canonical
  worktree path, `-Djdt.core.sharedIndexLocation` on one shared directory, a heap cap, and
  optionally the syntax server until the import finishes. Multiplexing: request ids remapped per
  client, `didOpen`/`didClose` reference-counted per URI (the last buffer wins), cancellation
  forwarded, a client's exit never stops a jdtls others use, idle eviction. jdtls pinned and
  checksum-verified. A `.lsp.json` plugin variant for E0 and E4 and `doc/eval/jfl-jdtls.md`.
  Tested against a scripted fake language server; one integration test with the real pinned
  jdtls on a synthetic single-module fixture with `java.import.gradle.enabled=false` when the
  sandbox can fetch jdtls (never a Gradle import in the sandbox). Authority per fnd §2.4.
- **Reuse considered:** jdtls's launch options; `lsp-server` for framing; Plumb's shared-daemon
  proxy and Serena's collision fix (ideas only); the stock `jdtls-lsp` plugin as is (unfair
  baseline, ADR 0028).
- **Accept:**
  - [x] two clients in one worktree share one backend and each sees only its own responses with its own ids (fake backend)
  - [x] two worktrees with the same basename get different `-data` directories
  - [x] a client that exits mid-request leaves the backend serving the others
  - [ ] (owner) with Claude Code and three subagents in one worktree, one jdtls process runs for it (withdrawn: FND-12 removed the proxy)
- **Later removed (FND-12, 2026-11-17, the owner's order):** jdtls is for testing and comparison
  only and never included, so the proxy and its plugin were deleted; the bench's jdtls arm starts
  stock jdtls itself. The card stays as the record of what was built; its owner criterion is
  withdrawn.
- **Impact:** none (tooling; for testing and comparison only, never a shipped product: ADR 0028, amended 2026-11-17).

#### EVAL-02: E0 task set from the target repository's history
- **Depends on:** DEC-01, EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-05.
- **Cites:** plan §9.2 (E0); ver §11; ADR 0004, ADR 0032 (amended 2026-11-17).
- Select 15 to 20 tasks from the target's history (bug fixes, signature changes, cross-module
  features, one rename), each with its starting commit, prompt, test command and known-good diff,
  in the EVAL-01 format. If the target is public, the agent does this and commits the task set
  under `doc/eval/e0/`; if private, the agent commits only a selection guide
  (`doc/eval/e0-selection.md`) and the owner builds the set outside this repository.
  Fineract (ADR 0032 amendment; Lombok and MapStruct, public) gets a smaller second set of 5 to 8
  tasks under `doc/eval/e0/fineract/`, at least two touching Lombok-generated or MapStruct-generated
  members, reported beside spring-framework's and never pooled with it. Its test commands run on
  `bench.yml` or the owner's machine, never as a Gradle build in the sandbox.
- **Accept:**
  - [ ] the task set (public target) or the selection guide (private target) committed
  - [ ] each task's test command fails at the starting commit and passes with the known-good diff (checked by the agent for a public target)
  - [ ] (owner) for a private target: the task set built and checked the same way, kept outside this repository
  - [ ] the fineract second set committed under `doc/eval/e0/fineract/` (5 to 8 tasks, two or more on Lombok or MapStruct members); (owner) its test commands checked at the starting commits on `bench.yml` or the owner's machine
- **Impact:** none.

#### EVAL-03: E0 arms, Harbor images and runbook
- **Depends on:** EVAL-01, EVAL-11. **Model:** standard. **Size:** M. **Parallel with:** FND-06.
- **Cites:** plan §1.3, §9.2 (E0), §2.6; ver §11; ADR 0028, ADR 0029.
- The four arms as EVAL-01 arm files with their Harbor images: grep only (no language-server
  plugin); the stock `jdtls-lsp` plugin (the kill-criterion baseline; the proxy arm went with
  FND-12); `kmp-lsp` at a pinned commit. `doc/eval/e0-runbook.md`: machine setup,
  pinned versions of Claude Code and each arm's server, how to build the images and run each task
  three times per arm from a clean worktree, the control set (EVAL-12) included, where transcripts
  go, how to record the test-command result, how to measure 10 concurrent worktrees, and how to run
  `jfl-eval report`. Includes the unverified facts E0 must settle (plan §9.2).
- **Accept:**
  - [ ] arm files and runbook committed; `jfl-eval` validates the arm files (a test)
  - [ ] (owner) one dry-run task per arm on the owner's machine, confirming the runbook works
- **Impact:** none.

#### EVAL-12: E0 control set from Multi-SWE-bench Java
- **Depends on:** EVAL-01. **Model:** standard. **Size:** S. **Parallel with:** FORMAL-01.
- **Cites:** plan §9.2 (E0); ver §11; ADR 0029.
- Pick 10 to 20 Gradle instances from Multi-SWE-bench's Java set (record the dataset version),
  mixed by category (bug fix, signature change, feature), convert them to the EVAL-01 task format,
  and commit them under `doc/eval/e0-control/` with the selection criteria. Public, so committed
  whole.
- **Reuse considered:** Multi-SWE-bench's images and test oracles; SWE-bench-java-verified and
  SWE-PolyBench as optional extra controls.
- **Accept:**
  - [ ] the control set committed with the dataset version; `jfl-eval` validates it (a test)
  - [ ] (owner) each instance's oracle fails at the base commit and passes with the reference patch, where Docker runs
- **Impact:** none.

#### EVAL-04: (owner) Run E0 and report
- **Depends on:** EVAL-02, EVAL-03, EVAL-12. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §1.2, §1.3, §9.2; ver §11.
- The owner runs every task of the target set and the control set (EVAL-12) three times per arm on
  Harbor and hands the trajectories to `jfl-eval report` locally. An agent turns the aggregated
  report into `doc/eval/e0-results.md`: the metrics of plan §1.2 rows 1 to 3 per arm, separately for
  the target and the control set, the LSP operation histogram, behaviour on empty references,
  diagnostics wait times, and answers to the unverified facts.
- **Accept:**
  - [ ] (owner) all runs done; the aggregated report produced
  - [ ] `doc/eval/e0-results.md` committed with only aggregates, each number traceable to the report
- **Impact:** none.

#### EVAL-05: E1 in the harness: parse and summarise the corpus on the bench runner and the target
- **Depends on:** BENCH-04. **Model:** standard. **Size:** S. **Parallel with:** FORMAL-02.
- **Cites:** plan §1.7, §5.5, §9.2 (E1); ver §10; ADR 0022, ADR 0031.
- E1 is answered in spirit by the spike recorded in `doc/bench-corpus.md` (measured 2026-11-16 on
  a 4 vCPU Xeon 2.3 GHz VM): a full parse and declaration walk takes 7.2 s warm and 8.7 s cold on
  elasticsearch with 4 threads, about 50 MB/s, 153 MB peak RSS. This task runs BENCH-02's `e1`
  suite in `bench.yml` on its fixed runner class (and on the target, owner-run): cold and warm wall time,
  CPU time, files per second, per-file p50, p95 and max, peak RSS of the in-memory summaries, and
  each grammar's parse-error rate (orchard and upstream), with machine facts.
- **Reuse considered:** the spike's harness and corpus instead of a synthetic generator; `ignore`
  for the walk.
- **Accept:**
  - [ ] `just bench e1` on the small, medium and large corpus in a `bench.yml` dispatch run (the sandbox runs small only); results in `doc/eval/e1-results.md` with machine facts, compared with the spike's numbers
  - [ ] cold runs drop the page cache or say they could not (never report warm as cold)
  - [ ] (owner) run on the target repository; aggregate numbers added to `doc/eval/e1-results.md`
- **Impact:** none.

#### EVAL-06: E4: stock jdtls at 1, 3 and 10 worktrees
- **Depends on:** FND-12, BENCH-02. **Model:** standard. **Size:** M. **Parallel with:** FND-05, EVAL-08.
- **Cites:** plan §1.3, §1.7, §9.2 (E4); ADR 0028 (amended 2026-11-17), ADR 0031.
- A `jfl-bench` suite `e4`: stock jdtls only (one per client, as a user gets it; started by the
  bench's launcher, `src/jdtls.rs`, and nothing of ours in between), driven with `initialize` and a fixed query script over LSP, with
  RSS over time, time to first useful answer and project import count for 1, 3 and 10 linked
  worktrees and 1 and 3 clients per worktree; runbook in `doc/eval/e4-runbook.md`. In the sandbox
  it runs on the synthetic multi-module fixture with `java.import.gradle.enabled=false` (no build
  of any repository here); the corpus and target runs, which import with Gradle, are `(owner)`.
- **Accept:**
  - [ ] `just bench e4` produces a report on the synthetic fixture, or the runbook-only fallback is recorded with the reason
  - [ ] (owner) run on the medium corpus with its Gradle import; aggregates in `doc/eval/e4-results.md`
  - [ ] (owner) run on the target repository at 1, 3 and 10 worktrees; aggregates in `doc/eval/e4-results.md`
- **Impact:** none.

#### EVAL-07: Golden resolution corpus and `java-fixtures` stage
- **Depends on:** FORMAL-01 (the pinned JDK). **Model:** deep. **Size:** L. **Parallel with:** FORMAL-02.
- **Cites:** plan §5.6, §8; ver §4, §7.1; fnd §9.
- `fixtures/java/resolution/`: 200+ small cases by construct (plan §5.6 list plus overloads,
  inheritance, generics, inner, anonymous and local classes, static imports, enums, records,
  sealed types, switch patterns), each a compilable snippet project with an expected-answer file
  (the target of every marked reference: `exact` with its symbol, or the `possible` reason).
  `fixtures/java/projects/`: the projects of ver §4 (single module, multi-module Gradle, nested
  worktrees as a script that builds them, Lombok and MapStruct with generated sources present and
  missing, Java 8, 17 and 21 syntax). The `java-fixtures` stage compiles every fixture with the
  pinned JDK (offline; Lombok and MapStruct jars from a pinned, checksum-verified download cached
  by a recipe, or the cases that need them stay syntax-only and say so); its CI job. These
  synthetic Lombok and MapStruct projects stay the offline gate; fineract (BENCH-06) is the real
  counterpart, read through BENCH-05's cases and never copied into a fixture.
- **Accept:**
  - [ ] at least 200 resolution cases, at least 8 per construct of plan §5.6; a coverage table in `fixtures/java/resolution/README.md`
  - [ ] `java-fixtures` green in `check` and CI; a broken fixture outside `fixtures/java/broken/` fails it
  - [ ] no fixture copies code from any real project (synthetic only; ADR 0004)
- **Impact:** none (fixtures).

#### EVAL-08: SCIP oracle (`jfl-oracle`)
- **Depends on:** EVAL-07. **Model:** deep. **Size:** M. **Parallel with:** EVAL-06.
- **Cites:** ver §7.1; plan §5.6; fnd §2.7.
- `tools/jfl-oracle` (tool only, independent): read a SCIP index with the `scip` crate (0.10), map
  occurrences to (file, range, symbol, role), classify each call site by construct (from its syntax,
  with its own tree-sitter use, not `jfl`'s), and compare a candidate's answers (the `jfl --json`
  envelope shape, or a bake-off adapter's output) with SCIP: precision of `exact`, coverage per
  construct, recall of `exact ∪ possible` when `complete=true`; report with aggregates only. A small
  SCIP fixture generated from a single-module resolution-corpus project (EVAL-07) by a pinned
  `scip-java` driving `javac` directly (no Gradle build in the sandbox), else a hand-written
  index with a test that it decodes.
- **Reuse considered:** `scip-java` (Apache-2.0) as the independent indexer and the `scip` crate
  (its `protobuf` =3.7.2 pin watched by cargo-deny); Kythe and Glean were rejected as heavy.
- **Accept:**
  - [ ] the oracle's metrics on hand-made inputs match hand-computed values (tests per metric)
  - [ ] the construct classifier labels the resolution corpus's cases as their directory says (a test)
  - [ ] `jfl-oracle` imports no production crate
- **Impact:** none (tooling).

#### EVAL-09: E2 resolver bake-off
- **Depends on:** EVAL-07, EVAL-08, EVAL-10, BENCH-05. **Model:** deep. **Size:** L. **Parallel with:** DEC-03.
- **Cites:** plan §5.6, §9.2 (E2), §9.3; ver §7.1; ADR 0036.
- Run each candidate resolver of plan §5.6 on the resolution corpus through thin wrappers in
  `jfl-eval` (a minimal tree-sitter resolver prototype of our own written as a spike; `ktcore`, which
  is not a crates.io crate, so the card's agent first records its repository URL, pinned commit
  and licence file, and records it as not runnable if it cannot; the
  `kmp-lsp` resolver at a pinned commit; the **javac-attributed** arm, scip-java's plugin in
  EVAL-10's warm worker, timed per file), on the resolution corpus and the real-corpus cases
  (BENCH-05), feed results to `jfl-oracle`, and write `doc/eval/e2-results.md`: precision, coverage
  by construct, latency and cost per candidate, and whether a hybrid (javac for `exact`, tree-sitter
  for `possible`) beats each alone. Stack Graphs is not a candidate (archived 2025-09). A candidate
  that cannot be built or run is recorded with the reason, never dropped silently.
- **Reuse considered:** the candidates themselves are reuse: `ktcore` and `kmp-lsp` (MIT),
  scip-java's plugin; Stack Graphs dropped (archived).
- **Accept:**
  - [ ] corpus results for every runnable candidate committed, with the oracle's raw aggregate files
  - [ ] (owner) a SCIP index of the target repository produced with `scip-java` and the bake-off run against it; aggregates added
- **Impact:** none.

#### EVAL-10: E3 `javac` worker spike: our own worker with a warm javac context
- **Depends on:** EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-06, BENCH-05.
- **Cites:** plan §3, §9.2 (E3); ver §10; ADR 0036, ADR 0032 (amended 2026-11-17).
- Write our own small worker in `java/javac-worker/` as the spike (ADR 0036; java-language-server
  and the JDK as reference only, no copied code, since its `ReusableCompiler.java` is GPL-2.0
  with the Classpath Exception): one warm JVM keeping one javac context warm across single-file
  compiles, falling back to a fresh compiler when reuse fails, `-proc:none -implicit:none` and a
  given `-sourcepath` and classpath, class folders from earlier builds read when present, with
  scip-java's plugin 0.14.2 optionally loaded; an ECJ batch arm (`-proceedOnError`). Two questions
  answered first: does javac check method bodies of dependency sources under `-implicit:none` (if
  so, body blanking of those sources goes in; if not, it is dropped), and what does the scip-java
  plugin emit for a file with errors, at what latency. `jfl-eval e3` drives both over a list of files and reports p50, p95, RSS,
  false errors, the plugin's attribution cost and, given several worktrees' classpaths, the number
  of distinct `EnvironmentKey`s (plan §5.4). Measured on the multi-module fixture project with a
  classpath the fixture declares (no Gradle run in the sandbox); a module of the small bench
  corpus, whose classpath needs a Gradle resolve, is an `(owner)` or `bench.yml` dispatch run.
  Fineract is the real Lombok and MapStruct target: whether `-proc:none` leaves false errors on
  its generated members, and what loading the processors costs, are measured there on the same
  kind of run (criterion S2 of `doc/eval/target-criteria.md`; spring-framework runs no such processor).
- **Reuse considered:** forking java-language-server's core (ADR 0027) rejected for its GPL file
  and its autocomplete-only body erasure (ADR 0036); ECJ as a comparison arm; jdtls's javac
  backend (incubating) and NetBeans' nb-javac for ideas only.
- **Accept:**
  - [ ] fixture results committed in `doc/eval/e3-results.md` with machine facts, for javac and ECJ, on JDK 17, 21 and 25, warm reuse and the fresh-compiler fallback
  - [ ] the `-implicit:none` body-checking answer and the scip-java plugin's output and latency on files with errors recorded there, each with the fixture that shows it
  - [ ] (owner) one module of the small corpus measured the same way; aggregates added
  - [ ] (owner) run on two modules of the target repository and over 10 worktrees' classpaths; aggregates added
  - [ ] (owner) run on two fineract modules that use Lombok and MapStruct, with and without the processors loaded; false errors and cost added to `doc/eval/e3-results.md`
- **Impact:** none.

#### DEC-03: Storage tier (from E1)
- **Depends on:** EVAL-05. **Model:** deep. **Size:** S. **Parallel with:** DEC-04, DEC-05, DEC-06, EVAL-09.
- **Cites:** plan §5.5; ADR 0009.
- From `doc/eval/e1-results.md`, a draft ADR (Proposed) picking the tier of plan §5.5 with the
  numbers that decide it. The E1 spike already points the way (plan §5.5): rebuild in memory at
  start for small and medium repositories (≤ 2 s cold), persisted per-file summaries keyed by
  blake3 with Git blob ids as a pre-filter above about 10–15k files or 50 MB of Java; the draft may
  be written from the spike and confirmed by EVAL-05's run.
- **Accept:**
  - [ ] the draft ADR committed, citing the E1 numbers
  - [ ] (owner) the ADR accepted, overridden or rejected; the Decisions table updated
- **Impact:** none (decision).

#### DEC-04: Resolver and the SCIP baseline (from E2)
- **Depends on:** EVAL-09. **Model:** deep. **Size:** S. **Parallel with:** DEC-03, DEC-05, DEC-06.
- **Cites:** plan §5.6, §9.3, §10.4; ADR 0009.
- A draft ADR (Proposed) choosing the resolver (possibly a hybrid: javac-attributed `exact`,
  tree-sitter `possible`) and the coverage share M4's gate will require, per construct where it
  differs, from `doc/eval/e2-results.md`; if our own, it names the crate `jfl-resolve` (pure) and
  the edges it adds. It also says whether M4 keeps the SCIP baseline import (SEM-08).
- **Accept:**
  - [ ] the draft ADR committed with the E2 numbers
  - [ ] (owner) decided; the Decisions table updated
- **Impact:** none (decision).

#### DEC-05: Semantic diagnostics in v1 (from E3)
- **Depends on:** EVAL-10. **Model:** deep. **Size:** S. **Parallel with:** DEC-03, DEC-04, DEC-06.
- **Cites:** plan §3.4, §9.3; ADR 0009.
- A draft ADR (Proposed): semantic diagnostics on every edit, or "on save, best effort", with the
  E3 numbers and the worker cap they imply.
- **Accept:**
  - [ ] the draft ADR committed
  - [ ] (owner) decided; the Decisions table updated
- **Impact:** none (decision).

#### DEC-06: `referencesMode` default and operation priority (from E0)
- **Depends on:** EVAL-04. **Model:** deep. **Size:** S. **Parallel with:** DEC-03, DEC-04, DEC-05.
- **Cites:** plan §2.5, §9.2; ADR 0034.
- A draft ADR (Proposed): the `referencesMode` default, the order in which LSP operations and CLI
  subcommands are built, and changes to the skill text, from `doc/eval/e0-results.md`.
- **Accept:**
  - [ ] the draft ADR committed
  - [ ] (owner) decided; the Decisions table updated
- **Impact:** none (decision).

#### DEC-02: ⛳ Go/no-go memo, the kill criterion and the speed targets
- **Depends on:** DEC-03, DEC-04, DEC-05, DEC-06, DEC-07, EVAL-06, BENCH-03. **Model:** deep. **Size:** M. **Parallel with:** none.
- The kill criterion comes first: this memo waits only on the measurements that decide it (E0 to
  E4, the speed baseline), not on the formal models (FORMAL-03 to FORMAL-05 follow it) or the
  coverage tooling.
- **Cites:** plan §1.3, §1.7, §9.3; ADR 0009, ADR 0028, ADR 0031.
- `doc/eval/go-no-go.md`: each criterion of plan §9.3 with its numbers and source report, the
  kill criterion of plan §1.3 against E0 and E4 (stock jdtls as the baseline), the §1.7
  speed targets against BENCH-03's baseline, the recommended decision and what changes in the
  plan if it is "go with changes" (a list of task-card edits). A draft ADR (Proposed) recording
  the decision. The kill criterion (plan §1.3): if jfastlsp cannot beat stock jdtls on
  speed with honest envelopes, the memo recommends stopping or a rescope (and lists the
  task-card edits a rescope needs); it never recommends shipping jdtls, which stays a
  testing and comparison arm (ADR 0028, amended).
- **Accept:**
  - [ ] memo and draft ADR committed; every number traces to a committed report
  - [ ] (owner) go, go with changes (a rescope), or stop, and the §1.7 targets confirmed; the ADR's status says so and `doc/tasks.md` reflects it
- **Impact:** none (decision).

### Lane IDX: M1, the shared engine

M1 (plan §10.1): syntax index, envelope, views, queries and daemon (IDX-01..IDX-12), carried to
agents first by the LSP lane (ADR 0034, LSP first). The CLI cards IDX-13..IDX-15 keep their ids
but follow in lane CLI (M2). Starts after the go decision.

#### IDX-01: Domain: paths, positions and encodings
- **Depends on:** DEC-02. **Model:** standard. **Size:** M. **Parallel with:** IDX-04, FORMAL-03.
- **Ordering:** started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton.
- **Cites:** plan §2.3, §6.1; fnd §2.3, §4.3; ver §2, §3.
- In `jfl-domain`: validated value types for worktree-relative paths and `file://` URIs (percent
  encoding, no `..` escape), a `LineIndex` over UTF-8 bytes (LF, CRLF and a lone CR, LSP 3.17's
  set and JLS §3.4; a final line without a newline), and pure conversions between byte offset,
  (line, UTF-16 unit), (line, UTF-8 byte) and (line, scalar) in `#![no_std]` code (with `line-index` used directly if it builds under `no_std` +
  `alloc`, otherwise as the property-test oracle, ADR 0020), with typed refusals for positions past
  a line or file end and inside a surrogate pair or a multi-byte sequence. `Range` and an encoding
  tag. No I/O.
- **Reuse considered:** `line-index` (rust-analyzer) for UTF-8 ↔ UTF-16; `text-size`.
- **Accept:**
  - [x] property tests: round trips over arbitrary UTF-8 including astral characters and CRLF; offsets never split a character (`crates/jfl-domain/tests/properties.rs`, seeded, `line-index` as the oracle: it is not `no_std`)
  - [x] unit tests for each refusal and for the CLI's 1-based byte column (plan §2.3)
  - [x] `ARCH-domain-purity` green; no `unwrap`, indexing or unchecked arithmetic on inputs
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): BENCH-05 has no cases yet, so `just bench-domain SIZE` (divan, in process) first checks every char boundary of every corpus file against `line-index`, then times the conversions
- **Impact:** public API (domain types).

#### IDX-02: Domain: summaries, symbols, cache keys, the envelope
- **Depends on:** IDX-01. **Model:** deep. **Size:** M. **Parallel with:** IDX-04, FORMAL-04.
- **Ordering:** started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton.
- **Cites:** plan §2.1, §2.2, §5.4; ADR 0014.
- In `jfl-domain`: `FileSummary` (package, imports, declarations with kind, name, signature,
  modifiers, Javadoc range, supertypes by name; reference sites by simple name, arity and syntactic
  shape, with comments and strings excluded), `SymbolId` (the plan §2.1 textual form, parsed and
  printed), `SyntaxKey`, `EnvironmentKey`, `SemanticKey` derivations (pure, over digests the
  caller supplies), and the `Envelope` with `exact`, `possible` (each with a `why`), `complete`,
  `incomplete_because`, `view` and `freshness`, plus a pure `completeness(visible modules, index
  state)` function that returns `complete=false` with reasons unless every input is indexed and
  fresh. Ordering: `exact` before `possible`, stable by URI and range.
- **Reuse considered:** `blake3` digests supplied by the adapters; the envelope itself has no
  published equivalent.
- **Done note (IDX-02, round 2):** the bench item was met with synthetic inputs sized from the
  corpus (spring-framework's and junit-framework's per-file declaration and identifier counts),
  not a corpus run: the envelope, completeness and summary types read no files before IDX-07.
  `just bench-domain-envelope` reports them.
- **Accept:**
  - [ ] unit tests: `complete` is `true` only when every visible module is indexed and fresh; each missing condition yields its reason
  - [ ] property test: shuffling inputs never changes the envelope's order or content
  - [ ] `SymbolId` round-trips its text form for every declaration kind (property test)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** public API (domain types).

#### IDX-03: Protocol v1: messages, versions, frame decoder, goldens
- **Depends on:** IDX-02. **Model:** deep. **Size:** M. **Parallel with:** IDX-05, IDX-06, FORMAL-05.
- **Ordering:** started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton.
- **Cites:** plan §2.4, §5.3, §6.6; ver §4; fnd §6; ADR 0021.
- `jfl-protocol`: the handshake (protocol major and minor, client kind), request and response
  messages for every operation of plan §2.4 and document sync (open, change, close with versions),
  typed errors, the envelope's JSON form with `schema_version` and `position_encoding`, all carried
  as JSON-RPC 2.0 payloads (method names and params) for the LSP base-protocol framing of ADR 0021,
  and a sans-io `Content-Length` frame decoder (`no_std`, about 60 lines) that checks the declared
  length against the maximum frame size before any allocation; the IPC and LSP adapters feed it
  bytes. Goldens for every message first, then the codec (golden first, rule 8).
- **Reuse considered:** `lsp_server::Message` as the JSON-RPC type in the adapters (ADR 0021);
  `serde_json` with `alloc`; `schemars` for the `--json` schema; `jsonrpsee` and `tarpc` rejected.
- **Accept:**
  - [ ] golden tests for every message; a decode of an unknown field succeeds (additive) and of a different major fails with a typed error
  - [ ] a frame over the limit is refused before allocating (a test with a lying `Content-Length`); junk and duplicate headers are refused
  - [ ] property test: encode then decode is identity for generated messages
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** protocol (v1).

#### IDX-04: `jfl-process`: argv-only child runner
- **Depends on:** DEC-02. **Model:** deep. **Size:** M. **Parallel with:** IDX-01, IDX-02, FORMAL-03.
- **Ordering:** started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton.
- **Cites:** plan §6.2, §6.5, §11; fnd §2.4, §4.4; ver §5.
- `jfl-process` (support): run a child from a program path and argv (no shell, no string splitting),
  a cleared environment plus an explicit allowlist, a working directory, a deadline, its own process
  group killed as a group on deadline or drop, stdout and stderr captured up to a byte limit (excess
  counted, not buffered), and an outcome that keeps exited, killed on deadline, failed to start and
  output-truncated apart. Also a long-lived child mode with framed stdin and stdout for the JVM
  helpers (SEM-04, PROJ-01), and a detached mode (left in the launcher's process group so the
  daemon's own `setsid` succeeds, which it cannot as a group leader; null stdin, output to a log
  file) for the daemon launcher (ADR 0021, amended 2026-11-17). The `process_runner` contract suite in `jfl-testkit`
  with a scripted fake.
- **Reuse considered:** std `Command` with `process_group(0)` (no `pre_exec`, which is `unsafe`);
  `rustix` 1.1.6 for the safe calls std lacks (the group kill, `waitid` with `WNOWAIT`, Linux's
  pidfd, `poll(2)`), so one thread waits on both pipes and the exit with no sleep (the crate holds
  no spawn or sleep grant); the daemon calls `setsid` itself (ADR 0021). `command-group` and
  `process-wrap` rejected: they add the group and kill-on-drop that `process_group(0)` and
  `killpg` already give, and neither bounds output or enforces a deadline without threads;
  `daemonize`/`fork` rejected. macOS has no safe exit descriptor (rustix's `kevent` is
  `unsafe`), so there a wait is cut into 5 ms slices between checks of the leader.
- **Accept:**
  - [x] integration tests: a child that ignores SIGTERM is killed with its group at the deadline; a grandchild does not survive; an argument with spaces and quotes arrives intact; the environment holds only the allowlist
  - [x] the contract suite passes for the fake and the real runner and fails for a deliberately broken runner
  - [x] clippy's lists refuse `std::process` in every crate without the grant (FND-02's negative case still passes)
  - [x] runs on the bench corpus (small at least, `just bench-process small`: the git adapter's two reconcile calls in the small corpus, through `jfl-process` and std) and reports the metrics it affects; no real-corpus case applies (the crate reads no Java)
- **Impact:** public API (support crate).

#### IDX-05: Git adapter: roots, identity, freshness stat, changed files
- **Depends on:** IDX-01, IDX-04. **Model:** standard. **Size:** M. **Parallel with:** IDX-03, IDX-06.
- **Cites:** plan §4.1, §4.3, §4.6; ver §5; ADR 0017, ADR 0024.
- `RepoProbe` port in `jfl-application`; `jfl-adapter-git`: the innermost worktree root of a path
  (cached per directory prefix), the common directory, the `HEAD` and index files to stat for a main
  or linked worktree, the changed files since a tree (`diff --name-only`, `status -z`, parsed
  defensively). Per ADR 0024: the `git` CLI through `jfl-process` on the reconcile path and
  stat-only files on the hot path; this task measures the spawn cost. The `repo_probe` contract
  suite with a scripted fake; `jfl-testkit` builders for temporary repositories with linked
  worktrees under `.claude/worktrees/`.
- **Reuse considered:** the `git` CLI (ADR 0024); `gix` (linked-worktree gap) and `git2` (C build)
  rejected for now.
- **Accept:**
  - [ ] contract suite green for fake and adapter; a broken fake fails it
  - [ ] integration tests: a path in a nested linked worktree resolves to it, not the parent; checkout, stash, reset and commit each change the stat and the changed-file set
  - [ ] the spawn cost per command measured on the fixtures and the small bench corpus, recorded in the commit body (and an amendment of ADR 0024 if it changes the decision)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** dependency graph.

#### IDX-06: FS adapter: scanning, nested-worktree exclusion, watcher hints
- **Depends on:** IDX-01. **Model:** standard. **Size:** M. **Parallel with:** IDX-03, IDX-05.
- **Cites:** plan §4.2, §4.3, §11; ver §5; ADR 0017.
- `SourceTree` port; `jfl-adapter-fs`: list `.java` files under a root, skipping directories with
  a `.git` file or directory, `.claude/worktrees/`, build output directories and symlinked
  directories that leave the root; read bounded bytes; stat (mtime, size, inode); a watcher
  (`notify` 8.2 with `notify-debouncer-mini`) whose events are hints with overflow reported; the
  walk on `ignore` (`.gitignore`-aware). The `source_tree` contract suite.
- **Reuse considered:** `notify` and `ignore` (ripgrep's walker); `watchman_client` (old
  dependencies, needs watchman) only as a later optional backend; `notify` 9 when final (REL-02).
- **Accept:**
  - [ ] contract suite green for fake and adapter; a broken fake fails it
  - [ ] the nested-worktree fixture (a main checkout with three `.claude/worktrees/*`) lists each file once, under its own root
  - [ ] a symlink to a directory outside the root is not followed (integration test)
  - [ ] CC0-1.0 (`notify`) and Unlicense (`ignore`) accepted by `deny.toml` as plan §16.2 lists
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** dependency graph.

#### IDX-07: Syntax adapter: tree-sitter summaries and the golden Java corpus
- **Depends on:** IDX-02. **Model:** deep. **Size:** L. **Parallel with:** IDX-08.
- **Cites:** plan §2.1, §2.2, §5.4, §11; ver §2, §4, §8; fnd §3.3; ADR 0022.
- `SyntaxIndexer` port; `jfl-adapter-java`: `tree-sitter` with `tree-sitter-java-orchard` parsing
  into `FileSummary` through our own `tags.scm` and `locals.scm` (nvim-treesitter's locals as the
  Apache-2.0 start, with attribution), syntax errors as diagnostics with ranges, comments and
  strings excluded from reference sites, Javadoc attached to declarations, method references matched
  by name only; bounded input size and depth, no panic on any bytes. `fixtures/java/summaries/`
  goldens for every corpus file (golden first), `fixtures.lock` and the `ARCH-fixtures-lock` rule
  (fnd §3.3). The `syntax_indexer` contract suite (canned fake vs adapter).
- **The switch-back boundary** (ADR 0022, amended 2026-11-17; about 1–2 days more):
  `tree-sitter-java-orchard` pinned exactly (`=`), and upstream `tree-sitter-java` as a
  dev-dependency; only `jfl-adapter-java` names grammar nodes and fields; `tags.scm` and
  `locals.scm` per grammar; every node kind and field name the adapter uses resolved into a table
  at indexer start-up, failing loudly with the missing name; a synthetic per-feature Java corpus
  (`fixtures/java/grammar/`, one file per feature: records, sealed types, switch patterns, text
  blocks, `import module`, flexible constructor bodies, annotations in every position) parsed by
  both grammars against per-grammar golden summaries, with the known upstream gaps in a committed
  list; the grammar's crate name and exact version in `SyntaxKey`.
- **Reuse considered:** `tree-sitter` + `tree-sitter-java-orchard` (ADR 0022); `tree-sitter-tags`
  rejected (hides ranges and scopes); `java-lang` too new.
- **Decided (orchestrator, IDX-01 review):** check the grammar for a CRLF-terminated line comment: a
  token ending on the `\n` of a `\r\n` is refused by `LineIndex` (`InsideLineTerminator`), so the
  adapter ends such a range at the line end (before the `\r`), with a test on a CRLF fixture.
- **Decided (orchestrator, IDX-01 review):** strip a leading UTF-8 BOM when reading a file from disk,
  before indexing, as editors drop it from document text; a test with a BOM fixture.
- **Decided (orchestrator, IDX-02 review):** before interning any name (declaration or reference
  site), decode `\uXXXX` escapes and drop the identifier-ignorable characters of JLS §3.8 (`Cf`,
  such as U+200B and U+FEFF), so that one identifier has one spelling and a reference spelled
  otherwise still matches its declaration; `SymbolId` names admit more than Java's identifier
  characters, and this extraction step is what keeps them Java's.
- **Decided (orchestrator, IDX-02 review):** `Declaration::signature` is the header as written,
  white space collapsed, *with* its annotations and their arguments and without body, Javadoc or a
  field's initializer; `annotations` lists their names; `permits` names go to `permits`, never to
  `supertypes`; parameter types go through `jfl_domain::erase_parameter_type` with the type
  parameters in scope (method first, then each enclosing type), so a type variable erases to its
  first bound.
- **Decided (orchestrator, IDX-02 review):** reference sites leave out uses bound to locals and
  parameters, using `locals.scm` (plan §5.5's memory decision); a use whose binding is unclear stays.
- **Accept:**
  - [ ] summary goldens match for every file of `fixtures/java/projects/` and `fixtures/java/resolution/`
  - [ ] property test: arbitrary bytes and mutated corpus files never panic and finish within a bound
  - [ ] `ARCH-fixtures-lock` fails on an edited golden with a stale lock (negative fixture)
  - [ ] both grammars parse the per-feature corpus to their goldens in the test stages and CI; a gap that opens or closes without the known-gaps list changing fails the test
  - [ ] a node or field name missing from the loaded grammar fails start-up naming it (test with a renamed name)
  - [ ] the tree-sitter dependencies (C code) named in the external allowlist, `deny.toml`'s build-script allowlist and the commit, with their licences
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** dependency graph; fixture corpus.

#### IDX-08: Summary store at the DEC-03 tier
- **Depends on:** IDX-02, DEC-03. **Model:** standard. **Size:** M. **Parallel with:** IDX-07.
- **Cites:** plan §5.4, §5.5, §5.10; fnd §6; ver §5; ADR 0025.
- `SummaryStore` port; `jfl-adapter-store` at the tier DEC-03 chose (by default: a flat on-disk
  cache keyed by `SyntaxKey` with an LRU size cap) on the crates of ADR 0025 (`blake3` keys,
  `postcard` blobs in our content-addressed directory, `fst` for names, `rusqlite` from the second
  tier): a versioned header (a different version is
  discarded, never migrated), atomic writes (temporary file and rename), a torn or corrupt entry
  treated as a miss. The `summary_store` contract suite.
- **Reuse considered:** `rusqlite`, `fst`, `postcard`, `blake3` (ADR 0025); cacache's layout copied,
  not the crate; `redb` the runner-up; `bincode` (unmaintained) and `rkyv` (unmeasured) rejected.
- **Decided (orchestrator, IDX-02 review):** on the large tier, reference sites stay in the
  persisted summaries and are not held in memory; memory holds a name → file postings index, and
  a `refs` query loads the files its name lists (plan §5.5). The §1.7 syntax-tier RSS budget
  (≤ 250 MB on elasticsearch) is unchanged; whole summaries in memory project to about 311 MiB
  there (325 MiB with IDX-02's final 80-byte declarations).
- **Accept:**
  - [ ] contract suite green for fake and adapter; a broken fake fails it
  - [ ] a truncated entry on disk is a miss, never a corrupt summary (integration test)
  - [ ] the cap evicts least-recently-used entries (test on fake clock)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** cache format (v1).

#### IDX-09: View registry, generations and freshness (views driver)
- **Depends on:** IDX-05, IDX-06, IDX-07, IDX-08, FORMAL-03. **Model:** deep. **Size:** L. **Parallel with:** LSP-01.
- **Cites:** plan §4, §7.1; ver §6; ADR 0007, ADR 0017.
- In `jfl-application`: the view registry (a view per worktree root, opened on first request),
  reconcile (watcher hints and the per-request stat start it; changed files re-summarised through
  the store), generations built beside the published one and published atomically, requests that
  pin a generation, `sync` as a barrier. Model first: `formal/views.qnt` exists (FORMAL-03); write
  the `views` conformance driver in `jfl-conformance` over the registry with testkit fakes, then
  the code; the negative control (publish before the last summary is stored) fails at its named
  step.
- **Reuse considered:** `salsa` weighed and deferred (per-process database, experimental
  persistence); turborepo's cookie file to prove the watcher caught up for `sync`.
- **Accept:**
  - [ ] `conformance-replay` replays the `views` regression and generated traces green
  - [ ] the negative control fails at the expected step
  - [ ] component tests: a request pinned to N while N+1 publishes answers wholly from N; a `sed -i`-style write seen by the stat is in the answer without `sync`
  - [ ] a `proptest-state-machine` test of the registry (open, reconcile, publish, pin, `sync`) against a reference machine (ADR 0007, amended; ver §6.4)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none (internal); model conformance.

#### IDX-10: Queries: `sym`, `def`, `hover`
- **Depends on:** IDX-09. **Model:** deep. **Size:** L. **Parallel with:** LSP-01.
- **Cites:** plan §2.1, §2.2, §2.7; ADR 0014.
- Application queries over a pinned generation: workspace and document symbols (search by
  prefix and camel-case humps, kinds), definition (lexical scope, imports, same-package and
  project types; bounded ambiguity over overloads of a proved owner type), hover (signature,
  Javadoc, declaring type and module, status line, and for declarations the reference summary once
  IDX-11 lands), each returning the envelope, each with its internal deadline returning a partial
  answer with `complete=false`.
- **Accept:**
  - [ ] component tests over the fixture projects for each operation, including bounded ambiguity never returning another type's member
  - [ ] a deadline hit returns `complete=false` with `deadline` in `incomplete_because` (fake clock)
  - [ ] envelope goldens for the cases chosen for M1's 50-case gate (started here, completed in LSP-06)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none (internal).

#### IDX-11: Queries: `refs`, the `possible` superset, completeness
- **Depends on:** IDX-10. **Model:** deep. **Size:** L. **Parallel with:** IDX-12.
- **Cites:** plan §2.1, §2.2; ver §7.2; ADR 0014.
- References: `exact` from lexical and import evidence, `possible` as the sound superset (simple
  name and arity in every file that can see the declaring module; method references by name), each
  with its `why`; `complete` from `jfl-domain`'s completeness function over the view's index state
  (before M3, the whole worktree as one module, plan §2.1); `--exact-only` filtering; the hover
  reference summary. The completeness oracle (ver §7.2) as an integration test over every module
  of the multi-module fixture.
- **Reuse considered:** nothing maintained computes completeness; codebase-memory-mcp's CALLS vs
  USAGE edges and Code-Index-MCP's fallback flag as ideas.
- **Decided (orchestrator, IDX-02 review):** the `possible` scan on the large tier reads reference
  sites from the store through the name → file postings (plan §5.5), not from memory.
- **Decided (orchestrator, IDX-02 review):** before M3 no project model names generated-source
  roots, so `ModuleIndex`'s `missing_generated_roots` is unknown, not 0: when annotation
  processors are detected (a processor path, or a known processor's annotation in a summary) and
  the roots are unknown, report `GeneratedSourcesMissing` rather than claim `complete`.
- **Accept:**
  - [ ] the completeness oracle passes for every module and seed in the gate's budget; a planted defect (ignore one file) makes it fail
  - [ ] references inside comments and strings never appear (golden cases)
  - [ ] an unindexed or stale file makes `complete=false` with its reason (component test)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none (internal).

#### IDX-12: Daemon and IPC: lock, socket, spawn, reconnect (daemon driver)
- **Depends on:** IDX-03, IDX-09, FORMAL-04. **Model:** deep. **Size:** L. **Parallel with:** IDX-11.
- **Cites:** plan §5.1, §5.2, §5.3, §7.2, §11; fnd §4.4; ADR 0016, ADR 0021.
- `jfl-adapter-ipc`: the state directory (created with `DirBuilderExt::mode(0o700)`; an existing
  one must be owned by the daemon's uid with mode `0700`, or the daemon refuses and names it;
  plan §5.3), the single-owner lock (std `File::try_lock`, held for life; no pid file trusted),
  Unix socket bound inside the directory (path-length limit refused with the limit named), both
  then set to `0600`, peer-uid check (Tokio `peer_cred()` on the daemon
  side; the CLI client on std `UnixStream`), JSON-RPC with LSP framing through `jfl-protocol`'s
  decoder and `lsp_server::Message`, server and client, the `endpoint` contract suite.
  `jfl-composition`: the daemon host (accept loop, per-connection state, supervision, idle exit,
  SIGTERM, `setsid` through `rustix`), the launcher (spawn on demand through `jfl-process`'s
  detached mode, a spawn lock and sccache's notify-socket readiness handshake with a bounded wait),
  a version handshake, reconnect with re-sent documents. Model first: the `daemon` conformance
  driver over the in-process host with real sockets and locks in a tempdir; negative control:
  release the lock before removing the socket.
- **Reuse considered:** std file locks (Rust 1.89), `rustix`, Tokio's `UnixListener`, `lsp-server`'s
  message type; sccache's spawn handshake, Biome's `lsp-proxy` and buck2's version check copied as
  designs (ADR 0021).
- **Accept:**
  - [ ] `conformance-replay` green for `daemon`; the negative control fails at its step
  - [ ] integration tests: a second daemon on the same state dir exits with a clear error; modes are `0700`/`0600` (also under a non-root uid via `setpriv`, since the sandbox runs as root); an existing state dir with mode `0755` or another owner is refused; a peer with another uid is refused; two clients racing start one daemon
  - [ ] after a daemon crash a client reconnects and the new daemon holds no overlay of a dead connection
  - [ ] a `proptest-state-machine` test of the host's lock, socket and connection states against a reference machine (ADR 0007, amended; ver §6.4)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** protocol (endpoint); new authority (sockets).

### Lane LSP: M1, the LSP carrier

The first carrier (ADR 0034): the engine of lane IDX reaches agents through `jfl lsp` and the LSP
plugin; LSP-06 is M1's gate and makes the bench gate blocking.

#### LSP-01: LSP adapter on `lsp-server` and the LSP conformance harness
- **Depends on:** IDX-03. **Model:** deep. **Size:** L. **Parallel with:** IDX-09, IDX-10.
- **Ordering:** started before DEC-02 by the owner's delegation (2026-11-17); DEC-02 still gates work beyond the walking skeleton.
- **Cites:** plan §2.5, §6.2, §11; ver §2, §4, §8; ADR 0020, ADR 0021.
- `jfl-adapter-lsp` on `lsp-server` 0.10 and `gen-lsp-types` 0.11 (exact pin): the synchronous main
  loop (notifications applied in order, requests on a bounded pool against a snapshot), framing
  through `jfl-protocol`'s bounded decoder, request ids of both types, error codes, cancellation
  bookkeeping. An in-process harness on `Connection::memory()` (rust-analyzer's slow-tests pattern).
  The black-box LSP conformance level: a transcript runner (`fixtures/lsp/*.jsonl`, client lines
  sent, server lines expected with documented normalisation) driving a binary over stdio, and the
  `lsp-conformance` stage. First transcripts: framing errors, unknown method, notification without a
  response.
- **Reuse considered:** `lsp-server` + `gen-lsp-types` (ADR 0020); `tower-lsp-server` 0.24 the
  runner-up; `lsp-types`, `tower-lsp` and `async-lsp` rejected; `pytest-lsp` (Python in CI) not
  needed.
- **Decided (orchestrator, IDX-01 review):** the LSP server clamps a column past the line end to the
  line end, as LSP 3.17's `Position.character` requires (`ColumnPastLineEnd` carries
  `line_length`); the CLI refuses it (plan §2.3).
- **Decided (orchestrator, IDX-01 review):** echo the client's URI string as sent for a document it
  opened or named, not `FileUri`'s canonical form; a transcript test with `+` or `@` in a directory.
- **Accept:**
  - [ ] property tests: arbitrary byte streams never panic the framer and never allocate past the limit
  - [ ] transcripts for malformed headers, an oversized frame, an unknown request (`MethodNotFound`) and an unknown notification (ignored) pass
  - [ ] `jfl-adapter-lsp` depends only on `jfl-protocol` and `jfl-domain` (architecture policy)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** new adapter.

#### LSP-02: `jfl lsp`: lifecycle, encodings, document sync, deadlines
- **Depends on:** LSP-01, IDX-12. **Model:** deep. **Size:** L. **Parallel with:** IDX-11.
- **Cites:** plan §2.3, §2.5, §2.7, §4.4, §5.2; fnd §3.3.
- The `jfl lsp` proxy: `initialize` (capabilities only for what exists; `positionEncoding`
  negotiation; answered without waiting for indexing), `initialized`, `shutdown`, `exit` (0 after
  shutdown, 1 without), `$/cancelRequest` (`RequestCancelled`, or the result if it already
  finished), document sync into daemon overlays with versions, internal deadlines with partial
  answers, reconnect to a restarted daemon with re-sent open documents. Add the
  `ARCH-position-arithmetic` item to the review checklist (fnd §3.3). Transcripts for each.
- **Accept:**
  - [ ] transcripts: lifecycle, both exit codes, cancellation before and after completion, UTF-16 positions on a line with astral characters, `utf-8` when the client offers it
  - [ ] a `jfl-bench` suite records `initialize` latency against a warm daemon (ver §10); the 200 ms budget is reported, not asserted in a parallel test
  - [ ] after the daemon is killed mid-session the next request is answered by a respawned daemon with the open documents restored (e2e)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** LSP capabilities (lifecycle).

#### LSP-03: Navigation methods
- **Depends on:** LSP-02, IDX-11, DEC-06. **Model:** standard. **Size:** L. **Parallel with:** LSP-04.
- **Cites:** plan §2.2, §2.5; ADR 0014, ADR 0034.
- `textDocument/definition`, `references` (`referencesMode` with DEC-06's default; `exact` first),
  `hover` (status line), `documentSymbol`, `workspace/symbol`, `implementation`; the
  implementation query in the application (proved subtype edges `exact`, unresolved supertypes
  `possible`). Transcripts. (`jfl impls` moved to IDX-13 with the CLI, ADR 0034.)
- **Accept:**
  - [ ] a transcript per method over the fixture projects; `references` in both modes
  - [ ] hover's status line matches the protocol envelope the daemon returns for the same position (a test comparing the LSP mapping with the protocol answer)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** LSP capabilities.

#### LSP-04: Syntax diagnostics and the coordinator (diagnostics driver)
- **Depends on:** LSP-02, FORMAL-05. **Model:** deep. **Size:** L. **Parallel with:** LSP-03, LSP-05.
- **Cites:** plan §3.2, §7.3; ver §6; ADR 0007.
- The diagnostics coordinator in `jfl-application`: per document, a version, jobs started per
  version (syntax now, `javac` from SEM-05), results accepted only when not older than the newest
  published or pending version, publication through the proxy as `publishDiagnostics` with
  `version`; close publishes an empty set and nothing after. Model first: the `diagnostics`
  conformance driver with the testkit's scripted compiler; negative control: publish every result.
- **Reuse considered:** none fits: the version-tagged publication rule is ours and modelled (ADR
  0007).
- **Accept:**
  - [ ] `conformance-replay` green for `diagnostics`; the negative control fails at its step
  - [ ] a transcript with two quick edits whose results arrive in reverse order publishes only the newer
  - [ ] a `proptest-state-machine` test of the coordinator (open, edit, result, close in any order) against a reference machine (ADR 0007, amended; ver §6.4)
  - [ ] syntax errors in the corpus's `broken/` files are published with correct UTF-16 ranges
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** LSP capabilities (diagnostics).

#### LSP-05: LSP plugin and `.lsp.json`
- **Depends on:** LSP-03. **Model:** standard. **Size:** M. **Parallel with:** LSP-04.
- **Cites:** plan §2.6; ADR 0034.
- `plugin/lsp/` (the LSP-only plugin with `.lsp.json` using only the documented keys of plan §2.6;
  the CLI plugin of IDX-14 later carries the same entry); a test that checks `.lsp.json` against the
  documented key list; coexistence with `jdtls-lsp` investigated and
  written down in `doc/plugins.md` (which wins for `.java`, or that they cannot coexist).
- **Accept:**
  - [ ] the key-list test passes and fails on a fixture with an unknown key
  - [ ] `doc/plugins.md` and the plugin README state the minimum Claude Code version the keys need (`requestTimeout` needs v2.1.288 or later, re-checked against the current plugins reference)
  - [ ] (owner) `claude plugin validate --strict` passes for the LSP plugin
  - [ ] `doc/plugins.md` states the coexistence result and how it was checked
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** distribution.

#### LSP-06: ⛳ M1 gate suite and the blocking bench gate
- **Depends on:** LSP-03, LSP-04, LSP-05. **Model:** deep. **Size:** M. **Parallel with:** none.
- **Cites:** plan §1.7, §10.1; ver §7.2, §10, §11; ADR 0014, ADR 0017, ADR 0031, ADR 0034.
- The M1 gate as tests, through `jfl lsp` (ADR 0034): the envelope on 50 golden cases (protocol
  goldens, and their LSP mapping); the completeness mutation test adding a caller in each module
  of each multi-module fixture; 10 concurrent worktrees (linked, under `.claude/worktrees/`)
  queried in parallel with no cross-view result and no duplicate workspace symbol; freshness under
  shell edits (`git checkout`, `git stash`, `sed -i`, `rm`) answered correctly without `sync`; the
  LSP conformance suite; `initialize` latency from the benchmark; the M1 rows of plan §1.7
  measured on the corpus through `jfl lsp`, with the `jfl-bench` runners that need a product:
  freshness lag (edit a file, time until the next request sees it) and the marginal cost of 1, 3
  and 10 linked worktrees. `bench.yml`'s `bench-gate` becomes blocking: from here a tracked metric
  that regresses by more than 10 % on its fixed runner class fails the run on `main`, and a red
  `bench-gate` is fixed like red CI. It stays out of `ci.yml`, `check` and `land.sh` (fnd §9).
  `doc/eval/m1.md` lists the gate items and their tests.
- **Accept:**
  - [ ] every automatable gate item has a passing test named in `doc/eval/m1.md`
  - [ ] `bench-gate` blocks on `main` and fails a planted 20 % regression that reproduces; the M1 rows of plan §1.7 measured, with the reference-machine run (owner) where the sandbox differs
  - [ ] (owner) a real Claude Code smoke test with shell edits across 3 worktrees using the LSP plugin
  - [ ] (owner) the E0 task subset rerun with the LSP plugin; aggregates in `doc/eval/m1.md`
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none.

### Lane CLI: M2, the JSON CLI carrier

The second carrier (ADR 0034): the `jfl` subcommands with the full envelope, the CLI plugin with
`bin/` and the skill, `AGENTS.md` and cloud sessions. The cards keep their IDX ids from plan v3.1
so citations stay valid; IDX-15 is M2's gate.

#### IDX-13: `jfl` CLI: subcommands, output, `--json`, exit codes, `jfl impls`
- **Depends on:** LSP-06. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §2.3, §2.4; ver §2, §4; ADR 0034.
- `jfl-cli`: `def`, `refs`, `hover`, `sym`, `impls` (over LSP-03's implementation query), `status`, `sync`, `daemon` with `--json`,
  `--exact-only`, `--state-dir`; `file:line:col` parsing (1-based, byte columns); compact output
  grouped by file, `exact` first, capped with a count; exit codes of plan §2.4. `--json` goldens
  first (`insta` JSON snapshots with redactions; the JSON Schema generated by `schemars`, committed
  and checked). The `e2e-tests` stage joins the CI job LSP-01's `lsp-conformance` started.
- **Reuse considered:** `clap`, `schemars`, `insta`, `assert_cmd`, `trycmd`; LSAP's compact "Showing
  2 of 12" output as an idea.
- **Accept:**
  - [ ] e2e tests with `assert_cmd` over copies of the fixture projects for each subcommand and exit code
  - [ ] `--json` goldens for every subcommand; compact output goldens for `refs` with more hits than the cap
  - [ ] the first command against a stopped daemon spawns it and answers (e2e)
  - [ ] `jfl impls` e2e and golden; for the same position the `--json` envelope and the LSP carrier's answer agree (a test comparing carriers)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** CLI (v1); `--json` schema (v1).

#### IDX-14: CLI plugin, skill, `AGENTS.md` and cloud setup
- **Depends on:** IDX-13. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §2.4, §2.6; ADR 0034.
- `plugin/cli/` (the CLI plugin: manifest, `bin/jfl` launcher, LSP-05's `.lsp.json` entry, `skills/jfl/SKILL.md` telling an
  agent when to prefer `jfl refs --json` over grep and how to read `complete`), `AGENTS.md` at the
  root (for agents that use `jfl`: commands, the envelope, completeness, freshness and `sync`),
  and `doc/cloud-setup.md` with a setup-script snippet that installs a release binary for cloud
  sessions. Every behavioural claim in the skill and `AGENTS.md` names the test that pins it.
- **Reuse considered:** the documented Claude Code plugin schema and `claude plugin validate`;
  LSAP's agent-shaped output and Code-Index-MCP's `index_unavailable` honesty pattern as ideas.
- **Accept:**
  - [ ] the plugin directory validates with the Claude Code plugin schema (a test of the manifest's keys against the documented list)
  - [ ] every command in `AGENTS.md` and the skill runs in an e2e test
  - [ ] (owner) the CLI plugin installed in a local Claude Code session and `jfl refs` used by an agent; the skill installed in a cloud session through the setup script
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** distribution.

#### IDX-15: ⛳ M2 gate: the CLI carrier
- **Depends on:** IDX-14. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §1.7, §10.2; ver §10, §11; ADR 0014, ADR 0031, ADR 0034.
- The M2 gate as tests, written down in `doc/eval/m2.md`: the 50 golden envelope cases of LSP-06
  through `jfl … --json`, agreeing with the LSP carrier; every subcommand and exit code e2e; the
  CLI rows of plan §1.7 (`jfl def`, `hover`, `refs`, `sym` end to end) measured on the corpus and
  tracked by `bench-gate`, which has blocked since LSP-06.
- **Accept:**
  - [ ] every automatable gate item has a passing test named in `doc/eval/m2.md`
  - [ ] the CLI rows of plan §1.7 measured, with the reference-machine run (owner) where the sandbox differs
  - [ ] (owner) the E0 task subset rerun with the CLI plugin; aggregates in `doc/eval/m2.md`
  - [ ] (owner) the cloud-session check of IDX-14 done
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none.

### Lane PROJ: M3, project model and library APIs

#### PROJ-01: Gradle model: wrap build-server-for-gradle, opt-in, fingerprint cache
- **Depends on:** IDX-15. **Model:** deep. **Size:** L. **Parallel with:** PROJ-03.
- **Cites:** plan §5.7, §11; fnd §2.4; ADR 0015, ADR 0026.
- `ProjectModelSource` port; `java/gradle-model/`, a Tooling API client of about 200 lines that
  runs microsoft/build-server-for-gradle's `GetSourceSetsAction` (its plugin injected by an init
  script, release pinned) and prints the `GradleSourceSet`s as JSON (ADR 0026);
  `jfl-adapter-gradle` launches it through `jfl-process`: refuse without the opt-in of plan §5.7;
  map source sets (main and test separately), dependencies, classpaths, the processor path, JDK
  and `--release`; cache by build-file fingerprint in the store; run in the background, one run per
  fingerprint at a time, never on the request path. First check on the Lombok and MapStruct
  fixtures that `compilerArgs` carries `-processorpath` and `-s`; if not, send the small patch
  upstream and read them as IntelliJ's model builders do meanwhile. The `project_model` contract
  suite. Gradle and its wrapper are pinned for tests in `mise.toml`; tests run offline against
  fixture builds; the bench corpus measures model time.
- **Reuse considered:** microsoft/build-server-for-gradle wrapped (ADR 0026); BSP to its server as
  the fallback; our own helper over `EclipseProject`/`IdeaProject` rejected (misses per-source-set
  detail); kmp-lsp's and ktlsp's Gradle-cache readers as the no-model reference.
- **Accept:**
  - [ ] without the opt-in, no Gradle process starts (integration test watching `jfl-process`)
  - [ ] models of the multi-module and Lombok fixtures match their goldens; two worktrees with the same build files share one model
  - [ ] contract suite green for fake and adapter; the `compilerArgs` finding recorded (and an amendment of ADR 0026 if the mechanism changes)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** new adapter; config (opt-in key).

#### PROJ-02: Modules, visibility and `EnvironmentKey` in the envelope
- **Depends on:** PROJ-01. **Model:** deep. **Size:** M. **Parallel with:** PROJ-04.
- **Cites:** plan §2.1, §5.4, §10.3; ver §7.2.
- With a project model: per-module views of the index, module visibility (which modules can see
  which; main never sees test), `complete` over the modules that can see the target's module,
  `incomplete_because` naming unindexed modules and missing generated-source roots,
  `EnvironmentKey` per module. The completeness oracle extended to module visibility.
- **Decided (orchestrator, IDX-02 review):** `lombok.config` (for example `accessors.fluent`, which
  renames generated members) and `module-info.java` (cross-module visibility) are inputs no key
  covers yet: add their content digests to `EnvironmentKey`'s inputs here (or to the key of the
  card that consumes them), with a test that changing either changes the key.
- **Accept:**
  - [ ] the completeness oracle passes per module with visibility; a main source set never sees test-only types (test)
  - [ ] two worktrees with diverging classpaths get different `EnvironmentKey`s and do not share semantic results (test)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** `--json` schema (additive: module fields).

#### PROJ-03: Class-file, JAR and `lib/modules` reader on cafebabe and ristretto_jimage
- **Depends on:** IDX-15, FND-10. **Model:** deep. **Size:** L. **Parallel with:** PROJ-01.
- **Cites:** plan §5.8, §11; ver §4, §8; ADR 0023 (amended 2026-11-17).
- `LibraryIndexer` port; `jfl-adapter-classfile` on `cafebabe` (pinned to the git commit with its
  2026-11-13 hostile-input panic fix until a release carries it, with the per-crate source
  exception in `deny.toml` and FND-08's N4 source check, each with its reason and a comment that
  says it is temporary and names DEP-01, which removes it; the pin is the owner's, 2026-11-17),
  `ristretto_jimage` `=0.35.0` (`cargo add`, Rust 1.99.1 from FND-10) for the JDK
  runtime image `lib/modules` from day one, and `zip` 9: read JARs with caps
  on entry count, sizes and compression ratio, never extracting by entry name; parse class files
  into API summaries (types, members, generic signatures with our own JVMS §4.7.9.1 parser,
  annotations, module info); our own multi-release selection by `--release`. Fixture JARs built from
  committed sources by a test; malformed inputs committed for refusal tests. The `library_indexer`
  contract suite.
- **Reuse considered:** `cafebabe` (0BSD, forbid-unsafe), `ristretto_jimage` and `zip` (ADR 0023,
  amended); `ristretto_classfile` the runner-up; our own image reader (about 500 lines) a later
  option; kmp-lsp's JAR indexer and ktlsp's `jar.rs` read for layout.
- **Accept:**
  - [ ] API summaries of the fixture JARs match goldens; multi-release selection picks the right version
  - [ ] the pinned JDK's own classes (`java.lang.Object`, `java.util.List`) read from `lib/modules` (integration test)
  - [ ] property tests: arbitrary bytes as a class file, a zip or a jimage never panic and stay within bounds; a zip bomb is refused
  - [ ] contract suite green for fake and adapter
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** new adapter.

#### PROJ-04: JDK API index from `lib/modules` and `ct.sym`, the shared library index
- **Depends on:** PROJ-03. **Model:** standard. **Size:** M. **Parallel with:** PROJ-02.
- **Cites:** plan §5.4, §5.8; fnd §6; ADR 0023 (amended 2026-11-17).
- JDK API summaries for the JDK's own release from `$JAVA_HOME/lib/modules` (PROJ-03's reader),
  and for an older `--release` from `$JAVA_HOME/lib/ct.sym`, which stubs only earlier releases
  (Temurin 24 and later ship no `jmods/`), and a `LibraryStore` in
  `jfl-adapter-store` keyed by content checksum, shared by every view, versioned like the summary
  cache.
- **Reuse considered:** `ct.sym` read with `zip`; `lib/modules` through `ristretto_jimage` (ADR 0023,
  amended).
- **Accept:**
  - [ ] `java.util.List` from `lib/modules` and a `--release 8`-only API from `ct.sym` resolve from the JDK index of the pinned JDK (integration test)
  - [ ] two views using the same JAR share one stored summary (test)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** cache format (library index v1).

#### PROJ-05: Library types in hover and definition; M3 gate
- **Depends on:** PROJ-02, PROJ-04. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §10.3; ver §8.
- Hover and definition into library types (signature, Javadoc when a sources JAR exists, the
  library's coordinates); `doc/eval/m3.md` lists the M3 gate items with their tests: classpath
  divergence isolates, no test-to-main leaks, malformed-JAR property tests, library hover and
  definition.
- **Accept:**
  - [ ] every M3 gate item has a passing test named in `doc/eval/m3.md`
  - [ ] golden envelopes for hover and definition on library types
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** `--json` schema (additive).

### Lane SEM: M4, resolver and semantic diagnostics

#### SEM-01: Resolver core per DEC-04
- **Depends on:** PROJ-05, DEC-04. **Model:** deep. **Size:** L. **Parallel with:** SEM-04.
- **Cites:** plan §5.4, §5.6, §6.1, §10.4; ADR 0014; DEC-04's ADR.
- The resolver DEC-04 chose (by default `jfl-resolve`, a pure crate: scopes, imports, type
  hierarchy, member lookup, overload selection, expression types for receivers on fields,
  parameters, locals and chains) over summaries and library APIs, at a pinned generation. A new
  crate's `members`, policy and edges in this commit. Results carry their evidence. If DEC-04 picks
  the javac-attributed arm, this task consumes SEM-04's worker and lands after it; the orchestrator
  re-orders the two.
- **Reuse considered:** whatever DEC-04 picks from E2's reused candidates (`ktcore`, `kmp-lsp`,
  scip-java's plugin); `salsa` only if invalidation proves hard (exact pin, behind our own trait).
- **Accept:**
  - [ ] the resolution corpus's static-call, field, parameter, chain, overload, inheritance and generics cases resolve as expected
  - [ ] an unsupported construct yields `possible` with its reason, never a guess (tests per construct)
  - [ ] the reads a resolution records meet plan §5.4's conditions: a new same-package type, a new type in an on-demand-imported package that collides with a resolved name, and an added Lombok annotation on a read declaration each change the dependent's `SemanticKey` (tests)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** dependency graph (new crate if our own).

#### SEM-02: Resolver: inferred receivers and generated members
- **Depends on:** SEM-01. **Model:** deep. **Size:** L. **Parallel with:** SEM-05.
- **Cites:** plan §3.3, §5.6; ver §7.1.
- `var`, lambdas (target typing), streams, method references, and members generated by Lombok and
  MapStruct (from the processors in the project model, marked as generated); what stays unproved
  falls to `possible` with its reason.
- **Accept:**
  - [ ] the corpus's `var`, lambda, stream, method-reference and Lombok cases resolve or fall to `possible` exactly as their expected files say
  - [ ] the per-construct coverage on the corpus is reported by `jfl-oracle` and committed in `doc/eval/sem-coverage.md`
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none (internal).

#### SEM-03: Exact answers in the envelope; envelope-honesty mutants
- **Depends on:** SEM-02. **Model:** deep. **Size:** M. **Parallel with:** SEM-05.
- **Cites:** plan §2.1, §2.2; ver §7.3; ADR 0014.
- Queries use resolver evidence for `exact` (definition, references, implementation edges, hover
  status); the curated envelope-honesty mutants of ver §7.3 (flip a scoping rule, drop an import,
  treat an unindexed module as indexed, ignore a deadline), each with the named test that must
  fail, run in `integration-tests`.
- **Accept:**
  - [ ] each curated mutant, applied, makes its named test fail; unapplied, the suite is green
  - [ ] the completeness oracle still passes with the resolver on
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none (internal).

#### SEM-04: `javac` worker (our own, scip-java plugin) and pool
- **Depends on:** PROJ-02, DEC-05. **Model:** deep. **Size:** L. **Parallel with:** SEM-01.
- **Cites:** plan §3.1, §3.3, §3.4, §5.4, §14; ver §5, §6.4; ADR 0036.
- `java/javac-worker/` hardened from EVAL-10's spike, our own code (one javac context kept warm,
  the fresh-compiler fallback, body blanking of dependency sources if EVAL-10 kept it, class
  folders from earlier builds, `-proc:none -implicit:none` with processors opt-in per repository
  and measured, `-sourcepath`, scip-java's plugin for attribution, length-prefixed JSON over stdin
  and stdout, cancellation), tested on JDK 17, 21 and 25 in CI, and `Compiler`
  port; `jfl-adapter-javac`: a pool per `EnvironmentKey` through `jfl-process`'s long-lived mode,
  lazy spawn, idle eviction, a cap with LRU eviction, restart on crash with backoff, version-tagged
  jobs. An ADR (reserved number) decides how the worker is built and launched (source-file launch or
  a prebuilt jar). The `compiler` contract suite.
- **Reuse considered:** our own worker and scip-java's plugin (ADR 0036); ECJ kept only as E3's
  comparison arm.
- **Accept:**
  - [ ] contract suite green for the scripted fake and the real worker (pinned JDK); a broken fake fails it
  - [ ] killing a worker mid-compile restarts it and the next job succeeds (integration test)
  - [ ] the pool never exceeds its cap (test on fake clock); a `proptest-state-machine` test of the pool (spawn, evict, crash, restart) against a reference machine
  - [ ] the worker's tests pass on JDK 17, 21 and 25, the forced reuse failure falling back to a fresh compiler
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** new adapter; Java source in the repository.

#### SEM-05: Semantic diagnostics, processors and `jfl diag`
- **Depends on:** SEM-04, LSP-04. **Model:** deep. **Size:** L. **Parallel with:** SEM-02, SEM-03, SEM-08.
- **Cites:** plan §3.2, §3.3, §7.3; ADR 0015, ADR 0032 (amended 2026-11-17).
- `javac` results through the diagnostics coordinator (the model and driver of LSP-04 cover them;
  extend the driver if the coordinator's inputs change), generated-source roots added, errors on
  processor-generated members suppressed and marked in hover, no semantic diagnostics without a
  classpath (and `status` says why), `jfl diag <file>` waiting for the current version with a
  deadline.
- **Reuse considered:** scip-java's plugin and our own worker (ADR 0036); IntelliJ's processor
  discovery as an idea.
- **Accept:**
  - [ ] the Lombok and MapStruct fixtures produce zero errors with generated sources missing; a real type error in the same files is reported
  - [ ] on the fineract corpus (BENCH-06), semantic diagnostics report no error on a Lombok-generated or MapStruct-generated member with generated sources missing (a `bench.yml` or `(owner)` run; counts only in the report)
  - [ ] `conformance-replay` still green for `diagnostics`
  - [ ] `jfl diag` e2e and golden, including its deadline path
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** LSP capabilities; CLI.

#### SEM-06: SCIP differential gates
- **Depends on:** SEM-03, EVAL-08. **Model:** deep. **Size:** M. **Parallel with:** none.
- **Cites:** plan §10.4; ver §7.1.
- Wire `jfl-oracle` (with `scip-java` as the oracle) to the built `jfl` over the golden corpus in
  the gate and over the real-corpus cases (BENCH-05) in `bench.yml` (precision, coverage per
  construct, envelope recall), with thresholds from DEC-04 for the corpus, and an owner runbook
  for the target repository (`doc/eval/sem-gates.md`).
- **Accept:**
  - [ ] the corpus differential runs in `integration-tests` with precision ≥ 99.5 % and zero recall misses when `complete=true`; a planted wrong `exact` fails it
  - [ ] (owner) the target-repository run; aggregates in `doc/eval/sem-gates.md`
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none.

#### SEM-08: SCIP baseline import (M6 folded in)
- **Depends on:** SEM-06. **Model:** deep. **Size:** M. **Parallel with:** SEM-05.
- **Cites:** plan §10.4, §10.6; ver §7.1; ADR 0009, ADR 0036.
- If DEC-04 keeps it: import a SCIP index produced by `scip-java` outside the request path as a
  per-commit baseline the resolver falls back on, with its staleness against the view tracked per
  file; `exact` evidence names the baseline; `status` shows its commit and coverage. If DEC-04
  drops it, this card is marked dropped here and SEM-07 no longer depends on it.
- **Reuse considered:** the `scip` crate 0.10 (its `protobuf` =3.7.2 pin; types generated with
  `prost` from `scip.proto` are the runner-up if the pin conflicts).
- **Accept:**
  - [ ] a baseline on the fixture projects raises coverage on the constructs DEC-04 named, with precision unchanged (oracle run)
  - [ ] a file changed since the baseline's commit never gets an `exact` answer from it (test)
  - [ ] goldens with baseline evidence; the completeness oracle and the SCIP differential pass with the baseline on
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** cache format (baseline); `--json` schema (additive).

#### SEM-07: ⛳ M4 gate
- **Depends on:** SEM-05, SEM-06, SEM-08. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §10.4; ver §7, §11.
- `doc/eval/m4.md` with each gate item and its evidence (SEM-08's baseline included if DEC-04 kept
  it): precision, coverage per construct,
  envelope recall, zero false errors on a clean checkout, diagnostics p95 within E3's budget.
- **Accept:**
  - [ ] every automatable item has a passing test or committed measurement named in `doc/eval/m4.md`
  - [ ] (owner) precision, coverage and recall on the target repository meet plan §10.4
  - [ ] (owner) zero false errors on a clean checkout of the target repository; p95 within budget
  - [ ] (owner) the E0 task subset rerun; aggregates in `doc/eval/m4.md`
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none.

### Lane REL: M5, call hierarchy, hardening and release

#### REL-01: Call hierarchy and `jfl calls`
- **Depends on:** SEM-07. **Model:** standard. **Size:** L. **Parallel with:** REL-03.
- **Cites:** plan §2.2, §2.4, §2.5.
- Incoming and outgoing calls from call sites with virtual-dispatch targets listed and marked;
  `prepareCallHierarchy`, `incomingCalls`, `outgoingCalls`; `jfl calls`. Transcripts and goldens.
- **Accept:**
  - [ ] transcripts for the three methods; dispatch marking on an interface with two implementations
  - [ ] `jfl calls` e2e and golden; the completeness oracle passes for incoming calls
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** LSP capabilities; CLI.

#### REL-02: Resource limits and watcher recovery
- **Depends on:** SEM-07. **Model:** deep. **Size:** L. **Parallel with:** REL-03.
- **Cites:** plan §5.9, §12; fnd §4.4, §5.
- Every limit of plan §5.9 as a named constant with its reason, measured values from benchmarks,
  counted refusals and backpressure shown in `jfl status`; watcher overflow and loss trigger a
  rescan (move to `notify` 9 if final, for its rescan events and `update_paths`); automate
  `ARCH-log-event-name` in the architecture test's source scan (fnd §3.3).
- **Reuse considered:** `notify` 9's rescan events; turborepo's cookie file; `dylint` only if the
  log-name rule needs name resolution.
- **Accept:**
  - [ ] a test per limit showing the refusal or backpressure and its `status` field
  - [ ] a simulated watcher overflow leads to a rescan and correct answers (integration test)
  - [ ] `ARCH-log-event-name` fails its negative fixture
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** config (limits); `--json` schema (additive status fields).

#### REL-03: Fuzz lane
- **Depends on:** SEM-07. **Model:** standard. **Size:** M. **Parallel with:** REL-01, REL-02.
- **Cites:** ver §8; fnd §7.1, §8.
- `fuzz/` (own workspace, `jfl-fuzz`, the nightly and cargo-fuzz pinned by mise), targets of ver
  §8, seed corpora from the fixtures and the bench corpus, `just fuzz`, `fuzz.yml` (nightly),
  `ARCH-fuzz-crate` rows.
- **Reuse considered:** cargo-fuzz with `arbitrary`; afl.rs weighed; Kani, loom or shuttle only
  where a component shows the need.
- **Accept:**
  - [ ] each target runs for its budget locally without a crash; a planted panic is found and reproduced with `--reproduce`
  - [ ] `fuzz.yml` green on dispatch
- **Impact:** none (tooling).

#### REL-04: Crash and restart soak
- **Depends on:** REL-02. **Model:** deep. **Size:** M. **Parallel with:** REL-05.
- **Cites:** plan §5.2, §7.2; ver §6, §12.
- A soak harness (`just soak`, owner-run for long durations, a short budget in CI): 10
  worktrees, proxies issuing queries and edits, the daemon and javac workers killed at random
  (seeded), checking that every answer is consistent with the files at its generation, no ghost
  buffers, no leaked processes, sockets or locks.
- **Reuse considered:** `proptest-state-machine` and shuttle weighed for the soak's scheduling; the
  soak drives real processes, so it is ours.
- **Accept:**
  - [ ] the short soak passes in the gate's budget; a planted ghost-buffer defect fails it
  - [ ] (owner) a long soak (hours) on the owner's machine with its report in `doc/eval/soak.md`
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none.

#### REL-05: Full mutation lane and budget ratchet
- **Depends on:** REL-02. **Model:** standard. **Size:** M. **Parallel with:** REL-04.
- **Cites:** ver §9, §10; fnd §7.1; plan §1.7; ADR 0031.
- `just mutation` (cargo-mutants over the scope of ver §9, sharded), `mutation.yml` (weekly), the
  first survivor triage in `doc/mutation-baseline.md`; the benchmarks have run since BENCH-02, so
  this task re-measures the plan §1.7 budgets on the reference machine, ratchets the ones measured
  better (owner approves) and writes `doc/performance.md`.
- **Reuse considered:** cargo-mutants (`--shard`); the BENCH harness and history.
- **Accept:**
  - [ ] a mutation run completes and its survivors are triaged
  - [ ] `doc/performance.md` records every plan §1.7 row with its measurement, machine and corpus
- **Impact:** none (tooling).

#### REL-06: ⛳ Release workflow and release preview
- **Depends on:** REL-01, REL-03, REL-04, REL-05. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §2.6, §10.5; fnd §7.1, §7.4; ADR 0030.
- dist (pinned by mise) generates `release.yml` (tag `v*` or dispatch with dry run): archives for
  the three targets of ADR 0006, `SHA256SUMS`, a CycloneDX SBOM (dist's support confirmed, or
  `cargo-cyclonedx`), GitHub artifact attestations, `cargo-auditable`, third-party notices, a draft
  release; release-plz prepares versions and `CHANGELOG.md` and tags; `just release-check` (package,
  extract, smoke the archive with no Java or Node on `PATH`); `doc/release.md`; `CHANGELOG.md` for
  the preview.
- **Reuse considered:** dist and release-plz (with git-cliff and cargo-semver-checks inside);
  `cargo-release` the manual alternative.
- **Accept:**
  - [ ] a dry-run release on dispatch produces every artefact and passes `release-check` on each target
  - [ ] (owner) the final E0 run at 10 concurrent worktrees against both baselines; aggregates in `doc/eval/m5.md`
  - [ ] (owner) tag pushed and the draft release published
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** distribution.

### Lane DEP: pinned-dependency follow-ups

Cards that undo a temporary pin once upstream releases what we need. The orchestrator checks
pinned git dependencies for a release at every ⛳ checkpoint ([`sdlc.md`](sdlc.md) §6) and
dispatches the card when its trigger holds.

#### DEP-01: Switch cafebabe from the git pin to the first release with the 2026-11-13 hostile-input fix
- **Depends on:** PROJ-03. **Trigger:** a `cafebabe` release newer than 0.9.x on crates.io whose
  source contains the commit PROJ-03 pinned (the 2026-11-13 hostile-input panic fix). **Model:**
  light. **Size:** S. **Parallel with:** any.
- **Cites:** fnd §3.3 (N4), §7.4; ADR 0023 (amended 2026-11-17).
- The owner's direction (2026-11-17): leave the git pin as soon as a release carries the fix.
  `cargo add cafebabe@<that release>` in `jfl-adapter-classfile`, replacing the git source;
  remove the per-crate git-source exception (and its DEP-01 comment) from `deny.toml` and from
  FND-08's N4 source check; the commit names the release and how it was checked to contain the
  fix.
- **Reuse considered:** none needed: a source change.
- **Accept:**
  - [ ] `jfl-adapter-classfile`'s `Cargo.toml` names a crates.io version of `cafebabe`, and `Cargo.lock` has no git source for it
  - [ ] the `deny.toml` git-source exception for `cafebabe` and its N4 counterpart are removed; `cargo deny check sources` and the architecture stage pass without them
  - [ ] the class-file fuzz target and the hostile-input property tests of PROJ-03 still pass, the input that triggered the 2026-11-13 panic among them
- **Impact:** dependency graph.
