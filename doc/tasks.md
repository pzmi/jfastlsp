# jfastlsp: task breakdown

This file cuts the build of [`plan.md`](plan.md) (product and architecture),
[`foundation.md`](foundation.md) (engineering rules) and [`verification.md`](verification.md)
(test levels, oracles, formal models) into small tasks in dependency order. The three documents
own *what*; a citation "plan §5.2", "fnd §3" or "ver §4" points to a section of one of them. This
file owns *order* and *size*. Each task is one focused commit that a fresh agent can finish in one
session after reading only what the card names.

**Live gate:** the fallback gate (`cargo fmt --all --check && cargo clippy --workspace
--all-targets --locked -- -D warnings && cargo test --workspace --locked`) until FND-02 lands,
then `cargo xtask check` (and `cargo xtask quick` while iterating). Until FND-03 lands, the
orchestrator lands commits by hand ([`sdlc.md`](sdlc.md) §2). Until FND-01 lands there is no
Cargo workspace and the gate is vacuous; BOOT-00 (plan, process and agent instructions) is the
only commit.

## Decisions

The ADR number is the decision id (ADR 0001). **Proposed** decisions are planner defaults the
owner may override; until then tasks follow them as written. Only the owner flips a decision to
Accepted or decides a `DEC-` task.

| ADR | Decision | Status |
|---|---|---|
| [0001](adr/0001-record-architecture-decisions.md) | Decisions are Nygard ADRs; the ADR number is the decision id; records, not rules | Accepted |
| [0002](adr/0002-integrate-on-main-one-signed-off-commit-per-task.md) | Trunk: one DCO-signed commit per task on `main`, reviewed and landed by the orchestrator (by hand until FND-03, then `cargo xtask land`) | Accepted |
| [0003](adr/0003-commit-identity-subject-and-trailer-order.md) | Author and sign-off `Patryk Żmigrodzki <pzmi@users.noreply.github.com>`; subject `<ID>: summary`; `Impact:` line; trailers `Signed-off-by`, `Co-Authored-By`, `Claude-Session` | Accepted |
| [0004](adr/0004-public-repository-no-private-projects.md) | Public repository: never name or allude to private projects; nothing from a private target repository | Accepted |
| [0005](adr/0005-dual-licence-mit-or-apache-2-0.md) | `MIT OR Apache-2.0` | Accepted |
| [0006](adr/0006-platforms-linux-x86-64-and-arm64-macos-arm64.md) | Linux x86-64 and arm64, macOS arm64; Windows out of scope | Accepted |
| [0007](adr/0007-formal-methods-required-quint-with-itf-conformance.md) | Quint models (views, daemon, diagnostics) with ITF conformance against the real code | Accepted |
| [0008](adr/0008-no-event-sourcing-the-index-is-a-rebuildable-cache.md) | No event sourcing; caches are versioned and rebuilt | Accepted |
| [0009](adr/0009-phase-0-first-owner-run-evaluations-and-decision-tasks.md) | Phase 0 first; `(owner)` items and `DEC-` tasks for owner decisions | Accepted |
| [0010](adr/0010-workspace-crate-map-and-dependency-law.md) | The crate map and dependency law of plan §6, in `architecture.toml` | Proposed |
| [0011](adr/0011-pinned-toolchains-rust-1-97-0-and-tools-versions-toml.md) | Rust 1.97.0, edition 2024; JDK and tools pinned in `tools/versions.toml` | Proposed |
| [0012](adr/0012-port-xtask-from-mayfly.md) | `tools/xtask` ported from mayfly | Proposed |
| [0013](adr/0013-two-carriers-over-one-query-engine-cli-first.md) | JSON CLI first, LSP second, over one engine | Proposed |
| [0014](adr/0014-result-envelope-exact-possible-complete.md) | The result envelope; `complete=true` never wrong | Proposed |
| [0015](adr/0015-no-jvm-and-no-build-on-the-request-path.md) | No JVM and no build on the request path | Proposed |
| [0016](adr/0016-one-daemon-per-user-with-thin-proxies.md) | One daemon per user, thin proxies, single owner by lock | Proposed |
| [0017](adr/0017-bind-views-per-request-uri-exclude-nested-worktrees.md) | Views per request URI; nested worktrees excluded | Proposed |
| [0018](adr/0018-agent-sandbox-and-fail-closed-tools.md) | Agent sandbox, hooks and fail-closed tools | Proposed |

`DEC-` tasks add rows here when the owner decides (DEC-01 target repository, DEC-03 storage tier,
DEC-04 resolver, DEC-05 diagnostics scope, DEC-06 `referencesMode` and operation priority,
DEC-02 go/no-go, DEC-07 M6).

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
6. **New crate or dependency:** `members` + `architecture.toml` in the same commit; third-party
   crates added with `cargo add` (latest stable, locked) and listed in the crate's external
   allowlist; named with its reason in the commit message.
7. **Model first:** a change to views and generations, the daemon lifecycle or diagnostics
   publication updates the Quint model, its scenarios and the conformance driver before the Rust
   code (ADR 0007).
8. **Golden first:** a change to a protocol message, the `--json` output or an LSP response
   writes or extends the golden fixture before the code (fnd §11.2).
9. **Decisions:** a new or reversed architectural decision adds an ADR (`cargo xtask adr new`,
   `--number N` when reserved; by hand before FND-03), or amends or supersedes one, in the same
   commit, with the tests and rules that pin it. Agents never flip Proposed to Accepted and never
   decide a `DEC-` task.
10. **Scope:** do not edit `CLAUDE.md` or `.claude/` (the orchestrator's); report a needed change.
11. **Owner-only items** start with `(owner)`: deliver everything up to them and report them as
    pending. Nothing from a private target repository is committed (ADR 0004).
12. **Parallelism:** sequential by default. Only tasks whose "Parallel with" cells name each other
    run concurrently, in separate worktrees with disjoint file sets; at most two at once. Shared
    files (`Cargo.toml`, `Cargo.lock`, `architecture.toml`, `tools/xtask/`, `formal/models.toml`,
    protocol goldens, `fixtures.lock`) follow the orchestrator's serialisation table; the later
    lane rebases.

**Model key:** `deep` — design, protocols, concurrency, the envelope, Quint models, the
architecture checker, untrusted-input parsers. `standard` — well-specified implementation.
`light` — mechanical docs and config. The orchestrator maps them to the strongest, middle and
fastest model it has.

**Size key:** `S` — under an hour of agent work, one module. `M` — one crate, up to about five
`Accept:` items. `L` — a full session; the upper bound for one task (split it if it grows).

**Review checkpoints (⛳):** after a ⛳ task lands green, the orchestrator runs `/review` plus an
adversarial second review over the range since the previous `checkpoint-*` tag, lands the fixes,
tags `checkpoint-<ID>` locally and reports to the owner with the open owner decisions.
Checkpoints: **FND-07** (gate, checker and CI), **FORMAL-05** (models and harness), **DEC-02**
(phase 0 go/no-go), **IDX-15** (M1), **LSP-06** (M2), **SEM-07** (M4), **REL-06** (release
preview).

## Overview

| ID | Title | Depends on | Model | Size | ⛳ | Parallel with |
|---|---|---|---|---|---|---|
| FND-01 | Workspace, toolchain, crate stubs, hygiene | — | standard | M | | DEC-01 |
| FND-02 | xtask core: command surface, stages, doctor, preflight | FND-01 | deep | L | | DEC-01 |
| FND-03 | xtask `adr` and `land` | FND-02 | standard | M | | EVAL-01 |
| FND-04 | Architecture checker: policy, graph, external allowlist | FND-03 | deep | L | | EVAL-03, EVAL-05 |
| FND-05 | Architecture checker: semantic rules | FND-04 | deep | L | | EVAL-06, EVAL-10 |
| FND-06 | Security and supply-chain stages | FND-05 | standard | M | | EVAL-08 |
| FND-07 | CI workflows and required checks | FND-06 | standard | M | ⛳ | EVAL-02 |
| FND-08 | nextest profiles, testkit `poll`, coverage stages | FND-07 | standard | M | | EVAL-02 |
| FORMAL-01 | Formal toolchain: Quint, Apalache, JDK pins, `MODELS`, formal stages | FND-08 | standard | M | | — |
| FORMAL-02 | Conformance harness: ITF replay, freshness, negative controls | FORMAL-01 | deep | L | | — |
| FORMAL-03 | `views.qnt`: generations, freshness, completeness | FORMAL-02 | deep | M | | EVAL-07 |
| FORMAL-04 | `daemon.qnt`: single owner, spawn, reconnect, no ghost buffers | FORMAL-03 | deep | M | | EVAL-07 |
| FORMAL-05 | `diagnostics.qnt`: staleness | FORMAL-04 | deep | M | ⛳ | EVAL-09 |
| DEC-01 | (owner) Choose the target Java repository | — | light | S | | FND-01, FND-02 |
| EVAL-01 | Evaluation harness: task sets, arms, transcript metrics | FND-02 | standard | L | | FND-03 |
| EVAL-02 | E0 task set from the target repository's history | DEC-01, EVAL-01 | standard | M | | FND-07, FND-08 |
| EVAL-03 | E0 arms and runbook | EVAL-01 | standard | M | | FND-04 |
| EVAL-04 | (owner) Run E0 and report | EVAL-02, EVAL-03 | standard | M | | — |
| EVAL-05 | E1 spike: parse and summarise a worktree | EVAL-01 | standard | M | | FND-04 |
| EVAL-06 | E4: `jdtls` at 10 worktrees | EVAL-01 | standard | M | | FND-05 |
| EVAL-07 | Golden resolution corpus and `java-fixtures` stage | FORMAL-02 | deep | L | | FORMAL-03, FORMAL-04 |
| EVAL-08 | SCIP oracle (`jfl-oracle`) | FND-05 | deep | M | | FND-06 |
| EVAL-09 | E2 resolver bake-off | EVAL-07, EVAL-08 | deep | L | | FORMAL-05 |
| EVAL-10 | E3 `javac` worker spike | EVAL-01 | standard | M | | FND-05 |
| DEC-03 | Storage tier (from E1) | EVAL-05 | deep | S | | DEC-04..06 |
| DEC-04 | Resolver (from E2) | EVAL-09 | deep | S | | DEC-03, DEC-05, DEC-06 |
| DEC-05 | Semantic diagnostics in v1 (from E3) | EVAL-10 | deep | S | | DEC-03, DEC-04, DEC-06 |
| DEC-06 | `referencesMode` default and operation priority (from E0) | EVAL-04 | deep | S | | DEC-03..05 |
| DEC-02 | Go/no-go memo and the kill criterion | DEC-03..06, EVAL-06, FORMAL-05 | deep | M | ⛳ | — |
| IDX-01 | Domain: paths, positions and encodings | DEC-02 | standard | M | | IDX-04 |
| IDX-02 | Domain: summaries, symbols, cache keys, the envelope | IDX-01 | deep | M | | IDX-04 |
| IDX-03 | Protocol v1: messages, versions, framing, goldens | IDX-02 | deep | M | | IDX-05, IDX-06 |
| IDX-04 | `jfl-process`: argv-only child runner | DEC-02 | deep | M | | IDX-01, IDX-02 |
| IDX-05 | Git adapter: roots, identity, freshness stat, changed files | IDX-01, IDX-04 | standard | M | | IDX-03, IDX-06 |
| IDX-06 | FS adapter: scanning, nested-worktree exclusion, watcher hints | IDX-01 | standard | M | | IDX-03, IDX-05 |
| IDX-07 | Syntax adapter: tree-sitter summaries and the golden Java corpus | IDX-02 | deep | L | | IDX-08 |
| IDX-08 | Summary store at the DEC-03 tier | IDX-02, DEC-03 | standard | M | | IDX-07 |
| IDX-09 | View registry, generations and freshness (views driver) | IDX-05..08, FORMAL-03 | deep | L | | LSP-01 |
| IDX-10 | Queries: `sym`, `def`, `hover` | IDX-09 | deep | L | | LSP-01 |
| IDX-11 | Queries: `refs`, the `possible` superset, completeness | IDX-10 | deep | L | | IDX-12 |
| IDX-12 | Daemon and IPC: lock, socket, spawn, reconnect (daemon driver) | IDX-03, IDX-09, FORMAL-04 | deep | L | | IDX-11 |
| IDX-13 | `jfl` CLI: subcommands, output, `--json`, exit codes | IDX-11, IDX-12 | standard | M | | LSP-02 |
| IDX-14 | Plugin, skill, `AGENTS.md` and cloud setup | IDX-13 | standard | M | | LSP-02 |
| IDX-15 | M1 gate suite | IDX-14 | deep | M | ⛳ | — |
| LSP-01 | JSON-RPC base protocol and the LSP conformance harness | IDX-03 | deep | L | | IDX-09, IDX-10 |
| LSP-02 | `jfl lsp`: lifecycle, encodings, document sync, deadlines | LSP-01, IDX-12 | deep | L | | IDX-13, IDX-14 |
| LSP-03 | Navigation methods and `jfl impls` | LSP-02, IDX-15, DEC-06 | standard | L | | LSP-04 |
| LSP-04 | Syntax diagnostics and the coordinator (diagnostics driver) | LSP-02, FORMAL-05 | deep | L | | LSP-03 |
| LSP-05 | Plugin variants and `.lsp.json` | LSP-03 | standard | M | | LSP-04 |
| LSP-06 | M2 gate | LSP-03, LSP-04, LSP-05 | standard | M | ⛳ | — |
| PROJ-01 | Gradle model: opt-in, fingerprint cache | LSP-06 | deep | L | | PROJ-03 |
| PROJ-02 | Modules, visibility and `EnvironmentKey` in the envelope | PROJ-01 | deep | M | | PROJ-04 |
| PROJ-03 | Class-file and JAR reader | LSP-06 | deep | L | | PROJ-01 |
| PROJ-04 | JDK API index and the shared library index | PROJ-03 | standard | M | | PROJ-02 |
| PROJ-05 | Library types in hover and definition; M3 gate | PROJ-02, PROJ-04 | standard | M | | — |
| SEM-01 | Resolver core per DEC-04 | PROJ-05, DEC-04 | deep | L | | SEM-04 |
| SEM-02 | Resolver: inferred receivers and generated members | SEM-01 | deep | L | | SEM-05 |
| SEM-03 | Exact answers in the envelope; envelope-honesty mutants | SEM-02 | deep | M | | SEM-05 |
| SEM-04 | `javac` worker and pool | PROJ-02, DEC-05 | deep | L | | SEM-01 |
| SEM-05 | Semantic diagnostics, processors and `jfl diag` | SEM-04, LSP-04 | deep | L | | SEM-02, SEM-03 |
| SEM-06 | SCIP differential gates | SEM-03, EVAL-08 | deep | M | | — |
| SEM-07 | M4 gate | SEM-05, SEM-06 | standard | M | ⛳ | — |
| REL-01 | Call hierarchy and `jfl calls` | SEM-07 | standard | L | | REL-03 |
| REL-02 | Resource limits and watcher recovery | SEM-07 | deep | L | | REL-03 |
| REL-03 | Fuzz lane | SEM-07 | standard | M | | REL-01, REL-02 |
| REL-04 | Crash and restart soak | REL-02 | deep | M | | REL-05 |
| REL-05 | Mutation and benchmark lanes | REL-02 | standard | M | | REL-04 |
| REL-06 | Release workflow and release preview | REL-01, REL-03, REL-04, REL-05 | standard | M | ⛳ | — |
| DEC-07 | (owner) Build M6 or not | SEM-07 | light | S | | REL-01 |
| SCIP-01 | SCIP baseline import | DEC-07, REL-06 | deep | L | | — |
| SCIP-02 | SCIP baseline in the envelope and gates | SCIP-01 | standard | M | | — |

## Tasks

### Lane FND: foundation

#### FND-01: Workspace, toolchain, crate stubs, hygiene
- **Depends on:** none. **Model:** standard. **Size:** M. **Parallel with:** DEC-01.
- **Cites:** plan §6.1, §6.3, §1.6; fnd §2.1, §4.1, §4.2; ADR 0005, ADR 0006, ADR 0010, ADR 0011.
- Create the Cargo workspace with every production, test-only and tool-only crate of plan §6.1 as
  a stub: `Cargo.toml` (explicit `members`, resolver 3, `[workspace.package]` with
  `version = "0.1.0"`, `edition = "2024"`, `rust-version = "1.97"`, `license = "MIT OR
  Apache-2.0"`, repository, readme; `[workspace.lints]` exactly as fnd §4.2), each crate's
  `Cargo.toml` (`publish = false`, `[lints] workspace = true`, only the path dependencies plan
  §6.3 allows) and `src/lib.rs` with a `//!` naming its single responsibility and role;
  `jfl-cli` with `[[bin]] name = "jfl"` whose `main` prints the version; `tools/xtask` printing
  usage; `tools/jfl-oracle` and `tools/jfl-eval` as binaries printing usage. `rust-toolchain.toml`
  (1.97.0, `rustfmt`, `clippy`, `llvm-tools-preview`); `.cargo/config.toml` (alias
  `xtask = "run --locked --package xtask --"`); `deny.toml` per fnd §7.4 with `[graph] targets`;
  `architecture.toml` with `schema = 1` and the `[workspace]`, `[roles]`, `[purity]` and
  `[dependencies.normal]` tables of fnd §3.1 and plan §6.3 filled, empty `[external.*]` and
  `[exceptions]` (the checker arrives in FND-04); `.editorconfig`, `.gitattributes`
  (`* text=auto eol=lf`); `Cargo.lock`. No third-party dependency.
- **Accept:**
  - [ ] the fallback gate is green; `cargo build --locked --workspace` succeeds
  - [ ] `cargo metadata --format-version 1 --no-deps` lists exactly the crates of plan §6.1 (minus `jfl-fuzz` and `jfl-resolve`)
  - [ ] `grep -L 'publish = false' crates/*/Cargo.toml tools/*/Cargo.toml` prints nothing; every member has `[lints] workspace = true`
  - [ ] `target/debug/jfl --version` prints `jfl 0.1.0`
  - [ ] `rustc --version` inside the repository reports 1.97.0
- **Impact:** dependency graph (initial).

#### FND-02: xtask core: command surface, stages, doctor, preflight
- **Depends on:** FND-01. **Model:** deep. **Size:** L. **Parallel with:** DEC-01.
- **Cites:** fnd §3.6, §8, §9; ADR 0011, ADR 0012, ADR 0018.
- Port from mayfly's `tools/xtask`: `main.rs` and `cli.rs` (clap; only the commands this task
  delivers exist), `repo.rs` (root and the preflight of fnd §3.6), `process.rs` (stages, fail-closed
  tool requirements, the stage table), `catalog.rs`, `check.rs`, `quick.rs`, `test.rs` (stages
  `unit-tests`, `integration-tests`, `doctests` on nextest), `tools.rs` (`doctor [--json] [--all]`,
  `install-tools`) and `tools/versions.toml` with cargo-nextest only (each later stage adds its
  tool). `check` stages for now: `fmt-check`, `clippy`, `unit-tests`, `integration-tests`,
  `doctests`. Unit tests from the mayfly originals, renamed: catalog ids unique, every subcommand
  in the catalog, built stages equal the catalog's `in_check` rows, a failing stage fails the gate,
  a missing tool is `UNAVAILABLE` and fails, `--only` with an unknown stage exits 2 listing the
  valid ones, the preflight refuses `RUSTC_WRAPPER` and a foreign checkout.
- **Accept:**
  - [ ] `cargo xtask quick` and `cargo xtask check` exit 0 on the tree and print the stage table
  - [ ] `cargo xtask list-checks` prints valid JSON with every stage and its owning task
  - [ ] `cargo xtask doctor` reports cargo-nextest found, missing or wrong, and exits non-zero when it is missing (a test with an empty `PATH`)
  - [ ] `cargo xtask check --only nope` exits 2 and lists the valid stages
  - [ ] `RUSTC_WRAPPER=x cargo xtask quick` is refused with exit 2 (a test, not an inline variable in a script)
  - [ ] `tasks.md`'s live gate line still holds: the fallback until this commit, `check` from it
- **Impact:** none (tooling).

#### FND-03: xtask `adr` and `land`
- **Depends on:** FND-02. **Model:** standard. **Size:** M. **Parallel with:** EVAL-01.
- **Cites:** fnd §9, §10.2, §10.3; ADR 0001, ADR 0002, ADR 0012.
- Port mayfly's `adr.rs` and `adr_files.rs` (`adr new "<title>" [--number N] [--date] [--status]`,
  `adr index [--fix]`, statuses `Proposed`, `Accepted (owner, …)`, `Rejected (owner, …)`,
  `Superseded by NNNN` with annotations, gaps in numbering tolerated) and `land.rs` (`land
  <ref|a..b>… (--push R B | --no-push) [--log F]`: refuse a dirty or stale `main`, cherry-pick on a
  detached head, resolve only `Cargo.lock` and `doc/adr/README.md`, run `check` on the merged
  tree, fast-forward and push only on green). Add the `docs` stage (the ADR index matches the
  files) to `check`. Tests on scratch repositories in tempdirs, as in mayfly.
- **Accept:**
  - [ ] `cargo xtask adr index` is clean over ADRs 0001–0018 as committed in BOOT-00 (fix the index or the parser, never the records' decisions)
  - [ ] `cargo xtask adr new "Test" --number 0099` in a scratch copy creates the file from `doc/adr/template.md` and the index row; a taken number is refused
  - [ ] `land --no-push` on a scratch repository lands a clean commit, refuses a dirty `main`, aborts on a non-lockfile conflict naming the files, and does not move `main` when `check` fails
  - [ ] the `docs` stage fails when an index row and a file disagree
- **Impact:** none (tooling).

#### FND-04: Architecture checker: policy, graph, external allowlist
- **Depends on:** FND-03. **Model:** deep. **Size:** L. **Parallel with:** EVAL-03, EVAL-05.
- **Cites:** plan §6.3; fnd §2.1, §2.2, §2.5, §2.7, §3.1, §3.2, §3.3, §3.4; ADR 0010, ADR 0012.
- Port mayfly's `architecture/{policy,inventory,graph,external,report,rules,fixtures,mod}.rs` with
  the data-driven selectors of fnd §3.1 (`[roles]`, `[purity]`), the rules fnd §3.3 assigns to
  FND-04, and the `architecture` stage in `quick` and `check`. Fixtures under
  `tools/xtask/tests/architecture-{invalid,valid}/`, renamed to jfastlsp's crates, at least one
  negative per rule, including: `domain-depends-on-application`, `adapter-depends-on-adapter`,
  `cli-names-adapter`, `testkit-in-production-closure`, `oracle-imports-domain`,
  `unknown-crate`, `missing-member`, `unlisted-external`, `git-source`, `empty-selector`; and a
  positive `full-topology` fixture. Fill `[external.*]` for the dependencies xtask itself does not
  need (none in production yet).
- **Accept:**
  - [ ] `cargo xtask architecture` is green on the tree and fails each negative fixture with its named rule and path
  - [ ] every rule has a fixture and every fixture a rule (catalog completeness runs in the stage)
  - [ ] a diagnostic names the full dependency path of a forbidden transitive edge
  - [ ] edges are evaluated for the three targets of ADR 0006
- **Impact:** none (tooling); architecture policy now enforced.

#### FND-05: Architecture checker: semantic rules
- **Depends on:** FND-04. **Model:** deep. **Size:** L. **Parallel with:** EVAL-06, EVAL-10.
- **Cites:** plan §6.5; fnd §2.3, §2.4, §3.2, §3.3, §3.5, §3.6; ADR 0010, ADR 0012.
- Port mayfly's `architecture/semantic/{collect,paths,flow,mod}.rs` and the generic rule modules
  (purity, hygiene, CQRS, seams, include, runtime, clippy-config), with every crate-name table
  moved into `architecture.toml` selectors where practical. Implement the rules fnd §3.3 assigns
  to FND-05, including jfastlsp's authority rules (`ARCH-process-authority` with `jfl-process`
  and composition's `launcher` module, `ARCH-child-environment`, `ARCH-socket-authority`,
  `ARCH-fs-authority`). Each pure crate gets its `clippy.toml`. Negative fixtures at least:
  `domain-std-fs`, `domain-system-time`, `domain-hashmap`, `adapter-spawns-command`,
  `child-without-env-clear`, `tcp-listener`, `application-std-fs`, `test-without-should`,
  `test-without-given`, `test-sleeps`, `allow-without-exception`, `cfg-macro-branch`,
  `unbounded-channel`, `anyhow-in-library`, `include-renamed`; positives: an adapter calling
  `jfl-process`, the IPC adapter binding a Unix socket in a tempdir.
- **Accept:**
  - [ ] `cargo xtask check` green; each negative fixture fails with its named rule and `file:line`
  - [ ] a rule's selector that matches nothing fails (`ARCH-missing-selector`)
  - [ ] fnd §3.3 lists exactly the rules the checker reports (`list-checks` and the table agree; a test)
- **Impact:** none (tooling); purity and authority now enforced.

#### FND-06: Security and supply-chain stages
- **Depends on:** FND-05. **Model:** standard. **Size:** M. **Parallel with:** EVAL-08.
- **Cites:** fnd §7.3, §7.4, §8; ADR 0005, ADR 0011.
- Port mayfly's `security.rs` and `action_pins.rs`: stages `deny` (cargo-deny over `deny.toml`),
  `secrets` (gitleaks over the full history, `.gitleaks.toml` with `useDefault` and no
  allowlist), `workflows` (actionlint with shellcheck), `action-pins` (40-hex SHAs with a version
  comment, image digests). Pin cargo-deny, gitleaks, actionlint and shellcheck in
  `tools/versions.toml` at their current releases (checked, never invented). Fixtures for an
  unpinned action, a tag-pinned action, a planted fake secret built at run time.
- **Accept:**
  - [ ] the four stages are in `check` and green on the tree
  - [ ] each fixture fails its stage with the offending file named
  - [ ] `cargo xtask doctor` lists the four tools
- **Impact:** none (tooling).

#### FND-07: ⛳ CI workflows and required checks
- **Depends on:** FND-06. **Model:** standard. **Size:** M. **Parallel with:** EVAL-02.
- **Cites:** fnd §7.1, §7.2, §7.3; ADR 0002, ADR 0006; [`required-checks.md`](required-checks.md).
- `.github/workflows/ci.yml` with the jobs of `required-checks.md` that exist now (`architecture`,
  `static`, `tests`, `linux-arm`, `macos`, `security`, `required`), each `cargo xtask check --only
  <stages>` (platform jobs `cargo xtask test --only platform-tests`, a stage this task adds),
  composite actions `setup-rust` (toolchain, cache keyed by toolchain, lockfile and
  `tools/versions.toml`, saved only on `main`) and `install-scanners`; `security.yml`,
  `codeql.yml`, `scorecard.yml`, `.github/dependabot.yml`, `.github/CODEOWNERS`,
  `.github/ISSUE_TEMPLATE/`, `.github/pull_request_template.md` (for outside contributors).
  Port mayfly's CI lockstep tests (every `in_check` stage in exactly one job; `required` needs
  every job; no `continue-on-error`, `if:`, swallowed failures or steering `env:` in gating jobs;
  `required-checks.md` in step with `ci.yml`). Rewrite `required-checks.md` from the real
  workflow, dropping the "planned" banner for the jobs that now exist.
- **Accept:**
  - [ ] every workflow green on `main` for this commit; `Required` green
  - [ ] actionlint clean; every `uses:` pinned by SHA
  - [ ] the lockstep tests fail on a fixture `ci.yml` with a stage in two jobs, with `continue-on-error`, and with `|| true`
  - [ ] (owner) a branch ruleset on `main` requiring only `Required`, if the owner wants one
- **Impact:** CI topology.

#### FND-08: nextest profiles, testkit `poll`, coverage stages
- **Depends on:** FND-07. **Model:** standard. **Size:** M. **Parallel with:** EVAL-02.
- **Cites:** ver §2, §3; fnd §3.3, §8, §9.
- `.config/nextest.toml` (`default`, `ci` with JUnit and no fail-fast, `serial`; `retries = 0`;
  slow-timeout 30 s × 4; a `daemon-or-jvm` test group bounded to 2); `jfl-testkit::poll`
  (`wait_until(deadline, reason, condition)`, `try_wait_until`), the only sleep `ARCH-test-sleep`
  exempts; `coverage` and `changed-lines` stages with cargo-llvm-cov pinned and
  `coverage-floor.toml` (`overall_line` and `changed_line` set to the measured values rounded
  down, at least 85 and 90; floors only ratchet up); the `coverage` CI job.
- **Accept:**
  - [ ] `check` runs nextest under the profile; a test that sleeps outside `poll` fails `ARCH-test-sleep`
  - [ ] `poll` times out with its reason in the message (a test)
  - [ ] the coverage job is green and in `Required`; an untested changed line fails `changed-lines` on a fixture
- **Impact:** none (tooling).

### Lane FORMAL: formal models and conformance

#### FORMAL-01: Formal toolchain: Quint, Apalache, JDK pins, `MODELS`, formal stages
- **Depends on:** FND-08. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** ver §6.1, §6.3; fnd §8, §9; ADR 0007, ADR 0011.
- `formal/package.json` and `package-lock.json` pinning Quint exactly; Node major, Apalache
  (version and SHA-256) and the JDK major in `tools/versions.toml`; the `MODELS` registry as
  `formal/models.toml` (file, main module, steps budget), read by xtask so adding a model does not
  touch `tools/xtask`; stages `formal-typecheck` (every registered `.qnt`; an unregistered `.qnt`
  fails) and `formal-test` (Quint tests; zero tests fails); `cargo xtask formal-verify [--model M]`;
  `formal.yml` (weekly and dispatch, one job per registered model, a test keeps the matrix equal
  to the registry); the `formal` CI job; a trivial `formal/smoke.qnt`.
- **Accept:**
  - [ ] the stages are green with `smoke.qnt` and fail closed (with the install command) when Node or Quint is missing
  - [ ] an unregistered `.qnt` fails `formal-typecheck`
  - [ ] `cargo xtask formal-verify --model smoke` passes locally with the pinned Apalache
  - [ ] `cargo xtask doctor --all` reports Node, Quint, Apalache and the JDK
- **Impact:** none (tooling).

#### FORMAL-02: Conformance harness: ITF replay, freshness, negative controls
- **Depends on:** FORMAL-01. **Model:** deep. **Size:** L. **Parallel with:** none.
- **Cites:** ver §6.2; plan §7; ADR 0007.
- `jfl-conformance` (test only): an ITF decoder (`#bigint`, sets, maps, variants, the MBT
  `action_taken` and `nondet_picks`), the regression manifest reader, a `Driver` trait (`apply`,
  `observe`), the replayer (compare after every step, initial state included) and the divergence
  report (model, seed, trace, step, action, expected, actual, diff); a binary that replays a
  manifest for xtask. Stages `formal-freshness` (regression traces regenerate byte for byte) and
  `conformance-replay` (seeded generated traces plus the committed ones; the seed printed;
  `cargo xtask conformance [--model] [--seed] [--traces] [--steps]` repeats a run). Prove the
  harness on `smoke.qnt` with a test-only driver over a counter in the harness's own tests, and a
  negative control where the counter skips a value and the replay fails at the named step.
- **Accept:**
  - [ ] `check` runs both stages green; the same seed reproduces a run
  - [ ] the negative control fails at the expected step with expected and actual state in the report
  - [ ] a stale regression trace fails `formal-freshness`
  - [ ] a malformed ITF file is `HARNESS_ERROR`, never a pass
- **Impact:** none (tooling).

#### FORMAL-03: `views.qnt`: generations, freshness, completeness
- **Depends on:** FORMAL-02. **Model:** deep. **Size:** M. **Parallel with:** EVAL-07.
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
- **Depends on:** FORMAL-03. **Model:** deep. **Size:** M. **Parallel with:** EVAL-07.
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
  - [ ] `cargo xtask formal-verify --model daemon` passes at the documented bound, or the report names the tooling blocker (never a fabricated pass)
  - [ ] regression traces committed and fresh
- **Impact:** none (model).

#### FORMAL-05: ⛳ `diagnostics.qnt`: staleness
- **Depends on:** FORMAL-04. **Model:** deep. **Size:** M. **Parallel with:** EVAL-09.
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
  - [ ] `doc/eval/target-criteria.md` committed
  - [ ] (owner) the target repository chosen; an ADR records the choice, naming it only if it is public (otherwise "a private repository meeting the criteria", with its size class) and the commit to pin
- **Impact:** none (decision).

#### EVAL-01: Evaluation harness: task sets, arms, transcript metrics
- **Depends on:** FND-02. **Model:** standard. **Size:** L. **Parallel with:** FND-03.
- **Cites:** plan §1.2, §9.2; ver §11; fnd §2.7.
- `tools/jfl-eval` (tool only, independent): a task-set format (TOML: id, starting commit, prompt,
  test command, known-good diff reference, category), an arm format (plugins, settings,
  environment, setup commands), a reader for Claude Code session transcripts (JSONL) that extracts
  per run: success (from the recorded test-command result), tool calls by kind, tokens, edits
  followed by a compile failure and whether a pushed diagnostic preceded the fix, the histogram of
  LSP operations, empty-reference responses and the next action, time to first LSP answer; and
  `report`, which aggregates runs per arm and per task with means and spreads and writes a report
  with no task text, paths or code (only ids and numbers). Synthetic transcript fixtures.
- **Accept:**
  - [ ] `cargo run --locked -p jfl-eval -- report <fixture dir>` produces the expected aggregate report (a golden)
  - [ ] the transcript reader extracts each metric from the synthetic fixtures (one test per metric)
  - [ ] a report never contains a prompt, path or source line from the input (a test plants one and asserts its absence)
  - [ ] `jfl-eval` imports no production crate (`architecture.toml` lists it independent)
- **Impact:** none (tooling).

#### EVAL-02: E0 task set from the target repository's history
- **Depends on:** DEC-01, EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-07, FND-08.
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

#### EVAL-03: E0 arms and runbook
- **Depends on:** EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-04.
- **Cites:** plan §9.2 (E0), §2.6; ver §11.
- The three arms as EVAL-01 arm files: grep only (no language-server plugin); a tuned
  `jdtls-lsp` (shared library indexes on, Gradle warmed); `kmp-lsp` as is. `doc/eval/e0-runbook.md`:
  machine setup, pinned versions of Claude Code and each arm's server, how to run each task three
  times per arm from a clean worktree, where transcripts go, how to record the test-command
  result, how to measure 10 concurrent worktrees, and how to run `jfl-eval report`. Includes the
  unverified facts E0 must settle (plan §9.2).
- **Accept:**
  - [ ] arm files and runbook committed; `jfl-eval` validates the arm files (a test)
  - [ ] (owner) one dry-run task per arm on the owner's machine, confirming the runbook works
- **Impact:** none.

#### EVAL-04: (owner) Run E0 and report
- **Depends on:** EVAL-02, EVAL-03. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §1.2, §1.3, §9.2; ver §11.
- The owner runs every task three times per arm and hands the transcripts to `jfl-eval report`
  locally. An agent turns the aggregated report into `doc/eval/e0-results.md`: the metrics of plan
  §1.2 rows 1 to 3 per arm, the LSP operation histogram, behaviour on empty references,
  diagnostics wait times, and answers to the unverified facts.
- **Accept:**
  - [ ] (owner) all runs done; the aggregated report produced
  - [ ] `doc/eval/e0-results.md` committed with only aggregates, each number traceable to the report
- **Impact:** none.

#### EVAL-05: E1 spike: parse and summarise a worktree
- **Depends on:** EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-04.
- **Cites:** plan §5.5, §9.2 (E1); ver §10.
- `jfl-eval e1 <root>`: walk a worktree's `.java` files (skipping nested worktrees), parse each
  with tree-sitter-java, extract a throwaway summary (package, imports, declarations, call sites by
  name and arity), and report cold and warm wall time, CPU time, files per second and the RSS of
  the in-memory summaries, with machine facts. A synthetic generator (`jfl-eval gen-java`) makes
  repositories of 1k, 10k and 50k files for a baseline. Spike code: production parsing comes in
  IDX-07.
- **Accept:**
  - [ ] `jfl-eval e1` runs on the generated repositories; results for them committed in `doc/eval/e1-results.md` with machine facts
  - [ ] cold measurement drops the file cache or says it could not (never reports warm as cold)
  - [ ] (owner) run on the target repository; aggregate numbers added to `doc/eval/e1-results.md`
- **Impact:** none.

#### EVAL-06: E4: `jdtls` at 10 worktrees
- **Depends on:** EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-05.
- **Cites:** plan §1.3, §9.2 (E4).
- `jfl-eval e4`: start a pinned `jdtls` per worktree (or shared workspace settings per the tuned
  arm), drive `initialize` and a fixed query script over LSP, and record RSS over time, time to
  first useful answer and project import count, for 1, 3 and 10 worktrees; runbook in
  `doc/eval/e4-runbook.md`. Tested against the multi-module fixture project with a real, pinned
  `jdtls` if the sandbox can fetch it, otherwise the runbook only.
- **Accept:**
  - [ ] `jfl-eval e4` produces a report for the fixture project, or the card's runbook-only fallback is recorded with the reason
  - [ ] (owner) run on the target repository at 1, 3 and 10 worktrees; aggregates in `doc/eval/e4-results.md`
- **Impact:** none.

#### EVAL-07: Golden resolution corpus and `java-fixtures` stage
- **Depends on:** FORMAL-02. **Model:** deep. **Size:** L. **Parallel with:** FORMAL-03, FORMAL-04.
- **Cites:** plan §5.6, §8; ver §4, §7.1; fnd §9.
- `fixtures/java/resolution/`: 200+ small cases by construct (plan §5.6 list plus overloads,
  inheritance, generics, inner, anonymous and local classes, static imports, enums, records,
  sealed types, switch patterns), each a compilable snippet project with an expected-answer file
  (the target of every marked reference: `exact` with its symbol, or the `possible` reason).
  `fixtures/java/projects/`: the projects of ver §4 (single module, multi-module Gradle, nested
  worktrees as a script that builds them, Lombok and MapStruct with generated sources present and
  missing, Java 8, 17 and 21 syntax). The `java-fixtures` stage compiles every fixture with the
  pinned JDK (offline; Lombok and MapStruct jars from a pinned, checksum-verified download cached
  by xtask, or the cases that need them stay syntax-only and say so); its CI job.
- **Accept:**
  - [ ] at least 200 resolution cases, at least 8 per construct of plan §5.6; a coverage table in `fixtures/java/resolution/README.md`
  - [ ] `java-fixtures` green in `check` and CI; a broken fixture outside `fixtures/java/broken/` fails it
  - [ ] no fixture copies code from any real project (synthetic only; ADR 0004)
- **Impact:** none (fixtures).

#### EVAL-08: SCIP oracle (`jfl-oracle`)
- **Depends on:** FND-05. **Model:** deep. **Size:** M. **Parallel with:** FND-06.
- **Cites:** ver §7.1; plan §5.6; fnd §2.7.
- `tools/jfl-oracle` (tool only, independent): read a SCIP index (protobuf), map occurrences to
  (file, range, symbol, role), classify each call site by construct (from its syntax, with its own
  tree-sitter use, not `jfl`'s), and compare a candidate's answers (the `jfl --json` envelope
  shape, or a bake-off adapter's output) with SCIP: precision of `exact`, coverage per construct,
  recall of `exact ∪ possible` when `complete=true`; report with aggregates only. A small SCIP
  fixture generated from a resolution-corpus project (by a pinned `scip-java` when available,
  else a hand-written index with a test that it decodes).
- **Accept:**
  - [ ] the oracle's metrics on hand-made inputs match hand-computed values (tests per metric)
  - [ ] the construct classifier labels the resolution corpus's cases as their directory says (a test)
  - [ ] `jfl-oracle` imports no production crate
- **Impact:** none (tooling).

#### EVAL-09: E2 resolver bake-off
- **Depends on:** EVAL-07, EVAL-08. **Model:** deep. **Size:** L. **Parallel with:** FORMAL-05.
- **Cites:** plan §5.6, §9.2 (E2), §9.3; ver §7.1.
- Run each candidate resolver of plan §5.6 on the resolution corpus through thin wrappers in
  `jfl-eval` (a minimal resolver prototype of our own written as a spike; Stack Graphs for Java;
  `ktcore`; the `kmp-lsp` resolver), feed results to `jfl-oracle`, and write
  `doc/eval/e2-results.md`: precision, coverage by construct and cost per candidate. A candidate
  that cannot be built or run is recorded with the reason, never dropped silently.
- **Accept:**
  - [ ] corpus results for every runnable candidate committed, with the oracle's raw aggregate files
  - [ ] (owner) a SCIP index of the target repository produced with `scip-java` and the bake-off run against it; aggregates added
- **Impact:** none.

#### EVAL-10: E3 `javac` worker spike
- **Depends on:** EVAL-01. **Model:** standard. **Size:** M. **Parallel with:** FND-05.
- **Cites:** plan §3, §9.2 (E3); ver §10.
- A spike Java program (`tools/jfl-eval/java/JavacSpike.java`) that keeps one JVM warm and
  compiles single files through `JavacTask` with `-proc:none -implicit:none` and a given
  `-sourcepath` and classpath; `jfl-eval e3` drives it over a list of files and reports p50, p95,
  RSS and, given several worktrees' classpaths, the number of distinct `EnvironmentKey`s (plan
  §5.4). Measured on the multi-module fixture project.
- **Accept:**
  - [ ] fixture results committed in `doc/eval/e3-results.md` with machine facts
  - [ ] (owner) run on two modules of the target repository and over 10 worktrees' classpaths; aggregates added
- **Impact:** none.

#### DEC-03: Storage tier (from E1)
- **Depends on:** EVAL-05. **Model:** deep. **Size:** S. **Parallel with:** DEC-04, DEC-05, DEC-06.
- **Cites:** plan §5.5; ADR 0009.
- From `doc/eval/e1-results.md`, a draft ADR (Proposed) picking the tier of plan §5.5 with the
  numbers that decide it.
- **Accept:**
  - [ ] the draft ADR committed, citing the E1 numbers
  - [ ] (owner) the ADR accepted, overridden or rejected; the Decisions table updated
- **Impact:** none (decision).

#### DEC-04: Resolver (from E2)
- **Depends on:** EVAL-09. **Model:** deep. **Size:** S. **Parallel with:** DEC-03, DEC-05, DEC-06.
- **Cites:** plan §5.6, §9.3, §10.4; ADR 0009.
- A draft ADR (Proposed) choosing the resolver and the coverage share M4's gate will require, per
  construct where it differs, from `doc/eval/e2-results.md`; if our own, it names the crate
  `jfl-resolve` (pure) and the edges it adds.
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

#### DEC-02: ⛳ Go/no-go memo and the kill criterion
- **Depends on:** DEC-03, DEC-04, DEC-05, DEC-06, EVAL-06, FORMAL-05. **Model:** deep. **Size:** M. **Parallel with:** none.
- **Cites:** plan §1.3, §9.3; ADR 0009.
- `doc/eval/go-no-go.md`: each criterion of plan §9.3 with its numbers and source report, the
  kill criterion of plan §1.3 against E0 and E4, the recommended decision and what changes in the
  plan if it is "go with changes" (a list of task-card edits). A draft ADR (Proposed) recording
  the decision. If the decision is to stop and ship a `jdtls` launcher instead, the memo lists the
  tasks that replace the IDX lane.
- **Accept:**
  - [ ] memo and draft ADR committed; every number traces to a committed report
  - [ ] (owner) go, go with changes, or stop; the ADR's status says so and `doc/tasks.md` reflects it
- **Impact:** none (decision).

### Lane IDX: M1, the CLI vertical slice

M1 (plan §10.1): syntax index, envelope, daemon, CLI and plugin. Starts after the go decision.

#### IDX-01: Domain: paths, positions and encodings
- **Depends on:** DEC-02. **Model:** standard. **Size:** M. **Parallel with:** IDX-04.
- **Cites:** plan §2.3, §6.1; fnd §2.3, §4.3; ver §2, §3.
- In `jfl-domain`: validated value types for worktree-relative paths and `file://` URIs (percent
  encoding, no `..` escape), a `LineIndex` over UTF-8 bytes (LF and CRLF, a final line without a
  newline), and pure conversions between byte offset, (line, UTF-16 unit), (line, UTF-8 byte) and
  (line, scalar), with typed refusals for positions past a line or file end and inside a
  surrogate pair or a multi-byte sequence. `Range` and an encoding tag. No I/O.
- **Accept:**
  - [ ] property tests: round trips over arbitrary UTF-8 including astral characters and CRLF; offsets never split a character
  - [ ] unit tests for each refusal and for the CLI's 1-based byte column (plan §2.3)
  - [ ] `ARCH-domain-purity` green; no `unwrap`, indexing or unchecked arithmetic on inputs
- **Impact:** public API (domain types).

#### IDX-02: Domain: summaries, symbols, cache keys, the envelope
- **Depends on:** IDX-01. **Model:** deep. **Size:** M. **Parallel with:** IDX-04.
- **Cites:** plan §2.1, §2.2, §5.4; ADR 0014.
- In `jfl-domain`: `FileSummary` (package, imports, declarations with kind, name, signature,
  modifiers, Javadoc range, supertypes by name; reference sites by simple name, arity and syntactic
  shape, with comments and strings excluded), `SymbolId` (the plan §2.1 textual form, parsed and
  printed), `SyntaxKey`, `EnvironmentKey`, `SemanticKey` derivations (pure, over digests the
  caller supplies), and the `Envelope` with `exact`, `possible` (each with a `why`), `complete`,
  `incomplete_because`, `view` and `freshness`, plus a pure `completeness(visible modules, index
  state)` function that returns `complete=false` with reasons unless every input is indexed and
  fresh. Ordering: `exact` before `possible`, stable by URI and range.
- **Accept:**
  - [ ] unit tests: `complete` is `true` only when every visible module is indexed and fresh; each missing condition yields its reason
  - [ ] property test: shuffling inputs never changes the envelope's order or content
  - [ ] `SymbolId` round-trips its text form for every declaration kind (property test)
- **Impact:** public API (domain types).

#### IDX-03: Protocol v1: messages, versions, framing, goldens
- **Depends on:** IDX-02. **Model:** deep. **Size:** M. **Parallel with:** IDX-05, IDX-06.
- **Cites:** plan §2.4, §5.3, §6.6; ver §4; fnd §6.
- `jfl-protocol`: the handshake (protocol major and minor, client kind), request and response
  messages for every operation of plan §2.4 and document sync (open, change, close with
  versions), typed errors, the envelope's JSON form with `schema_version` and
  `position_encoding`, and length-prefixed framing with a maximum frame size checked before
  allocation. Goldens for every message first, then the codec (golden first, rule 8).
- **Accept:**
  - [ ] golden tests for every message; a decode of an unknown field succeeds (additive) and of a different major fails with a typed error
  - [ ] a frame over the limit is refused before allocating (a test with a lying length prefix)
  - [ ] property test: encode then decode is identity for generated messages
- **Impact:** protocol (v1).

#### IDX-04: `jfl-process`: argv-only child runner
- **Depends on:** DEC-02. **Model:** deep. **Size:** M. **Parallel with:** IDX-01, IDX-02.
- **Cites:** plan §6.2, §6.5, §11; fnd §2.4, §4.4; ver §5.
- `jfl-process` (support): run a child from a program path and argv (no shell, no string
  splitting), a cleared environment plus an explicit allowlist, a working directory, a deadline,
  its own process group killed as a group on deadline or drop, stdout and stderr captured up to
  a byte limit (excess counted, not buffered), and an outcome that keeps exited, killed on
  deadline, failed to start and output-truncated apart. Also a long-lived child mode with framed
  stdin and stdout for the javac worker (SEM-04). The `process_runner` contract suite in
  `jfl-testkit` with a scripted fake.
- **Accept:**
  - [ ] integration tests: a child that ignores SIGTERM is killed with its group at the deadline; a grandchild does not survive; an argument with spaces and quotes arrives intact; the environment holds only the allowlist
  - [ ] the contract suite passes for the fake and the real runner and fails for a deliberately broken runner
  - [ ] the architecture checker refuses `std::process` anywhere else (a fixture from FND-05 still passes)
- **Impact:** public API (support crate).

#### IDX-05: Git adapter: roots, identity, freshness stat, changed files
- **Depends on:** IDX-01, IDX-04. **Model:** standard. **Size:** M. **Parallel with:** IDX-03, IDX-06.
- **Cites:** plan §4.1, §4.3, §4.6, §14; ver §5; ADR 0017.
- `RepoProbe` port in `jfl-application`; `jfl-adapter-git`: the innermost worktree root of a path
  (cached per directory prefix), the common directory, the `HEAD` and index files to stat for a
  main or linked worktree, the changed files since a tree (`diff --name-only`, `status -z`, parsed
  defensively). An ADR decides the `git` CLI through `jfl-process` versus `gix` on cost and
  fidelity (plan §14). The `repo_probe` contract suite with a scripted fake; `jfl-testkit`
  builders for temporary repositories with linked worktrees under `.claude/worktrees/`.
- **Accept:**
  - [ ] contract suite green for fake and adapter; a broken fake fails it
  - [ ] integration tests: a path in a nested linked worktree resolves to it, not the parent; checkout, stash, reset and commit each change the stat and the changed-file set
  - [ ] the ADR committed (reserved number) with the measurement it used
- **Impact:** dependency graph.

#### IDX-06: FS adapter: scanning, nested-worktree exclusion, watcher hints
- **Depends on:** IDX-01. **Model:** standard. **Size:** M. **Parallel with:** IDX-03, IDX-05.
- **Cites:** plan §4.2, §4.3, §11; ver §5; ADR 0017.
- `SourceTree` port; `jfl-adapter-fs`: list `.java` files under a root, skipping directories with
  a `.git` file or directory, `.claude/worktrees/`, build output directories and symlinked
  directories that leave the root; read bounded bytes; stat (mtime, size, inode); a watcher
  (`notify`) whose events are hints with overflow reported. The `source_tree` contract suite.
- **Accept:**
  - [ ] contract suite green for fake and adapter; a broken fake fails it
  - [ ] the nested-worktree fixture (a main checkout with three `.claude/worktrees/*`) lists each file once, under its own root
  - [ ] a symlink to a directory outside the root is not followed (integration test)
  - [ ] the watcher's licence is accepted by `deny.toml` (an exception with an ADR if CC0 needs one)
- **Impact:** dependency graph.

#### IDX-07: Syntax adapter: tree-sitter summaries and the golden Java corpus
- **Depends on:** IDX-02. **Model:** deep. **Size:** L. **Parallel with:** IDX-08.
- **Cites:** plan §2.1, §2.2, §5.4, §11; ver §2, §4, §8; fnd §3.3.
- `SyntaxIndexer` port; `jfl-adapter-java`: tree-sitter-java parsing into `FileSummary`, syntax
  errors as diagnostics with ranges, comments and strings excluded from reference sites, Javadoc
  attached to declarations, method references matched by name only; bounded input size and depth,
  no panic on any bytes. `fixtures/java/summaries/` goldens for every corpus file (golden first),
  `fixtures.lock` and the `ARCH-fixtures-lock` rule (fnd §3.3). The `syntax_indexer` contract
  suite (canned fake vs adapter).
- **Accept:**
  - [ ] summary goldens match for every file of `fixtures/java/projects/` and `fixtures/java/resolution/`
  - [ ] property test: arbitrary bytes and mutated corpus files never panic and finish within a bound
  - [ ] `ARCH-fixtures-lock` fails on an edited golden with a stale lock (negative fixture)
  - [ ] the tree-sitter dependency (C code) named in the external allowlist and the commit with its licence
- **Impact:** dependency graph; fixture corpus.

#### IDX-08: Summary store at the DEC-03 tier
- **Depends on:** IDX-02, DEC-03. **Model:** standard. **Size:** M. **Parallel with:** IDX-07.
- **Cites:** plan §5.4, §5.5, §5.10; fnd §6; ver §5.
- `SummaryStore` port; `jfl-adapter-store` at the tier DEC-03 chose (by default: a flat on-disk
  cache keyed by `SyntaxKey` with an LRU size cap): a versioned header (a different version is
  discarded, never migrated), atomic writes (temporary file and rename), a torn or corrupt entry
  treated as a miss. The `summary_store` contract suite.
- **Accept:**
  - [ ] contract suite green for fake and adapter; a broken fake fails it
  - [ ] a truncated entry on disk is a miss, never a corrupt summary (integration test)
  - [ ] the cap evicts least-recently-used entries (test on fake clock)
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
- **Accept:**
  - [ ] `conformance-replay` replays the `views` regression and generated traces green
  - [ ] the negative control fails at the expected step
  - [ ] component tests: a request pinned to N while N+1 publishes answers wholly from N; a `sed -i`-style write seen by the stat is in the answer without `sync`
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
- **Accept:**
  - [ ] the completeness oracle passes for every module and seed in the gate's budget; a planted defect (ignore one file) makes it fail
  - [ ] references inside comments and strings never appear (golden cases)
  - [ ] an unindexed or stale file makes `complete=false` with its reason (component test)
- **Impact:** none (internal).

#### IDX-12: Daemon and IPC: lock, socket, spawn, reconnect (daemon driver)
- **Depends on:** IDX-03, IDX-09, FORMAL-04. **Model:** deep. **Size:** L. **Parallel with:** IDX-11.
- **Cites:** plan §5.1, §5.2, §5.3, §7.2, §11; fnd §4.4; ADR 0016.
- `jfl-adapter-ipc`: the state directory (`0700`), lock (`0600`, held for life), Unix socket
  (`0600`, created with its mode; path-length limit refused with the limit named), peer-uid check,
  framing from `jfl-protocol`, server and client, the `endpoint` contract suite.
  `jfl-composition`: the daemon host (accept loop, per-connection state, supervision, idle exit,
  SIGTERM), the launcher (spawn on demand, detached, with a spawn lock and a bounded readiness
  wait), reconnect with re-sent documents. Model first: the `daemon` conformance driver over the
  in-process host with real sockets and locks in a tempdir; negative control: release the lock
  before removing the socket.
- **Accept:**
  - [ ] `conformance-replay` green for `daemon`; the negative control fails at its step
  - [ ] integration tests: a second daemon on the same state dir exits with a clear error; modes are `0700`/`0600` (also under a non-root uid via `setpriv`, since the sandbox runs as root); a peer with another uid is refused; two clients racing start one daemon
  - [ ] after a daemon crash a client reconnects and the new daemon holds no overlay of a dead connection
- **Impact:** protocol (endpoint); new authority (sockets).

#### IDX-13: `jfl` CLI: subcommands, output, `--json`, exit codes
- **Depends on:** IDX-11, IDX-12. **Model:** standard. **Size:** M. **Parallel with:** LSP-02.
- **Cites:** plan §2.3, §2.4; ver §2, §4; ADR 0013.
- `jfl-cli`: `def`, `refs`, `hover`, `sym`, `status`, `sync`, `daemon` with `--json`,
  `--exact-only`, `--state-dir`; `file:line:col` parsing (1-based, byte columns); compact output
  grouped by file, `exact` first, capped with a count; exit codes of plan §2.4. `--json` goldens
  first. The `e2e-tests` stage and its CI job (shared with LSP-01's `lsp-conformance`).
- **Accept:**
  - [ ] e2e tests with `assert_cmd` over copies of the fixture projects for each subcommand and exit code
  - [ ] `--json` goldens for every subcommand; compact output goldens for `refs` with more hits than the cap
  - [ ] the first command against a stopped daemon spawns it and answers (e2e)
- **Impact:** CLI (v1); `--json` schema (v1).

#### IDX-14: Plugin, skill, `AGENTS.md` and cloud setup
- **Depends on:** IDX-13. **Model:** standard. **Size:** M. **Parallel with:** LSP-02.
- **Cites:** plan §2.4, §2.6; ADR 0013.
- `plugin/cli/` (the CLI plugin: manifest, `bin/jfl` launcher, `skills/jfl/SKILL.md` telling an
  agent when to prefer `jfl refs --json` over grep and how to read `complete`), `AGENTS.md` at the
  root (for agents that use `jfl`: commands, the envelope, completeness, freshness and `sync`),
  and `doc/cloud-setup.md` with a setup-script snippet that installs a release binary for cloud
  sessions. Every behavioural claim in the skill and `AGENTS.md` names the test that pins it.
- **Accept:**
  - [ ] the plugin directory validates with the Claude Code plugin schema (a test of the manifest's keys against the documented list)
  - [ ] every command in `AGENTS.md` and the skill runs in an e2e test
  - [ ] (owner) the CLI plugin installed in a local Claude Code session and `jfl refs` used by an agent; the skill installed in a cloud session through the setup script
- **Impact:** distribution.

#### IDX-15: ⛳ M1 gate suite
- **Depends on:** IDX-14. **Model:** deep. **Size:** M. **Parallel with:** none.
- **Cites:** plan §10.1; ver §7.2, §11; ADR 0014, ADR 0017.
- The M1 gate as tests: the envelope on 50 golden cases; the completeness mutation test adding a
  caller in each module of each multi-module fixture; 10 concurrent worktrees (linked, under
  `.claude/worktrees/`) queried in parallel with no cross-view result and no duplicate workspace
  symbol; freshness under shell edits (`git checkout`, `git stash`, `sed -i`, `rm`) answered
  correctly without `sync`. `doc/eval/m1.md` lists the gate items and their tests.
- **Accept:**
  - [ ] every gate item has a passing test named in `doc/eval/m1.md`
  - [ ] (owner) the E0 task subset rerun with the CLI plugin; aggregates in `doc/eval/m1.md`
  - [ ] (owner) the cloud-session check of IDX-14 done
- **Impact:** none.

### Lane LSP: M2, the LSP carrier

#### LSP-01: JSON-RPC base protocol and the LSP conformance harness
- **Depends on:** IDX-03. **Model:** deep. **Size:** L. **Parallel with:** IDX-09, IDX-10.
- **Cites:** plan §2.5, §6.2, §11; ver §2, §4, §8.
- `jfl-adapter-lsp`: `Content-Length` framing (header parsing that refuses junk and duplicate
  lengths; maximum frame size refused before allocation), JSON-RPC 2.0 requests, notifications,
  responses and error codes, request ids of both types, a bounded inbound queue, cancellation
  bookkeeping. The LSP conformance level: a transcript runner (`fixtures/lsp/*.jsonl`, client
  lines sent, server lines expected with documented normalisation) driving a binary over stdio,
  and the `lsp-conformance` stage. First transcripts: framing errors, unknown method, notification
  without a response.
- **Accept:**
  - [ ] property tests: arbitrary byte streams never panic the framer and never allocate past the limit
  - [ ] transcripts for malformed headers, an oversized frame, an unknown request (`MethodNotFound`) and an unknown notification (ignored) pass
  - [ ] `jfl-adapter-lsp` depends only on `jfl-protocol` and `jfl-domain` (architecture policy)
- **Impact:** new adapter.

#### LSP-02: `jfl lsp`: lifecycle, encodings, document sync, deadlines
- **Depends on:** LSP-01, IDX-12. **Model:** deep. **Size:** L. **Parallel with:** IDX-13, IDX-14.
- **Cites:** plan §2.3, §2.5, §2.7, §4.4, §5.2; fnd §3.3.
- The `jfl lsp` proxy: `initialize` (capabilities only for what exists; `positionEncoding`
  negotiation; answered without waiting for indexing), `initialized`, `shutdown`, `exit` (0 after
  shutdown, 1 without), `$/cancelRequest` (`RequestCancelled`, or the result if it already
  finished), document sync into daemon overlays with versions, internal deadlines with partial
  answers, reconnect to a restarted daemon with re-sent open documents. Add the
  `ARCH-position-arithmetic` rule (fnd §3.3). Transcripts for each.
- **Accept:**
  - [ ] transcripts: lifecycle, both exit codes, cancellation before and after completion, UTF-16 positions on a line with astral characters, `utf-8` when the client offers it
  - [ ] a benchmark records `initialize` latency against a warm daemon (ver §10); the 200 ms budget is reported, not asserted in a parallel test
  - [ ] after the daemon is killed mid-session the next request is answered by a respawned daemon with the open documents restored (e2e)
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
- **Impact:** LSP capabilities; CLI.

#### LSP-04: Syntax diagnostics and the coordinator (diagnostics driver)
- **Depends on:** LSP-02, FORMAL-05. **Model:** deep. **Size:** L. **Parallel with:** LSP-03.
- **Cites:** plan §3.2, §7.3; ver §6; ADR 0007.
- The diagnostics coordinator in `jfl-application`: per document, a version, jobs started per
  version (syntax now, `javac` from SEM-05), results accepted only when not older than the newest
  published or pending version, publication through the proxy as `publishDiagnostics` with
  `version`; close publishes an empty set and nothing after. Model first: the `diagnostics`
  conformance driver with the testkit's scripted compiler; negative control: publish every result.
- **Accept:**
  - [ ] `conformance-replay` green for `diagnostics`; the negative control fails at its step
  - [ ] a transcript with two quick edits whose results arrive in reverse order publishes only the newer
  - [ ] syntax errors in the corpus's `broken/` files are published with correct UTF-16 ranges
- **Impact:** LSP capabilities (diagnostics).

#### LSP-05: Plugin variants and `.lsp.json`
- **Depends on:** LSP-03. **Model:** standard. **Size:** M. **Parallel with:** LSP-04.
- **Cites:** plan §2.6; ADR 0013.
- `plugin/lsp/` (the LSP-only plugin with `.lsp.json` using only the documented keys of plan §2.6)
  and the LSP server entry added to the CLI plugin; a test that checks `.lsp.json` against the
  documented key list; coexistence with `jdtls-lsp` investigated and written down in
  `doc/plugins.md` (which wins for `.java`, or that they cannot coexist).
- **Accept:**
  - [ ] the key-list test passes and fails on a fixture with an unknown key
  - [ ] (owner) `claude plugin validate --strict` passes for both variants
  - [ ] `doc/plugins.md` states the coexistence result and how it was checked
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
- **Impact:** none.

### Lane PROJ: M3, project model and library APIs

#### PROJ-01: Gradle model: opt-in, fingerprint cache
- **Depends on:** LSP-06. **Model:** deep. **Size:** L. **Parallel with:** PROJ-03.
- **Cites:** plan §5.7, §11, §14; fnd §2.4; ADR 0015.
- `ProjectModelSource` port; `jfl-adapter-gradle` through `jfl-process`: refuse without the opt-in
  of plan §5.7; compute modules, source sets (main and test separately), dependencies,
  classpaths, processor path, JDK and `--release`; cache by build-file fingerprint in the store;
  run in the background, one run per fingerprint at a time, never on the request path. An ADR
  (reserved number) chooses the mechanism (Tooling API helper or an init script) on fidelity over
  the fixture builds. The `project_model` contract suite. Gradle and its wrapper are pinned for
  tests in `tools/versions.toml`; tests run offline against fixture builds.
- **Accept:**
  - [ ] without the opt-in, no Gradle process starts (integration test watching `jfl-process`)
  - [ ] models of the multi-module and Lombok fixtures match their goldens; two worktrees with the same build files share one model
  - [ ] contract suite green for fake and adapter; the ADR committed
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
- **Impact:** `--json` schema (additive: module fields).

#### PROJ-03: Class-file and JAR reader
- **Depends on:** LSP-06. **Model:** deep. **Size:** L. **Parallel with:** PROJ-01.
- **Cites:** plan §5.8, §11; ver §4, §8.
- `LibraryIndexer` port; `jfl-adapter-classfile`: read JARs (zip) with caps on entry count, sizes
  and compression ratio, never extracting by entry name; parse class files into API summaries
  (types, members, generic signatures, annotations, module info); multi-release selection by
  `--release`. Fixture JARs built from committed sources by a test; malformed inputs committed for
  refusal tests. The `library_indexer` contract suite.
- **Accept:**
  - [ ] API summaries of the fixture JARs match goldens; multi-release selection picks the right version
  - [ ] property tests: arbitrary bytes as a class file or a zip never panic and stay within bounds; a zip bomb is refused
  - [ ] contract suite green for fake and adapter
- **Impact:** new adapter.

#### PROJ-04: JDK API index and the shared library index
- **Depends on:** PROJ-03. **Model:** standard. **Size:** M. **Parallel with:** PROJ-02.
- **Cites:** plan §5.4, §5.8; fnd §6.
- JDK API summaries from `ct.sym` for a `--release` (or `jmods`), and a `LibraryStore` in
  `jfl-adapter-store` keyed by content checksum, shared by every view, versioned like the summary
  cache.
- **Accept:**
  - [ ] `java.util.List` and a `--release 8`-only API resolve from the JDK index of the pinned JDK (integration test)
  - [ ] two views using the same JAR share one stored summary (test)
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
- **Impact:** `--json` schema (additive).

### Lane SEM: M4, resolver and semantic diagnostics

#### SEM-01: Resolver core per DEC-04
- **Depends on:** PROJ-05, DEC-04. **Model:** deep. **Size:** L. **Parallel with:** SEM-04.
- **Cites:** plan §5.6, §6.1, §10.4; ADR 0014; DEC-04's ADR.
- The resolver DEC-04 chose (by default `jfl-resolve`, a pure crate: scopes, imports, type
  hierarchy, member lookup, overload selection, expression types for receivers on fields,
  parameters, locals and chains) over summaries and library APIs, at a pinned generation. A new
  crate's `members`, policy and edges in this commit. Results carry their evidence.
- **Accept:**
  - [ ] the resolution corpus's static-call, field, parameter, chain, overload, inheritance and generics cases resolve as expected
  - [ ] an unsupported construct yields `possible` with its reason, never a guess (tests per construct)
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
- **Impact:** none (internal).

#### SEM-04: `javac` worker and pool
- **Depends on:** PROJ-02, DEC-05. **Model:** deep. **Size:** L. **Parallel with:** SEM-01.
- **Cites:** plan §3.1, §3.4, §5.4, §14; ver §5.
- `java/javac-worker/` (the worker: `JavacTask`, `-proc:none -implicit:none`, `-sourcepath`,
  length-prefixed JSON over stdin and stdout, cancellation) and `Compiler` port; `jfl-adapter-javac`:
  a pool per `EnvironmentKey` through `jfl-process`'s long-lived mode, lazy spawn, idle eviction,
  a cap with LRU eviction, restart on crash with backoff, version-tagged jobs. An ADR (reserved
  number) decides how the worker is built and launched (source-file launch or a prebuilt jar). The
  `compiler` contract suite.
- **Accept:**
  - [ ] contract suite green for the scripted fake and the real worker (pinned JDK); a broken fake fails it
  - [ ] killing a worker mid-compile restarts it and the next job succeeds (integration test)
  - [ ] the pool never exceeds its cap (test on fake clock)
- **Impact:** new adapter; Java source in the repository.

#### SEM-05: Semantic diagnostics, processors and `jfl diag`
- **Depends on:** SEM-04, LSP-04. **Model:** deep. **Size:** L. **Parallel with:** SEM-02, SEM-03.
- **Cites:** plan §3.2, §3.3, §7.3; ADR 0015.
- `javac` results through the diagnostics coordinator (the model and driver of LSP-04 cover them;
  extend the driver if the coordinator's inputs change), generated-source roots added, errors on
  processor-generated members suppressed and marked in hover, no semantic diagnostics without a
  classpath (and `status` says why), `jfl diag <file>` waiting for the current version with a
  deadline.
- **Accept:**
  - [ ] the Lombok and MapStruct fixtures produce zero errors with generated sources missing; a real type error in the same files is reported
  - [ ] `conformance-replay` still green for `diagnostics`
  - [ ] `jfl diag` e2e and golden, including its deadline path
- **Impact:** LSP capabilities; CLI.

#### SEM-06: SCIP differential gates
- **Depends on:** SEM-03, EVAL-08. **Model:** deep. **Size:** M. **Parallel with:** none.
- **Cites:** plan §10.4; ver §7.1.
- Wire `jfl-oracle` to the built `jfl` over the corpus in the gate (precision, coverage per
  construct, envelope recall), with thresholds from DEC-04 for the corpus, and an owner runbook
  for the target repository (`doc/eval/sem-gates.md`).
- **Accept:**
  - [ ] the corpus differential runs in `integration-tests` with precision ≥ 99.5 % and zero recall misses when `complete=true`; a planted wrong `exact` fails it
  - [ ] (owner) the target-repository run; aggregates in `doc/eval/sem-gates.md`
- **Impact:** none.

#### SEM-07: ⛳ M4 gate
- **Depends on:** SEM-05, SEM-06. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §10.4; ver §7, §11.
- `doc/eval/m4.md` with each gate item and its evidence: precision, coverage per construct,
  envelope recall, zero false errors on a clean checkout, diagnostics p95 within E3's budget.
- **Accept:**
  - [ ] every automatable item has a passing test or committed measurement named in `doc/eval/m4.md`
  - [ ] (owner) precision, coverage and recall on the target repository meet plan §10.4
  - [ ] (owner) zero false errors on a clean checkout of the target repository; p95 within budget
  - [ ] (owner) the E0 task subset rerun; aggregates in `doc/eval/m4.md`
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
- **Impact:** LSP capabilities; CLI.

#### REL-02: Resource limits and watcher recovery
- **Depends on:** SEM-07. **Model:** deep. **Size:** L. **Parallel with:** REL-03.
- **Cites:** plan §5.9, §12; fnd §4.4, §5.
- Every limit of plan §5.9 as a named constant with its reason, measured values from benchmarks,
  counted refusals and backpressure shown in `jfl status`; watcher overflow and loss trigger a
  rescan; the `ARCH-log-event-name` rule (fnd §3.3).
- **Accept:**
  - [ ] a test per limit showing the refusal or backpressure and its `status` field
  - [ ] a simulated watcher overflow leads to a rescan and correct answers (integration test)
  - [ ] `ARCH-log-event-name` fails its negative fixture
- **Impact:** config (limits); `--json` schema (additive status fields).

#### REL-03: Fuzz lane
- **Depends on:** SEM-07. **Model:** standard. **Size:** M. **Parallel with:** REL-01, REL-02.
- **Cites:** ver §8; fnd §7.1, §8.
- `fuzz/` (own workspace, `jfl-fuzz`, pinned nightly and cargo-fuzz), targets of ver §8, seed
  corpora from the fixtures, `cargo xtask fuzz`, `fuzz.yml` (nightly), `ARCH-fuzz-crate` rows.
- **Accept:**
  - [ ] each target runs for its budget locally without a crash; a planted panic is found and reproduced with `--reproduce`
  - [ ] `fuzz.yml` green on dispatch
- **Impact:** none (tooling).

#### REL-04: Crash and restart soak
- **Depends on:** REL-02. **Model:** deep. **Size:** M. **Parallel with:** REL-05.
- **Cites:** plan §5.2, §7.2; ver §6, §12.
- A soak harness (`cargo xtask soak`, owner-run for long durations, a short budget in CI): 10
  worktrees, proxies issuing queries and edits, the daemon and javac workers killed at random
  (seeded), checking that every answer is consistent with the files at its generation, no ghost
  buffers, no leaked processes, sockets or locks.
- **Accept:**
  - [ ] the short soak passes in the gate's budget; a planted ghost-buffer defect fails it
  - [ ] (owner) a long soak (hours) on the owner's machine with its report in `doc/eval/soak.md`
- **Impact:** none.

#### REL-05: Mutation and benchmark lanes
- **Depends on:** REL-02. **Model:** standard. **Size:** M. **Parallel with:** REL-04.
- **Cites:** ver §9, §10; fnd §7.1.
- `cargo xtask mutation` (cargo-mutants pinned, scope of ver §9), `mutation.yml` (weekly), the
  first survivor triage in `doc/mutation-baseline.md`; `cargo xtask bench` with the benchmarks of
  ver §10 and recorded machine facts, results in `doc/performance.md`.
- **Accept:**
  - [ ] a mutation run completes and its survivors are triaged
  - [ ] the benchmark suite runs and records the plan §2.5 and §2.7 budgets with their measurements
- **Impact:** none (tooling).

#### REL-06: ⛳ Release workflow and release preview
- **Depends on:** REL-01, REL-03, REL-04, REL-05. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §2.6, §10.5; fnd §7.1, §7.4.
- `release.yml` (tag `v*` or dispatch with dry run): archives for the three targets of ADR 0006,
  `SHA256SUMS`, CycloneDX SBOM, build-provenance attestation, third-party notices, a draft
  release; `cargo xtask release-check` (package, extract, smoke the archive with no Java or Node
  on `PATH`); `doc/release.md`; `CHANGELOG.md` for the preview.
- **Accept:**
  - [ ] a dry-run release on dispatch produces every artefact and passes `release-check` on each target
  - [ ] (owner) the final E0 run at 10 concurrent worktrees against both baselines; aggregates in `doc/eval/m5.md`
  - [ ] (owner) tag pushed and the draft release published
- **Impact:** distribution.

### Lane SCIP: M6, optional

#### DEC-07: (owner) Build M6 or not
- **Depends on:** SEM-07. **Model:** light. **Size:** S. **Parallel with:** REL-01.
- **Cites:** plan §10.6; ADR 0009.
- An agent summarises M4's coverage gap per construct and what a SCIP baseline would add for the
  E0 tasks, in `doc/eval/m6-question.md`.
- **Accept:**
  - [ ] the summary committed
  - [ ] (owner) decided; an ADR records it; if "no", SCIP-01 and SCIP-02 are marked dropped here
- **Impact:** none (decision).

#### SCIP-01: SCIP baseline import
- **Depends on:** DEC-07, REL-06. **Model:** deep. **Size:** L. **Parallel with:** none.
- **Cites:** plan §10.6; ver §7.1.
- Import a SCIP index produced outside the request path as a per-commit baseline the resolver
  falls back on, with its staleness against the view tracked per file. Design detail is in the
  ADR this task writes, starting from DEC-07's record.
- **Accept:**
  - [ ] a baseline on the fixture projects raises coverage on the constructs DEC-07 named, with precision unchanged (oracle run)
  - [ ] a file changed since the baseline's commit never gets an `exact` answer from it (test)
- **Impact:** cache format (baseline); config.

#### SCIP-02: SCIP baseline in the envelope and gates
- **Depends on:** SCIP-01. **Model:** standard. **Size:** M. **Parallel with:** none.
- **Cites:** plan §2.1, §10.6.
- The envelope's `exact` evidence names the baseline; `status` shows the baseline's commit and
  coverage; the M4 gates rerun with the baseline on.
- **Accept:**
  - [ ] goldens with baseline evidence; the completeness oracle and SCIP differential pass with the baseline on
  - [ ] (owner) the target-repository gates rerun; aggregates added to `doc/eval/m4.md`
- **Impact:** `--json` schema (additive).
