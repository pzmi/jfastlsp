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
   fail the architecture checker, or fail a named test, before a reviewer has to notice.
2. **Scope.** Every crate under `crates/` and `tools/`, the Java worker under `java/`, the Quint
   models under `formal/`, the fixtures under `fixtures/`, the workflows under `.github/`, and
   the repository's process files.
3. **Out of scope.** Product behaviour (plan) and the test strategy (verification).
4. **Precedence.** Accepted ADRs override this document; this document overrides earlier plan
   text on engineering matters. A conflict found while working is reported, not resolved by
   choosing the convenient side.
5. **Heritage.** The gate, the architecture checker, the ADR and land tooling are ported from the
   public repository mayfly, which ported them from site-preview (ADR 0012), and adapted to
   jfastlsp's crate names, authority table and lanes.

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
| **test_only** | `jfl-testkit`, `jfl-conformance` | never |
| **tool_only** | `xtask`, `jfl-oracle`, `jfl-eval` | never |
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
   none exist in v0.x. A third-party crate's build script (tree-sitter grammars compile C) is
   allowed and named in the external allowlist with the reason.
6. **Targets.** Edges are evaluated through `cargo metadata --filter-platform` for
   `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin` (ADR 0006);
   a finding on only some of them names those targets.

### 2.3 Pure-crate restrictions

The crates in `[purity] pure` name none of the following in production code, in any `cfg`
branch, through any alias, glob, re-export, type alias, `Self` or macro body
(`ARCH-domain-purity`): `std::fs`, `std::net`, `std::os::unix::net`, `std::process`, `std::env`
(beyond `env!("CARGO_*")`), `std::thread`, `SystemTime`, `Instant`, any random source, Tokio or
any async runtime, `HashMap`/`HashSet` with the default (randomly seeded) hasher (use
`BTreeMap`/`BTreeSet` or a fixed-seed hasher the policy names), atomics, locks or
interior-mutable statics, and `println!`/`eprintln!`/`dbg!`. Each pure crate carries a
`clippy.toml` with `disallowed-methods`, `-types` and `-macros` entries as a second, semantic
layer (`ARCH-pure-clippy-config`). Time and randomness reach application code only through
ports.

### 2.4 Authority

Plan §6.5's table is enforced:

- `ARCH-process-authority`: `std::process`, `Command`, `fork`/`exec` wrappers only in
  `[purity] process_authority` (`jfl-process`, `jfl-composition`'s launcher module);
  `ARCH-child-environment`: every child built starts from `env_clear()` plus an allowlist.
- `ARCH-socket-authority`: `std::net`, `std::os::unix::net`, Tokio's net types and socket
  crates only in `jfl-adapter-ipc`; any TCP listener or stream anywhere is refused.
- `ARCH-fs-authority`: `std::fs`, Tokio's fs and file-watching crates only in the crates that
  plan §6.5 lists; never in pure or application crates.
- `ARCH-detached-spawn`: threads and tasks are spawned only in the supervision modules
  composition and the adapters declare (`[purity] spawn_modules`), on a tracker handed down.

### 2.5 External allowlist

`[external.normal]`, `[external.dev]` and `[external.build]` list, per crate, each third-party
crate it may use and the features it may enable (`"default"` for default features, `"*"` for any).
A normal entry implies dev. Every entry carries a comment saying why. A crate not listed, a
feature not listed or a non-crates.io source fails (`ARCH-unlisted-external`,
`ARCH-external-feature`, `ARCH-external-source`).

### 2.6 Test-only, tool-only and fuzz-only crates

- `jfl-testkit` holds a fake for every port of plan §6.4, the contract suites (ver §5), fixture
  builders (temporary Git repositories with linked worktrees, fixture project copies) and the
  `poll` helper, the only sanctioned wait on a real-OS condition (ver §3).
- `jfl-conformance` holds the ITF replay harness and the drivers (ver §6).
- `xtask` is the command surface (§9). It may use anything it needs; it is never a dependency.
- `jfl-fuzz` is its own workspace on the pinned nightly (REL-03); `ARCH-fuzz-crate` checks its
  policy rows.

### 2.7 Independent oracles

`jfl-oracle` and `jfl-eval` judge the product from outside: they run the built `jfl`, read its
`--json`, and read SCIP indexes and Claude Code transcripts with their own code. They import no
production crate (`ARCH-independent-oracle`): an oracle that shares the resolver's assumptions
proves nothing.

---

## 3. Enforcement

Three layers: the compiler (crate boundaries, visibility, `trybuild` compile-fail tests for
critical APIs), the **policy** (`architecture.toml` over the resolved Cargo graph) and **semantic
rules** (`syn` over every source file a target reaches). A tool that is absent, a selector that
matches nothing, or a file that fails to parse is a failure, never a pass.

### 3.1 `architecture.toml` schema

```toml
schema = 1

[workspace]
production = [ ... ]          # §2.1
test_only  = ["jfl-testkit", "jfl-conformance"]
tool_only  = ["xtask", "jfl-oracle", "jfl-eval"]
fuzz_only  = ["jfl-fuzz"]     # once the fuzz crate exists

[roles]                       # §2.1, inside production
pure        = ["jfl-domain", "jfl-protocol"]
application = ["jfl-application"]
support     = ["jfl-process"]
adapter     = [ ... ]
root        = ["jfl-composition", "jfl-cli"]

[purity]                      # §2.3, §2.4, §2.7
pure = ["jfl-domain", "jfl-protocol"]
process_authority = ["jfl-process", "jfl-composition::launcher"]
socket_authority = ["jfl-adapter-ipc"]
fs_authority = [ ... ]        # plan §6.5
spawn_modules = [ ... ]
independent = ["jfl-oracle", "jfl-eval"]

[dependencies.normal]         # plan §6.3, one key per crate
[dependencies.build]

[external.normal]             # §2.5, crate = { dep = ["feature", ...] }
[external.dev]
[external.build]

[exceptions]
# [exceptions.sources] "<source>" = { adr = "NNNN" }
# [[exceptions.lints]] path = "...", lint = "...", adr = "NNNN"

[semantic]
# absent = [ ... ]            # fixtures only: crates this fixture workspace lacks
```

Parsed strictly: an unknown key or another `schema` value fails `ARCH-policy-schema`; a crate in
two categories fails `ARCH-duplicate-category`, a present but empty list `ARCH-empty-selector`;
a name that is not a workspace crate fails `ARCH-missing-crate`. An exception citing an ADR file
that does not exist, or an exception that no longer matches anything, fails. Selectors are data
in the policy, not crate names in the checker's code, so one file shows the whole law.

### 3.2 The checker (`cargo xtask architecture`)

Ported with its module layout (ADR 0012): policy parse, source inventory vs `members` vs policy,
the Cargo graph from `cargo metadata --locked` per target with edges by kind and full dependency
paths in each diagnostic, the external allowlist, and the semantic visitor. The visitor walks
each target from its root through `mod` (with `#[path]` and `cfg_attr(.., path)`), inline
modules and literal `include!`; resolves names through `use`, globs, `extern crate`, manifest
renames, type aliases, `Self` and qualified self types, and across workspace crates' re-exports;
scans macro token streams conservatively; and reads every `cfg` branch. Diagnostics are
`ARCH-<id>: message (at path[:line])`. Rules are declared in one `rules!` table (id, owning task,
description, fixtures) that also feeds `list-checks`.

### 3.3 Rules

The rules FND-04 and FND-05 port; each task that adds a rule adds its row here.

| Rule | What fails | Task |
|:--|:--|:--|
| `ARCH-policy-schema`, `ARCH-metadata` | the policy or `cargo metadata` cannot be read | FND-04 |
| `ARCH-missing-member`, `ARCH-not-a-member`, `ARCH-unknown-crate`, `ARCH-missing-crate` | inventory and policy disagree | FND-04 |
| `ARCH-empty-selector`, `ARCH-duplicate-category`, `ARCH-missing-selector` | malformed selectors | FND-04 |
| `ARCH-forbidden-edge`, `ARCH-forbidden-dev-edge`, `ARCH-undeclared-build-dep`, `ARCH-production-closure`, `ARCH-policy-cycle` | an edge outside plan §6.3 | FND-04 |
| `ARCH-independent-oracle` | an independent tool crate with any edge to a workspace crate | FND-04 |
| `ARCH-unlisted-external`, `ARCH-external-feature`, `ARCH-external-source` | §2.5 | FND-04 |
| `ARCH-fuzz-crate` | the fuzz crate outside its policy rows | FND-04 |
| `ARCH-source-parse`, `ARCH-foreign-source`, `ARCH-include-macro` | a file the visitor cannot read, or source from outside the package | FND-05 |
| `ARCH-domain-purity`, `ARCH-pure-clippy-config` | §2.3 | FND-05 |
| `ARCH-process-authority`, `ARCH-child-environment`, `ARCH-socket-authority`, `ARCH-fs-authority`, `ARCH-detached-spawn` | §2.4 | FND-05 |
| `ARCH-layer-isolation`, `ARCH-hidden-seam` | §2.2 rule 2; naming composition's hidden items | FND-05 |
| `ARCH-command-isolation`, `ARCH-query-purity` | application command code naming query modules or the reverse; a query mutating state | FND-05 |
| `ARCH-observability-isolation` | instrumentation in pure crates, or a `tracing` value reaching a branch | FND-05 |
| `ARCH-unsafe-code`, `ARCH-unfinished-code`, `ARCH-nightly-features` | `unsafe`, `todo!`/`unimplemented!`/`dbg!`, `#![feature]` | FND-05 |
| `ARCH-lint-config`, `ARCH-lint-suppression`, `ARCH-macro-export` | lint configuration loosened; `#[allow]`/`#[expect]`/`coverage(off)`/`#[macro_export]` without an ADR-cited exception | FND-05 |
| `ARCH-ignored-test`, `ARCH-test-name`, `ARCH-test-structure`, `ARCH-test-sleep` | `#[ignore]`; a test not named `should_<outcome>_when_<condition>`; missing `// given`, `// when`, `// then`; a sleep outside the testkit's `poll` | FND-05 (`poll` exemption from FND-08) |
| `ARCH-platform-branch` | a `cfg!(target_os…)` runtime branch | FND-05 |
| `ARCH-anyhow-in-library`, `ARCH-unbounded-channel` | `anyhow` in a library target; an unbounded channel in production | FND-05 |
| `ARCH-fixtures-lock` | a golden fixture (`fixtures/**`, `crates/*/tests/fixtures/**`) whose digest differs from `fixtures.lock` | IDX-07 |

Rules planned by later tasks bind by review until then:

| Rule | What will fail | Task |
|:--|:--|:--|
| `ARCH-log-event-name` | a `tracing` event whose message is not a literal `snake_case` name | REL-02 |
| `ARCH-position-arithmetic` | arithmetic on LSP `Position` fields outside `jfl-domain`'s conversion module | LSP-02 |

### 3.4 Fixtures

1. Every rule has at least one **negative fixture** under
   `tools/xtask/tests/architecture-invalid/<name>/`: a miniature workspace (manifest, committed
   lockfile, policy, sources, third-party crates stubbed as a vendored directory source) and an
   `expected.txt` with `rule:`, `at:`, optional `contains:`/`only:` and `absent:` lines.
2. Positive fixtures under `architecture-valid/` keep the rules from over-reaching (an adapter
   using `jfl-process`, the IPC adapter opening a Unix socket, a test using the `poll` helper).
3. Fixture crates carry jfastlsp's crate names; a crate named in a rule's selector is added to
   every fixture's `absent` list when the fixture lacks it.
4. `check` runs the fixtures through the `architecture` stage; catalog completeness (every rule
   has a fixture, every fixture a rule) is part of every run.

### 3.5 Hygiene

- Workspace lints only (§4.2); no crate-level overrides.
- No `#[allow]`/`#[expect]` or `#[macro_export]` without an ADR-backed exception row; no
  `#[ignore]`.
- Tests named `should_<outcome>_when_<condition>` with `// given`, `// when`, `// then`.
- No mocking frameworks; real objects over testkit fakes; tempdirs; sockets in tempdirs.
- A rule is never loosened to get green. A change to `architecture.toml` that widens an edge,
  allowlist, role, authority or exception needs the task to say so and an ADR.

### 3.6 Threat model of the checker

The checker defends against honest mistakes, shortcuts and drift, not a determined insider;
review is the backstop. The xtask preflight refuses to run from another checkout than the one it
was built from (a target directory shared between worktrees would otherwise allow it) and refuses
an environment that steers the build: `RUSTC_WRAPPER`, `RUSTC_WORKSPACE_WRAPPER`, `RUSTFLAGS`
other than `-D warnings`, `CLIPPY_CONF_DIR`, `CLIPPY_*`, `RUSTC_BOOTSTRAP`, `CARGO_ALIAS_*`,
`CARGO_UNSTABLE_*`, per-target runners. Never export them for the gate.

---

## 4. Language and lint policy

### 4.1 Baseline

Stable toolchain pinned exactly in `rust-toolchain.toml` (ADR 0011: 1.97.0, edition 2024,
resolver 3, `rustfmt`, `clippy`, `llvm-tools-preview`). `Cargo.lock` committed; every cargo
command in the gate runs `--locked`. `publish = false` everywhere. A dated nightly is pinned in
`tools/versions.toml` for `cargo-fuzz` only and never builds a shipped artefact.

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
| `ci.yml` | push to `main`, pull request, dispatch | the gate, split into jobs (7.2) | FND-07 |
| `security.yml` | weekly, dispatch | advisories, licences, secret scan over full history, workflow lint | FND-07 |
| `codeql.yml` | push to `main`, pull request, weekly | CodeQL for Rust and Actions | FND-07 |
| `scorecard.yml` | push to `main`, weekly | OpenSSF Scorecard, SARIF upload | FND-07 |
| `formal.yml` | weekly, dispatch | Apalache bounded verification per model; fresh-seed conformance | FORMAL-01, FORMAL-02 |
| `fuzz.yml` | nightly, dispatch | each fuzz target for a time budget; crash inputs uploaded | REL-03 |
| `mutation.yml` | weekly, dispatch | `cargo-mutants` over the configured scope | REL-05 |
| `release.yml` | tag `v*`, dispatch | build, SBOM, checksums, provenance, draft release | REL-06 |
| `.github/dependabot.yml` | weekly | cargo, github-actions, npm (`formal/`), grouped minor and patch | FND-07 |

### 7.2 The gate in CI

1. **Each job is `cargo xtask check --only <stages>`.** CI never re-implements a stage in YAML;
   local and CI run the same code. The jobs and their stages are in
   [`required-checks.md`](required-checks.md).
2. **One aggregate job, `required`**, runs `if: always()`, needs every other job of `ci.yml`, and
   fails unless every result is `success` (skipped and cancelled fail).
3. **Self-tests** in xtask parse `ci.yml` and fail when an `in_check` stage is in zero jobs or in
   two, when `required` does not need every job, when a gating step has `continue-on-error`,
   an `if:`, a failure-swallowing `run:` (`|| true`, `set +e`, `exit 0`) or a steering `env:`, and
   when `required-checks.md` and `ci.yml` disagree.
4. Native jobs on `ubuntu-24.04`, `ubuntu-24.04-arm` and `macos-15` (arm64) run the platform
   tests (ADR 0006).
5. Scheduled workflows are outside `required`; their failures are reported and fixed as tasks.
6. A red job is fixed, never re-run to green. One re-run is allowed only when the runner died
   before a test body ran, and the reason is written down.

### 7.3 Workflow hygiene

- `permissions: contents: read` at the top; a job adds only the scope one step needs, with the
  reason beside it.
- Every `uses:` is pinned to a 40-hex commit SHA with the version in a comment, or is a local
  action; container images by `@sha256:` digest (`action-pins` stage).
- `actions/checkout` with `persist-credentials: false`; no `pull_request_target`; inputs reach
  scripts through `env`, never interpolated into `run:`.
- Concurrency per ref, and per commit on `main`: a newer push to `main` neither cancels nor
  replaces a pending run; cancel-in-progress only for pull requests.
- Caches are keyed by toolchain, lockfile and `tools/versions.toml`, and saved only from `main`.

### 7.4 Supply chain

- `cargo-deny`: advisories (yanked denied), licences (permissive allowlist; each exception per
  crate with an ADR; `notify` is CC0-1.0 and tree-sitter grammars MIT, to confirm at the task
  that adds them), bans (no wildcards), sources (crates.io only); `[graph] targets` set to the
  three targets of ADR 0006.
- `gitleaks` over the full history; `.gitleaks.toml` allowlists nothing; fixtures build fake
  secrets at run time if they need any.
- CodeQL, Scorecard, Dependabot as in 7.1.
- Releases (REL-06): `SHA256SUMS`, build-provenance attestation, CycloneDX SBOM, third-party
  notices; the owner publishes the draft.

---

## 8. Tooling pins

1. `tools/versions.toml` pins every tool a stage needs, each with an install `method` (`cargo`,
   `rustup`, `manual`, `lockfile`), a version and a `probe` whose output must contain that
   version; only the tools a registered stage needs, each with a comment naming the stage. Entries
   arrive with their stages: cargo-nextest, cargo-deny, gitleaks, actionlint, shellcheck
   (FND-02, FND-06), cargo-llvm-cov (FND-08), Node, Quint (owned by `formal/package-lock.json`),
   Apalache (version and SHA-256) and the JDK major (FORMAL-01; the JDK also serves the Java
   fixtures and the javac worker), cargo-fuzz and its nightly (REL-03), cargo-mutants (REL-05),
   cargo-cyclonedx (REL-06). A version is never invented: the task that adds a pin checks the
   current release.
2. `cargo xtask doctor [--json] [--all]` reports each tool as found, missing or wrong version, with
   the install hint; it exits non-zero if any tool a `check` stage needs is not right, and with
   `--all` for every pinned tool. `cargo xtask install-tools [tool…]` installs what can be
   installed.
3. **Fail closed.** Every stage declares the tools it needs; a missing or wrong tool makes that
   stage fail with the hint. A stage is never reported as skipped-but-green. When local and CI
   disagree, run `doctor` first.
4. Quint always runs from `formal/node_modules` through the TypeScript backend and never
   downloads an evaluator at run time; Apalache is fetched by xtask and verified against its
   SHA-256 before use.
5. A version bump is its own commit, with the gate green.

---

## 9. Commands

Every repeatable job is an xtask subcommand; an ad-hoc command that becomes repeatable becomes a
subcommand. The two independent tool crates (§2.7) are the exception: their binaries are their
own command surface, kept apart from the gate's code on purpose, and the gate runs their
corpus-level checks as ordinary tests. `quick` is not `check`; never report one as the other. A command whose task has not
landed does not exist yet (clap exits 2); a `check --only` stage that does not exist yet is an
unknown name (exit 2, listing the valid ones), never a skipped stage. **Never prefix a command
with inline `JFL_*`, `QUINT_SEED` or `PROPTEST_*` variables**: a scenario that needs one becomes an
xtask flag.

| Command | What it does | Task |
|:--|:--|:--|
| `cargo xtask quick` | fmt-check, architecture, clippy, unit tests: the inner loop | FND-02 |
| `cargo xtask check` | the full blocking gate: every `in_check` stage, a stage table, never a downgraded failure | FND-02 |
| `cargo xtask check --only <a,b>` | named stages in gate order through the same code | FND-02 |
| `cargo xtask list-checks` | the catalog as JSON: stage, level, command, owning task, `in_check`, one row per ARCH rule | FND-02 |
| `cargo xtask doctor [--json] [--all]` | pinned tools: found, missing, wrong | FND-02 |
| `cargo xtask install-tools [tool…]` | install pinned tools | FND-02 |
| `cargo xtask adr new "<title>" [--number N]` / `adr index [--fix]` | allocate an ADR; check or rewrite the index | FND-03 |
| `cargo xtask land <ref>… (--push R B \| --no-push)` | cherry-pick onto a clean, current `main`, run `check`, fast-forward and push only on green | FND-03 |
| `cargo xtask architecture` | policy, semantic rules, fixtures, catalog completeness | FND-04, FND-05 |
| `cargo xtask test [--only <stage>]` | test stages alone (unit, integration, doctests, e2e, lsp-conformance, platform-tests) | FND-08 |
| `cargo xtask coverage [--changed-since <rev>]` | llvm-cov over in-process suites vs `coverage-floor.toml`; changed-line gate | FND-08 |
| `cargo xtask formal-typecheck` / `formal-test` | typecheck every registered `.qnt`; run Quint scenario tests | FORMAL-01 |
| `cargo xtask formal-verify [--model <name>]` | Apalache bounded verification plus long simulation | FORMAL-01 |
| `cargo xtask conformance [--model M] [--seed S] [--traces K] [--steps N]` | generate seeded traces and replay them with the regression traces through the drivers | FORMAL-02 |
| `cargo xtask java-fixtures` | compile the golden Java corpus with the pinned JDK; refresh nothing | EVAL-07 |
| `cargo run --locked -p jfl-eval -- <subcommand>` | the evaluation harness (E0 report, E1, E3, E4 measurements) | EVAL-01 |
| `cargo run --locked -p jfl-oracle -- <subcommand>` | the SCIP differential and completeness oracles | EVAL-08 |
| `cargo xtask fuzz <target> [--time S] [--reproduce F]` | bounded fuzz run on the pinned nightly | REL-03 |
| `cargo xtask soak [--seed S] [--minutes M]` | the crash and restart soak (a short budget in CI, long runs owner-run) | REL-04 |
| `cargo xtask mutation [--in-diff <rev>] [--shard k/n]` | `cargo-mutants` over the configured scope | REL-05 |
| `cargo xtask bench [--save <name>] [--compare <name>]` | the benchmark suite with recorded machine facts | REL-05 |
| `cargo xtask release-check [--target T]` / `sbom` | package, extract and smoke the archive; produce the SBOM | REL-06 |

Stages of `check` in order (each a catalog row): `fmt-check`, `architecture`, `clippy`, `docs`
(the ADR index matches the files), `unit-tests`, `integration-tests`, `doctests`, `e2e-tests`,
`lsp-conformance`, `formal-typecheck`, `formal-test`, `formal-freshness`, `conformance-replay`,
`java-fixtures`, `coverage`, `changed-lines`, `deny`, `secrets`, `workflows`, `action-pins`. A stage
enters `check` in the commit of the task that introduces it, together with its `ci.yml` job entry.

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
lands it with `cargo xtask land`, which starts from a clean `main` equal to the remote after a
fetch, cherry-picks on a detached head, runs the full `check` on the merged tree, and
fast-forwards and pushes only on green. Conflicts are auto-resolved only in `Cargo.lock` (take
integration's side, then `cargo update -w`) and in the ADR index (`adr index --fix`); anything
else aborts and names the files. One commit lands per green CI run on `main`. A red `main` is
fixed immediately, by a new commit. Until FND-03 lands `land`, the orchestrator does the same by
hand: cherry-pick onto a clean `main`, run the gate on the merged tree in the foreground, push
only on green ([`sdlc.md`](sdlc.md)).

### 10.3 ADRs

Nygard format in `doc/adr/NNNN-kebab-title.md`: `# NNNN. Title`, `Date:`, `## Status`
(`Proposed`, `Accepted (owner, YYYY-MM-DD)`, `Rejected (owner, YYYY-MM-DD)`,
`Superseded by NNNN`, with annotations such as `amended by NNNN`), `## Context`, `## Decision`,
`## Consequences`, optional `## Alternatives considered`. The index `doc/adr/README.md` is checked
against the files by the `docs` stage (FND-03). ADR 0001 records the practice; the ADR number is
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
7. Iterates with `cargo xtask quick`; runs the live gate **in the foreground** on the exact tree it
   commits, and commits only on green.
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
3. Gates run, each with its exit status: the exact command, tool versions from `doctor`, and the
   seeds used (`--seed` of conformance, proptest).
4. Acceptance criteria, each mapped to the test or command that proves it; `(owner)` criteria
   listed as pending.
5. Deviations from the card, with reasons; ADRs added; decisions needing the owner.
6. Anything not run, and why. No unverified claim of quality.

### 11.5 Definition of Done

Code, tests and models agree; the architecture rules and their fixtures pass; the gate is green
on the merged tree; affected documents and ADRs are updated; the report above is complete. An
exception needs the owner's explicit approval and a follow-up task, with no false green signal.
