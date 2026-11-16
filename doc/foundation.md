# jfastlsp: engineering foundation

**Status:** plan, 2026-11-16. Owner: Patryk Żmigrodzki.

This document owns the engineering rules: how the workspace is cut and enforced, the language and
lint policy, logging and error conventions, persistent-state compatibility, CI and supply chain,
tool pins, the command surface, commits, and what an implementing agent may do.
[`plan.md`](plan.md) owns *what* is built, [`verification.md`](verification.md) owns *how it is
shown to be right*, [`sdlc.md`](sdlc.md) owns how a change reaches `main`, and
[`tasks.md`](tasks.md) owns order. A citation "fnd §3" points here.

A rule in this document is either **enforced** (a check fails; the check is named) or
**reviewed** (a `/review` checklist item). Every enforced rule names the task that introduces its
check. Until that task lands, the rule binds by review.

---

## 1. Purpose and scope

1. **Purpose.** Make the properties of plan §1.5 structural: wrong code should fail to compile,
   fail the architecture test, cargo-deny or clippy, or fail a named test, before a reviewer has
   to notice.
2. **Scope.** Every crate under `crates/` and `tools/`, the Java worker under `java/`, the Quint
   models under `formal/`, the fixtures under `fixtures/`, the workflows under `.github/`, and
   the repository's process files.
3. **Out of scope.** Product behaviour (plan) and the test strategy (verification).
4. **Precedence.** Accepted ADRs override this document; this document overrides earlier plan
   text on engineering matters. A conflict found while working is reported, not resolved by
   choosing the convenient side.
5. **Tools before code** (ADR 0030, superseding ADR 0011 and ADR 0012). The foundation is
   assembled from maintained tools: mise pins them, just composes the gate, the compiler and
   cargo-deny enforce most of the dependency law, adrs keeps the decisions. What stays custom is
   small and named: the architecture test (about 200 lines), `scripts/land.sh` (about 150),
   per-crate coverage floors (about 100) and the replay of committed Quint traces (about 200).
   Nothing is ported from mayfly's `xtask`; its designs (fail closed, one gate locally and in CI,
   negative fixtures) are kept.

---

## 2. Workspace and dependency law

### 2.1 Crate categories

`architecture.toml` sorts every workspace member into exactly one category. A crate missing from
the policy, or listed but absent, fails the checker.

| Category | Crates (plan §6.1) | May be reached from production |
|:--|:--|:--|
| **pure** | `jfl-domain`, `jfl-protocol` (and `jfl-resolve` if DEC-04 adds it) | yes |
| **application** | `jfl-application` | yes |
| **support** | `jfl-process` | yes, from adapters that need it and composition |
| **adapter** | `jfl-adapter-{fs,git,java,classfile,store,ipc,lsp,javac,gradle}` | yes |
| **root** | `jfl-composition`, `jfl-cli` | yes |
| **test_only** | `jfl-testkit`, `jfl-conformance`, `jfl-architecture` | never |
| **tool_only** | `jfl-oracle`, `jfl-eval`, `jfl-jdtls`, `jfl-bench` | never |
| **fuzz_only** | `jfl-fuzz` (own workspace under `fuzz/`, REL-03) | never |

`[workspace]` holds the categories as `production`, `test_only`, `tool_only` and `fuzz_only`;
`[roles]` lists `pure`, `application`, `support`, `adapter` and `root` inside production.

### 2.2 Edges

Plan §6.3 is the edge table. Rules over it:

1. **Acyclic, explicit.** The table is acyclic (`ARCH-policy-cycle`); workspace `members` is an
   explicit list, never a glob. A new crate adds its `members` entry, its category, its role and
   its edges in the same commit.
2. **Adapters are siblings.** An adapter never names another adapter, composition or the CLI,
   in production or test code (`ARCH-layer-isolation`). `jfl-process` names no workspace crate.
3. **Dev edges.** A dev-dependency targets a test_only crate or a crate already in the
   dependent's normal list (`ARCH-forbidden-dev-edge`). `jfl-testkit`'s own dependencies use it
   from integration tests (`tests/`) only: a unit test's kit links a different build of the
   crate.
4. **Production closure.** No production crate reaches a test_only or tool_only crate by a
   normal or build edge, directly or through any package (`ARCH-production-closure`).
5. **Build dependencies** are declared in `[dependencies.build]` (`ARCH-undeclared-build-dep`);
   none exist in v0.x. A third-party crate's build script (tree-sitter grammars and bundled
   SQLite compile C) is allowed by name in `deny.toml`'s build-script allowlist with the reason.
6. **Targets.** Edges are evaluated through `cargo metadata --filter-platform` for
   `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin` (ADR 0006);
   a finding on only some of them names those targets.

### 2.3 Pure-crate restrictions

Pure crates (`jfl-domain`, `jfl-protocol`, and `jfl-resolve` if it joins) are **`#![no_std]`
with `extern crate alloc`** (ADR 0030). The compiler then refuses `std::fs`, `std::net`,
`std::os::unix::net`, `std::process`, `std::env`, `std::thread`, `SystemTime`, `Instant`, the
default-hashed `HashMap`/`HashSet`, `println!`/`eprintln!`/`dbg!` and any std-only dependency,
with no list to keep current (`ARCH-domain-purity`). Collections are `BTreeMap`/`BTreeSet` or a
fixed-seed hasher. What `core` still offers is denied by each pure crate's `clippy.toml`
(`disallowed-types` for atomics and `core::cell` statics; `ARCH-pure-clippy-config`), and
`cargo-deny` keeps randomness, async runtimes and I/O crates out of their closure (wrappers,
§2.5). Time and randomness reach application code only through ports.

### 2.4 Authority

Plan §6.5's table is enforced per crate, because clippy has no module-level configuration:

- The **root `clippy.toml`** holds the canonical `disallowed-methods`, `-types` and `-macros`:
  `std::process::Command`, `tokio::process::Command`; `std::net::*` and `std::os::unix::net::*`
  types, Tokio's net types; `std::fs` functions and types, Tokio's fs; `std::thread::spawn`,
  `std::thread::sleep`, `tokio::spawn`, `tokio::time::sleep`; unbounded channels
  (`tokio::sync::mpsc::unbounded_channel`, `std::sync::mpsc::channel`,
  `crossbeam_channel::unbounded`); `std::env::var`; `SystemTime::now`, `Instant::now`.
  `[workspace.lints.clippy]` sets `disallowed_methods`, `disallowed_types` and
  `disallowed_macros` to `deny`.
- A crate granted an authority carries **its own `clippy.toml`**: the canonical list minus
  exactly its grants (`jfl-process`: `Command`; `jfl-adapter-ipc`: Unix sockets; the
  fs-authority crates of plan §6.5: `std::fs`; composition and the adapters that supervise
  tasks: spawns). Clippy has no inheritance, so the architecture test checks every crate's list
  against the canonical one and the grants in `architecture.toml` (`ARCH-clippy-lists`).
- **Children** come only from `jfl-process`, whose API always starts from `env_clear()` plus an
  allowlist (`ARCH-process-authority`; `ARCH-child-environment` is its contract suite). The
  daemon launcher uses its detached mode, so composition holds no process grant.
- **Sockets**: only `jfl-adapter-ipc`, Unix domain; a TCP type anywhere is denied in every list
  (`ARCH-socket-authority`).
- **Spawned tasks** (`ARCH-detached-spawn`): only crates granted spawns; within them, the
  supervision modules and the tracker hand-off bind by review.

### 2.5 External allowlist

`architecture.toml`'s `[external.normal]`, `[external.dev]` and `[external.build]` list, per
crate, each third-party crate it may name directly and the features it may enable (`"default"` for
default features, `"*"` for any); every entry carries a comment saying why. The architecture test
compares them with `cargo metadata` (`ARCH-unlisted-external`, `ARCH-external-feature`).
`deny.toml` enforces the transitive side: a `wrappers` entry for every adapter-only crate (Tokio
only in composition and the IPC adapter; `rusqlite` only in the store; `tree-sitter` only in the
Java adapter; `lsp-server` only in the IPC and LSP adapters; `anyhow` only in binaries and tools;
`ARCH-anyhow-in-library`), `[[bans.features]]`, `use-instead`, and crates.io as the only source
(`ARCH-external-source`). FND-02 confirms with a negative test that `wrappers` also applies to
workspace path crates before relying on it.

### 2.6 Test-only, tool-only and fuzz-only crates

- `jfl-testkit` holds a fake for every port of plan §6.4, the contract suites (ver §5), fixture
  builders (temporary Git repositories with linked worktrees, fixture project copies) and the
  `poll` helper, the only sanctioned wait on a real-OS condition (ver §3).
- `jfl-conformance` holds the `quint-connect` drivers and the committed-trace replay (ver §6).
- `jfl-architecture` holds the architecture test (§3). It reads `cargo metadata` and the
  repository's files; it is never a dependency.
- There is no `xtask`: the command surface is the `justfile` (§9).
- `jfl-fuzz` is its own workspace on the pinned nightly (REL-03); `ARCH-fuzz-crate` checks its
  policy rows.

### 2.7 Independent oracles

`jfl-oracle`, `jfl-eval`, `jfl-jdtls` and `jfl-bench` judge or compete with the product from
outside: they run the built `jfl` (or jdtls), read its `--json`, and read SCIP indexes, benchmark
results and Claude Code transcripts with their own code. They import no production crate
(`ARCH-independent-oracle`): an oracle that shares the resolver's assumptions proves nothing.

---

## 3. Enforcement

Four layers, cheapest first: the **compiler** (crate boundaries, visibility, `#![no_std]` pure
crates, `[workspace.lints]`, `trybuild` compile-fail tests for critical APIs), **cargo-deny**
(advisories, licences, sources, bans with `wrappers` and features, build scripts), **clippy's
`disallowed-*` lists** per crate (§2.4) and **one architecture test** for what no tool expresses.
A tool that is absent, a selector that matches nothing, or a file that fails to parse is a
failure, never a pass.

### 3.1 `architecture.toml` schema

```toml
schema = 1

[workspace]
production = [ ... ]          # §2.1
test_only  = ["jfl-testkit", "jfl-conformance", "jfl-architecture"]
tool_only  = ["jfl-oracle", "jfl-eval", "jfl-jdtls", "jfl-bench"]
fuzz_only  = ["jfl-fuzz"]     # once the fuzz crate exists

[roles]                       # §2.1, inside production
pure        = ["jfl-domain", "jfl-protocol"]
application = ["jfl-application"]
support     = ["jfl-process"]
adapter     = [ ... ]
root        = ["jfl-composition", "jfl-cli"]

[grants]                      # §2.4: what each crate's clippy.toml may drop from the canonical list
process = ["jfl-process"]
socket  = ["jfl-adapter-ipc"]
fs      = [ ... ]             # plan §6.5
spawn   = [ ... ]
independent = ["jfl-oracle", "jfl-eval", "jfl-jdtls", "jfl-bench"]

[dependencies.normal]         # plan §6.3, one key per crate
[dependencies.build]

[external.normal]             # §2.5, crate = { dep = ["feature", ...] }
[external.dev]
[external.build]

[exceptions]
# [[exceptions.lints]] path = "...", lint = "...", adr = "NNNN"
```

Parsed strictly: an unknown key or another `schema` value fails `ARCH-policy-schema`; a crate in
two categories fails `ARCH-duplicate-category`, a present but empty list `ARCH-empty-selector`; a
name that is not a workspace crate fails `ARCH-missing-crate`. An exception citing an ADR file
that does not exist, or an exception that no longer matches anything, fails.

### 3.2 The architecture test (`jfl-architecture`)

A test-only crate in `tools/jfl-architecture`, run by `just check-only architecture` under
nextest. About 200 lines over the `cargo_metadata` crate (`cargo metadata --locked
--filter-platform` for each target of ADR 0006) plus a small source scan:

1. **Graph:** every workspace member is classified; every normal and dev edge is in plan §6.3's
   matrix; the production closure reaches no test-only or tool-only crate; independent tools
   reach no workspace crate; each crate's direct externals and features are in its allowlist;
   every member has `[lints] workspace = true` and `publish = false`.
2. **Lists:** every crate's `clippy.toml` equals the canonical list minus its grants.
3. **Source scan** (line-oriented, deliberately simple): tests are named
   `should_<outcome>_when_<condition>` and carry `// given`, `// when`, `// then`; no
   `#[ignore]`; no `#[allow]`/`#[expect]`/`#[macro_export]` without an `[exceptions]` row citing
   an ADR; no `cfg!(target_os…)` branch; `#![no_std]` present in each pure crate's `lib.rs`.
4. **CI lockstep:** `ci.yml`, the `justfile` (`just --dump --dump-format json`) and
   `required-checks.md` agree (fnd §7.2).

Diagnostics read `ARCH-<id>: message (at path[:line])` with the full dependency path for a
transitive edge.

### 3.3 Rules

Each rule names the layer that enforces it and the task that lands it.

| Rule | What fails | Enforced by | Task |
|:--|:--|:--|:--|
| `ARCH-policy-schema`, `ARCH-metadata`, `ARCH-missing-member`, `ARCH-not-a-member`, `ARCH-missing-crate`, `ARCH-empty-selector`, `ARCH-duplicate-category` | the policy, the metadata or the inventory disagree | architecture test | FND-02 |
| `ARCH-forbidden-edge`, `ARCH-forbidden-dev-edge`, `ARCH-undeclared-build-dep`, `ARCH-production-closure`, `ARCH-independent-oracle` | an edge outside plan §6.3 | architecture test | FND-02 |
| `ARCH-unlisted-external`, `ARCH-external-feature` | §2.5, direct dependencies | architecture test | FND-02 |
| `ARCH-external-source`, `ARCH-anyhow-in-library`, adapter-only crates outside their adapter | §2.5, transitive | cargo-deny | FND-02 |
| `ARCH-domain-purity` | std or I/O in a pure crate | compiler (`#![no_std]`) | FND-02 |
| `ARCH-pure-clippy-config`, `ARCH-clippy-lists`, `ARCH-process-authority`, `ARCH-socket-authority`, `ARCH-fs-authority`, `ARCH-detached-spawn`, `ARCH-unbounded-channel`, `ARCH-test-sleep` | §2.3, §2.4 | clippy lists, kept in step by the architecture test | FND-02 |
| `ARCH-child-environment` | a child without a cleared environment | `jfl-process`'s API and contract suite | IDX-04 |
| `ARCH-unsafe-code`, `ARCH-unfinished-code`, `ARCH-nightly-features` | `unsafe`, `todo!`/`unimplemented!`/`dbg!`, `#![feature]` | `[workspace.lints]` (`unsafe_code = "forbid"`, clippy `todo`, `dbg_macro`) and the stable toolchain | FND-01 |
| `ARCH-lint-config`, `ARCH-lint-suppression` | lints loosened; `#[allow]`/`#[expect]`/`#[macro_export]` without an ADR-cited exception | architecture test (source scan) | FND-02 |
| `ARCH-ignored-test`, `ARCH-test-name`, `ARCH-test-structure`, `ARCH-platform-branch` | `#[ignore]`; a misnamed test; missing sections; a `cfg!(target_os…)` branch | architecture test (source scan) | FND-02 |
| `ARCH-ci-lockstep` | a stage in no CI job or two; `required` missing a job; a swallowed failure | architecture test | FND-05 |
| `ARCH-fixtures-lock` | a golden fixture whose digest differs from `fixtures.lock` | architecture test | IDX-07 |
| `ARCH-fuzz-crate` | the fuzz crate outside its policy rows | architecture test | REL-03 |

Rules the ported `syn` checker would have enforced and no tool covers bind **by review** (the
review skill's checklist): command and query isolation in the application, observability values
reaching a branch, hidden seams into composition, `tracing` event names that are not literal
`snake_case` (`ARCH-log-event-name`, REL-02 may automate it), arithmetic on LSP `Position` fields
outside `jfl-domain` (`ARCH-position-arithmetic`, LSP-02). A rule that needs real name resolution
and proves necessary becomes a `dylint` lint through an ADR.

### 3.4 Fixtures

1. Graph rules are tested **table-driven over synthetic `cargo metadata` JSON** (one case per
   rule, negative and positive), plus **5 to 10 negative fixture workspaces** under
   `tools/jfl-architecture/fixtures/` (path dependencies only, so they resolve offline) for the
   end-to-end path: `domain-depends-on-application`, `adapter-depends-on-adapter`,
   `testkit-in-production-closure`, `oracle-imports-domain`, `unlisted-external`.
2. Clippy lists and `cargo-deny` bans are proven by one negative case each: a crate calling
   `std::process::Command` without the grant fails clippy; a path crate wrapped to the wrong
   parent fails `cargo deny check bans`.
3. The source scan has a positive and a negative snippet per rule.
4. Every rule in §3.3 has at least one negative case; the test fails when a rule has none.

### 3.5 Hygiene

- Workspace lints only (§4.2); no crate-level overrides.
- No `#[allow]`/`#[expect]` or `#[macro_export]` without an ADR-backed exception row; no
  `#[ignore]`.
- Tests named `should_<outcome>_when_<condition>` with `// given`, `// when`, `// then`.
- No mocking frameworks; real objects over testkit fakes; tempdirs; sockets in tempdirs.
- A rule is never loosened to get green. A change to `architecture.toml`, `deny.toml` or a
  `clippy.toml` that widens an edge, allowlist, role, grant or exception needs the task to say so
  and an ADR.

### 3.6 Threat model of the gate

The gate defends against honest mistakes, shortcuts and drift, not a determined insider; review
is the backstop. The `justfile`'s `_preflight` recipe, a dependency of every stage, refuses an
environment that steers the build: `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`, `RUSTFLAGS` other
than `-D warnings`, `CLIPPY_CONF_DIR`, `CLIPPY_*`, `RUSTC_BOOTSTRAP`, `CARGO_ALIAS_*`,
`CARGO_UNSTABLE_*`, per-target runners. Never export them for the gate.

---

## 4. Language and lint policy

### 4.1 Baseline

Stable toolchain pinned exactly in `rust-toolchain.toml` (ADR 0030, carrying ADR 0011's pin:
1.97.0, edition 2024, resolver 3, `rustfmt`, `clippy`, `llvm-tools-preview`). `Cargo.lock`
committed; every cargo command in the gate runs `--locked`. `publish = false` everywhere. A dated
nightly is pinned in `mise.toml` for `cargo-fuzz` only and never builds a shipped artefact.

### 4.2 Workspace lints

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
unused_must_use = "deny"
missing_docs = "warn"
unreachable_pub = "warn"
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(coverage)"] }

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
indexing_slicing = "deny"
arithmetic_side_effects = "deny"
await_holding_lock = "deny"
large_futures = "deny"
```

`clippy.toml` allows `unwrap`/`expect`/`panic`/indexing inside `#[cfg(test)]` and test targets
only. `arithmetic_side_effects` applies to production code that handles offsets, lengths and
counts from input; a crate whose arithmetic is all on trusted values may get an ADR-cited
exception. Zero warnings: `-D warnings` in the gate. Each crate's `lib.rs` opens with a `//!`
stating its single responsibility and its role.

### 4.3 Errors

1. **Libraries** use typed errors with `thiserror`; one error enum per port or use case, each
   variant meaningful to a caller. `anyhow` only in binary and tool targets
   (`ARCH-anyhow-in-library`).
2. **Refusals are values.** A position past the end, a path outside any worktree, a symbol not
   found, an unknown method: a typed refusal the carriers map to an exit code or a JSON-RPC
   error, never a panic.
3. **No panics on production paths.** No `unwrap`, `expect`, `panic!`, unchecked indexing,
   overflowing arithmetic or `unreachable!` that input can reach. An invariant violation found
   at run time fails the one request with an internal error, logs at ERROR and keeps the daemon
   serving.
4. Errors that cross a log or the protocol are classified (`error_class`) and carry no source
   text.

### 4.4 Async rules

1. **Ports are native `async fn` traits** used through generics; `dyn` ports only where object
   safety is proven necessary, recorded in an ADR. Returned futures are `Send`.
2. **Cancellation.** Every query is cancellable (LSP `$/cancelRequest`, a dropped CLI
   connection, a deadline). Every `async fn` on a port documents whether it is cancel-safe; a
   reconcile in flight that is not runs in a tracked task and publishes its generation or
   nothing.
3. **No blocking in async.** File reads, parsing and hashing run on a bounded blocking pool
   through the adapter that owns them, never on a runtime worker.
4. **Structured concurrency.** Composition owns one root cancellation token and a task tracker;
   every long-lived task is a child, joined on shutdown in a declared order (accept loop →
   queries → reconcilers → workers → store). No detached task (`ARCH-detached-spawn`).
5. **Bounded everything.** Channels are bounded and the capacity is a named constant with a
   reason (`ARCH-unbounded-channel`); a full channel applies backpressure or is a counted
   refusal (plan §5.9).
6. **Deadlines** come from the `Clock` port; a timeout returns a partial answer with
   `complete=false` (plan §2.7), never a success it did not compute.
7. **Locks.** No lock is held across `.await`; prefer message passing to an owning task, and
   immutable published generations (plan §4.5) over shared mutable indexes.

---

## 5. Logging, metrics and errors

1. **Events, not sentences.** Every `tracing` event's message is a fixed `snake_case`,
   noun-first name (`reconcile_finished`, `worker_restarted`); everything that varies is a field
   (`ARCH-log-event-name`, REL-02). Spans carry `view`, `generation`, `request_id`, `connection`,
   bound once at the start of the unit of work.
2. **Levels.** A healthy daemon logs (almost) nothing.
   - `DEBUG`: diagnosis; off by default.
   - `INFO`: an important change only: start, stop, a view opened or closed, a project model
     computed, a cache format discarded. **Zero INFO per request.**
   - `WARN`: a condition the system handles that deserves a look in volume (a limit reached, a
     worker restarted, a watcher overflow, a deadline hit).
   - `ERROR`: the operator must act (the state directory unusable, an internal invariant broken).
3. **No source text.** Logs, errors and `status` never carry file contents or source snippets;
   paths at DEBUG only.
4. **Counters** (requests, deadlines hit, cache hits, reconcile durations) are exposed through
   `jfl status --json`, never sent anywhere (plan §12).
5. **Tests do not assert log lines.** Logs are instrumentation; the checker pins their shape.

---

## 6. Persistent state and compatibility

There is no event store (ADR 0008). What persists:

1. **Caches** (summaries, library indexes, project models) carry a format version in their
   header; a different version is discarded and rebuilt, never migrated. Losing the state
   directory loses time, never correctness.
2. **The protocol** between proxies and the daemon is versioned (plan §6.6): additive minors,
   a major bump refused at the handshake.
3. **The `--json` schema, CLI flags, config keys and LSP settings** are never renamed or retyped;
   they gain optional fields. A breaking change is a new major version with an ADR.
4. **Ask of every change to the protocol, `--json`, config or a cache format:** is an older proxy,
   daemon or cache reader that meets it still correct? Yes: one commit. No: a version bump that
   refuses loudly.

---

## 7. CI and supply chain

### 7.1 Workflows

| Workflow | Trigger | Content | Task |
|:--|:--|:--|:--|
| `ci.yml` | push to `main`, pull request, dispatch | the gate, split into jobs (7.2), and the DCO check | FND-05 |
| `security.yml` | weekly, dispatch | advisories, licences, secret scan over full history, zizmor online audits, the private-name grep | FND-04, FND-05 |
| `codeql.yml` | push to `main`, pull request, weekly | CodeQL for Rust and Actions | FND-05 |
| `scorecard.yml` | push to `main`, weekly | OpenSSF Scorecard, SARIF upload | FND-05 |
| `bench.yml` | push to `main`, dispatch | the corpus benchmarks; history in `gh-pages`; a >10 % regression fails from M1 on | BENCH-04 |
| `formal.yml` | weekly, dispatch | Apalache bounded verification per model; fresh-seed conformance | FORMAL-01, FORMAL-02 |
| `fuzz.yml` | nightly, dispatch | each fuzz target for a time budget; crash inputs uploaded | REL-03 |
| `mutation.yml` | weekly, dispatch | `cargo-mutants` over the configured scope | REL-05 |
| `release.yml` | tag `v*`, dispatch | generated by dist; release-plz opens the release PR and tags | REL-06 |
| `.github/dependabot.yml` | weekly | cargo, github-actions (with a cooldown), grouped minor and patch | FND-05 |

### 7.2 The gate in CI

1. **Each job is `mise install --locked` then `just check-only <stage>…`.** CI never
   re-implements a stage in YAML; local and CI run the same recipes. The jobs and their stages are
   in [`required-checks.md`](required-checks.md).
2. **One aggregate job, `required`**, runs `if: always()`, needs every other job of `ci.yml`, and
   fails unless every result is `success` (skipped and cancelled fail).
3. **The lockstep test** (`ARCH-ci-lockstep`, in `jfl-architecture`) parses `ci.yml` and the
   `justfile` and fails when a stage of `just check` is in zero jobs or in two, when `required`
   does not need every job, when a gating step has `continue-on-error`, an `if:`, a
   failure-swallowing `run:` (`|| true`, `set +e`, `exit 0`) or a steering `env:`, and when
   `required-checks.md` and `ci.yml` disagree.
4. Native jobs on `ubuntu-24.04`, `ubuntu-24.04-arm` and `macos-15` (arm64) run the platform
   tests (ADR 0006).
5. Scheduled workflows are outside `required`; their failures are reported and fixed as tasks.
6. A red job is fixed, never re-run to green. One re-run is allowed only when the runner died
   before a test body ran, and the reason is written down.

### 7.3 Workflow hygiene

- `permissions: contents: read` at the top; a job adds only the scope one step needs, with the
  reason beside it.
- Every `uses:` is pinned to a 40-hex commit SHA with the version in a comment, or is a local
  action; container images by `@sha256:` digest. Checked by zizmor (`unpinned-uses` with a
  hash-pin policy, template injection, excessive permissions), `pinact run --check
  --verify-comment` and actionlint with shellcheck (`workflows` stage, FND-04).
- `actions/checkout` with `persist-credentials: false`; no `pull_request_target`; inputs reach
  scripts through `env`, never interpolated into `run:`.
- Concurrency per ref, and per commit on `main`: a newer push to `main` neither cancels nor
  replaces a pending run; cancel-in-progress only for pull requests.
- Caches are keyed by toolchain, `Cargo.lock` and `mise.lock`, and saved only from `main`.

### 7.4 Supply chain

- `cargo-deny`: advisories (yanked denied), licences (permissive allowlist; each exception per
  crate with an ADR; `notify` is CC0-1.0 and tree-sitter grammars MIT, to confirm at the task
  that adds them), bans (no wildcards), sources (crates.io only); `[graph] targets` set to the
  three targets of ADR 0006.
- `gitleaks` (the binary, pinned by mise) over the full history; `.gitleaks.toml` allowlists
  nothing; fixtures build fake secrets at run time if they need any. GitHub secret scanning and
  push protection are on (owner setting).
- **Private-name grep** (`private-names` stage): the patterns are never committed (that would
  name what they protect); the recipe reads them from a file outside the repository locally and
  from a repository secret in CI, and fails closed when the file is absent. Fork pull requests
  have no secrets, so the stage runs on pushes to `main` and before landing.
- **DCO:** a step of `ci.yml` checks that every pushed commit carries a `Signed-off-by` matching
  its author (about 15 lines; FND-03).
- CodeQL, Scorecard, Dependabot as in 7.1.
- Releases (REL-06): dist builds the archives with GitHub artifact attestations and
  `cargo-auditable`; `SHA256SUMS`, a CycloneDX SBOM (`cargo-cyclonedx` if dist's support is not
  confirmed) and third-party notices; release-plz prepares versions and the changelog; the owner
  publishes the draft.

---

## 8. Tooling pins

1. **mise** pins every tool other than Rust (ADR 0030): `mise.toml` names each tool and version,
   `mise.lock` records its checksums, and `mise install --locked` refuses a mismatch. Backends:
   `aqua:` (checksums, often provenance) for gitleaks, actionlint, shellcheck, zizmor, pinact,
   jq, hyperfine; `cargo:` for cargo-nextest, cargo-deny, cargo-llvm-cov, cargo-mutants,
   cargo-fuzz, adrs, dist, release-plz; `pipx:` for diff-cover; `npm:@informalsystems/quint`
   (with Node); `java` for a Temurin JDK; `github:`/`ubi:` for Apalache. Entries arrive with the
   stage that needs them, each with a comment naming it: just and cargo-nextest (FND-01),
   cargo-deny (FND-02), adrs (FND-03), gitleaks, actionlint, shellcheck, zizmor, pinact (FND-04),
   cargo-llvm-cov, diff-cover, cargo-mutants (FND-06), hyperfine (BENCH-02), Node, Quint,
   Apalache, the JDK (FORMAL-01; the JDK also serves the Java fixtures and the JVM helpers),
   cargo-fuzz and its nightly (REL-03), dist and release-plz (REL-06).
2. Versions seen on 2026-11-16 (each task re-checks the current release; a version is never
   invented): mise 2026.11.9, just 1.59.0, cargo-nextest 0.9.152, cargo-deny 0.21.0,
   cargo-llvm-cov 0.9.1, diff-cover 11.0.2, cargo-mutants 27.1.0, gitleaks 8.30.1, actionlint
   1.7.12, zizmor 1.32.0, pinact 5.0.2, adrs 0.13.0, Quint 0.33.1, Apalache 0.62.1 (Quint 0.33.x
   is validated against it, not 0.63.0), cargo-fuzz 0.13.2, dist 0.34.0, release-plz 0.4.3.
3. `just doctor` runs `mise doctor` and `mise ls --missing` and exits non-zero if any pinned tool
   is missing or wrong.
4. **Fail closed.** A recipe whose tool is missing fails with mise's install hint. A stage is
   never reported as skipped-but-green. When local and CI disagree, run `just doctor` first.
5. Quint runs from mise's pinned npm package and never downloads an evaluator at run time;
   Apalache's archive is verified against its checksum in `mise.lock`.
6. A version bump is its own commit, with the gate green. In CI, `jdx/mise-action` (pinned by
   SHA) installs from `mise.lock`.

---

## 9. Commands

Every repeatable job is a `just` recipe; an ad-hoc command that becomes repeatable becomes a
recipe. The independent tool crates (§2.7) are the exception: their binaries are their own
command surface, kept apart from the gate on purpose, and the gate runs their corpus-level checks
as ordinary tests. `quick` is not `check`; never report one as the other. A recipe whose task has
not landed does not exist yet (`just` exits non-zero naming it), never a skipped stage. **Never
prefix a command with inline `JFL_*`, `QUINT_SEED` or `PROPTEST_*` variables**: a scenario that
needs one becomes a recipe parameter (`just conformance seed=42`).

| Command | What it does | Task |
|:--|:--|:--|
| `mise install --locked` | install every pinned tool from `mise.lock` | FND-01 |
| `just quick` | fmt-check, architecture, clippy, unit tests: the inner loop | FND-02 |
| `just check` | the full blocking gate: every stage, in order, stopping at none silently | FND-02 |
| `just check-only <stage>…` | named stages through the same recipes (what each CI job runs) | FND-02 |
| `just --list`, `just --dump --dump-format json` | the stage catalog | FND-02 |
| `just doctor` | `mise doctor` and `mise ls --missing` | FND-01 |
| `adrs new "<title>"`, `just adr-index` | create an ADR; check the index against the files | FND-03 |
| `scripts/land.sh <sha>… [--no-push]` | cherry-pick onto a fresh `origin/main` worktree, `just check`, push only on green | FND-03 |
| `just coverage`, `just changed-lines`, `just mutants-diff` | llvm-cov vs `coverage-floor.toml`; diff-cover on changed lines; cargo-mutants `--in-diff` | FND-06 |
| `just bench-corpus` | fetch the pinned corpus of `doc/bench-corpus.md` into the cache directory | BENCH-01 |
| `just bench [suite] [corpus=small]` | the benchmark suites with machine facts, JSON results | BENCH-02 |
| `just formal-typecheck`, `just formal-test`, `just formal-verify [model]` | Quint typecheck and tests; Apalache bounded verification | FORMAL-01 |
| `just conformance [model] [seed] [traces] [steps]` | `quint-connect` runs plus committed-trace replay | FORMAL-02 |
| `just java-fixtures` | compile the golden Java corpus with the pinned JDK | EVAL-07 |
| `cargo run --locked -p jfl-eval -- <subcommand>` | the evaluation harness (E0 adapter, reports, E1, E3, E4) | EVAL-01 |
| `cargo run --locked -p jfl-oracle -- <subcommand>` | the SCIP differential and completeness oracles | EVAL-08 |
| `cargo run --locked -p jfl-jdtls -- <subcommand>` | jdtls behind the multiplexing proxy | EVAL-11 |
| `just fuzz <target> [time] [reproduce]` | bounded fuzz run on the pinned nightly | REL-03 |
| `just soak [seed] [minutes]` | the crash and restart soak | REL-04 |
| `just mutation` | cargo-mutants over the configured scope | REL-05 |
| `just release-check [target]` | package, extract and smoke the archive | REL-06 |

Stages of `check` in order (each a recipe): `fmt-check`, `architecture`, `clippy`, `docs` (the
ADR index matches the files), `unit-tests`, `integration-tests`, `doctests`, `e2e-tests`,
`lsp-conformance`, `formal-typecheck`, `formal-test`, `formal-freshness`, `conformance-replay`,
`java-fixtures`, `coverage`, `changed-lines`, `mutants-diff`, `deny`, `secrets`, `workflows`,
and from M1 `bench-gate`. A stage enters `check` in the commit of the task that introduces it,
together with its `ci.yml` job entry. `just private-names` is outside `check` (it needs the
pattern file, §7.4): `scripts/land.sh` runs it before every push and `security.yml` on `main`.

---

## 10. Commits, ADRs and integration

### 10.1 Commits

1. **One commit per task**, DCO-signed, subject `<ID>: <summary>`: imperative, under 72
   characters. The body says why, names new dependencies with their reason, and carries an
   `Impact:` line (none, dependency graph, public API, protocol, `--json` schema, CLI, LSP
   capabilities, cache format, config) (ADR 0003).
2. **Author** is the human driving the work: `Patryk Żmigrodzki <pzmi@users.noreply.github.com>`.
   The **committer** is the environment's own identity, which signs the commit.
3. **Trailer block last, in this order:** `Signed-off-by`, `Co-Authored-By`, `Claude-Session`,
   the last two exactly as the session's attribution instructions give them. Because
   `git commit -s` appends after an existing trailer block, the block is written by hand. No
   other trailers.
4. Stage only the task's files (never `git add -A`); never `--no-verify`; never force-push; never
   push tags. `checkpoint-*` tags are local.
5. No secrets, internal hostnames, private-repository content, personal paths, names of private
   projects or model ids in repository content (the attribution trailers are the only exception;
   ADR 0004).

### 10.2 Integration

Integration is directly on `main` (ADR 0002): the orchestrator reviews each task's commit, then
lands it with `scripts/land.sh` (ADR 0030), which fetches, creates a worktree at `origin/main`,
cherry-picks, runs `just check` on the merged tree, and pushes only on green, retrying once after a
fetch on a non-fast-forward. Conflicts are auto-resolved only in `Cargo.lock` (take integration's
side, then `cargo update -w`) and in the ADR index (regenerated, then `just adr-index`); anything
else aborts and names the files. One commit lands per green CI run on `main`. A red `main` is fixed
immediately, by a new commit. Until FND-03 lands `land`, the orchestrator does the same by hand:
cherry-pick onto a clean `main`, run the gate on the merged tree in the foreground, push only on
green ([`sdlc.md`](sdlc.md)).

### 10.3 ADRs

Nygard format in `doc/adr/NNNN-kebab-title.md`: `# NNNN. Title`, `Date:`, `## Status`
(`Proposed`, `Accepted (owner, YYYY-MM-DD)`, `Rejected (owner, YYYY-MM-DD)`,
`Superseded by NNNN`, with annotations such as `amended by NNNN`), `## Context`, `## Decision`,
`## Consequences`, optional `## Alternatives considered`. Records are created with `adrs new`
(ADR 0030); the index `doc/adr/README.md` is checked against the files by the `docs` stage
(`just adr-index`, FND-03). ADR 0001 records the practice; the ADR number is
the decision id. Only the owner moves a decision to Accepted or decides a `DEC-` task.

---

## 11. Agent contract

### 11.1 Definition of Ready

A task is ready when its card names the cited sections, its dependencies have landed on `main`,
its acceptance criteria are verifiable by a command or test, and its model impact and protocol
impact (if any) are stated ([`sdlc.md`](sdlc.md) has the full list).

### 11.2 What an implementing agent does

1. Reads `tasks.md`'s Decisions and rules, then only the sections the task cites, then the files
   it changes.
2. Works in **its own git worktree** on a branch from current `origin/main`, with **its own
   `CARGO_TARGET_DIR`** (deleted when done); temporary files go in the session scratchpad.
3. Updates policy and ADRs first when a new dependency, edge or authority is needed.
4. **Model first** (ADR 0007): a change to views and generations, the daemon lifecycle or
   diagnostics publication changes the Quint model, its invariants, named scenarios and the
   conformance driver before the Rust code.
5. **Golden first**: a change to the protocol, `--json` or an LSP response writes the golden
   before the code that produces it.
6. Writes failing tests first, then the smallest code that passes, at the lowest test level that
   can fail for the reason (ver §1).
7. Iterates with `just quick`; runs the live gate (`just check`) **in the foreground** on the exact
   tree it commits, and commits only on green. A product task also runs its benchmarks on the
   bench corpus (small at least) and reports the metrics it changes (ADR 0031).
8. Commits once, as §10.1 says.

### 11.3 What an implementing agent never does

Push, land, tag, merge or change remote settings; edit `CLAUDE.md` or `.claude/`; flip a decision
to Accepted or decide a `DEC-` task; delete or weaken a failing test, a modelled invariant, an
architecture rule, a budget or a golden fixture to get green; add `#[ignore]`, an unbacked
`#[allow]` or a sleep; run a gate in the background and end its turn; kill processes by name
pattern; run Gradle or a build on a target Java repository; report `quick` as `check`.

### 11.4 Report format

The final report states, in this order:

1. Task id, commit SHA and subject.
2. Files changed (paths).
3. Gates run, each with its exit status: the exact command, tool versions (`mise ls --current`),
   the seeds used (conformance, proptest) and, for a product task, the benchmark metrics it
   changed with the corpus size and machine.
4. Acceptance criteria, each mapped to the test or command that proves it; `(owner)` criteria
   listed as pending.
5. Deviations from the card, with reasons; ADRs added; decisions needing the owner.
6. Anything not run, and why. No unverified claim of quality.

### 11.5 Definition of Done

Code, tests and models agree; the architecture rules and their fixtures pass; the gate is green
on the merged tree; affected documents and ADRs are updated; the report above is complete. An
exception needs the owner's explicit approval and a follow-up task, with no false green signal.
