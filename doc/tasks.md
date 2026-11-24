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
| BOOT-05 | Record the owner's decisions of 2026-11-24: retire seven ADRs, IDX-10's speed sign-off | IDX-10 | light | S |  | IDX-12 |
| FND-10 | Toolchain 1.99.1 | FND-08 | standard | S |  | XT-01 |
| FND-11 | Remove the private-names hashes and check | FND-04, XT-02 | standard | S |  | EVAL lane |
| FND-12 | Delete the jdtls proxy; keep stock jdtls as a bench arm | FND-11, BENCH-03 | standard | M |  | EVAL lane |
| FND-13 | Make CI green again on main | XT-02 | standard | M |  | FORMAL-05 |
| FND-14 | Keep the queue-full reason when hints fold into a pending rescan | FND-13 | standard | S |  | — |
| FND-15 | Accept a rescan in place of an awaited hint, never a silent miss | FND-14 | standard | M |  | — |
| FND-16 | Two daemon races from CI: a peer gone before its accept, an exit read before it happened | FND-15, IDX-12 | standard | M |  | — |
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
| FORMAL-02 | Conformance harness: ITF drivers, committed-trace replay, negative controls | FORMAL-01 | deep | M |  | EVAL-07, EVAL-05 |
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
| IDX-09d | Settle a view in slices: the daemon's loop never held for the whole view | IDX-09, IDX-12, LSP-02 | deep | L |  | LSP-04 |
| IDX-09e | A rescan with deferred files, in slices (blocks M1's bench gate) | IDX-09d | deep | M |  | LSP-03 |
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
| DEP-02 | Bump similar to 3.2.1 (Dependabot pzmi/jfastlsp#1, re-made as a task commit) | FORMAL-02 | light | S |  | any |
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

#### BOOT-05: Record the owner's decisions of 2026-11-24
- **Depends on:** IDX-10. **Model:** light. **Size:** S. **Parallel with:** IDX-12.
- **Cites:** [`adr/README.md`](adr/README.md); plan §1.3, §5.5, §5.8, §9.2, §16; fnd §3.6, §10.3,
  §11; ver §11; [`sdlc.md`](sdlc.md) §5; ADR 0031, ADR 0038.
- The owner decided BOOT-04's ADR audit, option a: retire the detail records now, and consolidate
  the others into a new superseding record only when each decision next changes. Before each
  record is retired, its target section is checked to say what the record decided, and completed
  where it did not. A new status, `Retired YYYY-MM-DD (owner): detail lives in <doc §>`, which
  `scripts/adr-index.sh` checks (its pin moved, fnd §3.6). ADR 0010, also proposed for retirement,
  is superseded instead by IDX-12's new record. ADR 0038 stands as written. IDX-10's slower
  definition is accepted.
- **Accept:**
  - [x] 0018 retired into fnd §11.6 (new: the sandbox, "agents never push", `land.sh` the only pusher) and sdlc §5
  - [x] 0019 retired into plan §16 (the rule, the recorded reason, the licence rule, the survey's findings; the debouncer row corrected)
  - [x] 0020 retired into plan §16.2 (new: the LSP stack) and §5.3
  - [x] 0023 retired into plan §5.8 (new: the cafebabe pin and DEP-01, our parsers, the runner-ups) and §16
  - [x] 0025 retired into plan §5.5 (new: the storage crates, `fst` narrowed by IDX-08 and IDX-10) and §16.3 (`redb`, `rkyv`, `cacache`)
  - [x] 0028 retired into plan §1.3 and the BENCH-03 card (new: the pinned stock jdtls and its launcher) and EVAL-06
  - [x] 0029 retired into ver §11 (new: the stream-json fallback, `plugin eval`'s limits, owner-side only, the committed control set) and plan §9.2
  - [x] `just adr-index` accepts a well-formed `Retired` status and refuses a malformed one or a row that differs (`land_scripts.rs`: `should_pass_the_adr_index_check_when_a_record_is_retired_into_a_named_document`, `should_fail_the_adr_index_check_when_a_retired_status_is_malformed_or_its_row_differs`)
  - [x] `doc/adr/README.md` and fnd §10.3 say what `Retired` means; the "an ADR only for an important decision" rule kept
  - [x] IDX-10's speed sign-off recorded on its card and in `doc/bench/README.md`; the tree cache stays a named item
- **Decided:** 2026-11-24 (owner).
- **Impact:** none (docs, the ADR index check).

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

#### FND-13: Make CI green again on main
- **Depends on:** XT-02. **Model:** standard. **Size:** M. **Parallel with:** FORMAL-05.
- **Cites:** fnd §7.2; ver §5; plan §4.3; [`required-checks.md`](required-checks.md).
- CI on `main` has been red since XT-01. `dco` and `security` (and the weekly `security.yml`'s
  `secrets` and `zizmor-online`) die while `just` parses: the justfile builds xtask at parse time
  (XT-01) and the launcher needs the pinned toolchain installed (XT-02), which those jobs never
  installed; they get the toolchain step, and `ARCH-ci-lockstep` now fails a job whose first `just`
  precedes it. The `macos` job fails five tests that assumed Linux: `tool_config`'s per-case trees,
  named after files that differ only in letter case (XT-01), now named by index; the fake `quint`'s
  GNU-only `sed -i` (FORMAL-02), now one portable `sed`; the git and fs contract fixtures' file
  whose name is not UTF-8, which APFS refuses (IDX-05, IDX-06): a fixture now reports the refusal
  (`contracts::NonUtf8Name`) and the contract accepts it only where `PLATFORM_NON_UTF8_NAME` says
  the platform refuses such names; `watch_leaves_out_agent_worktrees` stops on a rescan too, which
  covers the path (IDX-06); and, a product bug, the recursive FSEvents watch never reported an
  unreadable directory (IDX-06): it now walks the accepted directories at the start, as the
  per-directory watch does.
- **Reuse considered:** none needed: fixes.
- **Accept:**
  - [ ] `ARCH-ci-lockstep` fails a job that runs `just` without, or before, `rustup toolchain install` (`ci_lockstep.rs`)
  - [ ] each adapter's real fixture writes or refuses a non-UTF-8 name exactly as `PLATFORM_NON_UTF8_NAME` says (`should_write_or_refuse_a_non_utf8_name_as_the_platform_does_when_the_fixture_makes_one`), and a refusal on Linux fails the case (`contracts::tests`)
  - [ ] `just check` green; every `ci.yml` job, `macos` included, green on the landed commit
- **Impact:** CI (two jobs gain the toolchain step; one lockstep rule); tests (macOS portability);
  `jfl-adapter-fs` (the recursive watch reports an unreadable directory).

#### FND-14: Keep the queue-full reason when hints fold into a pending rescan
- **Depends on:** FND-13. **Model:** standard. **Size:** S. **Parallel with:** none.
- **Cites:** plan §4.3; fnd §4.4 rule 5; ver §5.
- The last red `macos` case after FND-13: `watch_reports_queue_full` saw one rescan with only
  `Unnamed` and 34 paths dropped. FSEvents' event on the root made the rescan first; every path
  after it folded into that rescan, so the queue never filled and `QueueFull` was never raised,
  though far more paths changed than the queue holds (IDX-06). The queue now counts the distinct
  paths that fold into a waiting rescan as it would have held them, and more than its capacity
  adds `QueueFull`. Every other reason already folded into a waiting rescan (`State::rescan`
  inserts it); a test now pins each order.
- **Reuse considered:** none needed: a fix.
- **Accept:**
  - [ ] `hints::tests::should_add_queue_full_when_more_paths_than_the_queue_holds_fold_into_a_pending_rescan` fails before the fix and passes after; paths that fit, or repeat, add nothing (`should_not_add_queue_full_when_paths_that_fold_into_a_pending_rescan_fit_the_queue`)
  - [ ] every reason survives a later one (`should_keep_every_reason_when_any_reason_folds_into_a_pending_rescan`)
  - [ ] `just check` green; the `macos` job green on the landed commit
- **Impact:** `jfl-application` (the hint queue's reasons).

#### FND-15: Accept a rescan in place of an awaited hint, never a silent miss
- **Depends on:** FND-14. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** ADR 0037; plan §4.3; ver §5.
- The `macos` nightly of 265bcb4 (green the three nights before): `cookie_follows_earlier_changes`
  saw one rescan, `Unnamed`, 6 paths dropped, and no cookie hint until `HINT_DEADLINE` (20 s)
  ran out. The race: FSEvents may deliver an event on the root itself as the stream starts (the
  fixture made the root just before); the root has no `RelPath`, so the adapter raises an
  `Unnamed` rescan, and every path pushed before the consumer takes it folds into it (IDX-06),
  the cookie's create and remove included. The cookie is then never hinted on its own: the
  outcome depends on whether the consumer takes the rescan before the cookie's events arrive.
  Nothing in the adapter can prevent it (notify already starts the stream with latency 0 and
  `NoDefer`; `FSEventStreamFlushSync` orders delivery but the fold happens behind it, and
  dropping the root's events would hide a real change of the root), and any rescan waiting when
  a cookie is written does the same on every platform. The rescan is a legitimate outcome: the
  view service takes it as a loss and the queued cookie as lost, at once, so the answer is
  incomplete, never wrong. The contract now accepts, after the cookie is written, its hint (after
  the earlier change's) or a rescan, never neither; `watch_hints_a_write`, which waited for an
  exact hint, accepts a rescan as every other watch case already did, and so does the adapter's
  `should_not_hint_a_change_when_it_is_behind_a_link_out_of_the_root`, the one other wait for an
  exact hint that runs on macOS. So that the suite still proves hints and cookies work, a new strict
  case, `cookie_follows_earlier_changes_when_settled`, first settles the watch: a sentinel written
  after the watch starts is hinted, or a rescan it folded into is taken. A backend delivers in
  order (FSEvents by event id within one stream), so the root's start-up event is taken by then.
  After that the case requires the change's and the cookie's hints by name, in order, with no
  rescan at all: a few paths cannot overflow a roomy queue, and excusing any reason would let a
  watch that only ever reports that reason pass.
- **Reuse considered:** none needed: a fix.
- **Accept:**
  - [ ] the fake with an `Unnamed` rescan from the start of the watch (`event_on_root`, FSEvents' shape) fails the old contract in `cookie_follows_earlier_changes` and `watch_hints_a_write` and passes the new (`should_pass_the_source_tree_contract_when_a_rescan_waits_from_the_start_of_the_watch`)
  - [ ] a watch that drops cookies without a rescan fails both cookie cases (`should_fail_the_cookie_cases_when_a_cookie_is_dropped_without_a_rescan`)
  - [ ] a watch that turns every hint into a rescan fails exactly `cookie_follows_earlier_changes_when_settled` (`should_fail_the_settled_cookie_case_when_every_hint_is_a_rescan`); the fake with the root's start-up rescan passes it
  - [ ] a cookie folded into a waiting rescan is never handed out after it (`hints::tests::should_never_hand_out_a_cookie_when_it_folds_into_a_pending_rescan`)
  - [ ] a rescan in place of the cookie loses it at once, without waiting out a deadline, and the answer is incomplete; the next request is complete (`should_answer_incomplete_when_a_rescan_comes_in_place_of_the_cookie`)
  - [ ] `just check` green; the `macos` nightly green on the landed commit
- **Impact:** tests (the `source_tree` contract's two watch cases, one new strict case, the fake's
  root event, one adapter watch test); docs of the cookie port.

#### FND-16: Two daemon races from CI: a peer gone before its accept, an exit read before it happened
- **Depends on:** FND-15, IDX-12. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §5.2, §5.3; ver §2, §3; ADR 0007.
- **Race 1, a product bug on macOS** (the `macos` nightly of c03aa52, the first with IDX-12's
  daemon driver): the regression trace `daemon-two-proxies-spawn-at-once` diverged at step 16,
  `accept` of `c1` refused with `ENOTCONN` (os error 57). The interleaving: `d1` listens (step
  10); `p2`'s second check under the spawn lock (`spawn`, step 13) finds it with
  `client::listening`, which connects and closes at once, leaving a closed connection first in
  `d1`'s backlog; `p1` and `p2` connect (14, 15); `d1`'s accept of `c1` takes the probe first,
  and `Listener::accept` asks for its peer's credentials. Tokio's `peer_cred` on macOS reads
  `LOCAL_PEEREPID` before `getpeereid`, and XNU answers it from the live peer, so a peer already
  gone is `ENOTCONN`; the `?` made it the listener's error. On Linux `SO_PEERCRED` keeps the
  credentials past the peer's close, so the probe is served, reads its EOF and is dropped, as the
  host documents. Beyond the driver, the daemon's loop took the error for a failed accept and
  stopped accepting until its next tick (up to 2 s) after any client that connected and left
  before being accepted. The fix: a peer whose credentials cannot be read is closed unserved and
  the next one accepted (`Listener::accept`); only the kernel's accept is the listener's error.
  The design does not change (plan §5.3 says it now): the probe is no connection of the model and
  the peer check is not modelled (`doc/formal-models.md`), so `daemon.qnt` and its driver stay.
- **Race 2, a test race** (linux-arm, PR run 45355597699):
  `should_keep_serving_others_and_sigterm_when_a_client_stops_reading` read the daemon's exit
  (`try_wait`) as soon as its socket was gone and its lock free. The daemon's shutdown order is
  stop accepting, unlink, release, then return from `run` (the runtime and service dropped) and
  exit, so between the release and the exit the test read `None`. The test now waits for the exit
  itself (`Daemons::wait_exit`, testkit `poll`) and then checks that the socket is gone and the
  lock free, which the order guarantees by then. The daemon's own shutdown was right.
- **Reuse considered:** none needed: fixes.
- **Accept:**
  - [ ] a peer whose credentials fail as XNU fails them for a gone peer is skipped and the next peer served; with the old `?` the accept fails with `NotConnected`, every time (`server::tests::should_accept_the_next_peer_when_the_credentials_of_a_gone_peer_fail`); against the real kernel, a peer closed before its accept never fails the accept (`tests/endpoint.rs::should_serve_the_next_peer_when_one_closed_before_its_accept`, the one that fails on macOS before the fix)
  - [ ] a stopped daemon whose socket is gone and lock file replaced has not exited (the old test's read is `None` every time), and the wait on the exit sees it exit 0 (`should_not_have_exited_when_its_socket_is_gone_and_its_lock_free_while_it_runs`); the SIGTERM test waits on the exit
  - [ ] the affected tests repeated 50 times or more without a failure; `just check` green
  - [ ] the `macos` nightly green on the landed commit (owner-run, not reproducible here)
- **Impact:** `jfl-adapter-ipc` (a gone peer never fails the accept); tests (the SIGTERM test's
  wait, two new IPC tests, one new daemon test); plan §5.3; `doc/formal-models.md`.

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
- **Follow-up (FND-13):** since XT-01 every `just` call builds xtask while the justfile parses
  (`mise_bins`' `shell()`), so a recipe that runs no xtask (`dco-event`, `workflows-verify`) still
  needs the toolchain and a build. just's `set lazy` skips an unused variable but still evaluates
  an `export`ed one, so avoiding the build means moving the `PATH` export into the recipes that run
  pinned tools: not cheap, and once this card and XT-03 move DCO and the security stages into xtask
  those jobs run xtask anyway. Not worth doing before XT-08 removes the justfile.
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
- **The jdtls arm, as it stands** (ADR 0028 recorded the proxy and its removal; the owner retired
  it into plan §1.3 and this card on 2026-11-24): stock jdtls is a comparison arm and the
  kill-criterion baseline only, never part of anything jfastlsp ships, and nothing of ours sits
  between the client and it. The pinned release is 1.61.0, the newest that runs on Java 21,
  fetched and checksum-verified by `scripts/jdtls-fetch.sh`; `jfl-bench`'s own launcher
  (`tools/jfl-bench/src/jdtls.rs`) starts it with a fresh `-data` and configuration per run, a
  heap cap, argv only, a cleared environment and a deadline. jdtls is EPL-2.0, so it only ever
  runs as a separate process, never linked or bundled. E4 measures it alone (EVAL-06); E0's
  jdtls arm is the stock `jdtls-lsp` plugin (EVAL-03).
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

#### FORMAL-02: Conformance harness: ITF drivers, committed-trace replay, negative controls
- **Depends on:** FORMAL-01. **Model:** deep. **Size:** M. **Parallel with:** EVAL-07, EVAL-05.
- **Cites:** ver §6.2; plan §7; ADR 0007, ADR 0035 (which superseded ADR 0030).
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
- **Adapted 2026-11-17 (FORMAL-02), the card predating XT-01 and ADR 0035, and quint-connect not
  fitting:**
  - **quint-connect 0.1.2 is not taken** (ADR 0007, amended; ADR 0019, amended): its `Step` cannot
    be built outside the crate, so committed traces cannot go through its `Driver`; it runs
    `quint run` on Quint's default `rust` backend, which downloads an evaluator at run time; its
    seed and verbosity are compile-time `option_env!`s; its divergence report names no step; and
    it depends on `anyhow`, wrapped away from libraries in deny.toml. `jfl-conformance` holds the
    harness over `itf` 0.4 instead (with `serde`, `serde_json`, `toml` and `similar`): a `Driver`
    of quint-connect's shape, the ITF reading (anything but a trace with Quint's `--mbt`
    metadata is `HARNESS_ERROR`), the replay with its divergence report, the regression manifest
    reader, the driver registry (each registered model has a driver with its negative control,
    or is pending with the task that brings it) and a binary, `jfl-conformance replay [--model
    M] [--seed S --generated DIR]` (exit 0, 1 divergence, 2 usage, 3 harness error). `deny.toml`
    admits the build scripts of `dashu-int` and `rustversion` (itf's) by checksum.
  - **The stages are xtask's** (`tools/xtask/src/conformance.rs`), rows of the catalog after
    `formal-test` (fnd §9's order), each with its one-line just delegation; `conformance
    [MODEL|all] [SEED] [TRACES] [STEPS]` is an xtask command and a recipe, not a stage. xtask
    generates (it holds the process grant; `jfl-conformance` does not) and runs the binary with
    cargo. `formal-freshness` regenerates each trace of `formal/traces/regression/manifest.toml`
    (read in a pinned line shape by xtask and with a TOML parser by `jfl-conformance`; every
    registered model has a trace there, plan §7) and compares outside the top-level `#meta`. `conformance-replay` draws a fresh seed, prints it
    with the `just conformance` line that repeats the run, generates 10 traces of 25 steps per
    model twice and fails unless they are equal (the seed reproduces the run), then replays them
    and the committed traces, and fails unless each driver's negative control diverges on the
    generated ones. Traces live in a private scratch directory, kept and named on failure.
  - **CI:** only the `formal` job of ci.yml (the one with Node and Quint) runs the two stages;
    ARCH-ci-lockstep's inputs (ci.yml, doc/required-checks.md) and the frozen catalog gained rows
    only. `formal.yml` is unchanged: every ci.yml run is a fresh-seed run.
  - **The smoke driver** drives a test-only `Counter`; its control skips 3 and fails at step 3 of
    the committed trace `smoke-counts-up` (0 to 6, seed `0x2a`).
- **Reuse considered:** `quint-connect` and `itf` replace mayfly's `conformance.rs` (trace
  generation, seeds and replay, about 2,670 lines with the formal stages); `stateright` (models in
  Rust lose Quint and Apalache) was rejected; `proptest-state-machine` serves components without a
  model. As built: `itf` kept, `quint-connect` not taken (the adaptation above); the harness is
  about 1,200 lines with its unit tests, manifest reader and binary, the stages about 800 in xtask.
- **Accept:**
  - [x] `check` runs both stages green; the same seed reproduces a run
  - [x] the negative control fails at the expected step with expected and actual state in the report
  - [x] a stale regression trace fails `formal-freshness`
  - [x] a malformed ITF file is `HARNESS_ERROR`, never a pass
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
- **As built (FORMAL-03):** `formal/views.qnt` with two worktrees (`n` a linked worktree nested
  in `m`, its HEAD and index outside its root), three files, one in-order watcher instance, two
  requests and a `sync` per view; the six invariants plus `SyncIsBarrier`; seven scenarios, the
  card's four plus `twoViewsShareAGeneration` (completeness is tied to a (view, generation) pair,
  the IDX-02 review), `headMovedOutsideTheRoot` (round 2) and `concurrentEditsDuringRequest`
  (round 3); thirteen controls. Each scenario's
  regression trace comes from a scripted step that enables one action per state, so the trace is
  the same at every seed; its Quint test takes the same steps as named actions, since `quint
  test` runs a test that draws a value 10000 times. `views` is pending on IDX-09.
  - **Round 2 (the review's FIX FIRST):** the correct model broke `CompleteIsHonest` in 13 steps
    (a checkout reconciled through file events only kept the old HEAD under `complete=true`). The
    design is now ADR 0037 (Proposed): the pin re-stats HEAD and the index; HEAD-only moves and
    staging are stimuli; events arrive in order and the request's cookie is modelled as an event;
    a per-view watched/certain state (watch, then scan; every loss signal, an overflow for every
    view of the instance); git's racy-mtime rule. IDX-09's obligations are on its card.
  - **Round 3 (the re-verify's HIGH):** round 2's `CompleteIsHonest` bounded each file
    separately, so a complete answer could mix versions no disk state held (a call moved from A
    to B in neither). The invariant now requires a snapshot (the pinned index equals one state of
    the request's ghost history from sync point to pin), and the model has the mechanism: read
    stamps on summaries and a second cookie at the pin; the control
    `requestPinIgnoringLateReads` is the review's counterexample. ADR 0037 states the snapshot
    rule, the second cookie and the `mmap` limitation. A pin waits for a queued cookie, and `step`
    picks among four equally likely groups of stimuli, so the simulation reaches a complete answer
    through a second cookie.
  - **The registry carries two numbers:** `steps` (Apalache's bound) and `sim_steps` (the depth of
    `formal-verify`'s 20000-sample simulation and of `conformance-replay`'s generated traces);
    `formal-verify` reads Quint's IR from `--out` and checks it whole. The FORMAL-02 review's V1
    (a pending task must have an open `Accept:` box) and V2 (the replay owns the comparison, and
    the observation must name every trace variable) are in `jfl-conformance`.
- **Accept:**
  - [x] typecheck, Quint tests and simulation of `allInvariants` green at the registered budget
  - [x] each broken-model control violates its named invariant within the bound (a Quint test)
  - [x] regression traces committed and fresh
  - [x] `doc/formal-models.md` states stimuli, observation and invariants for `views`
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
- **As built (FORMAL-04):** `formal/daemon.qnt` with a state directory (the lock an flock on the
  inode `daemon.lock` names, the socket file and its binder, the spawn lock), two proxies, three
  daemon and three connection slots, two documents, and an initial state clean or with a crash's
  stale socket. The card's four invariants, plus `OwnerReachable` (plan §7.2's own control,
  release before unlink, breaks none of the four: the new owner becomes unreachable) and
  `ReconnectRestores` (documents and request re-sent, plan §5.2); four scenarios, the card's
  three plus `sigtermAfterStaleStart`; nine controls, at least one per invariant, and a liveness
  control. `EventuallyServed` is a `temporal` with fairness spelled out for Apalache, which
  accepts it but cannot check it at a bound that reaches its counterexamples; the model's
  `Progress` simulation checks what it reduces to, and fails on the liveness control. The start
  and shutdown orders and IDX-12's obligations are in `doc/formal-models.md`; `daemon` is pending
  on IDX-12.
  - **Review round (FIX FIRST, one MEDIUM):** `Progress` ran by hand only. The registry now takes
    an optional `progress` entry (step, invariant, control, steps, samples); `formal-verify` runs
    the liveness simulation after the safety one and then its control, which must report a
    violation, so `formal.yml` checks liveness weekly (`tools/xtask/src/formal.rs`, its pin
    moved). The review's LOWs are IDX-12 obligations: close-on-exec for every descriptor and
    workers that die with the daemon, the proxy's deadlines, the lock-path check, `setsid` first.
- **Accept:**
  - [x] typecheck, tests and simulation green; each control violates its named invariant
  - [x] `just formal-verify daemon` passes at the documented bound, or the report names the tooling blocker (never a fabricated pass)
  - [x] regression traces committed and fresh
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
- **As built (FORMAL-05):** `formal/diagnostics.qnt` with two documents, `a` reading `b` (a
  dependency's change makes `a` stale at an unchanged version), a disk the server sees late, open
  sessions whose versions restart at 1, Claude Code's Edit (disk write and `didChange`) and an
  agent's shell edit, one javac worker of three job slots whose replies come in any order, with
  crash (running jobs lost, written replies still read), restart and timeout, and the client's
  last `publishDiagnostics` per document. Results are keyed by (session, input epoch), never by
  the LSP version alone; disk content newer than the overlay is published without a version.
  The card's three invariants, stated on the input epoch within an open session, plus
  `FreshAfterEdit` (plan §3.2's "pending": diagnostics arriving after an edit are for it or newer,
  what Claude Code relies on); liveness `EventuallyUpToDate` as a `progress` simulation. Seven
  scenarios, the card's three plus `dependencyEditMakesStale`, `diskWinsOverOverlay`,
  `timeoutLateReply` and (round 2) `createdFileMakesStale`; eight controls, at least one per invariant, the card's
  `deliverEveryResult` among them, and a liveness control; witnesses for every interleaving the
  simulation must reach. LSP-04's obligations are in `doc/formal-models.md`; `diagnostics` is
  pending on LSP-04.
  - **Review round (FIX FIRST, one MEDIUM):** javac's inputs are dynamic: "cannot find symbol
    `C`" followed by creating `C.java` changed nothing the coordinator tracked, so the stale error
    stayed current. The model adds a file `c` that `a` reads only while it exists (a negative
    lookup), the scenario `createdFileMakesStale`, the control `noticeDiskTrackingReads`, and
    `FreshAfterEdit` now also compares the published inputs with the current ones; LSP-04 gains
    the obligation to record what javac actually read (files with hashes, negative lookups, class
    folders, classpath and `EnvironmentKey`). The LOWs: `didOpen` takes its version as a pick (a
    proxy re-sends at the client's version, never renumbered), unversioned diagnostics' ranges,
    the E0 dependency on Claude Code's `didOpen`, and `jfl diag`'s stat and cookie first.
- **Accept:**
  - [x] typecheck, tests and simulation green; the control violates `NeverOlderOverNewer`
  - [x] regression traces committed and fresh
  - [x] `doc/formal-models.md` covers the three models with their drivers' tasks (IDX-09, IDX-12, LSP-04)
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
- **Deferred from IDX-07 (2026-11-18):** each new `.java` file here gets its summary golden
  (`fixtures/java/summaries/orchard/`): `tests/goldens.rs` writes a missing one and fails once;
  review it and lock it (`just fixtures-lock`). `grammar/ImportModule.java` and
  `grammar/FlexibleConstructorBodies.java` need Java 25 where the pinned JDK is 21.
- **Deferred from IDX-10 (2026-11-23):** the `java-fixtures` stage also runs
  `just javac-bindings check`, which recompiles `projects/shop` and the queries' generated corpus
  (`crates/jfl-adapter-java/tests/fixtures/javac/generated`) and fails when javac's bindings
  differ from the recordings the javac differential reads in `check`
  (`jfl-adapter-java/tests/definitions/main.rs`); until then a wrong recording with right file
  digests is caught only by `fixtures.lock` review.
- **Accept:**
  - [ ] `java-fixtures` runs `just javac-bindings check` (IDX-10's javac differential), and a hand-edited recording under `crates/jfl-adapter-java/tests/fixtures/javac/` fails it
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
  - [x] ~~the draft ADR committed, citing the E1 numbers~~ not needed: the owner took the default (ADR 0038: the tier is a detail of plan §5.5, recorded there and on this card)
  - [x] (owner) the tier decided; plan §5.5 and §14 updated
- **Decided:** 2026-11-18 (owner, at the FORMAL checkpoint), the default of plan §5.5 without
  waiting for EVAL-05. Small and medium repositories: a per-view in-memory index rebuilt at open
  from a flat on-disk summary cache keyed by `SyntaxKey` (an LRU size cap, no snapshots or GC).
  Large repositories (above about 10–15k files or 50 MB of Java): persisted per-file summaries,
  reference sites kept on disk, and an in-memory name → file postings index (plan §5.5 as
  amended by the IDX-02 review). EVAL-05's numbers are recorded when they come; they reopen the
  choice only if they move a repository across a row of §5.5's table.
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
- **Done note (IDX-03):** the decoded envelope is a wire type (`WireEnvelope`, its `complete` a
  `ClaimedCompleteness`) with no conversion back into the domain's, so `jfl_domain::completeness`
  stays the only source of `complete=true` (`tests/claims.rs`); a `DocumentUri` keeps a URI's
  text as sent beside its `FileUri` (LSP-01's echo); the view carries the worktree root (IDX-09).
  The bench item was met with synthetic inputs sized from BENCH-03's fan-out (hits over 1,600
  files), not a corpus run: the protocol reads no source file. `just bench-protocol` reports it:
  a 10,000-location `jfl/references` response costs about 9 ms to encode and decode on the typed
  path and about 20 ms through `serde_json::Value` (1,000 locations: about 1 ms and 1.9 ms), so
  result payloads take the typed path (plan §5.3). Round 2 (review): a minor never adds a member
  `complete` covers (plan §6.6); an oversized frame is skipped and reported, not fatal; the
  cross-major contract (`jfl/handshake`'s `protocol`, -32010, `jfl/stop`) is frozen.
- **Accept:**
  - [x] golden tests for every message; a decode of an unknown field succeeds (additive) and of a different major fails with a typed error (`crates/jfl-protocol/tests/golden.rs`, `tests/compat.rs`, fixtures in `tests/fixtures/{v1,compat}/`)
  - [x] a frame over the limit is refused before allocating (a test with a lying `Content-Length`); junk and duplicate headers are refused (`src/frame.rs` tests, `tests/properties.rs::should_refuse_before_allocating_when_content_length_lies`)
  - [x] property test: encode then decode is identity for generated messages (`tests/properties.rs`)
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): no corpus input applies; `just bench-protocol` times encode, frame decode and envelope decode at 1k and 10k locations
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
  - [x] contract suite green for fake and adapter; a broken fake fails it (`jfl_testkit::contracts::repo_probe`, 20 cases against `FakeRepoProbe` and, in `crates/jfl-adapter-git/tests/contract.rs`, real `git`; seven broken implementations each fail exactly their cases)
  - [x] integration tests: a path in a nested linked worktree resolves to it, not the parent; checkout, stash, reset and commit each change the stat and the changed-file set (`tests/worktrees.rs`, `tests/stat_set.rs`; staging moves the stat only)
  - [x] the spawn cost per command measured on the fixtures and the small bench corpus, recorded in the commit body (and an amendment of ADR 0024 if it changes the decision): `just bench-git SIZE`, numbers in `doc/bench/README.md`; they do not change the decision (ADR 0024 amended 2026-11-18 for the configuration it reads, not the numbers)
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): BENCH-05 has no cases yet, so `just bench-git SIZE` first checks that `tracked_java` of the corpus checkout is exactly `git ls-files '*.java'`, that a tracked file resolves to the checkout and that a status leaves the stat set unmoved (small and medium agree)
- **Impact:** dependency graph.
- **Decided (IDX-05):** the stat set is `$GIT_DIR/HEAD`, each loose ref a symbolic `HEAD` leads to
  (a commit or `reset --soft` moves only the branch's ref), the common directory's `packed-refs`
  and `$GIT_DIR/index`; absent files are recorded as absent. Each file is stat'ed before it is read,
  and only one stat calls regular is opened. A reftable repository is refused
  (`ReftableUnsupported`) until a pinned git lets a test pin its set. `StatRecord` carries device, inode,
  size, mtime and ctime with nanoseconds, and `settled_before` is the racy-mtime rule.
- **Decided (IDX-05):** the directory walk to the innermost root is redone on every call (one
  `lstat` per level, about 20 µs on the corpus, crossing filesystem boundaries); only Git's
  confirmation of a root is cached per root, keyed by a `.git` directory's (or a linked
  directory's) device, inode and `config` stat (never its times, which every `git add` moves) or a
  `.git` file's whole stat. Before any spawn, a `.git` that Git could not open (no regular `HEAD`,
  a missing gitdir, a FIFO, a dangling link) is refused, re-checked by stat on every request. A
  refusal the repository's structure decides is cached; a deadline or a failed or killed `git` is
  retried on the 1st, 2nd, 4th, 8th, … request after it (at most every 1024), with no clock. A directory holding a `.git` entry of any type, links not followed, is a nested root:
  `jfl_application::source_tree::nested_root`, the one rule the fs walk and resolution share. A cached directory prefix would miss a `git init`
  or `git worktree add` inside it, which no hint announces (ADR 0017, amended 2026-11-18).
- **Decided (IDX-05):** `git` runs with no system or user configuration, ignore or attributes
  file, `GIT_OPTIONAL_LOCKS=0`, `GIT_NO_REPLACE_OBJECTS=1`, and no command the repository's
  configuration names (`core.fsmonitor`, hooks, every filter driver turned off before `diff` and
  `status`, external diff, textconv, lazy fetch, bare-repository discovery); `jfl-adapter-git`'s
  `cli` module lists each (ADR 0024, amended 2026-11-18). A user's global `safe.directory` no
  longer applies.
- **Left to IDX-09 (IDX-05):** take `snapshot` before `changed_since` (the port doc says why); read
  the wall clock before `snapshot` and compare with `GitSnapshot::settled_before` (on NFS or SMB,
  cover the server's clock skew or treat the files as never settled); pass the last `head_tree` back as the base; treat
  `ChangedFiles::everything`, an error or `unrepresentable > 0` as a rescan; union the listing with
  `tracked_java`. `status` with optional locks off cannot refresh a racy index, so it rehashes those
  entries on each call; `--untracked-files=all` is three quarters of its cost on medium (39 ms
  against 10 ms with `no`): if the watcher's hints and the listing are trusted for untracked files,
  a parameter can drop it. The `repo_probe` fake cannot yet be told to fail with an I/O error as ver
  §5 asks: add that when the views driver needs it. A reftable repository is refused today; IDX-09
  or IDX-15 lifts the refusal once a pinned git (≥ 2.45) lets a test pin the stacks' `tables.list`.

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
  - [x] contract suite green for fake and adapter; a broken fake fails it (`jfl_testkit::contracts::source_tree`, 20 cases; nine broken implementations in its tests; `crates/jfl-adapter-fs/tests/contract.rs`)
  - [x] the nested-worktree fixture (a main checkout with three `.claude/worktrees/*`) lists each file once, under its own root (`crates/jfl-adapter-fs/tests/worktrees.rs`, real `git worktree add`)
  - [x] a symlink to a directory outside the root is not followed (integration test: `tests/symlinks.rs`, the walk and the watcher)
  - [x] CC0-1.0 (`notify`) and Unlicense (`ignore`) accepted by `deny.toml` as plan §16.2 lists; `notify`'s Linux backend `inotify`/`inotify-sys` is ISC, a per-crate exception (ADR 0019, amended 2026-11-18)
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): BENCH-05 has no cases yet, so `just bench-fs SIZE` first checks that the listing of the corpus checkout is exactly `git ls-files '*.java'` (small, medium and lombok agree), then times it
- **Decided (IDX-06):** no `notify-debouncer-mini`: it drops the backend's overflow event (no path)
  and feeds its thread through an unbounded channel; a bounded, coalescing hint queue in
  `jfl-application` replaces it (ADR 0019, amended 2026-11-18). The port is a native `async fn`
  trait (fnd §4.4) whose adapter runs its blocking jobs on an injected `BlockingPool` (plan §6.4).
  Build output is `build`, `target`, `out` or `bin` only beside a build file, plus `.gradle`:
  spring-framework has a package named `build`.
- **Decided (IDX-06 review, orchestrator):** watch, then hint. On Linux the adapter keeps one
  non-recursive inotify watch per directory the exclusion rules accept (none for `.git`, nested
  worktrees and repositories, `.claude/worktrees/` or build output) and watches a new directory
  (and what the rules accept below it) before its hint is handed out; a directory it cannot watch
  is a `Rescan(WatcherError)`. macOS keeps one recursive FSEvents stream. A pending rescan keeps
  every reason, and the end of a watch is a last `Rescan(WatchEnded)`. The extension rule of
  `hint_excluded` applies to files only (`java.base` is a directory).
- **Left to IDX-09 (IDX-06):** the walk honours `.gitignore` as ripgrep does, so a tracked `.java`
  file that also matches a `.gitignore` pattern is neither listed nor reported as a gap (Git keeps
  tracking it), and neither is a tracked file under a skipped build-output directory (`bin/`,
  `out/` beside a build file). The view unions the listing with the repository's tracked files
  (`RepoProbe`), skipped `BuildOutput` directories included, before it can answer
  `complete=true`; `just bench-fs` shows the corpus has no such file. The watcher's own gaps are
  IDX-09's obligations (its card).
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
- **Deferred (IDX-07, 2026-11-18):** `fixtures/java/projects/` and `fixtures/java/resolution/`
  do not exist yet (EVAL-07's); `crates/jfl-adapter-java/tests/goldens.rs` covers every `.java`
  under `fixtures/java/`, so their summary goldens are written, reviewed and locked when EVAL-07
  lands them. BENCH-05's real-corpus cases have not landed either; the adapter ran on the corpus
  (`just bench-syntax`: no file refused, none with a syntax error).
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
- `SummaryStore` port; `jfl-adapter-store` at the tier DEC-03 chose (decided 2026-11-18: for
  small and medium repositories a flat on-disk cache keyed by `SyntaxKey` with an LRU size cap,
  read into a per-view in-memory index at open; for large ones, above about 10–15k files or 50 MB
  of Java, persisted per-file summaries with reference sites on disk and an in-memory
  name → file postings index) on the crates of ADR 0025 (`blake3` keys,
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
- **Decided (IDX-08, 2026-11-18):** the reference-name postings are a sorted name table, not
  `fst`, because they are rebuilt at every open and an `fst` build cost about 50 ms more on
  spring-framework for no faster lookup (plan §5.5). `fst` stays ADR 0025's choice for
  `workspace/symbol` (IDX-10); `rusqlite` is not taken at this tier.
- **Accept:**
  - [x] contract suite green for fake and adapter; a broken fake fails it
  - [x] a truncated entry on disk is a miss, never a corrupt summary (integration test)
  - [x] the cap evicts least-recently-used entries (test on fake clock)
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
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
- **Obligations from `formal/views.qnt` and ADR 0037 (Accepted, owner, 2026-11-18; doc/formal-models.md lists
  them with the model's names):** watch, then scan, then publish (at open, start and every
  re-established watch); scan each new directory after adding its watch; treat an `add_watch`
  error (ENOSPC, EMFILE), `IN_IGNORED`, `IN_DELETE_SELF`, `IN_UNMOUNT`, `IN_Q_OVERFLOW` (every
  view on the instance) and FSEvents' drop, `RootChanged` and `EventIdsWrapped` flags as losses
  that leave the view uncertain until a full scan started after the watch is back; check the
  filesystem type with `statfs` and never claim `complete` on network, FUSE or overlay
  filesystems (NFS, SMB, sshfs, 9p, virtiofs, an overlayfs lower layer); apply git's racy-mtime
  rule in every stat; a cookie per request through the in-order event stream, the cookie and the
  view's files on one inotify instance or FSEvents stream with no debouncer or other reordering
  layer between (`FSEventStreamFlushSync` on macOS); a read stamp on every summary and a second
  cookie at the pin when the pinned generation holds a summary read after the first (at most two
  round trips per completeness-claiming request, measured against plan §1.7's 100 ms freshness
  target), so a complete answer is a snapshot of one real disk state; re-stat `HEAD` and the
  index at the pin (a linked worktree's live under the common directory's `worktrees/<name>/`);
  writes through a writable `mmap` are an accepted limitation (ADR 0037). The driver observes the
  view identity of each pin, read and completeness, and every type of its observation, nested
  ones included, uses `deny_unknown_fields` and a derived `PartialEq`.
- **Reuse considered:** `salsa` weighed and deferred (per-process database, experimental
  persistence); turborepo's cookie file to prove the watcher caught up for `sync`.
- **As built (IDX-09, slice 1, 2026-11-18):** the registry's core, `jfl_application::views`,
  does no I/O: each method is one transition of `views.qnt` with the model's guards (a step the
  model does not enable is a `ViewError`), generic over what a stat sees of a file (`FileSeen`:
  the stat and the racy rule) and of `HEAD` and the index, and a summary's key; generations are
  `Arc`s a pin keeps while the next publishes; the pin computes completeness through IDX-02's
  `completeness` over the request's own view, adding `PendingChanges` (cookie not back, a late
  read, dirty paths, a due rescan, a build, a moved `HEAD` or index), `NotWatched` and
  `WatcherLostEvents` (two `IncompleteReason`s added, as the mapping asked). The `views` driver
  (`jfl-conformance`) replays the model over it with the scripted world (tree, watcher, store)
  beside the driver, every observed type `deny_unknown_fields` with derived `PartialEq`; no
  model change. Two negative controls: the registered one shows the registry a store lagging by
  its last write and diverges at `publish` (`NotStored`); the second publishes each build as it
  starts and diverges at `reconcileStart` (`doc/formal-models.md`). A core with the ports inside
  was not taken: the model's steps are finer than a port call. Round 2 (review): a cookie event
  carries its cookie's stamp and only the latest queued cookie's counts (a real cookie raises
  several events); a rescan takes the view's listing, reads new paths and certifies only with
  one (file creation is not modelled: `doc/formal-models.md`, "Not modelled yet"); a module
  count `ModuleIndex::new` refuses is incomplete, never empty. `just bench-views` measures it on
  the corpus (`doc/bench/README.md`).
- **As built (IDX-09, slice 2, 2026-11-19):** `jfl_application::view_service`, the async shell
  over the ports (`SourceTree`, `RepoProbe`, `SyntaxIndexer`, `SummaryStore`, and a new
  `WallClock` port: the time read before each stat, the cookie's deadline). A request resolves its
  file's innermost worktree and opens that view before it starts (the bind hazard); a view opens
  by `statfs` (`SourceTree::filesystem`; only an allowlist of local filesystems is watched, so
  network, FUSE, overlay and unknown ones answer `NotWatched`), watch, the clock, `HEAD`'s stat
  set, the listing unioned with `tracked_java` (nested worktrees, repositories, agent worktrees and
  links left out, build output kept), then publish; its files are read and summarised four at a
  time on the ports' pool (`ServiceConfig::parallelism`), and stale cookies older than a minute
  are swept first (`SourceTree::sweep_cookies`: regular files of a cookie's exact name, never
  through a link). Hints: a `.java` file the view holds is a file event, any other path (a new
  file, a directory, `.gitignore`, a build file) a rescan, this daemon's cookie its request's
  barrier, another daemon's ignored; a `Rescan` that `degrade_the_watch()` is `watch_lost` and
  the view is watched again at once (`watch_restored`, so the next rescan lists after the watch is
  up), any other `overflow`; either loses the view's queued cookies. Every rescan takes the
  listing. **A parent never holds a nested view's file, and is never handed a hint under a nested
  root** (the slice-1 obligation, met): each view keeps the nested roots its last scan skipped and
  the views opened inside it since, and drops a hint under one; a hint for a file the view does
  not hold asks for a rescan rather than a read (it may be under a worktree just created or a
  `.gitignore`d directory); a listed rescan leaves out of the build every path outside the listing
  unioned with the tracked files (`ViewRegistry::exclude`, not a model transition: the model's
  views index fixed paths), so anything that leaked in leaves, while a tracked file deleted on
  disk stays as an absent entry; a view opened inside an open one makes the parent rescan
  (`tests/view_service.rs`: `should_keep_a_nested_worktree_file_out_of_the_parent_when_it_is_edited`,
  `should_keep_a_new_worktree_out_of_the_parent_when_it_is_created_under_it`,
  `should_keep_an_ignored_file_out_of_the_view_when_it_is_hinted_or_asked_for`,
  `should_drop_a_nested_view_s_files_from_the_parent_when_the_nested_view_opens`). A directory on
  another device than the root (a mount point) is not walked and is a gap
  (`UnlistedReason::OtherFilesystem`: `jfl-adapter-fs/tests/files.rs`), so a network mount inside a
  worktree is never trusted as watched. The cookie (`SourceTree::write_cookie`:
  `.jfl-cookie-<owner>-<stamp>` created 0600 in the view's root and removed at once, through the
  view's own watch; a file already of that name is left alone and `-<attempt>` is appended) is
  waited for with a 200 ms deadline, `cookie_lost` when it passes. The cookie file exists for the
  microseconds between its create and remove: another watcher of the worktree sees it come and
  go. The Git observation (`GitObserved`) puts `HEAD`, the
  ref it names and `packed-refs` in the registry's `head` and the index in its `index`, settled
  by the racy rule. Each generation entry carries its summary (`Summary`), so the store's cap
  evicting an entry between its summary and the publish never sticks a build; the store is the
  cache a view opens from. A file the indexer refuses or that cannot be read, and every gap of the
  listing, is `ModulePartlyIndexed`. Core changes: `Pin::complete_view` is the root
  `completeness_at` read; `ViewRegistry::forget` drops a finished request and its pin (the shell
  forgets each request it finishes). The `repo_probe` fake fails every call on request
  (`fail_with`); the `source_tree` fake answers a filesystem kind, writes or loses cookies and can
  break its watches; the contract gained `reports_a_local_filesystem` and
  `cookie_follows_earlier_changes` (a tree on the port's defaults fails both). The store's
  directories are created 0700; a read that missed a racing write re-checks under the lock before
  forgetting the entry, and a discard removes the file only while it still holds the torn bytes it
  was given (a rewrite can get the freed inode back, so the inode cannot tell). The protocol golden
  `jfl.definition.response.unwatched` words the two new reasons. The reference machine
  (`tests/views_machine.rs`, plain `proptest`, 512 cases of up to 80 steps at a fixed seed) checks
  every step's acceptance, every generation and every pin's completeness against an independent
  model, and that a complete answer is a snapshot of one disk state since the request started.
  Its generator mixes in late-read fragments (a request, writes behind its cookie, the deliveries,
  a reconcile, the pin), also a named test
  (`should_refuse_complete_when_a_reconcile_read_after_the_cookie_published_a_mix`): dropping the
  late-read rule from the registry alone fails both on the reference's completeness, and dropping
  it from the registry and the reference fails both on the snapshot check, which restates no
  rule. **Decided (IDX-09, slice 2):** a rescan
  re-stats every indexed file and lists the view; it does not narrow the work with
  `changed_since`, whose `status` alone costs more (39 ms on medium) than every stat (9 ms), so
  IDX-05's ordering obligations for it do not arise (the clock is still read before every
  snapshot). `just bench-views` measures the shell on the corpus (`doc/bench/README.md`): cookie
  round trip p50 0.026 ms, p95 0.050 ms on medium; a cold open 5.7 s with four parses at once
  (12.8 s one at a time).
- **As built (IDX-09, slice 3, 2026-11-19: the cold open's speed, plan §1.7):** profiled first
  (`just bench-views` now prints each stage of a cold open alone on 1 and 4 threads). On medium the
  parse is the floor (1.92 s on four threads); a store write cost 1.5–3 s on the way to the publish
  and did not scale across threads; the reads, hashes and store calls ran on one thread. Three
  changes: **the store is written after the publish** (`ViewService::write_back(budget)`, four
  writes in flight; a summary the indexer made is looked up in memory before the store until then,
  and an empty store is not asked), which changes no model transition, since a generation holds its
  own summaries (slice 2) and the model's `store` is them; **every port runs on the blocking pool
  with 16 files in flight** (`ServiceConfig::parallelism`: each step is a hand-off, 66 µs on this
  VM, so the pool needs more files in flight than threads); and **the open answers before the whole
  view is read**: a view of more than `ServiceConfig::open_whole` (256) files reads only the
  request's directory and publishes every other file as `Summary::Pending`, a gap under a racy
  entry, so the answer is `ModulePartlyIndexed` and no stat proves such a file fresh;
  `ViewService::settle` reads the rest as one build of shell-made file events. A request on a
  pending file reads just that file. The source bytes held by the files in flight are capped
  (`ServiceConfig::read_budget`, 32 MiB; a larger file is read alone), and so are the summaries
  waiting for the store (`ServiceConfig::unwritten_cap`, 65,536; past it a summary is not kept for
  the store). The open is the model's `init`. A pending entry is the model's entry at the listed
  version with a racy summary, plus a coverage gap the model does not contain; the guard is the
  shell's coverage rule (`count` and `indexed < files`), and atomic publish holds only when a
  declared gap counts as stored (`doc/formal-models.md`, "The open in two steps"). No transition
  is added, and `views.qnt` is unchanged (the review agreed). Tests: `tests/view_service.rs`
  (`should_answer_partly_indexed_and_then_complete_when_the_open_defers_the_rest`,
  `should_read_only_the_requested_file_when_a_request_finds_it_pending`,
  `should_settle_from_the_disk_when_a_pending_file_changes_before_settle`,
  `should_read_every_pending_file_when_sync_runs_before_settle`,
  `should_store_the_summaries_only_when_written_back`,
  `should_index_the_same_content_once_when_a_second_view_opens_before_the_write_back`,
  `should_drop_a_pending_file_from_the_count_when_it_is_deleted_before_settle`,
  `should_keep_at_most_the_cap_for_the_store_when_more_summaries_wait`); counting a pending file
  as indexed, or letting a stat prove it fresh, fails two of them each. `bounded`'s limit and the
  read budget have unit tests (removing the limit fails the first). Measured
  (`doc/bench/README.md`, medium, the other lane idle, median of three), against 5.7 s for the
  whole open before:
  - after a page-cache drop (`just bench-views medium cold`, plan §1.7's meaning of cold): first
    answer 385 ms, under the 2 s row; whole view published 2,742 ms, **over the 2.5 s row**;
  - with a warm page cache and an empty store: 194 ms and 2,207 ms;
  - small, after a drop: 133 ms and 524 ms.
  Under a neighbour's build (load 3 to 4) the parse alone takes 2.5 s. **Reuse considered:** `rayon` (in the graph through
  `jfl-bench` only) for the parse; not taken, because the ports already run their blocking work on
  the injected `BlockingPool` (fnd §4.4), so the parallelism belongs to that pool, and
  `jfl-application` holds no spawn grant; the shell bounds what it keeps in flight with its own
  small `bounded` combinator, std only (`futures`' `buffer_unordered` is not in the graph).
- **Decided (orchestrator, 2026-11-18):** not `proptest-state-machine`, whose `proptest` default
  features bring 16 crates the workspace keeps out; the reference machine is written over plain
  `proptest`, std only.
- **Left after slice 2 (IDX-09):** lifting the reftable refusal (`ReftableUnsupported`) once a
  pinned git (≥ 2.45) lets a test pin the stacks' `tables.list`, here or IDX-15; a model action
  for a pin whose cookie deadline passes (today a deadline is `cookie_lost`, which the model's
  `dropEvent` of a cookie covers; `doc/formal-models.md`, "Not modelled yet"), model first;
  `FSEventStreamFlushSync` on macOS, which `notify` does not expose (a cookie there waits for the
  stream's latency, bounded by the deadline); **the full syntax index after a page-cache drop is
  over plan §1.7's 2.5 s on medium** (2,742 ms against 2,207 ms warm). About 170 ms of the
  difference is before the first answer (the cold walk, the watch, `git`); about 300 ms is
  `settle`'s reads, which a larger pool (6 or 8 threads) did not hide. The rest of the gap is the
  parse (1.9 s on four threads, against E1's 1.04 s warm for its lighter extract): a faster
  summary, or prefetching reads behind the parse, before M1 (orchestrator's decision of
  2026-11-19 stands); after slice 3, **the daemon's part of the cold open (IDX-12, LSP-02 wiring):**
  - call `settle` as soon as the request that opened a view has its answer;
  - call `write_back` in slices between requests (each write holds the service; a medium cold
    open's writes take 1.5–4.3 s on this VM, after the publish);
  - bind the open's, `settle`'s and the rescan's bulk work to one bounded blocking pool;
  - **keep a request's own stats inline by default** (decided by the orchestrator, 2026-11-19). A
    stat costs a few µs and the pool's hand-off 66 µs: on the pool a request takes 0.3 ms p50
    against 0.06 ms inline. A move onto the pool must show it does not regress the M1-gated
    request rows;
  - **never cancel `settle`, a reconcile or a request mid-flight.** A dropped future between
    `reconcile_start` and `publish` leaves the build set, and every later reconcile fails with
    `BuildActive`; `settle` holds that window for about 2 s on medium. Otherwise the build must
    be aborted (with the abort transition below, model first);
  - wire `ServiceConfig::read_budget` and `unwritten_cap` to the tier's memory budget.

  Also left: a cheaper store write (one file per entry, renamed under the store's lock, costs
  30–250 µs an entry here depending on order and what the disk did before) for IDX-08's store or
  DEC-03's large tier; the 2.5 s row under a neighbour's load (the parse alone is 1.9 s quiet,
  2.5 s at load 3 to 4) and on CI hardware (EVAL-05); an abort transition
  for a request that fails after `request_start` (it stays `Waiting`, holding no pin; model
  first); the reference machine never draws a racy stat; the large tier's summaries on disk
  rather than in each
  generation (DEC-03, IDX-11); requests served one at a time (`&mut self`; the daemon, IDX-12,
  serialises them); a hint that names a directory costs a full rescan; the backend gaps the
  shell cannot see (`notify` drops `IN_UNMOUNT` and `IN_IGNORED`; FSEvents' `RootChanged`) stay
  the fs adapter's (IDX-06's list); placing the store's root under the daemon's 0700 state
  directory is the daemon's wiring (IDX-12; the store creates its own directories 0700).
- **Accept:**
  - [x] `conformance-replay` replays the `views` regression and generated traces green (`crates/jfl-conformance/src/views.rs`; the seven regression traces, and 10 × 30-step generated traces at each `check`'s seed; seeds 0x1 to 0x5 also run)
  - [x] the negative control fails at the expected step (`tests/replay.rs::should_diverge_at_the_reconcile_when_the_views_control_publishes_early`: step 3, `reconcileStart`)
  - [x] component tests: a request pinned to N while N+1 publishes answers wholly from N; a `sed -i`-style write seen by the stat is in the answer without `sync` (`crates/jfl-application/tests/views.rs`)
  - [x] a `proptest-state-machine` test of the registry (open, reconcile, publish, pin, `sync`) against a reference machine (ADR 0007, amended; ver §6.4), over plain `proptest` (Decided above): `crates/jfl-application/tests/views_machine.rs`
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): `just bench-views SIZE` (small and medium, `doc/bench/README.md`); BENCH-05 has no cases yet, and every request over the unchanged corpus came out complete
- **Obligations from the watcher (IDX-06 review), with FORMAL-03's (watch, then scan; a loss on a
  watch error; a filesystem-type check; racy mtimes):** hints are not a complete record, so a view
  answers `complete=true` only from what it scanned or stat'ed, never from the absence of a hint.
  - Watch, then scan: open a view by `watch` and then `list`; on a new directory's hint, scan it
    (the adapter has watched it by then); on any `Rescan`, rescan the view.
  - A `Rescan` whose reasons `degrade_the_watch()` (a directory that could not be watched, the
    watch ended) leaves the view unwatched in part: treat it as unwatched (restart the watch,
    rescan, and stat on every request) until a fresh watch has been established.
  - Gaps the backend does not report: `notify` ignores inotify's `IN_UNMOUNT` and `IN_IGNORED`
    (a watched directory's filesystem unmounted, a watch the kernel dropped); FSEvents never
    reports the root moved or deleted (`notify` does not ask for `RootChanged`); network and FUSE
    filesystems may deliver no events (refuse to rely on the watcher there, after a `statfs`
    filesystem-type check); and a directory created during the first walk is seen by the listing
    that follows the watch, not by a hint.
  - The union of the listing with the repository's tracked files (IDX-06's card), skipped
    build-output directories included.
- **Obligations from the git adapter (IDX-05 and its review):** take `RepoProbe::snapshot` before
  `changed_since`; apply `settled_before` with a wall-clock reading taken before the snapshot
  (network filesystems: cover server clock skew, or never settled); rescan on
  `ChangedFiles::everything`, an error or `unrepresentable > 0`; union the listing with
  `tracked_java`. A reftable repository is refused (`ReftableUnsupported`) until a pinned git lets
  a test pin its stat set (here or IDX-15). The `repo_probe` fake cannot yet be told to fail with
  an I/O error (ver §5): add it with the views driver.
- **Impact:** none (internal); model conformance.

#### IDX-09d: Settle a view in slices
- **Depends on:** IDX-09, IDX-12, LSP-02. **Model:** deep. **Size:** L. **Parallel with:** LSP-04.
- **Cites:** plan §4, §7.1, §1.7; ver §6; ADR 0007, ADR 0014, ADR 0031; IDX-09's "Left after slice 2"
  (`settle` holds the daemon's loop) and the LSP-02 review (a request queued behind `settle` on
  large passes `CALL_BUDGET`; SIGTERM, accepts and ticks wait).
- **Why:** `ViewService::settle` read the rest of a deferred view as one build, which the daemon's
  run loop awaited to its end: about 2 s on medium, 9 s projected on large, over the 8 s per-call
  budget there. Cancelling it was no way out: a build dropped between `reconcile_start` and
  `publish` stays set.
- **As built (2026-11-24):** model first. `views.qnt` gained `reconcilePart(view, take)`: a build
  of part of the delivered events, never while a rescan is due (a rescan is whole) and only with
  every path a waiting request needs (`needed`: each waiting request's file while the generation
  it waits for is unpublished); the rest stay delivered, so no answer is complete until a build
  takes them. Scenario `settleInSlices` (regression trace `views-settle-in-slices`, 26 steps),
  guard tests (`partLeavingOutANeededPathIsRefusedTest` and its twin, `partDuringARescanIsRefusedTest`
  and its twin), and the broken control `reconcilePartIgnoringRequests`, which violates
  `FreshAfterStat` (`partIgnoringRequestsViolatesFreshAfterStatTest`). The step picks `take` as one
  delivered path, alone or with every needed path, so the simulation takes parts while a request
  waits: with the control in `reconcilePart`'s place, 20,000 × 30 steps find its violation in
  about 1,800 samples at each of seeds 0x1 to 0x5 (with any subset, review round 1, it found
  none); Apalache's length 5 cannot reach that fault (about 10 steps), so the simulation and the
  scenario carry it. Every other committed views trace changed only by the new `take: None`
  pick. The registry's `ViewRegistry::reconcile_part` and `needed` follow the model (refusals
  `RescanDue`, `NotDirty`, `NeededNotTaken`; `tests/views.rs`); the `views` driver replays the
  action and a third control, `control_part_without_needed`, diverges at step 18 of the new trace
  through the registry's `NeededNotTaken` guard
  (`tests/replay.rs::should_diverge_at_the_part_when_the_views_control_leaves_out_a_needed_path`).
  The shell: `ViewService::settle(slice)` delivers every deferred file on its first call and then
  reads and publishes at most `slice` of them per call (`reconcilePart`, or the whole build when a
  rescan is due), returning whether any remain; a request's reconcile takes the delivered files
  that are not deferred and the files waiting requests need, never the rest
  (`should_publish_a_generation_per_slice_and_answer_between_them_when_settled_in_slices`). The
  daemon runs one slice of `SETTLE_SLICE` (512) per background step, then `write_back` in its
  slices as before (`crates/jfl-composition/tests/daemon.rs::should_answer_a_request_between_the_slices_of_a_settle_when_the_view_is_large`:
  three slices, three generations, every answer before the last incomplete). No abort transition:
  each step still runs to its end, bounded by its slice, so nothing is ever cancelled mid-build.
  **The bound holds only while no rescan is due:** a rescan is whole, so it reads every file still
  deferred in one build, in the settle's step or inline in the request that finds it due; that
  build holds the loop as long as before (IDX-09e). **A publish costs its slice, not the view**
  (review round 1: copying the whole index per slice made the settle quadratic, +8.7 to +11.6 % on
  medium with a warm store, about +30 % projected on large): generations share their untouched
  entries (`jfl_application::SharedIndex`, sorted chunks behind `Arc`s; **Reuse considered:** `im`
  and `rpds`, both MPL-2.0, outside `deny.toml`'s allowlist, and larger than the five operations
  needed), the registry's publish adjusts its counts and checks the store over the touched paths
  only (the others passed at their publish, and a store only grows), and the shell keeps the
  deferred set and the count beside the view instead of walking it.
- **Measured** (`doc/bench/README.md`, "The settle in slices", medium, 2026-11-24, a loaded VM):
the loop is held p95 299–307 ms per slice (the whole 4.1–4.3 s settle before); a request between
  slices 1.6–2.4 ms p50 in process, and a definition during the settle p50 217–268 ms over
  `jfl lsp` (each waits for its slice); SIGTERM to exit 239–354 ms; the full index within noise of
  the one build, cold and with a warm store (median 723 against 720 ms), and a slice's fixed cost
  no longer grows with the view, so large's projection is the same.
- **Left:**
  - a rescan due while deferred files remain: IDX-09e;
  - quiet-machine numbers, and large measured rather than projected;
  - the reference machine (`tests/views_machine.rs`) does not draw parts yet; the generated
    conformance traces do, rarely (14 parts in 200 traces of 30 steps at seed 0x99: a rescan,
    which refuses a part, is often due), so the committed trace and the simulation carry them;
  - `write_back` was already sliced (`WRITE_SLICE`, p95 about 130–160 ms here); a cheaper store
    write stays with IDX-08 and DEC-03.
- **Accept:**
  - [x] `views.qnt` models the part, its scenario, guard tests and broken control, and the simulation finds the control (`doc/formal-models.md`, controls); `formal-test` and `formal-freshness` green; `formal-verify views` green within `formal/models.toml`'s budget: Apalache at length 5 in 1169 s (858 s before), the simulation 185 s, 23 min in all against the 60-minute job, about 14 minutes of room at CI's doubling (`doc/formal-models.md`, Budget)
  - [x] the driver replays `views-settle-in-slices` and its control diverges at the part (`tests/replay.rs`)
  - [x] a request between slices is answered from the latest generation, incomplete until the last slice (`tests/view_service.rs`, `tests/daemon.rs`)
  - [x] measured on medium with large projected: full index, loop hold, request latency during the settle, SIGTERM to exit (`doc/bench/README.md`)
- **Impact:** while no rescan is due, the daemon answers, accepts and stops within one slice during a settle (a rescan still reads every deferred file in one build: IDX-09e); no protocol change.

#### IDX-09e: A rescan with deferred files, in slices
- **Depends on:** IDX-09d. **Model:** deep. **Size:** M. **Parallel with:** LSP-03. **Blocks:** M1's
  bench gate (LSP-06): until it lands, a settle's hold is bounded by one slice only while no
  rescan is due.
- **Cites:** plan §4, §7.1, §1.7; ver §6; ADR 0007, ADR 0014, ADR 0031; the IDX-09d review.
- **Why:** a rescan is whole (`reconcileStart`): it re-stats every entry and reads every one a stat
  cannot prove, and every deferred entry is racy, so a rescan due while a settle has files left
  reads them all in one build: about 2 s on medium, 9 s on large, in the settle's background step
  or, when a request finds the rescan due, inline in that request, past the 8 s call budget on
  large. Every trigger, each common during a settle in agent use:
  - a file created in the view during the settle: a request's part takes it, the shell finds it
    unknown to the published generation and escalates to `Event::Git`
    (`ViewService::reconcile_part`, the `escalate` branch);
  - a linked worktree's `git add` or commit (`.claude/worktrees/…`, the target layout): its `HEAD`
    and index live outside its root, so no watcher event is raised, and the next request's stat
    sets the rescan (`requestStart`) and that request performs the whole build inline;
  - a lost watch (`watchLost`, a degrading `Rescan`), and its restore;
  - an overflow (`IN_Q_OVERFLOW`, `dropEvent`: every view on the instance);
  - a `.git` hint (the main worktree's `HEAD` or index, or any non-`.java` path);
  - `sync`, whose rescan is its barrier.
- **Proposed model change (the reviewer's):** a rescan that may leave the delivered deferred files
  in `dirty`: `reconcileStart` on a rescan takes every changed path except, at its choice, paths
  that are delivered and unread (a deferred file is delivered by the settle's first slice), which
  stay `dirty` for the settle's parts. `completeAt` stays honest because `dirty` stays non-empty
  until a part takes them; `certifies` is unaffected (the scan still started with the watch up,
  and a delivered path is known changed). `sync`'s barrier then needs its rescan and every part
  after it until `dirty` holds nothing delivered before the `sync` (`SyncIsBarrier` decides
  whether `syncDone` waits for them). Model first: the scenario (a created file during a settle;
  a linked worktree's commit during a settle, with a request), a broken control (a rescan that
  drops the left paths from `dirty`, violating `CompleteIsHonest`), the driver, then the
  registry and the shell (a rescan leaves `Opened::deferred` delivered; a request's rescan then
  reads only its own and the non-deferred files). Apalache's budget is nearly spent: `views` at
  length 5 took 1169 s after IDX-09d, leaving about 14 minutes of room in the 60-minute job at
  CI's doubling (`doc/formal-models.md`, Budget), so measure first, and split
  the job or lower the simulation's share if it does not fit.
- **Accept:**
  - [ ] the model change above with its scenario, control (found by the simulation) and driver; `formal-verify views` within the budget
  - [ ] a rescan during a settle reads at most one slice of deferred files per step, and a request that finds a rescan due answers within its budget on large (`tests/view_service.rs`, `tests/daemon.rs`)
  - [ ] measured on medium with large projected: the longest hold during a settle with each trigger above
- **Impact:** the IDX-09d bound (one slice) holds with a rescan due; no protocol change.

#### IDX-10: Queries: `sym`, `def`, `hover`
- **Depends on:** IDX-09. **Model:** deep. **Size:** L. **Parallel with:** LSP-01.
- **Cites:** plan §2.1, §2.2, §2.7; ADR 0014.
- Application queries over a pinned generation: workspace and document symbols (search by
  prefix and camel-case humps, kinds), definition (lexical scope, imports, same-package and
  project types; bounded ambiguity over overloads of a proved owner type), hover (signature,
  Javadoc, declaring type and module, status line, and for declarations the reference summary once
  IDX-11 lands), each returning the envelope, each with its internal deadline returning a partial
  answer with `complete=false`.
- **Reuse considered:** `fst` (ADR 0025) for `workspace/symbol`, measured and not taken (Decided
  below); a resolver crate is SEM-01's question (DEC-04), so the syntax-only rules here are this
  crate's own, kept small and conservative; `salsa`'s memoised queries were not weighed again
  (IDX-09 deferred it): the one cache that pays is a generation's name tables.
- **Defined (IDX-10, 2026-11-19, after two reviews): the allowlist of `exact`.** The principle:
  the queries do not model Java; when syntax cannot prove a binding the answer is `possible` with
  a reason (or nothing, with the reason it is unresolved), never `exact`, since a false `exact` is
  the worst answer they can give. `jfl_application::queries` answers over the request's pinned
  generation only, and a file's text (positions in the carrier's encoding, Javadoc) is read only
  while its bytes are the ones the generation's key names; a file that moved on is left out with
  `ModuleStale`, and a target is named only with its location. **`exact` is admitted from one
  place, `queries::admit`, and only for constructs on its allowlist: the cursor on a
  declaration's own name** (no name resolution), **and a site the syntax-tree resolver proves**
  (slice 2, below: the constructs of `resolve/prove.rs`'s `ALLOWLIST`, every node around the site
  on `jfl-adapter-java`'s `scopes::KINDS`, checked against javac). Every name the scope rules
  below bind without that proof is answered `possible`, its candidate kept (`name_and_arity`, "bound by scope rules read from
  syntax, not proved"; a proved owner's overloads `overload_not_proved`, no target) and
  incomplete (`TargetUnresolved`). Two adversarial reviews with javac as the oracle found the
  rules, which re-derive Java's structure from summary ranges and a text scanner, binding wrong
  declarations with `complete=true` (the first: element keys, pattern variables, `case` labels,
  `Outer.this`, private and interface-static inheritance, overrides by parameter types as written,
  enum constant bodies, default-package qualified names, same-package and on-demand types, fixed;
  the second: anonymous classes behind a nested `new`, a `[` or a lambda block in their arguments,
  comments before element keys and `case` labels, class headers and `super(...)` resolved in the
  body's scope, a type's own name taken before an inherited member type of that name (JLS 6.4.1),
  annotations before a type-parameter clause). The rules now rank candidates; a construct joins
  the allowlist only with the syntax-tree resolver and the javac differential test of it. The scope rules (what they offer as the one candidate):
  - the cursor on a declaration's own name is that declaration;
  - a type name bound by the enclosing types and their member types (a declared member type hides
    inherited ones), the file's own top-level types, or a single-type import (one view type of
    that qualified name; none is `TargetNotIndexed`);
  - a qualified type name by its first segment through that same scope, then member types; a
    first segment that is no type in scope may be a package, and a view type of the whole name is
    then only `possible`; a default-package type is never visible from a named package;
  - a field or method name: the innermost enclosing type with a member of that name is the owner,
    then single static imports. Members are walked through supertypes that resolve as above (16
    deep; cycles, a step budget and the deadline end the walk as "outside"), leaving out what Java
    does not inherit: private members, an interface's static methods, and a package-private member
    unless every type from the owner down to it is in its package. A supertype's method counts as
    overridden only when its parameter types are proved the same (equal as written, and each a
    primitive, the same view type from both files, the same import, or one package's name under
    the same on-demand imports); otherwise both are overloads. `Object`'s, `Enum`'s and an
    annotation's implicit methods count as outside members of those names;
  - **bounded ambiguity:** with the owner proved, the method overloads of that name among the
    owner's members are listed as `possible` (`overload_not_proved`), target `ambiguous (n
    overloads)`; one applicable by arity (equal, or varargs with at least all but the last) in a
    closed hierarchy is `exact`. Never a same-name member of another type. When a type outside the
    index may add overloads, the answer says `TargetUnresolved`;
  - constructors are not inherited: `new T(…)`, `this(…)`, `super(…)` and `T::new` resolve `T`,
    then its constructors by arity; a type with none declared is its own constructor's target;
  - a receiver: `this` is the enclosing type and `Outer.this` the enclosing type it names; a bare
    name the extractor kept that no field in scope can be (Java reads a variable before a type,
    JLS 6.4.2), that no local of the member may declare, and that binds to one type is that type.
  What the rules never offer as their one candidate (always `possible`, the reason given):
  - **the file's package and its on-demand imports** (`possible`, `name_and_arity` "same package"
    or `import_unresolved` "on-demand import"): the package may hold a type the snapshot lacks (a
    `Pending` file, a gap, a generated-source root) that is the one meant, and the view does not
    tell source sets apart, so a test-only type would be offered to main code. **PROJ** (the
    project model: source sets and their visibility, generated roots) lets these steps answer
    `exact` again when the pin has no `Pending` file or gap, the file's source set sees exactly
    one candidate, and no generated root of that set is missing;
  - a site inside an anonymous class, a local class or an enum constant's body (their members are
    not in the summary), found by scanning the member's text;
  - a bare name that is an annotation's element key or a `case` label (unresolved: the element or
    the selector's constant is not given), or that a local of its member may declare: a pattern
    variable, a lambda, catch, for or resource variable, written after a type or before `->`
    (`possible`, the field as candidate);
  - any other receiver (`possible`: every member of that name, and arity, in the view,
    `receiver_type_unknown`, `method_reference` for `::`, capped at 64 with `ResultsCapped`);
  - an answer with no hit is never complete (`TargetUnresolved` when nothing else says why), and
    a workspace-symbol query over 256 bytes says `TargetUnresolved`.
  Three reasons were added to `IncompleteReason` (`TargetUnresolved`, `TargetNotIndexed`,
  `ResultsCapped`) and one to `Reason` (`SupertypeNotIndexed`, wire code `supertype_not_indexed`;
  additive, plan §6.6).
- **As built (IDX-10, 2026-11-19):** `queries::{definition, hover, document_symbols,
  workspace_symbols}` over an `Answer` (`Pinned`: the answer, `HEAD`, freshness), reading text
  through `Sources` (`ViewService::sources`, 16 reads in flight), with a `Deadline` on the
  `WallClock` (`ClockDeadline`: plan §2.7's 300 ms and 2 s, `QueryConfig`) read between files and
  reads and inside the resolution (at its first lookup and every 64 after; each lookup is a few
  binary searches); at the deadline the answer holds what was found and says `deadline`. A
  generation's name tables (every declaration by simple name, the types apart, each type's members
  by parent, the distinct names in one text for the symbol scan) are built once per generation and
  kept in `QueryCache`, which the daemon holds beside the view service. Hover is the definition's
  envelope plus the declarations it binds to (signature, Javadoc's words), the module (`:` before
  M3) and `status_line()`. Workspace symbols match equal, prefix, case-insensitive prefix, then
  camel humps (`NPE`, `NuPoEx`), best first, 100 by default (`ResultsCapped` past it);
  constructors are left out (their type is listed). Tests: `crates/jfl-application/tests/queries.rs`
  over `fixtures/java/projects/shop` (javac 21 compiles it; summaries in
  `fixtures/java/summaries/orchard/projects/`), with the review's probes as fixtures
  (`com.acme.probe`, a default-package `Map`, `shop/Discount.java`, `shop/Retry.java`): each probe
  records where javac binds the site (read with `javap -c` from the compiled fixture) and its
  answer must be `exact` on that declaration or not `exact` at all
  (`should_match_javac_or_stay_possible_when_the_review_s_probes_are_asked`, 26 probes of both
  reviews), and every declaration name and reference site of the project is asked once: only a
  declaration's own name is `exact`
  (`should_answer_exact_only_on_a_declaration_s_own_name_when_any_name_is_asked`). Goldens in
  `crates/jfl-application/tests/fixtures/queries/` (36 definitions and 2 symbol answers, rendered
  through `jfl-protocol`'s wire types by `jfl_testkit::query_golden`); unit tests of the scanner,
  the matcher and the resolver (a supertype cycle, a spent budget, a 3,000-class hostile file cut
  at the deadline, qualified names). Mutation checks, each made by hand and reverted, each failing
  a test: a supertype outside the index ignored for overloads, the anonymous-class scope, a member
  type's shadow, the field-before-type receiver rule, the cap, a silent "outside", the deadline of
  reads and of the resolution, an unknown receiver's reason, a long symbol query, private,
  interface-static and package-private inheritance, the textual override, an enum constant's body,
  element keys, `case` labels, local binders, the same-package and on-demand steps made exact
  again, default-package visibility and the first segment of a qualified name; and the gate
  (`admit` removed, or overloads let through).
  `just bench-queries SIZE` measures it (`doc/bench/README.md`): on medium a definition takes
  0.21 ms p50 and 2.3 ms p95 alone, 0.55 and 2.7 ms with its request; a symbol query 9.0 ms p50
  and 12.1 ms p95 (prefix, 100 symbols); the first query of a generation builds the tables in
  74–82 ms. Of 200 sampled definitions at reference sites on medium none is exact (by design),
  123 are possible only, 33 outside the index, 44 unresolved; the scope rules had called 46 exact
  after the first review (66 in the first version).
- **Decided (IDX-10, 2026-11-19; a narrowed mechanism, not a changed decision, per the
  orchestrator and ADR 0038):** `workspace/symbol` scans the generation's name table rather than
  an `fst` set: on spring-framework's 96k distinct names an `fst` build took 91–122 ms (plus
  the sort), against a scan of 4.5 ms; the names are rebuilt with each generation, and camel humps
  are a scan either way (plan §5.5). `fst` stays ADR 0025's choice for name sets built once and
  kept (a library JAR's, PROJ-04).
- **As built (IDX-10 slice 2, 2026-11-19, fixed after its review 2026-11-23): the syntax-tree
  resolver.** The indexer's grammar gives the queries the tree's facts about one site:
  `SyntaxIndexer::site_syntax` (a provided method answering `None`, so the testkit's canned
  indexer proves nothing), reached through `Sources::site_syntax`, and asked only for a site whose
  shape the allowlist can prove (`queries::provable`: a qualified site only behind `this.`).
  `jfl-adapter-java`'s `scopes.rs` parses the request's file (the work budget of indexing; no
  facts for a file with Unicode escapes or a syntax error, or over 1 MiB, whose parse could pass
  the deadline), finds the site's path from the unit in one descent (O(depth), at most
  `MAX_SITE_DEPTH` = 512 levels, deeper sites give no facts) and returns plain data
  (`jfl_application::SiteSyntax`): the site's form (bare name, call, `this.` name or call, type,
  annotation, `new T(…)` without a body, or other, from two explicit place lists; the qualifier
  of `X.this` and `X.super` is a type name, never a bare name, JLS 15.8.4), every node around it to
  the unit classified by `scopes::KINDS` (all 146 named kinds of the orchard grammar, each code,
  callable, field, type, body, unit or denied;
  `scopes::tests::should_classify_every_kind_once_when_the_grammar_is_read` fails when the
  grammar's kinds and the table differ) with the positions a kind is denied in (a class body
  under `new` or an enum constant, a type declared in a block, a member's initializer, a block in
  a class body, a callable outside a type body, a record's header), and whether a variable,
  parameter, pattern, lambda, catch, `for` or resource variable, local type or type parameter of
  the site's name is declared anywhere in a callable or type around it (one pre-order scan, only
  for a walk that reached the unit). The descent, the walk out and the scan are charged to the
  parse's work budget (`scopes::tests::should_walk_in_linear_steps_when_a_site_is_deep_or_its_method_is_long`
  counts steps on 60,000 parentheses and a 6,000-term concatenation); the adapter holds no clock
  (fnd §2.4), so the deadline is read before the parse and by the resolution after it.
  `queries::resolve::prove` holds the allowlist, in one place (`ALLOWLIST`: form, innermost
  frame and summary shape to a construct), checks every frame against the summary (each type a
  member of the next, the callable a member of the innermost), and implements, conservatively:
  a bare name or call and `this.` in a method's or constructor's body (the innermost type with a
  member of that name owns it, through closed hierarchies; a type whose hierarchy leaves the index
  ends the proof; the one field declared or inherited, the one overload applicable by arity);
  a simple type or annotation name in a body, a member's header or a field's header (member
  types first, inherited ones included, then the type's own name, JLS 6.4.1; the file's
  top-level types; single-type imports) and in a type's header from the type around it (JLS
  6.3, 8.1.6); `new T(…)` of a class by its one constructor applicable by arity. A field or member
  type a supertype declares hides every one of that name further up that branch, inherited or not
  (JLS 8.3, 8.5; javac's access error sends the search outward: the review's H1). A supertype's
  method is overridden only by a same-signature method of a proper subtype on the same branch of
  the walk (JLS 8.4.8.1); same-signature methods from two branches both stay, so the call is not
  exact (javac chooses by JLS 15.12.2.5, which the summary cannot prove; the third review). A file that uses
  Lombok counts as having members outside the index (`resolve::generates_members`). The round-1
  inheritance rules stand. One scoping order everywhere, the JLS one (the second review's N4 and
  N5): three slice-1 goldens that pinned a candidate javac does not bind were corrected
  (`def.probe2.inherited-member-type-over-own-name` now lists `SelfBase.Self`;
  `def.probe2.type-parameter-after-annotation` and `def.probe2.member-type-parameter-after-annotation`
  offer no candidate for a type variable). **The javac differential**
  (`jfl-adapter-java/tests/definitions/main.rs`): every reference site of `projects/shop` (203,
  the reviews' probes among them, `com.acme.probe3.QualThis` the slice-2 review's) and of a corpus
  generated from a fixed seed (`tests/definitions/generator.rs`: 32 case files of anonymous classes
  in arguments, behind a nested `new`, a `[` or a lambda, in array initialisers, lambdas, local
  classes, enum constant bodies, `switch` rules, locals, pattern and `for` variables named like a
  field, comments and strings holding braces, annotations whose strings spell a name, member,
  static nested and generic classes, class headers and type-parameter bounds naming a member type,
  a member class of a library supertype whose inherited members share the outer class's names,
  private and cross-package package-private fields and member types hiding public ones (a
  superclass, an interface constant, a second top-level class, `new` of the hidden type), `X.this`
  beside a field named `X`, a record's implicit field and accessor, same-signature methods from two
  branches of a hierarchy (an abstract superclass method against an interface default, covariant
  returns across a class and an interface or two interfaces, identical abstract methods); 922
  sites) is asked through
  the view service over the real indexer, and every `exact` answer must name the declaration javac
  binds the site to. javac's bindings are recorded by `just javac-bindings write`
  (`tools/javac-bindings/JavacBindings.java`, the Compiler Tree API, attribution only, the pinned
  JDK; an implicit constructor as its class, a record's implicit accessor as its component) into
  `tests/fixtures/javac/{shop,generated}.tsv` with each file's FNV-1a digest, so the test runs in
  `check` without a JDK and fails on a stale recording; `just javac-bindings check` recompiles and
  compares, the step EVAL-07's `java-fixtures` stage takes on (its card). Floors: 49 of shop's 203
  sites and 232 of the corpus's 922 are `exact`, none wrong. Mutation checks, each made by hand
  and reverted: listed in the slice's commit. On medium, 34 of the 200 sampled reference sites are
  `exact` (small 37), 89 possible only, 33 outside, 44 unresolved (`just bench-queries`).
- **IDX-10 slice 2: the facts are read at query time, (a), not kept in the summary, (b).
  Accepted (owner, 2026-11-24), as the speed sign-off ADR 0031 requires for a regression over
  10 %: a definition at about 1.1 ms p50 and 8.0 ms p95 on medium, against slice 1's 0.21 and
  2.3 ms without the exact proof (BOOT-05).** Medians of three runs, 2026-11-23, the 4 vCPU VM: a definition takes 1.0 ms p50 and
  7.5 ms p95 alone on medium, 1.35 and 8.1 ms with its request, hover 0.94 and 8.1 ms (small 0.83
  and 6.5 / 1.18 and 6.9 / 0.75 and 6.8 ms), against slice 1's 0.21 and 2.3 ms alone and 0.55 and
  2.7 ms with its request; the slice's first version took 3.1 and 15.4 ms before the walk was made
  linear and the parse skipped for unprovable shapes. All inside the `jfl def` row of plan §1.7
  (p50 < 20, p95 < 50 ms). On medium (9,211 files with sites, one thread) a file's parse takes
  0.18 ms p50, 1.4 ms p95, 34–55 ms for the largest (316 KB). Keeping the facts would add every
  site's walk to the index, 14.2 s over medium beside 3.5 s of parsing, a store format bump and a
  new summary golden for every file. A cache of parsed trees by content key, for definitions
  repeated in one file, is the next step if the M1 definition rows need it.
- **Left after IDX-10:**
  - **the syntax-tree resolver's next constructs** (each with its frames in `scopes.rs` and
    `prove.rs`'s `ALLOWLIST`, generator cases and the differential): lambda bodies (their
    parameters already shadow), field initializers and initializer blocks, `super(...)` and
    `this(...)` (the superclass's constructors), `switch` bodies and `case` labels (an enum
    selector's constants), qualified type names and `Outer.this`, single static imports, import
    sites, a record's header and its implicit members, annotation element keys, receivers typed
    by a field's or parameter's declared type; same-package and on-demand steps with PROJ;
  - **a JDK stage** runs `just javac-bindings check` (EVAL-07's `java-fixtures`); until it lands
    the recording is checked only when a person runs the recipe;
  - a cache of the last parsed trees by `SyntaxKey`, if the M1 definition rows need it
    (definitions in one file reparse it each time);
  - the daemon's part (IDX-12, LSP-02): keep one `QueryCache`; resolve `HEAD` for the view;
    fill `Freshness`; map hover to `HoverResult` (the status line, the first declaration, the
    reference summary once IDX-11 lands); pin a request for `workspace/symbol`, which names no
    file (the registry's requests are per file: a request on any file of the view, or a model
    change for a view-level pin, model first);
  - **the name tables' build after each publish (named item, due before the M1 freshness and
    definition rows of plan §1.7 are gated; orchestrator, 2026-11-19):** the first query after a
    publish builds the generation's tables, 74–82 ms on medium (about 290 ms projected on large),
    more than the `jfl def` row's 50 ms p95 alone, and uninterruptible. Build them from the
    previous generation's for the files that did not change (or keep them per file and merge), and
    measure the first query after an edit in `just bench-queries`;
  - source sets and generated roots (PROJ): the same-package and on-demand steps answer `exact`
    again once the view knows them (Defined, above);
  - the large tier (DEC-03): the snapshot assumes every summary of the generation is in memory;
  - LSP-06 completes the 50 golden cases (38 here) and their LSP mapping; the multi-module
    fixtures are PROJ's; BENCH-05's real-corpus cases do not exist yet;
  - `super.` receivers and qualified receivers (`a.B.m()`) stay unknown until the resolver
    (SEM-*).
- **Accept:**
  - [x] component tests over the fixture projects for each operation, including bounded ambiguity never returning another type's member (`tests/queries.rs`: `should_list_every_overload_of_the_owner_and_no_other_type_s_when_the_overload_is_not_proved`, `should_list_inherited_overloads_and_never_another_type_s_when_a_subclass_calls_one`, and one test or more per operation)
  - [x] a deadline hit returns `complete=false` with `deadline` in `incomplete_because` (fake clock): `should_answer_incomplete_with_deadline_when_the_clock_passes_it` (`FakeWallClock::stepping`)
  - [x] envelope goldens for the cases chosen for M1's 50-case gate (started here, completed in LSP-06): 38 in `crates/jfl-application/tests/fixtures/queries/`, 26 of them the reviews' probes with javac's binding as the oracle
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): `just bench-queries small` and `medium` (`doc/bench/README.md`); BENCH-05 has no cases yet
  - [x] an `exact` answer beyond a declaration's own name only where javac binds the site the same, at every site of the fixture projects and the generated corpus (slice 2): `jfl-adapter-java/tests/definitions/main.rs`'s `should_match_javac_or_stay_possible_when_any_site_of_shop_is_asked` and `…_of_the_generated_corpus_is_asked`
- **Impact:** protocol (additive: the `supertype_not_indexed` reason code and three new
  `incomplete_because` wordings); fixtures (the `projects/shop` corpus and its summaries; the
  generated corpus and javac's recorded bindings); ports (`SyntaxIndexer::site_syntax` and
  `Sources::site_syntax`, provided methods); tooling (`just javac-bindings`).

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
- **As built (IDX-11, 2026-11-24):** `queries::references` (`ReferencesQuery`: the position,
  `exact_only`, `include_declaration`) over the pinned generation, sharing definition's binding
  (`bind`): the target is the declaration under the cursor, or what a use is proved bound to
  (`exact`); otherwise every site of the name is `possible` and the answer `TargetUnresolved`.
  **The superset is by name, never by proof:** every indexed summary's own reference sites of the
  target's simple name (never the IDX-08 name list), less only what Java rules out without name
  resolution (`references::compatible`: a call of a type or a field, `new` of a method, a type use
  of a method; an argument count no parameter list accepts, varargs counted; outside the file of a
  `private` target and outside the package of a package-private one, JLS 6.6.1), plus each import
  naming the target in a qualifier (`import a.Outer.Inner;`, `import static a.Type.m;`: the
  extractor keeps only an import's last segment, and the differential found javac binding the
  others). A site `resolve::prove` (IDX-10's allowlist) binds to the target is `exact`; every
  other candidate stays `possible` with its reason, a site proved bound elsewhere included, so the
  superset does not lean on the proofs; files are read and proved 16 at a time, one parse per file
  (`SyntaxIndexer::sites_syntax`). **`complete`** starts from the pin (indexed, fresh, watched, no
  pending change) and is withdrawn with a reason for: an unresolved target; `ImplicitUses` (a
  method or record component whose type's hierarchy leaves the index or has `Object`'s name, a
  parameterless or enum constructor, an annotation's `value`); `GeneratedSourcesUnknown` (a file
  uses a known annotation processor, `resolve::PROCESSOR_PACKAGES`); `TargetDeclaredTwice` (the
  top-level type's qualified name declared twice: source sets not told apart; `exact` demoted);
  deadline, `ModuleStale`, `ResultsCapped` (20,000) and `PossibleLeftOut` (`exact_only`); and,
  for references and definitions alike, `SyntaxErrors` when a file of the view had a syntax error
  when summarised (the parser's recovery may lose sites): a flag of the summary
  (`SummaryParts::syntax_errors`, set by the view service from `Indexed.diagnostics`, kept by the
  store from its format 2: an older entry is a miss, never migrated). Hover
  on a declaration counts its references (`Hover::references`, `12 exact, 3 possible, complete`)
  from the scan, reading and proving only the declaration's own file. Four `IncompleteReason`s
  were added, and `SyntaxErrors` after the review (wire strings, additive). Tests: the javac
  differential of references (`jfl-adapter-java`'s `tests/definitions/references.rs`: 200
  declarations of `shop`, 184 javac sites, 66 exact; 1,208 of the generated corpus, 601 sites, 232
  exact; 183 uses of `shop`, 1,733 sites; none missing, none wrong), the completeness oracle and
  its planted defect, `shop`'s new
  `com.acme.refs` fixtures (comments, strings, a text block, method references, `this(…)`,
  `super(…)`, an enum constructor, an annotation's `value`, a record's accessor, varargs; and after
  the review `Implicit.java`: `super(…)` on an annotated superclass, a variable-arity and a
  private constructor reached by implicit `super()`, a record pattern), the
  javac oracle extended to method references (`T::new` as a constructor, as `new T(…)` is), and
  component tests of each reason in `tests/queries.rs`. Mutation checks, each made by hand and
  reverted, each failing a named test: one file ignored by the scan (the differential and the
  oracle), method references, import qualifiers, varargs arity, any proof taken as `exact`, and
  each completeness reason ignored (implicit uses, twice, processors, deadline, stale, cap).
- **Fixed after the review (IDX-11, 2026-11-24):** `super(…)` on a superclass with a type-use
  annotation (`extends @A B`) had no site (`rightmost_type_name` now reads through
  `annotated_type`, for `new` and `::new` too); a constructor a zero-argument call reaches through
  variable arity, or a private one a nested subclass's implicit `super()` calls, and a record
  component or its accessor (record patterns, JLS 14.30.2) now say `ImplicitUses`; a syntax error
  in the view withdraws `complete` (above). These implicit uses have no positioned tree, so no
  oracle sees them: `references::implicit`'s documentation names the component test of each rule.
- **Decided (orchestrator, 2026-11-24):** `resolve::PROCESSOR_PACKAGES` stays as built,
  `jakarta.persistence` and `javax.inject` included: conservative and honest until PROJ knows the
  processor path (every medium answer is then incomplete).
- **Decided (IDX-11, 2026-11-24):** the IDX-08 limit (`decode_declarations` trusts the head's
  name list) is accounted for, not resolved: references read each summary's own sites, and at the
  small and medium tier the generation holds whole summaries (`SummaryStore::get`); the large
  tier's references path must read the sites from the store too, or check the list against them,
  before it may claim `complete` (Left). The orchestrator's processor rule above is reported as
  `GeneratedSourcesUnknown` (how many files use a known processor), not `GeneratedSourcesMissing`,
  whose count of missing roots would be made up before the project model names any. Hover's
  reference summary counts unread candidates as
  `possible` rather than reading every file: reading them took 22–28 ms p50 and 84–117 ms p95 on
  medium's common names, past the `jfl hover` row; counted, 11.2 and 16.1 ms.
- **Left after IDX-11:**
  - the large tier (DEC-03): reference sites from the store through the name postings (plan
    §5.5), with the name list checked or not used;
  - the known-processor list is a heuristic towards `complete=false`; PROJ's processor path and
    generated roots replace it (on medium every answer is incomplete for `jakarta.persistence` and
    `javax.inject` imports: a decision for the owner whether those stay on the list);
  - a bytecode oracle for implicit uses: compile the fixtures, read each class file's
    `invokespecial`, `invokevirtual` and `invokeinterface` targets, and require each call in
    `exact ∪ possible` of its target or the answer incomplete with a reason covering it (implicit
    `super()`, record accessors, synthetic calls are claims pinned by component tests until then);
  - hover's counts read only the declaration's own file, so a candidate file changed after the pin
    raises no `ModuleStale` there while `refs` would (LSP-03 maps hover's summary);
  - the multi-module fixture's oracle runs (PROJ-02), module visibility narrowing the scan (M3);
  - the CLI's `jfl refs` and `--exact-only` (M2), the LSP mapping of references and of hover's
    summary (LSP-02, LSP-03);
  - a `possible` site whose receiver's type syntax gives (a field's or parameter's declared type)
    could be proved by SEM-*; today only the allowlist's constructs are `exact` (3 of 39,371 sites
    for medium's common names, 46 of 169 for small's rare ones).
- **Accept:**
  - [x] the completeness oracle passes for every module and seed in the gate's budget; a planted defect (ignore one file) makes it fail: `jfl-adapter-java`'s `tests/definitions/references.rs`, `should_find_every_added_use_or_say_incomplete_when_the_oracle_adds_one_to_each_project` (48 seeded trials over each of `shop` and the generated corpus, each one module before M3) and `should_find_a_counterexample_when_the_index_ignores_the_edited_file`; the multi-module fixture is PROJ-02's
  - [x] references inside comments and strings never appear (golden cases): `tests/queries.rs`'s `refs.method.comments-and-strings` and `refs.field.text-block-and-comment` (Javadoc, line and block comments, a string, a text block)
  - [x] an unindexed or stale file makes `complete=false` with its reason (component test): `should_answer_incomplete_with_its_reason_when_a_file_is_pending_stale_or_unwatched`
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): `just bench-queries small` and `medium` (`doc/bench/README.md`); BENCH-05 has no cases yet
  - [x] (orchestrator) javac's references within `exact ∪ possible`, every `exact` javac's, for every declaration of the fixture projects and the generated corpus: `should_hold_every_javac_site_and_only_javac_s_as_exact_when_any_declaration_of_shop_is_asked`, `…_of_the_generated_corpus_is_asked`, `should_hold_every_javac_site_of_the_bound_declaration_when_refs_are_asked_at_a_use`
- **Impact:** protocol (additive: five `incomplete_because` wordings); cache format (the store's
  entry format 2: the syntax-error flag); fixtures (`projects/shop`'s `com.acme.refs`, its summaries, `shop.tsv` re-recorded with member references, two query goldens); ports (`SyntaxIndexer::sites_syntax`, `Sources::sites_syntax`, provided methods); tooling (`tools/javac-bindings` records method references).

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
- **Decided (orchestrator, IDX-03 review):** result payloads take the typed path, as LSP-01's card
  records (plan §5.3): the daemon writes a response's result from the typed struct to the frame
  body, and the CLI client decodes it with `codec::decode_response`, not through `Value`.
- **Decided (orchestrator, LSP-01 review):** never call `lsp_server::Message::write` or `read`, nor
  `Connection::stdio`, `connect` or `listen`: they pass each message's text to `log::debug!`, so a
  `log` bridged into `tracing` at DEBUG would log source text (fnd §5); the root `clippy.toml`
  denies them in every crate, and frames go through `jfl-protocol`. If `log` is bridged
  (`tracing-subscriber`'s default `tracing-log` feature does it), keep the `lsp_server` target below
  DEBUG. The daemon host in `jfl-composition`
  is the first to install a subscriber.
- **From FORMAL-04:** the driver's obligations are `doc/formal-models.md`'s "IDX-12's
  obligations" (the daemon section): the start order lock, unlink, bind, ready; the shutdown order
  stop accepting, unlink, release; the lock file never removed nor its descriptor inherited; a
  crash (the host's abort) that closes descriptors without the shutdown; overlays and requests
  keyed by connection; the spawn lock's second check and the bounded handshake; re-sent documents
  and request on every connection; a host the driver can step one action at a time. The
  `proptest-state-machine` test also checks `EventuallyServed`'s reduction against the host: once
  faults stop, a waiting client is served within a bounded number of steps. Decide whether the
  daemon notices its socket file removed under it (a tmp cleaner on plan §5.3's `/tmp` fallback)
  and stops; the model does not cover that yet, so it changes first (ADR 0007). From the FORMAL-04
  review: close-on-exec for every descriptor (the lock, the listener, connections, the proxy's
  spawn lock; only the notify socket is inherited) and workers that die with the daemon
  (`PR_SET_PDEATHSIG` or a pipe-close watch); deadlines on the proxy's wait for the accept, the
  handshake and each reply, an expiry treated as a reset; after locking and on every idle tick,
  the lock path still names the locked inode (device and inode), else shut down; `setsid` as the
  daemon's first act, before the lock.
- **Reuse considered:** std file locks (Rust 1.89), `rustix`, Tokio's `UnixListener`, `lsp-server`'s
  message type; sccache's spawn handshake, Biome's `lsp-proxy` and buck2's version check copied as
  designs (ADR 0021). As built: all of these; `proptest-state-machine` weighed and not taken (below);
  no daemonizing or lock crate (plan §16.3).
- **Decided (IDX-12, 2026-11-19):** the daemon notices its socket file removed under it and stops:
  on its tick it compares the file at the path with the one it bound, and shuts down as on SIGTERM
  (no proxy can reach it, and a spawned daemon fails the lock it holds, so serving on would starve
  every new client). Model first: `formal/daemon.qnt`'s fault `sweep` and daemon move
  `noticeSwept`, `OwnerReachable` allowing the swept state, the scenario `sweptSocketStops` (its
  regression trace committed), the liveness control `progressStepIgnoringSweep` and its cycle test;
  Apalache at `steps = 8` took 552 s (was 462 s), inside `formal.yml`'s budget
  (`doc/formal-models.md`, decision 8).
- **Decided (IDX-12):** the notify socket is a path, one per spawn (`<state>/n-<12 hex digits>`:
  the process's pid and its spawn count, a fixed width checked with `sock` against `sun_path` when
  the state directory's socket path is made), not an inherited descriptor: every descriptor stays
  close-on-exec, with no exception. A shared path let a notice from an earlier, abandoned spawn's
  daemon reach the next spawner (found by the driver's generated `spawnTimeout` steps). The
  proxy's waits are `poll(2)` with the time left (its socket non-blocking; the notify socket
  polled with the child's pidfd on Linux, in slices of `jfl_process::EXIT_RECHECK` elsewhere, as
  `jfl-process` waits), and a proxy refused the spawn lock blocks on it (`Flock::acquire`): no
  sleep and no socket timeout in `jfl-adapter-ipc` or the proxy.
- **Decided (owner, 2026-11-24, H1; ADR 0039):** a timed wait is a deadline on an event or a
  periodic tick, and only in `jfl-composition`'s `daemon/timer.rs` (the daemon's idle and
  lock-path tick, its handshake and write deadlines: Tokio's `interval` and `timeout`), under the
  new `timer` grant `architecture.toml` gives `jfl-composition` alone; plain sleeps and polling
  loops stay banned everywhere, and the `sleep` grant stays `jfl-testkit`'s. ADR 0039 supersedes
  ADR 0010's sleeping rule; plan §6.5 and fnd §2.4 say the same.
- **Decided (IDX-12):** a version mismatch replaces the old daemon (ADR 0021, plan §6.6): the
  daemon answers `VersionMismatch` and keeps the connection open for `jfl/stop` alone (frozen
  across majors), which it honours; the proxy sends `jfl/stop` on that connection, reads the
  answer and the connection's end, connects again and spawns its own. The proxy decides, not the
  daemon, because only the proxy knows it needs its own version: a daemon cannot tell an upgrade
  from a stale client. A proxy whose own spawn mismatches too (a broken install) gives up with
  `Incompatible` instead of looping. Pinned by
  `tests/daemon.rs::should_stop_the_daemon_of_another_major_and_start_its_own_when_a_proxy_connects`
  and `::should_serve_only_stop_when_the_handshake_announced_another_major`.
- **Decided (IDX-12 review, M1):** each connection has a writer task draining a bounded outbox
  (64 replies), each write under a 30 s deadline; an expired write or a full outbox severs the
  connection (its tasks end, its socket closes), so the loop never waits on a peer and no
  half-written frame is followed by another. Pinned by
  `tests/daemon.rs::should_keep_serving_others_and_sigterm_when_a_client_stops_reading`.
- **Decided (IDX-12):** plan §5.3's proposed `/tmp/jfl-<euid>/` fallback is not built: a socket path
  over `sun_path`'s limit is refused with the limit named (`jfl_adapter_ipc::socket`), so the
  fallback stays Proposed.
- **As built (IDX-12):** `jfl-adapter-ipc` (state directory, `Flock`, socket path, Tokio listener
  with the peer-uid check, std client with deadlines, notify datagrams, `lsp_server::Message`
  classification of frames from `jfl-protocol`'s bounded decoder); `jfl-composition::daemon` (the
  steppable host, `stepped` for the driver, the proxy, the run loop with `setsid` first, SIGTERM,
  the idle tick with the lock-path and socket-path checks, the shutdown order, `jfl/stop`, the
  `ProcessLauncher` on `jfl-process`'s detached mode, and the request seam `service::Service`, whose
  `Unwired` answers status without views and every query with an error, never `complete=true`);
  `jfl daemon` in `jfl-cli`. The state directory reaches the services through `run::run`'s
  factory, called with the checked `StateDir` once the lock is held (the summary store's place,
  IDX-09). Not wired yet: `lsp_server`'s `log` is not bridged (no subscriber is installed);
  a ranged `didChange` is LSP-02's document sync (the host keeps full-text changes).
- **Remaining (IDX-12 lands as a slice, 2026-11-19):** the `endpoint` contract suite (below; LSP-02
  needs the daemon, not the suite); the tracing subscriber the
  daemon is first to install, keeping the `lsp_server` target below DEBUG; workers that die with
  the daemon belong to the first worker (SEM-04), the daemon starts none yet. Review LOWs left: the
  proxy's `connect` has no deadline of its own (a Unix connect blocks only on a full backlog); a
  proxy that dies while awaiting leaves its `n-*` notify file; a connection's overlays are not
  bounded (plan §5.9); `Proxy::call` bounds each wait by its reply budget, not by what is left of
  the call's; the liveness control `progressStepIgnoringSweep` runs by hand only (the registry
  takes one control per model); the state directory is checked by its last component only, and
  the shutdown's unlink compares then removes (a window after a lost lock only); nothing is logged
  (refused peers, accept and write failures); `daemon.log` is neither reset to `0600` nor bounded;
  the bench can leave a daemon on an error path; the test daemon is a bin of a production crate
  (a release builds `--bin jfl` only). Fixed in the review round: a malformed request is answered
  with its id when one can be read; a failed accept pauses accepting until the next tick; a proxy
  reaps the daemons it spawned; test guards kill only unreaped children.
- **Accept:**
  - [x] `conformance-replay` green for `daemon`; the negative control fails at its step (the five regression traces and 240 generated traces at six seeds conform; the control diverges at the first shutdown step of each committed trace with one: `tests/replay.rs::should_diverge_at_the_shutdown_when_the_daemon_control_releases_first`)
  - [x] integration tests: a second daemon on the same state dir exits with a clear error; modes are `0700`/`0600` (also under a non-root uid via `setpriv`, since the sandbox runs as root); an existing state dir with mode `0755` or another owner is refused; a peer with another uid is refused; two clients racing start one daemon (`jfl-adapter-ipc/tests/endpoint.rs`, `jfl-composition/tests/daemon.rs`)
  - [x] after a daemon crash a client reconnects and the new daemon holds no overlay of a dead connection (the `daemon-crash-with-open-documents` trace on the host in process; `tests/daemon.rs::should_reconnect_to_a_new_daemon_with_its_document_when_the_daemon_crashed` with a killed process)
  - [x] a reference-machine test of the host's lock, socket and connection states, with `EventuallyServed`'s reduction (once faults stop, every waiting client is served within 60 system moves): plain `proptest`, not `proptest-state-machine`, whose default features add 16 crates against the std-only proptest of `architecture.toml` (orchestrator's decision, IDX-12) (`jfl-composition/tests/host_machine.rs`; ADR 0007, amended; ver §6.4)
  - [x] reports the metrics it affects (`just bench-daemon`, `doc/bench/README.md`); no corpus applies (none of it reads a Java file; the orchestrator to confirm this reading of the clause)
  - [ ] the `endpoint` contract suite (ver §5): one suite over an in-memory duplex and `jfl-adapter-ipc`, with a deliberately broken implementation failing it
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
  bookkeeping. An in-process harness in rust-analyzer's slow-tests pattern (amended: on bounded byte
  pipes, not `Connection::memory()`; ADR 0020).
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
- **Decided (orchestrator, IDX-03 review):** result payloads take the typed path: `lsp_server::Message`
  frames requests, notifications and errors, but a response's result is serialized from
  `jfl-protocol`'s typed struct to the frame body and decoded with `codec::decode_response`, never
  round-tripped through `serde_json::Value` (`lsp_server::Response::result` is a `Value`: about
  20 ms against 9 ms for a 10,000-location response, `just bench-protocol`; plan §5.3). An oversized
  frame is reported by the decoder (`Progress::oversized`) and skipped; answer it with an error and
  carry on.
- **Done note (LSP-01):** `serve` runs one session: a reader thread over `jfl-protocol`'s decoder
  handing each message over a rendezvous channel, the synchronous main loop, a bounded pool
  (4 workers, a queue of 16) and a writer, all scoped threads (the crate's new `spawn` grant, fnd
  §2.4); input held in memory is bounded by frames (worst case in `server.rs`'s
  `INCOMING_CAPACITY`). The `Handler` trait applies notifications in order and answers a request
  now, on the pool against a snapshot, or `MethodNotFound`; a panic in the handler, the snapshot or
  a job is caught (`InternalError` for a request, a lost notification) and the session goes on.
  `$/cancelRequest` is the loop's own bookkeeping: a request cancelled in flight is answered
  `RequestCancelled`, one cancelled in the queue never starts, a later cancel is ignored; a request
  reusing an in-flight id is a null-id `InvalidRequest`. Parsing is one typed pass: a body that is
  not JSON is `ParseError`; JSON that is not a JSON-RPC 2.0 message (`jsonrpc` other than
  `"2.0"`, an `id` that is not an `i32` or a string, which is never taken for a notification) is
  `InvalidRequest`, under the id when it reads; an oversized frame is a null-id `InvalidRequest`
  with `{declared, limit}` as data; all with the session going on. Lost framing is a null-id
  `ParseError` and the end of the session. Results are framed from their typed value
  (`Reply::result`), errors through `jfl-protocol`'s `Response`; `lsp_server::Message::write`,
  `read` and its transports are denied in the root `clippy.toml` (they log message text). The
  in-process harness runs on bounded byte pipes, not `Connection::memory()` (ADR 0020, amended: the
  typed path and the decoder under test). Until `jfl lsp` exists, the `lsp-conformance` stage runs
  the transcripts against the adapter's test-only `jfl-lsp-stub`
  (`crates/jfl-adapter-lsp/tests/lsp_transcripts.rs`, left out of `integration-tests`; runner,
  normalisation and the optional `exit` step in `jfl_testkit::lsp_transcript`). No corpus input
  applies (the adapter reads no source); `just bench-lsp` times the round trip and the
  10k-location answer.
- **Accept:**
  - [x] property tests: arbitrary byte streams never panic the framer and never allocate past the limit (`crates/jfl-adapter-lsp/tests/properties.rs`: arbitrary and header-like bytes in arbitrary reads; every request of generated mixed traffic answered once)
  - [x] transcripts for malformed headers, an oversized frame, an unknown request (`MethodNotFound`) and an unknown notification (ignored) pass (`fixtures/lsp/`: seven malformed-header cases, `oversized-frame`, `malformed-body`, `unknown-request` with both id types, `unknown-notification`, `uri-echo` with `+` and `@`; a negative control fails)
  - [x] `jfl-adapter-lsp` depends only on `jfl-protocol` and `jfl-domain` (architecture policy: `architecture.toml`'s edge row, the `architecture` and `deny` stages)
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): no corpus input applies; `just bench-lsp` times one request's round trip in memory and a 1k- and 10k-location answer on the typed path against a JSON value
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
- **From LSP-01:** point the `lsp-conformance` stage and `fixtures/lsp/` at `jfl lsp`
  (`crates/jfl-cli/tests/lsp_*.rs`, ver §2; the stage's target and `integration-tests`' exclusion
  of it move together) and delete `jfl-lsp-stub`; `serve` returns only once its input ends, so
  `exit` needs a way to stop the session (the reader blocks in `read`). The transcript runner
  checks an exit code only when a transcript has an `{"exit": N}` line: the lifecycle transcripts
  state theirs, and the `malformed-header-*` ones gain one once `jfl lsp`'s exit after lost framing
  is decided.
- **Decided (orchestrator, LSP-01 review):** never call `lsp_server::Message::write` or `read`, nor
  `Connection::stdio`, `connect` or `listen`: they pass each message's text to `log::debug!`, so a
  `log` bridged into `tracing` at DEBUG would log source text (fnd §5); the root `clippy.toml`
  denies them in every crate, and frames go through `jfl-protocol`. If `log` is bridged
  (`tracing-subscriber`'s default `tracing-log` feature does it), keep the `lsp_server` target below
  DEBUG.
- **As built (LSP-02, 2026-11-24):** `jfl lsp [--state-dir DIR] [--idle-timeout SECONDS]`
  (`jfl-composition::daemon::lsp` on the LSP adapter's session): `initialize` answered on the main
  loop at once with `positionEncoding` (`utf-8` when offered), `textDocumentSync` (open/close,
  incremental) and `definitionProvider`, and nothing else; before `initialize` every request is
  `ServerNotInitialized` (-32002) and a notification dropped; after it an unknown request is
  `MethodNotFound` (LSP-01's transcripts now send `initialize` first);
  after `shutdown` every request is `InvalidRequest`; `exit` ends the session (the adapter's new
  `Handler::ended`; its reader thread is detached, so the session does not wait for the client to
  close its input) and the process exits 0 only when `shutdown` came first, 1 otherwise, the
  input's end or lost framing included (the `malformed-header-*` transcripts now state `exit 1`).
  Document sync: the session keeps each open document's text (`jfl_adapter_lsp::document`),
  applies `didChange`'s ranged edits through `jfl-domain` and sends the daemon the whole text with
  the client's version; a change that does not fit, a version that does not increase, or a text
  past `MAX_DOCUMENT_BYTES` (a sixth of the 16 MiB frame, so any text fits a frame escaped)
  parts the document (`ContentModified` until it is opened again). One link thread owns the
  daemon proxy and takes notifications and requests in the order read (`LINK_QUEUE` 64); a cancel
  for a request still waiting for the link is answered `RequestCancelled` at once and the request
  is never asked (the adapter's new `Handler::cancelled`). Each request has one deadline,
  `CALL_BUDGET` (8 s) from its dispatch: the link asks the daemon with what is left (every wait
  of the proxy's call cut there), and a request whose deadline passed in the queue is not asked;
  either way it is answered `ServerCancelled` (-32802), which the client may retry. A proxy write
  whose deadline passed (part of a frame may be on the wire) drops the connection. The daemon serves the views (`daemon::views`: the
  view service on the real adapters, the store under `<state>/summaries`, one `QueryCache`,
  `settle` after a request opened a view and `write_back` in slices of 256 between requests, never
  dropped half done; ports on Tokio's blocking pool of 4 threads and the system clock, the cookie
  deadline in `daemon/timer.rs`), `jfl/definition` and `jfl/hover` over a pinned generation under
  the 300 ms deadline, the view's `HEAD` read from its files (`jfl_adapter_git::snapshot::head_commit`).
  The service seam is async (`Service::answer` returns a future the loop awaits;
  `Service::background` is the work between requests, run when nothing else is ready).
- **Decided (LSP-02):** an open document whose text differs from the disk is answered
  `ContentModified`, not from the disk: the index holds the disk's text, the client's position
  its buffer's, and a wrong `exact` is the worst answer (ADR 0014). Plan §4.4 decides which is
  current: the daemon stamps each overlay with the file's mtime when it arrives, and a disk
  written after it wins (answered from the disk). Indexing overlays is later work.
- **Decided (orchestrator, LSP-02 review): the LSP definition mapping.** `textDocument/definition`
  returns the `exact` locations; or, with none, the bounded overload set of a proved owner (target
  `ambiguous`, plan §2.5); otherwise `[]` (`null` when no name is at the position). The other
  `possible` candidates (same-name members of any type, plan §2.2) are never sent. LSP's
  definition result cannot carry completeness: an empty, incomplete answer is `[]` by necessity;
  hover's status line is LSP's completeness channel (LSP-03), and the `possible` set and
  completeness stay in `jfl/definition` and the CLI (`jfl_adapter_lsp::translate::definition`).
- **Decided (orchestrator, LSP-02 review): a column past its line's end is clamped** for the LSP
  carrier (LSP 3.17, IDX-01's decision): `jfl-domain`'s `LineIndex::clamp`, and an additive
  `PositionParams::clamp` (written only when `true`) that `jfl lsp` sets, so the daemon clamps on
  the indexed text of a file the client did not open; `jfl/*` callers without it keep the strict
  refusal (`PositionOutOfRange`).
- **Decided (orchestrator, LSP-02 review):** a hit in an open document whose text differs from
  the disk is refused like the request's own file (`ContentModified`): its range would be placed
  by the disk's text in a buffer that holds another.
- **Decided (LSP-02):** `uri-echo.jsonl` (LSP-01) pinned the stub's invented answer; it is
  replaced by `uri-echo-worktree.jsonl` against a real worktree under `a+b/c@d/`, the same rule
  (echo as sent for a document opened or named, canonical otherwise). Transcripts that name a
  worktree's files write `{{root}}` (the client's spelling) and `{{root_uri}}` (the daemon's),
  substituted by `crates/jfl-cli/tests/lsp_transcripts.rs` over a git copy of
  `fixtures/lsp/worktree/`; the runner keeps `PATH` (the daemon runs `git`).
- **Decided (LSP-02):** a cancellation that comes before the answer cannot be a transcript (the
  runner writes the whole input before the server starts, so nothing holds a request in flight);
  both orders are forced in process (`crates/jfl-composition/tests/lsp.rs`, the link played by
  the test over channels, every reply matched by its id, since the pool answers in any order;
  1,100 runs without a failure, 600 of them six at a time), the after-completion order is also a
  transcript.
- **Accept:**
  - [x] transcripts: lifecycle, both exit codes, cancellation before and after completion, UTF-16 positions on a line with astral characters, `utf-8` when the client offers it (`fixtures/lsp/`: `lifecycle-shutdown-exit` (0), `lifecycle-exit-without-shutdown` (1), `cancel-after-completion`, `position-encoding-utf16-astral`, `position-encoding-utf8`, `document-sync`, `uri-echo-worktree`, run by `crates/jfl-cli/tests/lsp_transcripts.rs` with its negative controls; cancellation before completion in process, Decided above: `crates/jfl-composition/tests/lsp.rs`)
  - [x] a `jfl-bench` suite records `initialize` latency against a warm daemon (ver §10); the 200 ms budget is reported, not asserted in a parallel test (`just bench jfl-lsp SIZE`: `initialize_warm_ms`, beside the cold session's `initialize_ms` and `first_definition_ms`; `doc/bench/README.md`)
  - [x] after the daemon is killed mid-session the next request is answered by a respawned daemon with the open documents restored (e2e) (`crates/jfl-cli/tests/lsp_daemon.rs::should_answer_from_a_restarted_daemon_with_the_open_documents_when_the_daemon_was_killed`: the re-sent overlay makes the new daemon answer `ContentModified` again)
  - [x] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05): `just bench jfl-lsp small` and `medium` (`doc/bench/README.md`); BENCH-05 has no cases yet
- **Left after LSP-02:**
  - **hover over LSP** is LSP-03's, and needs a protocol decision first: `HoverResult` carries no
    completeness, so the status line (plan §2.2, LSP's only completeness channel) cannot say
    `(incomplete: …)` for a resolved target; adding `complete`/`incomplete_because` is a member
    `complete` covers (a new major by `messages.rs`'s rule), a `status` string a minor. The daemon
    answers `jfl/hover` already; no `hoverProvider` is announced until then, and hover round trips
    through the proxy are not measured yet;
  - `jfl/status` still lists no view; `Freshness::synced_at` is `null` (the view service does not
    record when a generation was reconciled);
  - `ServiceConfig::read_budget` and `unwritten_cap` keep their defaults (no memory tier exists
    yet, DEC-03), and the store's cap is a constant (2 GiB);
  - the `ARCH-position-arithmetic` item for the review checklist (fnd §3.3): `.claude/` is the
    orchestrator's, so it is reported, not written;
  - the tracing subscriber (none is installed; `lsp_server`'s `log` stays unbridged);
  - `$/progress`, and a daemon that answers a cancelled request's work early (cancellation is
    the proxy's bookkeeping; the daemon finishes what it started);
  - a daemon spawned with no `PATH` cannot run `git` (`jfl lsp` passes on its own environment's
    allowlist; a client that starts it with none gets an error naming git);
  - **`settle` holds the daemon's loop** for the whole view's read once a request opened it
    (about 2 s on medium, about 9 s projected on large): a request behind it waits, past
    `CALL_BUDGET` (8 s) on large, and is answered with an error the client may retry. IDX-09
    says never to drop `settle` half done, so the fix is settling in slices between requests
    (a views model change first, ADR 0007);
  - the session's open documents, like the daemon's overlays, are not bounded in number (plan
    §5.9); each is bounded by `MAX_DOCUMENT_BYTES`;
  - `head_commit` may read `packed-refs` inline on the loop, after the pin (`view.head` can name a
    commit `HEAD` moved to after the pin); a re-sent overlay after a reconnect is stamped with the
    disk's current mtime (the safe direction: `ContentModified`);
  - a "second request during `settle`" row in the `jfl-lsp` suite (its untimed rounds hide it);
  - an open document that differs from the disk costs a read of the file on each request (to
    compare it), inline on the daemon's loop.
- **Impact:** LSP capabilities (lifecycle, document sync, definition); CLI (`jfl lsp`); daemon
  (serves the views); fixtures (LSP transcripts, the LSP fixture worktree).

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
- **From FORMAL-05:** the driver's obligations are `doc/formal-models.md`'s "LSP-04's
  obligations" (the diagnostics section): results keyed by (document, open session, input
  epoch), the epoch bumped on every change of any input of the compile (a dependency's text
  included) and on every `didOpen`, never ordered by the LSP version; publish only a reply for
  the current epoch in the current session, after every such reply; the version tag of the text
  compiled, none when the disk has overtaken the overlay; close publishes an empty set and drops
  later replies; a crash re-queues what it lost and a timeout re-queues its document; the compile
  reads every input's winning text, overlays included, and the scripted compiler echoes inputs,
  session, epoch and incarnation; one publication is one epoch's whole set (a change for SEM-05's
  javac results beside the syntax ones goes through the model first); a coordinator the driver can
  step one action at a time. Each job records what javac actually read: the source files with
  content hashes, the negative lookups (package directories listed), the class folders, the
  classpath and `EnvironmentKey`; a change to any of them bumps the epoch, and the worker reads
  sources through a file manager the daemon controls, or records hashes, so a disk change between
  the server's read and javac's is detected. A document re-sent after a daemon restart keeps the
  client's version (never renumbered); `jfl diag` takes the views per-request stat and cookie
  before it waits. "Only open documents get diagnostics" depends on Claude Code sending `didOpen`
  for the files it edits, unverified until E0 (plan §9.2). The `proptest-state-machine` test also checks `EventuallyUpToDate`'s
  reduction: once edits and faults stop, every open document is up to date within a bounded
  number of steps.
- **Reuse considered:** none fits: the version-tagged publication rule is ours and modelled (ADR
  0007).
- **As built (LSP-04, first slice, 2026-11-24): the coordinator core and the model's driver.**
  `jfl_application::diagnostics::Coordinator<K>` (no I/O, never waits): per document an open
  session and an input epoch, bumped by the model's rules (every `didOpen`; every change of the
  winning text of a file the compile reads, found through a readers index); `start` records the
  job's session, epoch, version tag and the winning text of every input; `deliver` publishes a
  reply only for its document's current epoch in its current open session (`Delivery::Publish`,
  else `Stale`); `did_close` hands back the empty set to publish; `worker_lost` and `timed_out`
  re-queue a current compile; `needs_worker` asks for a spawn while a compile is pending; a
  transition the model does not enable is a `DiagnosticsError`. The overlay-or-disk choice
  compares the caller's stamps (`Text`: a disk text's mtime; an overlay stamped with the file's
  mtime when it arrived, as LSP-02's overlays are), and a compile with the disk ahead carries no
  version. What a compile reads is `track`ed (the syntax pass: the document alone). The testkit's
  `fakes::compiler::ScriptedWorker` runs jobs in any order, crashes and respawns, and its
  diagnostics echo session, epoch, incarnation and every input's text (`Echo`). The driver
  (`crates/jfl-conformance/src/diagnostics.rs`) observes all eleven variables exactly
  (`deny_unknown_fields`, derived `PartialEq`), the ghosts kept by the model's rules from what the
  coordinator returned; `diagnostics` moved from `PENDING` to `DRIVERS` (`PENDING` is empty).
  `just bench-diagnostics SIZE` (`crates/jfl-composition/benches/diagnostics.rs`): the in-process
  cycle from `did_change` through the syntax pass to the publication (`doc/bench/README.md`).
- **Decided (LSP-04):** the reference-machine test is plain `proptest`, not
  `proptest-state-machine` (IDX-09's and IDX-12's decision: its default features add 16 crates
  against the std-only proptest of `architecture.toml`).
- **Decided (LSP-04):** the negative control is a fault of the driver's shell, as the `views`
  control's lagging store is: it publishes the replies the coordinator returns as `Stale` too
  (plan §7.3's "publish every result"), and diverges at step 12 of
  `diagnostics-two-quick-edits-reverse-order` (and at step 6 of `diagnostics-close-during-compile`,
  where a reply is read after the close).
- **Decided (LSP-04):** the coordinator keeps a closed document's entry (its session and epoch
  counters, and the last version), so a reopened document never repeats a key; plan §5.9's bound
  on documents applies to it with the daemon's overlays (Left below).
- **Accept:**
  - [x] `conformance-replay` green for `diagnostics`; the negative control fails at its step (the seven regression traces conform; generated: 100 traces at `0x2a`, 1,000 at `0x5eed`, 40,949 states; `tests/replay.rs::should_diverge_at_the_stale_reply_when_the_diagnostics_control_publishes_every_reply`, step 12; each of six coordinator mutants (publish every reply, track only the document's own file, a crash or a timeout that does not re-queue, the client's version on disk content, a reply published after the close) diverges and fails the reference machine)
  - [ ] a transcript with two quick edits whose results arrive in reverse order publishes only the newer (in process today: `crates/jfl-application/tests/diagnostics_machine.rs::should_publish_only_the_newest_when_two_quick_edits_reply_in_reverse_order`; the transcript needs the publication path, Left below)
  - [x] a `proptest-state-machine` test of the coordinator (open, edit, result, close in any order) against a reference machine (ADR 0007, amended; ver §6.4), over plain `proptest` (Decided above), with `EventuallyUpToDate`'s reduction (60 system moves after the random prefix, then nothing enabled and every open document up to date): `crates/jfl-application/tests/diagnostics_machine.rs`
  - [ ] syntax errors in the corpus's `broken/` files are published with correct UTF-16 ranges
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05) (in process today: `just bench-diagnostics small` and `medium`, `doc/bench/README.md`; didChange to `publishDiagnostics` through `jfl lsp` comes with the publication path)
- **Left after LSP-04's first slice:**
  - **the publication path:** a coordinator per view in the daemon (`daemon::views`), the syntax
    pass on the blocking pool as the worker, `see_disk` from the view service's stat and watch,
    stamps from mtimes; a daemon-to-proxy notification (`jfl/publishDiagnostics`, protocol golden
    first, an additive minor); a server-initiated notification in `jfl-adapter-lsp`'s `serve` (it
    has none: every frame today answers a request); the proxy forwarding it as
    `textDocument/publishDiagnostics` with `version`, the ranges in the negotiated encoding
    through `jfl-domain`'s `LineIndex`; the transcripts (two quick edits in reverse order, close
    publishes the empty set; a reply order a transcript cannot hold is forced in process, as
    LSP-02's cancellation is) and the `broken/` goldens; didChange to `publishDiagnostics` p50
    and p95 through `jfl lsp` on small and medium;
  - obligation 11 in that path (a re-sent document opened at the client's version: the
    coordinator takes any version, the daemon must pass it on); obligations 2 and 8 (what javac
    read, and one publication per epoch when javac's results join the syntax ones) and 12
    (`jfl diag`) are SEM-05's;
  - the coordinator's documents, like the daemon's overlays, are not bounded in number (plan
    §5.9);
  - `jfl-conformance`'s pending path (a model read, not replayed) has no real pending model now:
    `tests/replay.rs::should_warn_of_each_pending_model_when_the_traces_are_replayed` passes over
    an empty list, and the read-only path of generated traces is untested until a model is pending
    again.
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
- **Note (IDX-03 review, L4):** a decoded `WireEnvelope` keeps the daemon's `schema_version`; a
  printer that re-encodes it after dropping a newer minor's unknown members must write
  `SCHEMA_VERSION`, or forward the daemon's bytes unchanged.
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
  - [ ] contract suite green for fake and adapter; the `compilerArgs` finding recorded (and a new ADR superseding 0026 if the mechanism changes, ADR 0038)
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
- **Decided (orchestrator, LSP-01 review):** never call `lsp_server::Message::write` or `read`, nor
  `Connection::stdio`, `connect` or `listen`: they pass each message's text to `log::debug!`, so a
  `log` bridged into `tracing` at DEBUG would log source text (fnd §5); the root `clippy.toml`
  denies them in every crate, and frames go through `jfl-protocol`. If `log` is bridged
  (`tracing-subscriber`'s default `tracing-log` feature does it), keep the `lsp_server` target below
  DEBUG. Check the subscriber's filter does.
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

Cards that undo a temporary pin once upstream releases what we need, and Dependabot's bumps
re-made as signed task commits (fnd §7.4: the bot's pull request is never merged). The
orchestrator checks pinned git dependencies for a release at every ⛳ checkpoint
([`sdlc.md`](sdlc.md) §6) and dispatches the card when its trigger holds.

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

#### DEP-02: Bump similar to 3.2.1
- **Depends on:** FORMAL-02 (the replay harness that uses `similar`). **Trigger:** Dependabot's
  pull request pzmi/jfastlsp#1 ("Bump similar from 2.7.0 to 3.2.1"), whose DCO check rightly
  failed on the bot's unsigned commits. **Model:** light. **Size:** S. **Parallel with:** any.
- **Cites:** fnd §2.5, §7.4; sdlc §5; ADR 0002.
- The first Dependabot bump through the flow fnd §7.4 now records: the same change as the bot's
  (`similar = "3"` in `jfl-conformance`, `Cargo.lock` at 3.2.1) made as one signed task commit
  and landed by `land.sh`; the bot's pull request is not merged and closes itself once `main`
  has the update. similar 3's changelog read against our one call site
  (`Divergence::diff`: `TextDiff::from_lines`, `unified_diff`, `context_radius`, `header`).
- **Reuse considered:** none needed: a version bump of a crate already chosen (FORMAL-02).
- **Accept:**
  - [x] `jfl-conformance` requires `similar = "3"` and `Cargo.lock` holds 3.2.1; the build is `--locked`
  - [x] the divergence report's diff is unchanged: the replay tests that assert its header and hunks pass (`crates/jfl-conformance/tests/replay.rs`: `should_fail_at_the_skipped_value_when_the_negative_control_replays_a_trace`)
  - [x] `bstr`, new in `similar`'s lockfile entry, is named in the commit with its licence and build script; `just check-only deny` passes
  - [x] fnd §7.4 and sdlc §5 record the Dependabot flow: bumps re-made as `DEP-nn` task commits, the bot's pull request never merged, no DCO exemption
- **Impact:** dependency graph (test-only crate).
