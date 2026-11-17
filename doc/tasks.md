# jfastlsp: task breakdown

This file cuts the build of [`plan.md`](plan.md) (product and architecture),
[`foundation.md`](foundation.md) (engineering rules) and [`verification.md`](verification.md)
(test levels, oracles, formal models) into small tasks in dependency order. The three documents
own *what*; a citation "plan §5.2", "fnd §3" or "ver §4" points to a section of one of them. This
file owns *order* and *size*. Each task is one focused commit that a fresh agent can finish in one
session after reading only what the card names.

**Live gate:** `just check` (and `just quick` while iterating), with tools from
`mise install --locked`, since FND-02; before it, the fallback gate (`cargo fmt --all --check &&
cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked`).
Since FND-03 the orchestrator lands commits with `scripts/land.sh` ([`sdlc.md`](sdlc.md) §2). mise and just come from `scripts/bootstrap-tools.sh` (fnd §8), which
the SessionStart hook runs in cloud sessions. Until FND-01 lands there is no Cargo workspace and
the gate is vacuous; BOOT-00 (plan, process and agent instructions), BOOT-01 (the reuse survey
and early benchmarks applied) and BOOT-02 (the bootstrap review applied) are the only commits.

## Decisions

The ADR number is the decision id (ADR 0001). **Proposed** decisions are orchestrator or planner
defaults the owner may override; until then tasks follow them as written. Only ADRs 0001–0004
and 0032 are the owner's decisions so far; DEC-07 puts the rest to the owner at the FND checkpoint. Only
the owner flips a decision to Accepted or decides a `DEC-` task.

| ADR | Decision | Status |
|---|---|---|
| [0001](adr/0001-record-architecture-decisions.md) | Decisions are Nygard ADRs; the ADR number is the decision id; records, not rules | Accepted; amendment proposed (`adrs`) |
| [0002](adr/0002-integrate-on-main-one-signed-off-commit-per-task.md) | Trunk: one DCO-signed commit per task on `main`, reviewed and landed by the orchestrator (by hand until FND-03) | Accepted; amendment proposed (`scripts/land.sh`) |
| [0003](adr/0003-commit-identity-subject-and-trailer-order.md) | Author and sign-off `Patryk Żmigrodzki <pzmi@users.noreply.github.com>`; subject `<ID>: summary`; `Impact:` line; trailers `Signed-off-by`, `Co-Authored-By`, `Claude-Session` | Accepted |
| [0004](adr/0004-public-repository-no-private-projects.md) | Public repository: never name or allude to private projects; nothing from a private target repository | Accepted |
| [0005](adr/0005-dual-licence-mit-or-apache-2-0.md) | `MIT OR Apache-2.0` | Proposed (DEC-07) |
| [0006](adr/0006-platforms-linux-x86-64-and-arm64-macos-arm64.md) | Linux x86-64 and arm64, macOS arm64; Windows out of scope | Proposed (DEC-07) |
| [0007](adr/0007-formal-methods-required-quint-with-itf-conformance.md) | Quint models (views, daemon, diagnostics) with ITF conformance against the real code | Proposed (DEC-07) |
| [0008](adr/0008-no-event-sourcing-the-index-is-a-rebuildable-cache.md) | No event sourcing; caches are versioned and rebuilt | Proposed (DEC-07) |
| [0009](adr/0009-phase-0-first-owner-run-evaluations-and-decision-tasks.md) | Phase 0 first; `(owner)` items and `DEC-` tasks for owner decisions | Proposed (DEC-07) |
| [0010](adr/0010-workspace-crate-map-and-dependency-law.md) | The crate map and dependency law of plan §6, in `architecture.toml`, enforced by tools (amended) | Proposed |
| [0011](adr/0011-pinned-toolchains-rust-1-97-0-and-tools-versions-toml.md) | Rust 1.97.0; tools in `tools/versions.toml` | Superseded by 0030 |
| [0012](adr/0012-port-xtask-from-mayfly.md) | `tools/xtask` ported from mayfly | Superseded by 0030 |
| [0013](adr/0013-two-carriers-over-one-query-engine-cli-first.md) | JSON CLI first, LSP second, over one engine | Proposed |
| [0014](adr/0014-result-envelope-exact-possible-complete.md) | The result envelope; `complete=true` never wrong | Proposed |
| [0015](adr/0015-no-jvm-and-no-build-on-the-request-path.md) | No JVM and no build on the request path | Proposed |
| [0016](adr/0016-one-daemon-per-user-with-thin-proxies.md) | One daemon per user, thin proxies, single owner by lock | Proposed |
| [0017](adr/0017-bind-views-per-request-uri-exclude-nested-worktrees.md) | Views per request URI; nested worktrees excluded | Proposed |
| [0018](adr/0018-agent-sandbox-and-fail-closed-tools.md) | Agent sandbox, hooks and fail-closed tools | Proposed |
| [0019](adr/0019-build-or-reuse-per-subsystem.md) | Reuse by default; build only what nothing maintained does; `Reuse considered:` on every card | Proposed |
| [0020](adr/0020-lsp-stack-lsp-server-gen-lsp-types-line-index.md) | `lsp-server` + `gen-lsp-types` + `line-index` | Proposed |
| [0021](adr/0021-daemon-ipc-lsp-framing-over-a-unix-socket.md) | IPC is JSON-RPC with LSP framing over the Unix socket; std file lock; sccache's handshake | Proposed |
| [0022](adr/0022-java-grammar-tree-sitter-java-orchard-with-own-queries.md) | `tree-sitter-java-orchard` with our own queries | Proposed |
| [0023](adr/0023-class-files-cafebabe-and-ct-sym.md) | `cafebabe` + `zip`; the JDK API from `ct.sym` | Proposed |
| [0024](adr/0024-git-cli-on-the-reconcile-path-not-gitoxide.md) | The `git` CLI on the reconcile path; stat-only freshness | Proposed |
| [0025](adr/0025-storage-rusqlite-fst-postcard-blake3.md) | `rusqlite`, `fst`, `postcard`, `blake3` (the tier stays DEC-03) | Proposed |
| [0026](adr/0026-gradle-model-by-wrapping-build-server-for-gradle.md) | Gradle model by wrapping microsoft/build-server-for-gradle | Proposed |
| [0027](adr/0027-javac-worker-forked-from-java-language-server-with-scip-java.md) | javac worker forked from java-language-server, with scip-java's plugin | Proposed |
| [0028](adr/0028-jdtls-behind-our-multiplexing-proxy-baseline-and-fallback.md) | jdtls behind our multiplexing proxy: baseline and fallback product | Proposed |
| [0029](adr/0029-e0-on-harbor-with-a-multi-swe-bench-java-control.md) | E0 on Harbor with a Multi-SWE-bench Java control | Proposed |
| [0030](adr/0030-foundation-from-maintained-tools.md) | Foundation from maintained tools: just, mise, cargo-deny, `no_std`, one architecture test | Proposed |
| [0031](adr/0031-benchmark-early-on-a-pinned-real-corpus.md) | Benchmarks from FND-01 on a pinned real corpus; >10 % regressions block from M1 | Proposed |
| [0032](adr/0032-evaluation-target-spring-framework-at-the-bench-pin.md) | E0 to E4 target: spring-framework (public, Apache-2.0) at the bench pin `4cb62d893e09e23791d661e8d9b35aedee7006d0` (DEC-01) | Accepted (owner, 2026-11-17) |

`DEC-` tasks add rows here when the owner decides (DEC-01 target repository, DEC-03 storage tier,
DEC-04 resolver and the SCIP baseline, DEC-05 diagnostics scope, DEC-06 `referencesMode` and
operation priority, DEC-02 go/no-go and the speed targets, DEC-07 the Proposed defaults, DEC-08
the skills plugin).

## Rules for every task

1. **Read first:** this file's Decisions, then the sections the card cites, then the existing
   files it changes. Do not read the documents whole.
2. **Green after every task:** the live gate passes on the exact tree you commit.
3. **Commit:** exactly one DCO-signed commit per task, subject `<ID>: <summary>`, body says why and
   carries an `Impact:` line, trailer block last (`Signed-off-by`, `Co-Authored-By`,
   `Claude-Session`, written by hand). Implementing agents never push, tag or change remote
   settings.
4. **Tests:** `should_<outcome>_when_<condition>`, `// given` / `// when` / `// then`. No sleeps
   (fake clock, `tokio::time::pause`, channels, the testkit `poll` helper); no mocking frameworks;
   tempdirs. A test lives at the lowest level that can fail for the reason it exists (ver §2).
5. **No silencing:** no `#[ignore]`, no `#[allow]`/`#[expect]` without an `architecture.toml`
   exception citing an ADR, no production `todo!`/`unimplemented!`/`dbg!`. Never delete a failing
   test, weaken a modelled invariant, loosen a budget or rewrite a golden fixture to get green.
6. **New crate or dependency:** `members`, `architecture.toml`, `deny.toml` and the crate's
   `clippy.toml` in the same commit; third-party crates added with `cargo add` (latest stable,
   locked) and listed in the crate's external allowlist; named with its reason in the commit
   message. New tools are pinned in `mise.toml`.
7. **Model first:** a change to views and generations, the daemon lifecycle or diagnostics
   publication updates the Quint model, its scenarios and the conformance driver before the Rust
   code (ADR 0007).
8. **Golden first:** a change to a protocol message, the `--json` output or an LSP response
   writes or extends the golden fixture before the code (fnd §11.2).
9. **Decisions:** a new or reversed architectural decision adds an ADR (`adrs new`, or the
   template copied by hand for a reserved number), or amends or supersedes one,
   in the same commit, with the tests and rules that pin it. Agents never flip Proposed to
   Accepted and never decide a `DEC-` task.
10. **Scope:** do not edit `CLAUDE.md` or `.claude/` (the orchestrator's); report a needed change.
11. **Owner-only items** start with `(owner)`: deliver everything up to them and report them as
    pending. Nothing from a private target repository is committed (ADR 0004).
12. **Parallelism:** sequential by default. Only tasks whose "Parallel with" cells name each other
    run concurrently, in separate worktrees with disjoint file sets; at most two at once. Shared
    files (`Cargo.toml`, `Cargo.lock`, `architecture.toml`, `deny.toml`, the `clippy.toml` lists,
    the `justfile`, `mise.toml`, `mise.lock`, `tools/jfl-architecture/`, `formal/models.toml`,
    protocol goldens, `fixtures.lock`, `doc/bench-corpus.md`) follow the orchestrator's
    serialisation table; the later lane rebases.
13. **Reuse first** (ADR 0019): a card that adds a subsystem says what it reused or why not
    (`Reuse considered:`); a hand-built subsystem with a maintained alternative needs an ADR.
14. **Speed is measured, always** (ADR 0031): every product task runs on the bench corpus (small
    at least, `just bench`) and reports the metrics it affects; its correctness tests also run
    against the real-corpus cases that apply (BENCH-05). From M1 on, a tracked metric that
    regresses by more than 10 % fails `bench.yml`'s `bench-gate` on its fixed runner class; a
    local or sandbox run reports and never gates (fnd §9).

**Model key:** `deep` — design, protocols, concurrency, the envelope, Quint models, the
architecture test, untrusted-input parsers. `standard` — well-specified implementation.
`light` — mechanical docs and config. The orchestrator maps them to the strongest, middle and
fastest model it has.

**Size key:** `S` — under an hour of agent work, one module. `M` — one crate, up to about five
`Accept:` items. `L` — a full session; the upper bound for one task (split it if it grows).

**Review checkpoints (⛳):** after a ⛳ task lands green, the orchestrator runs `/review` plus an
adversarial second review over the range since the previous `checkpoint-*` tag, lands the fixes,
tags `checkpoint-<ID>` locally and reports to the owner with the open owner decisions.
Checkpoints: **FND-05** (gate, dependency law and CI; the owner confirms the Proposed defaults,
DEC-07), **FORMAL-05** (models and harness), **DEC-02** (phase 0 go/no-go and the speed
targets), **IDX-15** (M1 and the blocking bench gate), **LSP-06** (M2), **SEM-07** (M4),
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
| FND-06 | Coverage, changed lines, mutation in the diff | FND-05 | standard | M |  | EVAL-03, EVAL-10, BENCH-03 |
| DEC-07 | (owner) Confirm the Proposed defaults | FND-05 | light | S |  | DEC-08 |
| DEC-08 | (owner) Shared skills plugin or repo-local skills | FND-05 | light | S |  | DEC-07 |
| BENCH-01 | Benchmark corpus: pinned real repositories and the fetch script | FND-01 | standard | S |  | FND-02 |
| BENCH-02 | Bench harness: `jfl-bench`, hyperfine, divan, the `rg` floor | BENCH-01, FND-02 | standard | M |  | FND-03 |
| BENCH-03 | Comparison arms and the first baseline | BENCH-02, EVAL-11 | standard | M |  | FND-06 |
| BENCH-04 | Results history and the regression gate on one runner class | BENCH-03, FND-05 | standard | S |  | FORMAL-01 |
| BENCH-05 | Real-corpus golden cases | BENCH-04, EVAL-08 | deep | M |  | EVAL-10 |
| FORMAL-01 | Formal toolchain: Quint, Apalache, JDK pins, `MODELS`, formal stages | FND-05 | standard | M |  | EVAL-12, BENCH-04 |
| FORMAL-02 | Conformance harness: quint-connect drivers, committed-trace replay, negative controls | FORMAL-01 | deep | M |  | EVAL-07, EVAL-05 |
| FORMAL-03 | `views.qnt`: generations, freshness, completeness | FORMAL-02, DEC-02 | deep | M |  | IDX-01, IDX-04 |
| FORMAL-04 | `daemon.qnt`: single owner, spawn, reconnect, no ghost buffers | FORMAL-03 | deep | M |  | IDX-02 |
| FORMAL-05 | `diagnostics.qnt`: staleness | FORMAL-04 | deep | M | ⛳ | IDX-03 |
| DEC-01 | (owner) Choose the target Java repository | — | light | S |  | FND-01, FND-02 |
| EVAL-01 | Evaluation harness: Harbor adapter, arms, transcript metrics | FND-02 | standard | M |  | FND-07, FND-03 |
| EVAL-11 | jdtls behind our multiplexing proxy (`jfl-jdtls`) | FND-02 | deep | M |  | FND-07, FND-03, FND-04 |
| EVAL-02 | E0 task set from the target repository's history | DEC-01, EVAL-01 | standard | M |  | FND-05 |
| EVAL-03 | E0 arms, Harbor images and runbook | EVAL-01, EVAL-11 | standard | M |  | FND-06 |
| EVAL-12 | E0 control set from Multi-SWE-bench Java | EVAL-01 | standard | S |  | FORMAL-01 |
| EVAL-04 | (owner) Run E0 and report | EVAL-02, EVAL-03, EVAL-12 | standard | M |  | — |
| EVAL-05 | E1 in the harness: parse and summarise the corpus on the bench runner and the target | BENCH-04 | standard | S |  | FORMAL-02 |
| EVAL-06 | E4: jdtls at 1, 3 and 10 worktrees, stock and behind the proxy | EVAL-11, BENCH-02 | standard | M |  | FND-05, EVAL-08 |
| EVAL-07 | Golden resolution corpus and `java-fixtures` stage | FORMAL-01 | deep | L |  | FORMAL-02 |
| EVAL-08 | SCIP oracle (`jfl-oracle`) | EVAL-07 | deep | M |  | EVAL-06 |
| EVAL-09 | E2 resolver bake-off | EVAL-07, EVAL-08, EVAL-10, BENCH-05 | deep | L |  | DEC-03 |
| EVAL-10 | E3 `javac` worker spike from the forked compiler core | EVAL-01 | standard | M |  | FND-06, BENCH-05 |
| DEC-03 | Storage tier (from E1) | EVAL-05 | deep | S |  | DEC-04, DEC-05, DEC-06, EVAL-09 |
| DEC-04 | Resolver and the SCIP baseline (from E2) | EVAL-09 | deep | S |  | DEC-03, DEC-05, DEC-06 |
| DEC-05 | Semantic diagnostics in v1 (from E3) | EVAL-10 | deep | S |  | DEC-03, DEC-04, DEC-06 |
| DEC-06 | `referencesMode` default and operation priority (from E0) | EVAL-04 | deep | S |  | DEC-03, DEC-04, DEC-05 |
| DEC-02 | Go/no-go memo, the kill criterion and the speed targets | DEC-03, DEC-04, DEC-05, DEC-06, DEC-07, EVAL-06, BENCH-03 | deep | M | ⛳ | — |
| IDX-01 | Domain: paths, positions and encodings | DEC-02 | standard | M |  | IDX-04, FORMAL-03 |
| IDX-02 | Domain: summaries, symbols, cache keys, the envelope | IDX-01 | deep | M |  | IDX-04, FORMAL-04 |
| IDX-03 | Protocol v1: messages, versions, frame decoder, goldens | IDX-02 | deep | M |  | IDX-05, IDX-06, FORMAL-05 |
| IDX-04 | `jfl-process`: argv-only child runner | DEC-02 | deep | M |  | IDX-01, IDX-02, FORMAL-03 |
| IDX-05 | Git adapter: roots, identity, freshness stat, changed files | IDX-01, IDX-04 | standard | M |  | IDX-03, IDX-06 |
| IDX-06 | FS adapter: scanning, nested-worktree exclusion, watcher hints | IDX-01 | standard | M |  | IDX-03, IDX-05 |
| IDX-07 | Syntax adapter: tree-sitter summaries and the golden Java corpus | IDX-02 | deep | L |  | IDX-08 |
| IDX-08 | Summary store at the DEC-03 tier | IDX-02, DEC-03 | standard | M |  | IDX-07 |
| IDX-09 | View registry, generations and freshness (views driver) | IDX-05, IDX-06, IDX-07, IDX-08, FORMAL-03 | deep | L |  | LSP-01 |
| IDX-10 | Queries: `sym`, `def`, `hover` | IDX-09 | deep | L |  | LSP-01 |
| IDX-11 | Queries: `refs`, the `possible` superset, completeness | IDX-10 | deep | L |  | IDX-12 |
| IDX-12 | Daemon and IPC: lock, socket, spawn, reconnect (daemon driver) | IDX-03, IDX-09, FORMAL-04 | deep | L |  | IDX-11 |
| IDX-13 | `jfl` CLI: subcommands, output, `--json`, exit codes | IDX-11, IDX-12 | standard | M |  | LSP-02 |
| IDX-14 | Plugin, skill, `AGENTS.md` and cloud setup | IDX-13 | standard | M |  | LSP-02 |
| IDX-15 | M1 gate suite and the blocking bench gate | IDX-14 | deep | M | ⛳ | — |
| LSP-01 | LSP adapter on `lsp-server` and the LSP conformance harness | IDX-03 | deep | L |  | IDX-09, IDX-10 |
| LSP-02 | `jfl lsp`: lifecycle, encodings, document sync, deadlines | LSP-01, IDX-12 | deep | L |  | IDX-13, IDX-14 |
| LSP-03 | Navigation methods and `jfl impls` | LSP-02, IDX-15, DEC-06 | standard | L |  | LSP-04 |
| LSP-04 | Syntax diagnostics and the coordinator (diagnostics driver) | LSP-02, FORMAL-05 | deep | L |  | LSP-03, LSP-05 |
| LSP-05 | Plugin variants and `.lsp.json` | LSP-03 | standard | M |  | LSP-04 |
| LSP-06 | M2 gate | LSP-03, LSP-04, LSP-05 | standard | M | ⛳ | — |
| PROJ-01 | Gradle model: wrap build-server-for-gradle, opt-in, fingerprint cache | LSP-06 | deep | L |  | PROJ-03 |
| PROJ-02 | Modules, visibility and `EnvironmentKey` in the envelope | PROJ-01 | deep | M |  | PROJ-04 |
| PROJ-03 | Class-file and JAR reader on cafebabe | LSP-06 | deep | L |  | PROJ-01 |
| PROJ-04 | JDK API index from `ct.sym` and the shared library index | PROJ-03 | standard | M |  | PROJ-02 |
| PROJ-05 | Library types in hover and definition; M3 gate | PROJ-02, PROJ-04 | standard | M |  | — |
| SEM-01 | Resolver core per DEC-04 | PROJ-05, DEC-04 | deep | L |  | SEM-04 |
| SEM-02 | Resolver: inferred receivers and generated members | SEM-01 | deep | L |  | SEM-05 |
| SEM-03 | Exact answers in the envelope; envelope-honesty mutants | SEM-02 | deep | M |  | SEM-05 |
| SEM-04 | `javac` worker (forked core, scip-java plugin) and pool | PROJ-02, DEC-05 | deep | L |  | SEM-01 |
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
puts the Proposed defaults and the skills question to the owner (DEC-07, DEC-08).

#### FND-01: Workspace, toolchain, crate stubs, mise
- **Depends on:** none. **Model:** standard. **Size:** M. **Parallel with:** DEC-01.
- **Cites:** plan §6.1, §6.3, §6.5, §1.6, §16; fnd §2.1, §2.3, §2.4, §4.1, §4.2, §8; ADR 0005, ADR 0006, ADR 0010, ADR 0030.
- Create the Cargo workspace with every production, test-only and tool-only crate of plan §6.1 as
  a stub: `Cargo.toml` (explicit `members`, `exclude = ["spikes"]` so the archived E1 spike stays
  outside, resolver 3, `[workspace.package]` with `version = "0.1.0"`, `edition = "2024"`,
  `rust-version = "1.97"`, `license = "MIT OR Apache-2.0"`, repository, readme;
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
  - [ ] the fallback gate is green; `cargo build --locked --workspace` succeeds
  - [ ] `cargo metadata --format-version 1 --no-deps` lists exactly the crates of plan §6.1 (minus `jfl-fuzz` and `jfl-resolve`), and nothing under `spikes/`
  - [ ] `grep -L 'publish = false' crates/*/Cargo.toml tools/*/Cargo.toml` prints nothing; every member has `[lints] workspace = true`
  - [ ] `target/debug/jfl --version` prints `jfl 0.1.0`; `rustc --version` inside the repository reports 1.97.0
  - [ ] `bash scripts/bootstrap-tools.sh` succeeds in the sandbox, then `mise install --locked` succeeds from an empty mise data directory and `just doctor` exits 0
  - [ ] each pure crate's `lib.rs` starts with `#![no_std]` (a grep in the commit body; FND-07 makes it a rule) and `cargo build --locked -p jfl-domain -p jfl-protocol --target x86_64-unknown-none` succeeds
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
  - [ ] `just quick` and `just check` exit 0 on the tree; `just check-only nope` exits non-zero naming the valid recipes
  - [ ] a path crate wrapped to the wrong parent fails `cargo deny check bans` (confirms `wrappers` on workspace crates)
  - [ ] `std::process::Command` in a crate without the `process` grant, and `std::thread::sleep` in a test outside `poll`, each fail clippy; `poll` times out with its reason in the message
  - [ ] a pure fixture crate with a path dependency on a std crate builds on the host and fails the `nostd` stage
  - [ ] `_preflight` refuses `RUSTC_WRAPPER` (a test that sets it on the child process, not an inline variable in a script)
  - [ ] `tasks.md`'s live gate line still holds: the fallback until this commit, `just check` from it
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
  - [ ] each rule fnd §3.3 assigns to FND-07 fails its negative case with its id and path; a rule without a negative case fails the meta-test
  - [ ] `pure-uses-std` (a `#![no_std]` crate naming `std::fs`) is built by a test and fails to compile
  - [ ] a crate's `clippy.toml` that drops a lint its grants do not cover fails `ARCH-clippy-lists`
  - [ ] `just quick` runs the stage; the tree is green
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
  - [ ] `just adr-index` is clean over ADRs 0001–0031 as committed (fix the index or the check, never the records' decisions)
  - [ ] `adrs new "Test"` in a scratch copy creates the next record from the template and the index check passes after the row is added
  - [ ] `land.sh --no-push` on a scratch repository lands a clean commit, refuses a dirty checkout, aborts on a non-lockfile conflict naming the files, and moves nothing when `just check` fails
  - [ ] `dco-check.sh` fails on a commit without a matching sign-off; the `docs` stage fails when an index row and a file disagree
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
- **Accept:**
  - [ ] the stages, `private-names` included, are in `check` and green on the tree
  - [ ] each fixture fails its stage with the offending file named
  - [ ] `just private-names` finds the planted name written as `Foo Bar`, `foo-bar` and `FooBar` in a changed line and in a commit message, naming the path and line but never printing the name; a missing hash file fails under `CI=true` and in `land.sh` and only warns otherwise
  - [ ] `just doctor` lists the five tools
  - [ ] (owner) further private names added with `scripts/private-name-add.sh` and committed (only hashes)
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
  - [ ] every workflow green on `main` for this commit; `Required` green
  - [ ] zizmor, pinact and actionlint clean; every `uses:` pinned by SHA
  - [ ] the lockstep test fails on a fixture `ci.yml` with a stage in two jobs, with `continue-on-error`, and with `|| true`
  - [ ] (owner) GitHub secret scanning and push protection on
  - [ ] (owner) a branch ruleset on `main` requiring only `Required`, if the owner wants one
- **Impact:** CI topology.

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
  - [ ] the list is in the checkpoint report
  - [ ] (owner) each record decided; statuses read `Accepted (owner, date)` or `Rejected (owner, date)`; the ADR index and the Decisions table updated
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
  - [ ] `doc/skills-plugin.md` committed
  - [ ] (owner) decided; an ADR records it; if "shared", a follow-up task is added here
- **Impact:** none (decision).

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
  - [ ] `just bench-corpus small` fetches and verifies into an empty cache, and is a no-op on the second run; a wrong commit fails verification
  - [ ] the file counts match `doc/bench-corpus.md`; `git status` is clean after a fetch
  - [ ] the helper creates 10 linked worktrees of the small repository
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
  they arrive with IDX-15 for `jfl` and with E4 (EVAL-06) for jdtls.
- **Reuse considered:** hyperfine, divan, criterion, github-action-benchmark's JSON; a bespoke
  timer was rejected.
- **Accept:**
  - [ ] `just bench rg-floor small` writes JSON with machine facts and p50 and p95 per query
  - [ ] `just bench e1 small` reproduces the spike's junit-framework row of `doc/bench-corpus.md` within the noise band the report states (same machine class), or the commit body says why not
  - [ ] two runs on the same machine agree within that noise band
  - [ ] the JSON is accepted by the history tool's format (a test against its schema or examples)
  - [ ] `jfl-bench` imports no production crate
- **Impact:** none (tooling).

#### BENCH-03: Comparison arms and the first baseline
- **Depends on:** BENCH-02, EVAL-11. **Model:** standard. **Size:** M. **Parallel with:** FND-06.
- **Cites:** plan §1.3, §1.7, §9.2; ver §10; ADR 0028, ADR 0031.
- Arms in `jfl-bench`: jdtls behind our proxy (`jfl-jdtls`, EVAL-11) driven over LSP
  (definition, references, workspace symbol, hover, `initialize`), kmp-lsp at a pinned commit, and
  the `rg` floor. In the sandbox the jdtls arm runs only on the synthetic multi-module fixture with
  `java.import.gradle.enabled=false` and `java.import.maven.enabled=false` (a jdtls import runs the
  project's build); kmp-lsp and `rg` run on the small and medium corpus where they need no build.
  The jdtls arm on the corpus is a `bench.yml` dispatch job (BENCH-04 adds it) or an `(owner)`
  run. `doc/bench/baseline.md`: the numbers with machine facts and, per §1.7 target, a
  recommendation to confirm or correct it.
- **Reuse considered:** kmp-lsp as shipped; jdtls's own sharing options (`sharedIndexLocation`,
  the syntax server).
- **Accept:**
  - [ ] the baseline report committed; an arm that cannot run is recorded with the reason, never dropped
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
  reports here and becomes blocking at IDX-15. Dispatch jobs for corpus work the sandbox must not
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
  mise) from the **Apache-2.0 repositories only** (spring-framework, kafka, gradle; never
  elasticsearch, and not junit-framework's EPL-2.0 code), with expected answers from a `scip-java`
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

### Lane FORMAL: formal models and conformance

The toolchain and the conformance harness (FORMAL-01, FORMAL-02) come right after CI, beside
phase 0; the three models (FORMAL-03 to FORMAL-05) describe a product that is built only after
the go decision, so they follow DEC-02 and still precede the tasks that implement what they model
(model first, ADR 0007): `views.qnt` before IDX-09, `daemon.qnt` before IDX-12, `diagnostics.qnt`
before LSP-04.

#### FORMAL-01: Formal toolchain: Quint, Apalache, JDK pins, `MODELS`, formal stages
- **Depends on:** FND-05. **Model:** standard. **Size:** M. **Parallel with:** EVAL-12, BENCH-04.
- **Cites:** ver §6.1, §6.3; fnd §8, §9; ADR 0007, ADR 0030.
- Node with Quint 0.33.1 (`npm:@informalsystems/quint`), Apalache 0.62.1 (checksum in
  `mise.lock`) and a Temurin JDK 21 pinned by mise (current releases re-checked); the `MODELS`
  registry as `formal/models.toml` (file, main module, steps budget), read by the recipes and by a
  test in `jfl-conformance`; recipes `formal-typecheck` (every registered `.qnt`; an unregistered
  `.qnt` fails) and `formal-test` (Quint tests; zero tests fails); `just formal-verify [model]`;
  `formal.yml` (weekly and dispatch, one job per registered model, a test keeps the matrix equal
  to the registry); the `formal` CI job; a trivial `formal/smoke.qnt`.
- **Reuse considered:** mise's npm, java and github backends for Node, Quint, the JDK and Apalache
  (replaces mayfly's `formal.rs` and `apalache.rs` downloads and digests); Quint's TypeScript
  backend, never a run-time download.
- **Accept:**
  - [ ] the stages are green with `smoke.qnt` and fail closed (with the install command) when Node or Quint is missing
  - [ ] an unregistered `.qnt` fails `formal-typecheck`
  - [ ] `just formal-verify smoke` passes locally with the pinned Apalache
  - [ ] `just doctor` reports Node, Quint, Apalache and the JDK
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
- **Decided:** 2026-11-17, option B. [ADR 0032](adr/0032-evaluation-target-spring-framework-at-the-bench-pin.md) (Accepted, owner) names spring-framework (public, Apache-2.0) at the bench pin `4cb62d893e09e23791d661e8d9b35aedee7006d0`. Lombok and MapStruct stay with the synthetic fixtures (EVAL-07, SEM-05, EVAL-10).
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
  - [ ] `cargo run --locked -p jfl-eval -- report <fixture dir>` produces the expected aggregate report (a golden)
  - [ ] the transcript reader extracts each metric from the synthetic fixtures (one test per metric)
  - [ ] a report never contains a prompt, path or source line from the input (a test plants one and asserts its absence)
  - [ ] `jfl-eval` imports no production crate (`architecture.toml` lists it independent)
- **Impact:** none (tooling).

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
  - [ ] two clients in one worktree share one backend and each sees only its own responses with its own ids (fake backend)
  - [ ] two worktrees with the same basename get different `-data` directories
  - [ ] a client that exits mid-request leaves the backend serving the others
  - [ ] (owner) with Claude Code and three subagents in one worktree, one jdtls process runs for it
- **Impact:** none (tooling; the fallback product if DEC-02 says stop).

#### EVAL-02: E0 task set from the target repository's history
- **Depends on:** DEC-01, EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-05.
- **Cites:** plan §9.2 (E0); ver §11; ADR 0004.
- Select 15 to 20 tasks from the target's history (bug fixes, signature changes, cross-module
  features, one rename), each with its starting commit, prompt, test command and known-good diff,
  in the EVAL-01 format. If the target is public, the agent does this and commits the task set
  under `doc/eval/e0/`; if private, the agent commits only a selection guide
  (`doc/eval/e0-selection.md`) and the owner builds the set outside this repository.
- **Accept:**
  - [ ] the task set (public target) or the selection guide (private target) committed
  - [ ] each task's test command fails at the starting commit and passes with the known-good diff (checked by the agent for a public target)
  - [ ] (owner) for a private target: the task set built and checked the same way, kept outside this repository
- **Impact:** none.

#### EVAL-03: E0 arms, Harbor images and runbook
- **Depends on:** EVAL-01, EVAL-11. **Model:** standard. **Size:** M. **Parallel with:** FND-06.
- **Cites:** plan §1.3, §9.2 (E0), §2.6; ver §11; ADR 0028, ADR 0029.
- The four arms as EVAL-01 arm files with their Harbor images: grep only (no language-server
  plugin); the stock `jdtls-lsp` plugin; jdtls behind our proxy (`jfl-jdtls`, EVAL-11, the
  kill-criterion baseline); `kmp-lsp` at a pinned commit. `doc/eval/e0-runbook.md`: machine setup,
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

#### EVAL-06: E4: jdtls at 1, 3 and 10 worktrees, stock and behind the proxy
- **Depends on:** EVAL-11, BENCH-02. **Model:** standard. **Size:** M. **Parallel with:** FND-05, EVAL-08.
- **Cites:** plan §1.3, §1.7, §9.2 (E4); ADR 0028, ADR 0031.
- A `jfl-bench` suite `e4`: jdtls stock (one per client, `-data` as its launcher picks it) and
  behind our proxy (`jfl-jdtls`), driven with `initialize` and a fixed query script over LSP, with
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
  by a recipe, or the cases that need them stay syntax-only and say so); its CI job.
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
- **Cites:** plan §5.6, §9.2 (E2), §9.3; ver §7.1; ADR 0027.
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

#### EVAL-10: E3 `javac` worker spike from the forked compiler core
- **Depends on:** EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-06, BENCH-05.
- **Cites:** plan §3, §9.2 (E3); ver §10; ADR 0027.
- Fork the compiler core of georgewfraser/java-language-server (MIT; pinned base commit,
  licence and notices kept) into `java/javac-worker/` as the spike: one warm JVM compiling single
  files through its long-lived `JavacTask` with body erasure, `-proc:none -implicit:none` and a
  given `-sourcepath` and classpath, with scip-java's plugin optionally loaded; an ECJ batch arm
  (`-proceedOnError`). `jfl-eval e3` drives both over a list of files and reports p50, p95, RSS,
  false errors, the plugin's attribution cost and, given several worktrees' classpaths, the number
  of distinct `EnvironmentKey`s (plan §5.4). Measured on the multi-module fixture project with a
  classpath the fixture declares (no Gradle run in the sandbox); a module of the small bench
  corpus, whose classpath needs a Gradle resolve, is an `(owner)` or `bench.yml` dispatch run.
- **Reuse considered:** the forked compiler core (ADR 0027) instead of a blank spike; ECJ as a
  comparison arm; jdtls's javac backend (incubating) and NetBeans' nb-javac for ideas only.
- **Accept:**
  - [ ] fixture results committed in `doc/eval/e3-results.md` with machine facts, for javac and ECJ
  - [ ] (owner) one module of the small corpus measured the same way; aggregates added
  - [ ] (owner) run on two modules of the target repository and over 10 worktrees' classpaths; aggregates added
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
- **Cites:** plan §2.5, §9.2; ADR 0013.
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
  kill criterion of plan §1.3 against E0 and E4 (jdtls behind our proxy as the baseline), the §1.7
  speed targets against BENCH-03's baseline, the recommended decision and what changes in the
  plan if it is "go with changes" (a list of task-card edits). A draft ADR (Proposed) recording
  the decision. If the decision is to stop and ship `jfl-jdtls` instead, the memo lists the
  tasks that promote it from tool to product and replace the IDX lane.
- **Accept:**
  - [ ] memo and draft ADR committed; every number traces to a committed report
  - [ ] (owner) go, go with changes, or stop, and the §1.7 targets confirmed; the ADR's status says so and `doc/tasks.md` reflects it
- **Impact:** none (decision).

### Lane IDX: M1, the CLI vertical slice

M1 (plan §10.1): syntax index, envelope, daemon, CLI and plugin. Starts after the go decision.

#### IDX-01: Domain: paths, positions and encodings
- **Depends on:** DEC-02. **Model:** standard. **Size:** M. **Parallel with:** IDX-04, FORMAL-03.
- **Cites:** plan §2.3, §6.1; fnd §2.3, §4.3; ver §2, §3.
- In `jfl-domain`: validated value types for worktree-relative paths and `file://` URIs (percent
  encoding, no `..` escape), a `LineIndex` over UTF-8 bytes (LF and CRLF, a final line without a
  newline), and pure conversions between byte offset, (line, UTF-16 unit), (line, UTF-8 byte) and
  (line, scalar) in `#![no_std]` code (with `line-index` used directly if it builds under `no_std` +
  `alloc`, otherwise as the property-test oracle, ADR 0020), with typed refusals for positions past
  a line or file end and inside a surrogate pair or a multi-byte sequence. `Range` and an encoding
  tag. No I/O.
- **Reuse considered:** `line-index` (rust-analyzer) for UTF-8 ↔ UTF-16; `text-size`.
- **Accept:**
  - [ ] property tests: round trips over arbitrary UTF-8 including astral characters and CRLF; offsets never split a character
  - [ ] unit tests for each refusal and for the CLI's 1-based byte column (plan §2.3)
  - [ ] `ARCH-domain-purity` green; no `unwrap`, indexing or unchecked arithmetic on inputs
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** public API (domain types).

#### IDX-02: Domain: summaries, symbols, cache keys, the envelope
- **Depends on:** IDX-01. **Model:** deep. **Size:** M. **Parallel with:** IDX-04, FORMAL-04.
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
- **Accept:**
  - [ ] unit tests: `complete` is `true` only when every visible module is indexed and fresh; each missing condition yields its reason
  - [ ] property test: shuffling inputs never changes the envelope's order or content
  - [ ] `SymbolId` round-trips its text form for every declaration kind (property test)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** public API (domain types).

#### IDX-03: Protocol v1: messages, versions, frame decoder, goldens
- **Depends on:** IDX-02. **Model:** deep. **Size:** M. **Parallel with:** IDX-05, IDX-06, FORMAL-05.
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
- **Cites:** plan §6.2, §6.5, §11; fnd §2.4, §4.4; ver §5.
- `jfl-process` (support): run a child from a program path and argv (no shell, no string splitting),
  a cleared environment plus an explicit allowlist, a working directory, a deadline, its own process
  group killed as a group on deadline or drop, stdout and stderr captured up to a byte limit (excess
  counted, not buffered), and an outcome that keeps exited, killed on deadline, failed to start and
  output-truncated apart. Also a long-lived child mode with framed stdin and stdout for the JVM
  helpers (SEM-04, PROJ-01), and a detached mode (`process_group(0)`, null stdin, output to a log
  file) for the daemon launcher (ADR 0021). The `process_runner` contract suite in `jfl-testkit`
  with a scripted fake.
- **Reuse considered:** std `Command` with `process_group(0)` (no `pre_exec`, which is `unsafe`);
  `rustix` for the child's `setsid`; `daemonize`/`fork` rejected.
- **Accept:**
  - [ ] integration tests: a child that ignores SIGTERM is killed with its group at the deadline; a grandchild does not survive; an argument with spaces and quotes arrives intact; the environment holds only the allowlist
  - [ ] the contract suite passes for the fake and the real runner and fails for a deliberately broken runner
  - [ ] clippy's lists refuse `std::process` in every crate without the grant (FND-02's negative case still passes)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
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
- **Reuse considered:** `tree-sitter` + `tree-sitter-java-orchard` (ADR 0022); `tree-sitter-tags`
  rejected (hides ranges and scopes); `java-lang` too new.
- **Accept:**
  - [ ] summary goldens match for every file of `fixtures/java/projects/` and `fixtures/java/resolution/`
  - [ ] property test: arbitrary bytes and mutated corpus files never panic and finish within a bound
  - [ ] `ARCH-fixtures-lock` fails on an edited golden with a stale lock (negative fixture)
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
  - [ ] envelope goldens for the cases chosen for M1's 50-case gate (started here, completed in IDX-15)
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
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** protocol (endpoint); new authority (sockets).

#### IDX-13: `jfl` CLI: subcommands, output, `--json`, exit codes
- **Depends on:** IDX-11, IDX-12. **Model:** standard. **Size:** M. **Parallel with:** LSP-02.
- **Cites:** plan §2.3, §2.4; ver §2, §4; ADR 0013.
- `jfl-cli`: `def`, `refs`, `hover`, `sym`, `status`, `sync`, `daemon` with `--json`,
  `--exact-only`, `--state-dir`; `file:line:col` parsing (1-based, byte columns); compact output
  grouped by file, `exact` first, capped with a count; exit codes of plan §2.4. `--json` goldens
  first (`insta` JSON snapshots with redactions; the JSON Schema generated by `schemars`, committed
  and checked). The `e2e-tests` stage and its CI job (shared with LSP-01's `lsp-conformance`).
- **Reuse considered:** `clap`, `schemars`, `insta`, `assert_cmd`, `trycmd`; LSAP's compact "Showing
  2 of 12" output as an idea.
- **Accept:**
  - [ ] e2e tests with `assert_cmd` over copies of the fixture projects for each subcommand and exit code
  - [ ] `--json` goldens for every subcommand; compact output goldens for `refs` with more hits than the cap
  - [ ] the first command against a stopped daemon spawns it and answers (e2e)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** CLI (v1); `--json` schema (v1).

#### IDX-14: Plugin, skill, `AGENTS.md` and cloud setup
- **Depends on:** IDX-13. **Model:** standard. **Size:** M. **Parallel with:** LSP-02.
- **Cites:** plan §2.4, §2.6; ADR 0013.
- `plugin/cli/` (the CLI plugin: manifest, `bin/jfl` launcher, `skills/jfl/SKILL.md` telling an
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

#### IDX-15: ⛳ M1 gate suite and the blocking bench gate
- **Depends on:** IDX-14. **Model:** deep. **Size:** M. **Parallel with:** none.
- **Cites:** plan §1.7, §10.1; ver §7.2, §10, §11; ADR 0014, ADR 0017, ADR 0031.
- The M1 gate as tests: the envelope on 50 golden cases; the completeness mutation test adding a
  caller in each module of each multi-module fixture; 10 concurrent worktrees (linked, under
  `.claude/worktrees/`) queried in parallel with no cross-view result and no duplicate workspace
  symbol; freshness under shell edits (`git checkout`, `git stash`, `sed -i`, `rm`) answered
  correctly without `sync`; the M1 rows of plan §1.7 measured on the corpus, with the
  `jfl-bench` runners that need a product: freshness lag (edit a file, time until the next
  request sees it) and the marginal cost of 1, 3 and 10 linked worktrees. `bench.yml`'s
  `bench-gate` becomes blocking: from here a tracked metric that regresses by more than 10 % on
  its fixed runner class fails the run on `main`, and a red `bench-gate` is fixed like red CI. It
  stays out of `ci.yml`, `check` and `land.sh` (fnd §9). `doc/eval/m1.md` lists the gate items and
  their tests.
- **Accept:**
  - [ ] every gate item has a passing test named in `doc/eval/m1.md`
  - [ ] `bench-gate` blocks on `main` and fails a planted 20 % regression that reproduces; the M1 rows of plan §1.7 measured, with the reference-machine run (owner) where the sandbox differs
  - [ ] (owner) the E0 task subset rerun with the CLI plugin; aggregates in `doc/eval/m1.md`
  - [ ] (owner) the cloud-session check of IDX-14 done
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none.

### Lane LSP: M2, the LSP carrier

#### LSP-01: LSP adapter on `lsp-server` and the LSP conformance harness
- **Depends on:** IDX-03. **Model:** deep. **Size:** L. **Parallel with:** IDX-09, IDX-10.
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
- **Accept:**
  - [ ] property tests: arbitrary byte streams never panic the framer and never allocate past the limit
  - [ ] transcripts for malformed headers, an oversized frame, an unknown request (`MethodNotFound`) and an unknown notification (ignored) pass
  - [ ] `jfl-adapter-lsp` depends only on `jfl-protocol` and `jfl-domain` (architecture policy)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** new adapter.

#### LSP-02: `jfl lsp`: lifecycle, encodings, document sync, deadlines
- **Depends on:** LSP-01, IDX-12. **Model:** deep. **Size:** L. **Parallel with:** IDX-13, IDX-14.
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

#### LSP-03: Navigation methods and `jfl impls`
- **Depends on:** LSP-02, IDX-15, DEC-06. **Model:** standard. **Size:** L. **Parallel with:** LSP-04.
- **Cites:** plan §2.2, §2.5; ADR 0013, ADR 0014.
- `textDocument/definition`, `references` (`referencesMode` with DEC-06's default; `exact` first),
  `hover` (status line), `documentSymbol`, `workspace/symbol`, `implementation`; the
  implementation query in the application (proved subtype edges `exact`, unresolved supertypes
  `possible`) and `jfl impls`. Transcripts and `--json` goldens.
- **Accept:**
  - [ ] a transcript per method over the fixture projects; `references` in both modes
  - [ ] hover's status line matches the CLI envelope for the same position (a test comparing carriers)
  - [ ] `jfl impls` e2e and golden
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** LSP capabilities; CLI.

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
  - [ ] syntax errors in the corpus's `broken/` files are published with correct UTF-16 ranges
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** LSP capabilities (diagnostics).

#### LSP-05: Plugin variants and `.lsp.json`
- **Depends on:** LSP-03. **Model:** standard. **Size:** M. **Parallel with:** LSP-04.
- **Cites:** plan §2.6; ADR 0013.
- `plugin/lsp/` (the LSP-only plugin with `.lsp.json` using only the documented keys of plan §2.6)
  and the LSP server entry added to the CLI plugin; a test that checks `.lsp.json` against the
  documented key list; coexistence with `jdtls-lsp` and with `jfl-jdtls`'s plugin investigated and
  written down in `doc/plugins.md` (which wins for `.java`, or that they cannot coexist).
- **Accept:**
  - [ ] the key-list test passes and fails on a fixture with an unknown key
  - [ ] `doc/plugins.md` and the plugin README state the minimum Claude Code version the keys need (`requestTimeout` needs v2.1.288 or later, re-checked against the current plugins reference)
  - [ ] (owner) `claude plugin validate --strict` passes for both variants
  - [ ] `doc/plugins.md` states the coexistence result and how it was checked
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** distribution.

#### LSP-06: ⛳ M2 gate
- **Depends on:** LSP-03, LSP-04, LSP-05. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §10.2; ver §11.
- The M2 gate written down in `doc/eval/m2.md` with each item's test: the LSP conformance suite;
  `initialize` latency from the benchmark; shell edits across 3 worktrees through `jfl lsp`.
- **Accept:**
  - [ ] every automatable gate item has a passing test named in `doc/eval/m2.md`
  - [ ] (owner) a real Claude Code smoke test with shell edits across 3 worktrees using the LSP plugin
  - [ ] (owner) the E0 task subset rerun with the LSP plugin; aggregates in `doc/eval/m2.md`
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** none.

### Lane PROJ: M3, project model and library APIs

#### PROJ-01: Gradle model: wrap build-server-for-gradle, opt-in, fingerprint cache
- **Depends on:** LSP-06. **Model:** deep. **Size:** L. **Parallel with:** PROJ-03.
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
- **Accept:**
  - [ ] the completeness oracle passes per module with visibility; a main source set never sees test-only types (test)
  - [ ] two worktrees with diverging classpaths get different `EnvironmentKey`s and do not share semantic results (test)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** `--json` schema (additive: module fields).

#### PROJ-03: Class-file and JAR reader on cafebabe
- **Depends on:** LSP-06. **Model:** deep. **Size:** L. **Parallel with:** PROJ-01.
- **Cites:** plan §5.8, §11; ver §4, §8; ADR 0023.
- `LibraryIndexer` port; `jfl-adapter-classfile` on `cafebabe` 0.9 and `zip` 9: read JARs with caps
  on entry count, sizes and compression ratio, never extracting by entry name; parse class files
  into API summaries (types, members, generic signatures with our own JVMS §4.7.9.1 parser,
  annotations, module info); our own multi-release selection by `--release`. Fixture JARs built from
  committed sources by a test; malformed inputs committed for refusal tests. The `library_indexer`
  contract suite.
- **Reuse considered:** `cafebabe` (0BSD, forbid-unsafe) and `zip` (ADR 0023); `ristretto_classfile`
  the runner-up; kmp-lsp's JAR indexer and ktlsp's `jar.rs` read for layout.
- **Accept:**
  - [ ] API summaries of the fixture JARs match goldens; multi-release selection picks the right version
  - [ ] property tests: arbitrary bytes as a class file or a zip never panic and stay within bounds; a zip bomb is refused
  - [ ] contract suite green for fake and adapter
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** new adapter.

#### PROJ-04: JDK API index from `ct.sym` and the shared library index
- **Depends on:** PROJ-03. **Model:** standard. **Size:** M. **Parallel with:** PROJ-02.
- **Cites:** plan §5.4, §5.8; fnd §6; ADR 0023.
- JDK API summaries from `$JAVA_HOME/lib/ct.sym` for a `--release` (or `jmods`, after the 4-byte
  header; `ristretto_jimage` for `lib/modules` only if a JDK has neither, after a toolchain bump,
  ADR 0023), and a `LibraryStore` in
  `jfl-adapter-store` keyed by content checksum, shared by every view, versioned like the summary
  cache.
- **Reuse considered:** `ct.sym` read with `zip`; `ristretto_jimage` only if needed (MSRV above our
  pin).
- **Accept:**
  - [ ] `java.util.List` and a `--release 8`-only API resolve from the JDK index of the pinned JDK (integration test)
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
- **Cites:** plan §5.6, §6.1, §10.4; ADR 0014; DEC-04's ADR.
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

#### SEM-04: `javac` worker (forked core, scip-java plugin) and pool
- **Depends on:** PROJ-02, DEC-05. **Model:** deep. **Size:** L. **Parallel with:** SEM-01.
- **Cites:** plan §3.1, §3.4, §5.4, §14; ver §5; ADR 0027.
- `java/javac-worker/` hardened from EVAL-10's fork of java-language-server's compiler core (the
  long-lived `JavacTask` with body erasure, `-proc:none -implicit:none`, `-sourcepath`, scip-java's
  plugin for attribution, length-prefixed JSON over stdin and stdout, cancellation) and `Compiler`
  port; `jfl-adapter-javac`: a pool per `EnvironmentKey` through `jfl-process`'s long-lived mode,
  lazy spawn, idle eviction, a cap with LRU eviction, restart on crash with backoff, version-tagged
  jobs. An ADR (reserved number) decides how the worker is built and launched (source-file launch or
  a prebuilt jar). The `compiler` contract suite.
- **Reuse considered:** the forked compiler core and scip-java's plugin (ADR 0027); ECJ kept only if
  E3 favoured it.
- **Accept:**
  - [ ] contract suite green for the scripted fake and the real worker (pinned JDK); a broken fake fails it
  - [ ] killing a worker mid-compile restarts it and the next job succeeds (integration test)
  - [ ] the pool never exceeds its cap (test on fake clock)
  - [ ] runs on the bench corpus (small at least, `just bench`) and reports the metrics it affects; its correctness tests also run against the real-corpus cases that apply (BENCH-05)
- **Impact:** new adapter; Java source in the repository.

#### SEM-05: Semantic diagnostics, processors and `jfl diag`
- **Depends on:** SEM-04, LSP-04. **Model:** deep. **Size:** L. **Parallel with:** SEM-02, SEM-03, SEM-08.
- **Cites:** plan §3.2, §3.3, §7.3; ADR 0015.
- `javac` results through the diagnostics coordinator (the model and driver of LSP-04 cover them;
  extend the driver if the coordinator's inputs change), generated-source roots added, errors on
  processor-generated members suppressed and marked in hover, no semantic diagnostics without a
  classpath (and `status` says why), `jfl diag <file>` waiting for the current version with a
  deadline.
- **Reuse considered:** scip-java's plugin and the forked worker; IntelliJ's processor discovery as
  an idea.
- **Accept:**
  - [ ] the Lombok and MapStruct fixtures produce zero errors with generated sources missing; a real type error in the same files is reported
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
- **Cites:** plan §10.4, §10.6; ver §7.1; ADR 0009, ADR 0027.
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
