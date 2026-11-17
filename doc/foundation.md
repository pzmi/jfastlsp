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
5. **One `cargo xtask` surface, reached piece by piece** (ADR 0035, superseding ADR 0030, which
   had superseded ADR 0011 and ADR 0012). FND-01..FND-08 assembled the foundation from maintained
   tools: mise pins them, just composes the gate, the compiler and cargo-deny enforce most of the
   dependency law, adrs keeps the decisions, and a few named scripts and the architecture test
   stay custom. That foundation is the gate this document describes **today**. The owner decided
   on 2026-11-17 to port mayfly's `tools/xtask` after all: lane XT (`doc/tasks.md`) replaces the
   justfile, mise and the shell scripts one piece per card, external tools (cargo-deny, gitleaks,
   zizmor, actionlint, shellcheck, hyperfine, nextest, Quint) stay and xtask runs them, tool
   versions move to `tools/versions.toml` with `cargo xtask doctor`, and every check the gate runs
   keeps running. Each XT card updates the sections here that name what it replaces.

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
| **tool_only** | `jfl-oracle`, `jfl-eval`, `jfl-jdtls`, `jfl-bench`, `xtask` | never |
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
   SQLite compile C) is allowed in `deny.toml`, by name in the build-script allowlist or, for a
   script read and found to only probe for cfgs, by `bypass` with its checksum, with the reason.
6. **Targets.** Edges are evaluated through `cargo metadata --filter-platform` for
   `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin` (ADR 0006);
   a finding on only some of them names those targets.

### 2.3 Pure-crate restrictions

Pure crates (`jfl-domain`, `jfl-protocol`, and `jfl-resolve` if it joins) are **`#![no_std]`
with `extern crate alloc`** (ADR 0030). In the crate's own code the compiler then refuses
`std::fs`, `std::net`, `std::os::unix::net`, `std::process`, `std::env`, `std::thread`,
`SystemTime`, `Instant`, the default-hashed `HashMap`/`HashSet` and `println!`/`eprintln!`/`dbg!`,
with no list to keep current. `#![no_std]` alone does **not** keep std out of the crate's
dependencies: on a hosted target a `no_std` crate may depend on a std crate and still build. So
the `nostd` stage also builds every pure crate for a target that has no `std` at all,
`cargo build --locked -p jfl-domain -p jfl-protocol --target x86_64-unknown-none` (the target is
in `rust-toolchain.toml`'s `targets`; FND-01), which fails on any std-only crate in the closure,
`line-index` included if it ever needs std (ADR 0020). Both halves are `ARCH-domain-purity`; the
negative fixture `tools/jfl-architecture/fixtures/pure-uses-std/` proves the first (FND-07).
Collections are `BTreeMap`/`BTreeSet` or a
fixed-seed hasher. What `core` still offers is denied by each pure crate's `clippy.toml`
(`disallowed-types` for atomics and `core::cell` statics; `ARCH-pure-clippy-config`), and
`cargo-deny` keeps randomness, async runtimes and I/O crates out of their closure (wrappers,
§2.5). Time and randomness reach application code only through ports.

### 2.4 Authority

Plan §6.5's table is enforced per crate, because clippy has no module-level configuration:

- The **root `clippy.toml`** holds the canonical `disallowed-methods`, `-types` and `-macros`,
  grouped by the grant that lifts them:
  - `process`: `std::process::Command`, `tokio::process::Command`;
  - `socket`: `std::os::unix::net::*` types and Tokio's Unix types (TCP and UDP types are in no
    grant: denied everywhere);
  - `fs`: `std::fs` functions and types, the `Path` methods that touch the filesystem, Tokio's fs;
  - `spawn`: `std::thread::spawn`, `Builder::spawn`, `scope`, `Scope::spawn`,
    `Builder::spawn_scoped`, `tokio::spawn` and its `spawn_blocking`/`spawn_local`;
  - `sleep`: every timed wait, not only a sleep (FND-08): `std::thread::sleep`, `park_timeout`,
    `Receiver::recv_timeout`, `Condvar::wait_timeout*`, the Unix sockets' `set_read_timeout` and
    `set_write_timeout`, crossbeam's `recv_timeout`/`recv_deadline`, and Tokio's `sleep`,
    `sleep_until`, `timeout`, `timeout_at`, `interval` and `interval_at`;
  - `clock`: `SystemTime::now`, `Instant::now`, `SystemTime::elapsed`, `Instant::elapsed`,
    Tokio's `Instant::now`;
  - `env`: `std::env::var`, `var_os`, `vars`, `vars_os`, `set_var`, `remove_var`, `home_dir`,
    `temp_dir`, `current_dir`, `set_current_dir`, `current_exe`;
  - no grant: unbounded channels (`tokio::sync::mpsc::unbounded_channel`,
    `std::sync::mpsc::channel`, `crossbeam_channel::unbounded`).

  `[workspace.lints.clippy]` sets `disallowed_methods`, `disallowed_types` and
  `disallowed_macros` to `deny`.
- A crate granted an authority carries **its own `clippy.toml`**: the canonical list minus
  exactly its grants. Clippy has no inheritance, so the architecture test checks every crate's
  list against the canonical one and the `[grants]` table of `architecture.toml`
  (`ARCH-clippy-lists`). Only entries whose crate may be absent (Tokio, crossbeam, and std in
  the no_std pure crates) carry `allow-invalid` (`ARCH-clippy-lists` refuses it on a std, core
  or alloc path of the root list, and any `clippy.toml` or `.clippy.toml` other than the root's
  and the members' own, since clippy takes the nearest one walking up from a crate); the `clippy`
  stage fails on any entry that names no path, since clippy only warns about it. The lists are generated: `just clippy-lists` writes each crate's file
  from the root one (whose sections are marked `# grant: <name>`; the pure crates' extra entries
  are `#pure ` lines), and the `clippy` stage fails first when a file differs. The grants, kept tight for production crates (plan §6.5):

  | Crate | Grants | Why |
  |:--|:--|:--|
  | `jfl-process` | process, clock, env | it spawns; deadlines need `Instant`; it copies the allowlisted variables into the child |
  | `jfl-adapter-ipc` | socket, fs, spawn | the endpoint, the state directory and lock, per-connection tasks |
  | fs adapters of plan §6.5 (`-fs`, `-git`, `-store`, `-classfile`, `-javac`, `-gradle`) | fs | their own files only |
  | `jfl-composition` | fs, spawn, clock, env | the system `Clock`, configuration, task supervision |
  | `jfl-cli` | env | `--state-dir` defaults (`XDG_*`, `HOME`) |
  | `jfl-testkit` | process, fs, sleep, clock | fixture builders run `git` and create tempdirs; sleep and clock inside `poll` only (by review) |
  | `jfl-conformance` | fs, env | committed traces; seed and step parameters from the recipe |
  | `jfl-architecture` | process, fs, env | runs `cargo metadata`, `git`, `bash scripts/*.sh` on scratch repositories |
  | `jfl-bench`, `jfl-eval` | process, fs, clock, env | drive binaries and JVMs, write results, time runs |
  | `jfl-jdtls` | process, fs, socket, spawn, sleep, clock, env | its per-user daemon on a Unix socket, one jdtls per worktree; its idle and hand-shake deadlines are timed waits (ADR 0028, amended) |
  | `jfl-oracle` | fs | reads indexes and reports |
  | `xtask` | process, fs, env | runs the gate's stages (argv only, never `sh -c`), reads the tool and cargo configuration, judges the environment (XT-01) |

  Pure and application crates get none. A crate's tests share its `clippy.toml`, so a test that
  needs files or a child in a crate without the grant uses `jfl-testkit`'s builders.
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
- The command surface is the `justfile` (§9) today; the gate is moving to `cargo xtask` (ADR 0035,
  lane XT). Since XT-01 the gate's composition (the stage catalog, its order, each stage's
  command and the preflight) is the tool-only crate `xtask` (`tools/xtask`), which the justfile's
  gate recipes run through `scripts/xtask.sh`.
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

**Standing principle: allowlists and pinned shapes, never lists of bad patterns** (FND-08). Every
check of the gate states what may be there, and everything else fails: the environment
variables `_preflight` lets through, the keys a cargo or mise configuration may hold, the inputs
of an action, the shape of a `run:` line, the sources a dependency may come from, the keys of
`.config/nextest.toml`, the bytes of the gitleaks policy, the hashes of the gate's own recipes.
A list of known-bad spellings (a regex for `runner`, a denylist of TOML forms) is refused in
review: each of FND-02, FND-07 and FND-05 needed two or three review rounds because a reviewer
found one more spelling (quoted keys, literal keys, unicode escapes, a C raw string). Where the
allowed shape cannot be parsed whole, it is a strict subset that fails closed: a line outside
the subset fails, even when it would have been harmless.

### 3.1 `architecture.toml` schema

```toml
schema = 1

[workspace]
production = [ ... ]          # §2.1
test_only  = ["jfl-testkit", "jfl-conformance", "jfl-architecture"]
tool_only  = ["jfl-oracle", "jfl-eval", "jfl-jdtls", "jfl-bench", "xtask"]
fuzz_only  = ["jfl-fuzz"]     # once the fuzz crate exists

[roles]                       # §2.1, inside production
pure        = ["jfl-domain", "jfl-protocol"]
application = ["jfl-application"]
support     = ["jfl-process"]
adapter     = [ ... ]
root        = ["jfl-composition", "jfl-cli"]

[grants]                      # §2.4: what each crate's clippy.toml may drop from the canonical list
process = ["jfl-process", "jfl-testkit", "jfl-architecture", "jfl-bench", "jfl-eval", "jfl-jdtls", "xtask"]
socket  = ["jfl-adapter-ipc", "jfl-jdtls"]
fs      = [ ... ]             # §2.4's table
spawn   = ["jfl-adapter-ipc", "jfl-composition", "jfl-jdtls"]
sleep   = ["jfl-testkit", "jfl-jdtls"]
clock   = ["jfl-process", "jfl-composition", "jfl-testkit", "jfl-bench", "jfl-eval", "jfl-jdtls"]
env     = ["jfl-process", "jfl-composition", "jfl-cli", "jfl-conformance", "jfl-architecture", "jfl-bench", "jfl-eval", "jfl-jdtls", "xtask"]
independent = ["jfl-oracle", "jfl-eval", "jfl-jdtls", "jfl-bench", "xtask"]

[dependencies.normal]         # plan §6.3, one key per crate
[dependencies.build]

[external.normal]             # §2.5, crate = { dep = ["feature", ...] }
[external.dev]
[external.build]

[exceptions]
# [[exceptions.lints]] path = "...", lint = "...", adr = "NNNN"
```

Parsed strictly: an unknown key or another `schema` value fails `ARCH-policy-schema`; a crate in
two categories or two roles, or a role outside `production`, fails `ARCH-duplicate-category`, a
present but empty list `ARCH-empty-selector`; a name that is not a workspace crate fails
`ARCH-missing-crate`. A workspace member in no category, a production crate without a role, or a
member without a `[dependencies.normal]` row fails `ARCH-missing-member`; a crate directory under
`crates/` or `tools/` left out of `members`, or a glob in `members`, fails `ARCH-not-a-member`.
An exception citing an ADR file that does not exist, or an exception that no longer matches
anything, fails `ARCH-lint-suppression`; a row names exactly one lint, and every lint of an
attribute needs its own row. The edge table itself follows the layer law (pure on pure, the
application on pure, support on nothing, adapters on pure, application and support, roots on
production, test-only crates never on tool-only ones, tool-only crates on nothing): a row past it
fails `ARCH-forbidden-edge`, and meta-tests compare `[dependencies.normal]` with plan §6.3's table
and `[grants]` with §2.4's.

### 3.2 The architecture test (`jfl-architecture`)

A test-only crate in `tools/jfl-architecture` (FND-07), run by `just check-only architecture`
under nextest. About 2,000 formatted lines plus 1,500 of tests (ADR 0030 guessed 200, amended; the strict policy parse, the
clippy-list model and a small lexer that drops literals and comments make up most of it) over the
`cargo_metadata` crate (`cargo metadata --locked --filter-platform` for each target of ADR 0006)
plus a small source scan:

1. **Graph:** every workspace member is classified; every normal and dev edge is in plan §6.3's
   matrix; the production closure reaches no test-only or tool-only crate; independent tools
   reach no workspace crate; each crate's direct externals and features are in its allowlist;
   every member has `[lints] workspace = true` and `publish = false`.
2. **Lists:** every crate's `clippy.toml` equals the canonical list minus its grants.
3. **Source scan** (line-oriented, deliberately simple): tests are named
   `should_<outcome>_when_<condition>` and carry `// given`, `// when`, `// then`; no
   `#[ignore]`; no `#[allow]`/`#[expect]`/`#[macro_export]` without an `[exceptions]` row citing
   an ADR; no `cfg!(target_os…)` branch; `#![no_std]` present in each pure crate's `lib.rs`.
4. **CI lockstep:** `ci.yml` (its `on:` triggers included), the `justfile` (its recipe headers,
   read line by line), xtask's stage catalog (`bash scripts/xtask.sh list --json`, XT-01) and
   `required-checks.md` agree (fnd §7.2).
5. **Gate configuration** (`ARCH-gate-config`, FND-08, §3.6): the gate's own recipes and scripts
   match their pins in `.config/gate.sha256`; `.config/nextest.toml`, `mise.toml` and
   `rust-toolchain.toml` keep their allowlisted shapes; the tree holds no cargo configuration
   file and no other mise or toolchain file.

Diagnostics read `ARCH-<id>: message (at path[:line])` with the full dependency path for a
transitive edge.

### 3.3 Rules

Each rule names the layer that enforces it and the task that lands it.

| Rule | What fails | Enforced by | Task |
|:--|:--|:--|:--|
| `ARCH-policy-schema`, `ARCH-metadata`, `ARCH-missing-member`, `ARCH-not-a-member`, `ARCH-missing-crate`, `ARCH-empty-selector`, `ARCH-duplicate-category` | the policy, the metadata or the inventory disagree | architecture test | FND-07 |
| `ARCH-forbidden-edge`, `ARCH-forbidden-dev-edge`, `ARCH-undeclared-build-dep`, `ARCH-production-closure`, `ARCH-independent-oracle`, `ARCH-layer-isolation`, `ARCH-policy-cycle` | an edge outside plan §6.3; an adapter naming a sibling adapter, composition or the CLI (by any edge, or in the table); a cycle in the table | architecture test | FND-07 |
| `ARCH-unlisted-external`, `ARCH-external-feature` | §2.5, direct dependencies, by name and by source (a member's own path or crates.io; FND-08); any package of the graph from elsewhere; `[patch]` or `[replace]` | architecture test | FND-07 |
| `ARCH-external-source`, `ARCH-anyhow-in-library`, adapter-only crates outside their adapter | §2.5, transitive | cargo-deny | FND-02 |
| `ARCH-domain-purity` | std or I/O in a pure crate or its closure | compiler (`#![no_std]`) and the `nostd` stage (§2.3) | FND-02 (stage), FND-07 (negative fixture) |
| `ARCH-pure-clippy-config`, `ARCH-process-authority`, `ARCH-socket-authority`, `ARCH-fs-authority`, `ARCH-clock-authority`, `ARCH-env-authority`, `ARCH-detached-spawn`, `ARCH-unbounded-channel`, `ARCH-test-sleep` | §2.3, §2.4 | clippy lists | FND-02 |
| `ARCH-clippy-lists` | a crate's list differs from the canonical list minus its grants | architecture test | FND-07 |
| `ARCH-child-environment` | a child without a cleared environment | `jfl-process`'s API and contract suite | IDX-04 |
| `ARCH-unsafe-code`, `ARCH-unfinished-code`, `ARCH-nightly-features` | `unsafe`, `todo!`/`unimplemented!`/`dbg!`, `#![feature]` | `[workspace.lints]` (`unsafe_code = "forbid"`, clippy `todo`, `dbg_macro`) and the stable toolchain | FND-01 |
| `ARCH-lint-config`, `ARCH-lint-suppression` | lints loosened; `#[allow]`/`#[expect]`/`#[macro_export]` without an ADR-cited exception | architecture test (source scan) | FND-07 |
| `ARCH-ignored-test`, `ARCH-test-name`, `ARCH-test-structure`, `ARCH-platform-branch` | `#[ignore]`; a misnamed test; missing sections; a `cfg!(target_os…)` branch | architecture test (source scan) | FND-07 |
| `ARCH-hidden-source` | `#[path]`, `include!`, a target root outside its crate's directory, or a file the scan's lexer cannot follow to its end (it fails closed): code the source scan would not read | architecture test (source scan, graph) | FND-07 |
| `ARCH-ci-lockstep` | a stage in no CI job or two; `required` missing a job; a swallowed failure; `on:` other than push to main, pull requests and dispatch | architecture test | FND-05 |
| `ARCH-gate-config` | a gate recipe, xtask composition source (`tools/xtask/Cargo.toml`, every file of `tools/xtask/src`; XT-01), script (every `scripts/test-*.sh` and `scripts/xtask.sh` included), stage policy (`.github/zizmor.yml`, `deny.toml`) or tool pin (`mise.toml`, `mise.lock`) that differs from its pin; `.config/nextest.toml`, `mise.toml`, `mise.lock` or `rust-toolchain.toml` outside its shape; a cargo, mise, toolchain or rustfmt configuration file elsewhere in the tree; a dot-entry or `mise*` entry of the root, or an entry of `.github/` or `.config/`, outside its allowlist | architecture test (and xtask's `tool-config` before any tool runs) | FND-08, FND-09, XT-01 |
| `ARCH-workflow-yaml` | a YAML file under `.github/` that does not parse, holds two documents or an anchor, alias, tag or merge key; a decoded `run` value with a shellcheck directive or zizmor ignore outside `allowed_suppressions` of `scripts/workflows-check.sh` (or one twice, or `disable=all`); a `shell` other than bash or sh | architecture test (`yaml-rust2`, the decoded authority over the `workflows` stage's line checks) | FND-09 |
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
   `testkit-in-production-closure`, `oracle-imports-domain`, `unlisted-external`, and
   `pure-uses-std` (a `#![no_std]` crate naming `std::fs`, which a test builds and expects to
   fail; a `compile_fail` doctest cannot show this, since rustdoc links each doctest as a std
   binary).
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
is the backstop. **The gate is not a sandbox**: `just check` (and `land.sh`, which runs it on the
merged tree) compiles and runs the tree's build scripts, proc-macros and tests with the user's
own privileges, so that code can change the tools, caches and scripts a later gate uses (mise's
install directories, `~/.cargo`, `~/.local/bin`). The checks below keep configuration files from
steering the gate silently; for code, review before landing is the control (the FND-09
review's G2). xtask's preflight (`tools/xtask/src/preflight.rs`; the justfile's `_preflight`
recipe until XT-01 ported it), which every gate command of xtask runs first, refuses an
environment that steers the build, by allowlist: every `CARGO*`, `RUSTC*`, `RUSTDOC*`,
`RUSTUP_*`, `CLIPPY_*` and `NEXTEST_*` variable is refused except `CARGO_TARGET_DIR`,
`CARGO_BUILD_JOBS`, `CARGO_INCREMENTAL`, `CARGO_HOME`, `CARGO_TERM_*`, `CARGO_NET_*`,
`CARGO_HTTP_*`, `CARGO_REGISTRIES_CRATES_IO_PROTOCOL`, `RUSTUP_HOME` and rustup's download
settings, `RUSTUP_TOOLCHAIN` naming the pinned toolchain (bare or with one of ADR 0006's host
triples), `RUSTFLAGS` empty or `-D warnings`, and `NEXTEST_PROFILE` naming a profile of
`.config/nextest.toml`; `__CARGO_*` and `PROPTEST_*` are refused too (FND-07); the variables
`cargo run -p xtask` sets for the program it starts (`CARGO`, `CARGO_MANIFEST_*`, `CARGO_PKG_*`,
rustup's `RUSTUP_TOOLCHAIN_SOURCE`) pass only at exactly the values of xtask's own build and are
taken out of every stage's environment (XT-01). Cargo
configuration files are allowlisted by place and shape (FND-08): inside the repository none may
exist (tracked or untracked, at any depth, or in an ancestor of the working directory up to the
root), since one can set a runner, an alias or flags before any check runs (the FND-05
checkpoint's N1 spelled `target` and `runner` with TOML unicode escapes past a line grep); outside
it (an ancestor of the working directory, `CARGO_HOME`) a file passes only when every line is a
comment, a blank, a header of `[net]`, `[http]`, `[term]`, `[build]` or `[registries.crates-io]`,
or a bare key that table allows with a plain value (`plain_config` in the preflight), so any other
TOML form fails closed. mise is held the same way: `mise.toml` is only `[tools]`, each tool
`just` or an `aqua:` or `github:` backend (a plugin backend's plugin is code) pinned by a plain
version string, `mise.lock` holds only checksummed GitHub release assets, no other mise
configuration file inside the repository may load (`mise config ls`, failing closed when mise
cannot read its configuration), and the stages run the pinned tools from their install
directories (xtask's tool cache first, then `mise bin-paths`; XT-02, §8) rather than through shims, so no mise `[env]` reaches a tool
after `_preflight` has looked. Since mise runs code from a trusted configuration as it loads it
(`[env] _.source`, `{{exec(...)}}` templates), nothing calls mise on a tree before xtask's
`tool-config` (`tools/xtask/src/tool_config.rs`, no mise; `scripts/tool-config-check.sh` in bash
and awk until XT-01 ported it) has passed its tool configuration (FND-09, the FND-08
re-verification's F3): the justfile's top-level lines run it (`bash scripts/xtask.sh tool-config`)
before `mise bin-paths`, the preflight runs it again, `scripts/land.sh` runs the landing
checkout's own xtask on each cherry-picked tree before anything there runs, the `cargo update` that resolves a
Cargo.lock conflict included, and no longer trusts the tree for mise (the check also refuses a
cargo configuration file at any depth and a toolchain file beyond the root
`rust-toolchain.toml`'s shape, which any cargo command would obey: a `rustc-wrapper`, a
toolchain `path`; the FND-09 review's G2),
`scripts/bootstrap-tools.sh` runs it before `mise install` and no longer runs `mise trust`
(it removes the trust `mise install` records by itself), and the Stop hook runs it
(`bash scripts/xtask.sh tool-config`) before `just`, which may be mise's shim, blocking the stop
when the check refuses or cannot run. xtask itself is built by `scripts/xtask.sh` and then run with
the caller's environment, and that build is the one step before any preflight, so nothing may
steer it: it refuses, before cargo runs, any cargo configuration file in an ancestor of the
directory it builds from (`/` included) and a CARGO_HOME configuration outside the plain shape
above; it builds with a CARGO_HOME of its own (exactly two links, to the registry and git caches,
and nothing else: a directory there is refused, since cargo would compile its sources unverified;
the XT-01 review's Y1), a cleared environment plus an allowlist, RUSTFLAGS `-D warnings`, the
pinned toolchain's own `cargo`, `rustc` and `rustdoc` from
`${RUSTUP_HOME:-~/.rustup}/toolchains/<channel>-<host>/bin` (no rustup, and nothing of the
caller's PATH: the launcher runs with `/usr/bin:/bin`, the build with the toolchain's directory
before it; XT-02); and it never starts xtask through `cargo run`, whose configured `runner` would
replace it. A steered build
could link a binary that exits 0, and the gate would pass having run nothing (the XT-01 review's
X1). It builds into one directory per checkout (keyed by the SHA-256 of its path), since cargo
relinks no fresh binary and two checkouts sharing `CARGO_TARGET_DIR` (land.sh and its worktree)
would otherwise run each other's xtask. Two launchers of one checkout serialise on a lock in that
directory, across the build and a copy of the binary to a path of that run alone, which is what
runs (one launcher's cargo replaced `debug/xtask` while another started it: "No such file or
directory" after a green build, XT-02). `scripts/land.sh`'s tree check builds into
`$XDG_CACHE_HOME/jfastlsp/land-xtask` (else `~/.cache/...`), a target directory no gate is given
(land.sh refuses one as CARGO_TARGET_DIR), so no binary that gated code left in a shared target
directory, which cargo would keep as fresh, judges the next tree (the XT-01 review's LOW). The
tree's own xtask, `build.rs` included, is built from
source before its `tool-config` runs: like every build script, proc-macro and test the gate
compiles, that is code, and review before landing is its control (XT-01). An untrusted `mise.toml` with only `[tools]` loads; one with
an `[env]` makes mise fail without running it. The same check holds the implicit configuration
of the other tools of the gate to allowlists: the root's dot-entries and `mise*` entries, and
the entries of `.github/` and `.config/` (so `.shellcheckrc`, `.yamllint`, `.tool-versions`,
`.pinact.yaml`, `.envrc`, `.github/actionlint.yaml` and `.config/mise/` fail, whatever a tool
reads next); the linters also run with explicit configuration (shellcheck `--norc`, actionlint
`-config-file /dev/null`, zizmor `--config .github/zizmor.yml`, pinned), and no `rustfmt.toml`
may exist. The architecture test repeats these checks
with a real TOML parser (`ARCH-gate-config`) and pins the gate itself: the recipes of `check`,
`quick`, `check-only`, `ci-parity`, `platform-tests` and `_preflight`, every stage of xtask's
catalog and their dependencies, the justfile's top-level lines, the xtask composition (its
manifest and every file of `tools/xtask/src`, with the launcher `scripts/xtask.sh`; XT-01), the scripts the stages run (every `scripts/test-*.sh`, which
`script-tests` runs by glob, included), the policies a stage reads by path
(`.github/zizmor.yml`, `deny.toml`) and the tool pins (`mise.toml`, whose new tool would land on
the stages' PATH, `mise.lock` and `tools/versions.toml`, XT-02) are hashed in `.config/gate.sha256` (FND-09 added all but the
recipes and scripts),
and `.config/nextest.toml` keeps its allowlisted keys (no `default-filter`, no `[scripts]`,
`retries = 0`). A pin moves only with its line in that file, in the same commit, which review
reads as a change of the gate (the first FND-05 review found `check: (check-only quick_stages)`
would have kept every check green). Not pinned, each for its reason: `architecture.toml` and the
`clippy.toml` files change with every new crate and are held to their schema by the
architecture test itself (`ARCH-policy-schema`, `ARCH-clippy-lists`; widening one needs an ADR,
§3.5); the checker's own sources are code under its meta-test (`tests/rules.rs`, a negative case
per rule; `tests/gate_config.rs` fails on a crude edit of `gate.rs`), so a neuter must edit both
in plain sight; `.gitleaks.toml` is compared byte for byte with the pinned generator's output. Independently, every cargo of the gate runs with `RUSTFLAGS="-D
warnings"`, empty `RUSTDOCFLAGS`, `RUSTC_WRAPPER` and `RUSTC_WORKSPACE_WRAPPER` (which beat any
config file), and nextest with `--retries 0 --no-tests fail`. Never export a steering variable for
the gate. Each refusal has a negative case in `tools/jfl-architecture/tests/gate_negative_cases.rs`
(through the justfile) or `tools/xtask/tests/` (`preflight.rs`, `tool_config.rs`, `launcher.rs`,
`stages.rs`).
A later recipe that needs a `PROPTEST_*` setting passes it to the test process inside the recipe,
after `_preflight`, never through the caller's environment.

---

## 4. Language and lint policy

### 4.1 Baseline

Stable toolchain pinned exactly in `rust-toolchain.toml` (ADR 0035, amended 2026-11-17, carrying
ADR 0011's pin forward: 1.99.1, edition 2024, resolver 3, `rust-version = "1.99"`, `rustfmt`,
`clippy`, `llvm-tools-preview`). `Cargo.lock` committed; every cargo command in the gate runs
`--locked`. `publish = false` everywhere. A dated nightly is pinned in `mise.toml` for
`cargo-fuzz` only and never builds a shipped artefact.

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
stating its single responsibility and its role; a pure crate's opens with `#![no_std]` and puts
the `//!` right after it (`ARCH-domain-purity` checks the first line, §2.3).

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
| `security.yml` | weekly, dispatch | advisories, licences, secret scan over full history, zizmor online audits | FND-04, FND-05 |
| `codeql.yml` | push to `main`, pull request, weekly | CodeQL for Rust and Actions | FND-05 |
| `scorecard.yml` | push to `main`, weekly | OpenSSF Scorecard, SARIF upload | FND-05 |
| `bench.yml` | push to `main`, dispatch | the corpus benchmarks on one fixed runner class; history in `gh-pages` per runner class; the `bench-gate` job (§9) fails a >10 % regression from M1 on | BENCH-04 |
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
   tests (ADR 0006): on x86-64 the `tests` job's stages, on the other two `just platform-tests`
   (the unit, integration and doctest stages), which is not a stage of `check`, so each stage
   still runs in one job.
5. Scheduled workflows are outside `required`; their failures are reported and fixed as tasks.
6. A red job is fixed, never re-run to green. One re-run is allowed only when the runner died
   before a test body ran, and the reason is written down.

### 7.3 Workflow hygiene

- `permissions: contents: read` at the top; a job adds only the scope one step needs, with the
  reason beside it.
- Every `uses:` is pinned to a 40-hex commit SHA with the version in a comment, or is a local
  action; container images by `@sha256:` digest. Checked by zizmor (`unpinned-uses` with a
  hash-pin policy, template injection, excessive permissions), pinact and actionlint with
  shellcheck. The `workflows` stage (FND-04, `scripts/workflows-check.sh`) is offline and
  deterministic: actionlint with `-config-file /dev/null`, zizmor `--offline` with the pinned
  `.github/zizmor.yml`, `pinact run --fix=false --no-api` (every `uses:` is a SHA), and
  shellcheck `--norc` (no tool reads a configuration file the command line does not name;
  FND-09, §3.6) over `scripts/*.sh` and
  `.claude/hooks/*.sh` (`spikes/e1-parse/*.sh` is excluded: archived evidence for plan §1.7, kept
  as it ran and outside the workspace); a tree with no workflow file passes with a notice.
  Inline suppressions are held like `#[allow]` (FND-09, its review's G3): every shellcheck
  directive comment in a linted shell file or workflow and every `zizmor: ignore` comment must
  be listed, by file and exact line with its reason, in `allowed_suppressions` of
  `scripts/workflows-check.sh` (pinned), and a `disable=all` never is. Every `run:` is a block
  scalar (`|`, `>`) or a plain one-line scalar, never a quoted string or a flow form: actionlint
  decodes a double-quoted string (`\x23` is `#`) before shellcheck reads it, past the raw-text
  scan (FND-09 review, I1). And the YAML of every file under `.github/` the stage reads holds a
  shape where nothing is decoded (J1): outside block scalars no backslash on a line with a double
  quote (no escape sequence), no `?` complex key, no quoted mapping key, no tag, anchor, alias,
  merge key or directive; so `"\x72un":` or `? run` cannot become a `run:` key unseen. These are
  line checks of a subset, a first layer; **the authority is the decoded check**
  `ARCH-workflow-yaml` (FND-09, §3.3): `yaml-rust2` parses every YAML file under `.github/`,
  refuses more than one document and any anchor, alias, tag or merge key, walks every mapping,
  applies the same `allowed_suppressions` (one list, read from the pinned script) to every
  decoded `run` value, and allows `shell:` only bash or sh. The xtask port keeps the decoded
  check and may drop the line checks. The comment check,
  `pinact run --check --verify-comment`, asks the GitHub API, so it is the recipe
  `workflows-verify`, outside `check` (like `bench-gate`): `ci.yml`'s security job runs it with a
  token (FND-05). The sandbox refuses the API, which is why it cannot be a stage of `check`.
- `actions/checkout` with `persist-credentials: false`; no `pull_request_target`; inputs reach
  scripts through `env`, never interpolated into `run:`.
- Concurrency per ref, and per commit on `main`: a newer push to `main` neither cancels nor
  replaces a pending run; cancel-in-progress only for pull requests.
- Caches are keyed by toolchain, `Cargo.lock` and `mise.lock`, and saved only from `main`.

### 7.4 Supply chain

- `cargo-deny`: advisories (yanked denied), licences (permissive allowlist; each exception per
  crate with a recorded reason, in the commit message or an ADR when the context is worth keeping; `notify` is CC0-1.0 and tree-sitter grammars MIT, to confirm at the task
  that adds them), bans (no wildcards), sources (crates.io only); `[graph] targets` set to the
  three targets of ADR 0006.
- `gitleaks` (the binary, pinned by mise) over the full history (`secrets` stage,
  `scripts/secrets-check.sh`: redacted, and a shallow clone fails rather than scanning part of
  the history); `.gitleaks.toml` is gitleaks' own default configuration without its global
  `[allowlist]` (FND-08: that table skips images, PDFs, fonts, `node_modules/`, `vendor/`,
  `go.sum`, `package-lock.json` and other lockfiles, and `useDefault = true` inherited it),
  generated from the pinned binary by `scripts/gitleaks-config.sh`; the stage refuses a policy
  that is not byte for byte the generated one, a `.gitleaksignore` in the history, and runs
  gitleaks with `--ignore-gitleaks-allow`, so no inline comment, ignore file, disabled rule or
  allowlist can wave a finding through. **The policy holds no secret-shaped literal** (FND-09,
  after GitHub's secret scanning flagged the public test keys of the default `gcp-api-key`
  rule's allowlist): the generator scans the policy with its own rules and none of their
  allowlists, and rewrites every text they match, which must be a regex, into the same regex
  with one character in a class (`...tJ[F]px...`); the policy then scans clean with itself
  (`scripts/test-secrets.sh` checks it, and that a key appended at run time is found) and has no
  allowlist of its own at all, so a token committed in `.gitleaks.toml` and removed in the next
  commit (the FND-08 re-verification's F2) is found like any other, and no `.gitleaks.toml` may
  exist anywhere but the root (`ARCH-gate-config`). Each rule keeps its own value allowlist (template
  placeholders and stopwords, which never match a real token). The stage runs `gitleaks dir` over
  every blob reachable from a branch, a tag, a remote-tracking ref or HEAD (what is or can be
  pushed) and over every commit and annotated tag message (pushed and public), and names the
  commits and paths of a blob with a finding; not `gitleaks git`, since `git log -p` shows
  nothing of a file git takes for binary (a NUL byte, a `-diff` or `binary` attribute) or of a
  merge resolution. gitleaks itself sniffs each file's type and
  silently skips what it takes for binary (an `MZ`, `%PDF-`, ELF or zip header before plain
  text; the FND-09 review's G1), so each blob is written as text (a leading newline, every byte
  outside tab, newline and printable ASCII turned into a newline; a token is printable ASCII),
  the pass fails on any file gitleaks reports skipping, and a blob in one of the common archive
  and compression formats (zip, gzip, xz, bzip2, zstd, 7z, rar, tar, by magic number), whose
  content no pass would read, fails the stage: the repository commits none. Commit objects are
  scanned whole (headers, author, committer, signature and message), and so are annotated tag
  objects. The only blobs not scanned are listed by exact object id in
  `scripts/secrets-check.sh` with their reason: history on `main` that cannot change (FND-08's
  policy, which still carried gitleaks' public test keys). Out of scope by design: a token split by
  non-printable bytes, or encoded or compressed in any other way (zlib or raw deflate, lz4,
  brotli, PDF streams, base64 beyond gitleaks' own decoding, UTF-16, encryption), is not
  decoded or reassembled; and gitleaks' rules stand
  in for GitHub's own patterns (a key only GitHub recognises is not refused here). Every
  `.gitattributes` in the history holds only allowlisted attributes (`text`, `text=auto`,
  `eol=lf`, `eol=crlf`, `linguist-*`), so no `binary`, `-diff`, `-text`, `diff=` or `filter=`
  changes what git shows of a file. **No token-shaped text is committed whole**, in a fixture,
  a test or the policy: a test that needs a fake secret assembles it at run time from pieces
  (`"ghp_""aB3d..."`), so the committed text never forms the token. GitHub secret
  scanning and push protection are on (owner setting).
- **Private names** (ADR 0004, amended 2026-11-17): no private project is named or alluded to
  anywhere in the repository, and the names are not written down in any form. Review enforces
  it: the orchestrator's review of every commit before landing and the `/review` skill's
  "Public repository" check, which read code, docs, fixtures and commit messages. FND-04's hashed
  check (a salted SHA-256 list and a stage over the diff) was removed by FND-11 on the owner's
  order: a salted hash of a short name is reversed by guessing in seconds, so the list confirmed
  what it meant to hide.
- **DCO:** `scripts/dco-check.sh` (`just dco [range]`, FND-03) checks that every commit of a range
  carries a `Signed-off-by` matching its author, read as a trailer (a quoted line does not count);
  a step of `ci.yml` runs it over the pushed commits, and `land.sh` over the ones it lands.
- CodeQL, Scorecard, Dependabot as in 7.1.
- Releases (REL-06): dist builds the archives with GitHub artifact attestations and
  `cargo-auditable`; `SHA256SUMS`, a CycloneDX SBOM (`cargo-cyclonedx` if dist's support is not
  confirmed) and third-party notices; release-plz prepares versions and the changelog; the owner
  publishes the draft.

---

## 8. Tooling pins

0. **Bootstrap.** mise and just themselves come from `scripts/bootstrap-tools.sh` (BOOT-02):
   each at a pinned version, verified against a SHA-256 recorded in the script, installed into
   `~/.local/bin`, a no-op when already present; then `mise install --locked` once `mise.toml`
   exists and xtask's `tool-config` (`scripts/xtask.sh`, which needs cargo and the pinned Rust
   toolchain) has passed the tool configuration, never trusting
   the repository for mise (FND-09, §3.6). The SessionStart hook runs it in cloud sessions; a cloud environment's setup script may
   call it too (`bash scripts/bootstrap-tools.sh`). Verified in the cloud sandbox on 2026-11-16:
   GitHub release downloads (`github.com/<owner>/<repo>/releases/download/…`) and the static
   crates.io files pass the network proxy, while `api.github.com`, GitHub pages and
   `codeload.github.com` are refused. mise's aqua backend installed `just` and `hyperfine` there
   from a `mise.lock` that records each asset URL, but an install without a locked URL (for
   example `mise exec aqua:koalaman/shellcheck@0.11.0`) failed on the GitHub API: in the sandbox
   every tool is installed with `--locked`. `mise lock` itself worked in the sandbox for `just` and
   `hyperfine`, and on 2026-11-17 for gitleaks, actionlint, zizmor, pinact, shellcheck and ripgrep
   (each lock entry has a checksum); a tool whose lock entry needs the API is locked where the API is reachable (CI,
   or the owner's machine) and the reason recorded. A version bump of mise or just changes the script's version and every checksum
   in one commit, and `mise.toml` pins just at the same version.
1. **mise** pins every tool other than Rust (ADR 0030): `mise.toml` names each tool and version,
   `mise.lock` records its checksums, and `mise install --locked` refuses a mismatch. `mise lock`
   records a checksum only where the backend's registry has one (on 2026-11-16 it did for `just`
   and not for `hyperfine`): every lock entry has a `checksum`, or the task that adds the tool
   records why not and what verifies it instead. Backends: prefer prebuilt, checksummed binaries
   (`aqua:`, then `github:`) over `cargo:`, which compiles from source in every fresh session and
   uncached CI job; `aqua:` for gitleaks, actionlint, shellcheck, zizmor, pinact, jq, hyperfine,
   ripgrep and, where the registry has them, cargo-nextest, cargo-deny, cargo-llvm-cov and
   cargo-mutants; `cargo:` only for a tool with no prebuilt release (release-plz if so,
   cargo-fuzz); `github:` for adrs, which has no aqua entry but ships dist release archives
   (`mise.lock` records each archive's URL and the SHA-256 of the release's `sha256.sum`; mise
   installs it with `--locked` in the sandbox, where the GitHub API is refused); `pipx:` for diff-cover; `npm:@informalsystems/quint` (with Node); `java` for a
   Temurin JDK; `github:`/`ubi:` for Apalache. Entries arrive with the stage that needs them,
   each with a comment naming it: just and cargo-nextest (FND-01), cargo-deny (FND-02), adrs
   (FND-03), gitleaks, actionlint, shellcheck, zizmor, pinact (FND-04), cargo-llvm-cov,
   diff-cover, cargo-mutants (FND-06), hyperfine and ripgrep (BENCH-02; `rg` is the floor arm, so
   its version is part of the measurement), Node, Quint, Apalache, the JDK (FORMAL-01; the JDK
   also serves the Java fixtures and the JVM helpers), cargo-fuzz and its nightly (REL-03), dist
   and release-plz (REL-06).
2. Versions seen on 2026-11-16 (each task re-checks the current release; a version is never
   invented): mise 2026.11.9 (the bootstrap pins 2026.10.10, a month old), just 1.59.0, cargo-nextest 0.9.152, cargo-deny 0.21.0,
   cargo-llvm-cov 0.9.1, diff-cover 11.0.2, cargo-mutants 27.1.0, gitleaks 8.30.1, actionlint
   1.7.12, zizmor 1.32.0, pinact 5.0.2, adrs 0.13.0, Quint 0.33.1, Apalache 0.62.1 (Quint 0.33.x
   is validated against it, not 0.63.0), cargo-fuzz 0.13.2, dist 0.34.0, release-plz 0.4.3.
3. **`tools/versions.toml` and xtask** (XT-02, ADR 0035): every tool `mise.toml` pins, with its
   version, the `mise.toml` key it stands for, the command that prints its version, and per
   platform of ADR 0006 the release asset's URL, its SHA-256 and the program's path in the
   archive (mise.lock's URL and checksum). The file has a pinned line shape (`tools/xtask/src/
   versions.rs`): a URL is a GitHub release asset (`https://github.com/<owner>/<repo>/releases/
   download/<tag>/<asset>`), never `api.github.com`, so every download xtask makes passes the
   sandbox's proxy. `just install-tools [TOOL...]` (`bash scripts/xtask.sh install-tools`)
   downloads each asset with `curl --proto =https` (from `/usr/bin:/bin`, as `tar`), checks its
   SHA-256 in process and refuses a mismatch, installing nothing, then moves the one program into
   `$XDG_CACHE_HOME/jfastlsp/tools/<tool>/<version>/<platform>/bin` (else `~/.cache/...`; a cache
   inside the repository is refused); `--archives DIR` takes the assets from a directory instead.
   The stages' PATH puts those directories first, then mise's install directories (`mise
   bin-paths`), mise's shims, the inherited PATH. `just doctor` (`xtask doctor`) fails when
   versions.toml disagrees with mise.toml or mise.lock (a version, tool, platform, URL or digest),
   then resolves every pinned program on the stages' PATH and runs its probe, naming each missing
   or wrong tool with where it was found and the fix. Verified in the cloud sandbox on
   2026-11-17: `install-tools` installed all eleven tools for linux-x64 in about 9 s, and the 33
   assets of the three platforms matched their pins. mise stays installed and in CI until XT-08;
   a bump changes `mise.toml`, `mise.lock` and `tools/versions.toml` in one commit, which
   `doctor` and `tools/xtask/tests/versions.rs` (a real TOML parser) hold together.
4. **Fail closed.** A recipe whose tool is missing fails with the install hint (`install-tools`,
   or `mise install --locked`). A stage is
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
| `bash scripts/bootstrap-tools.sh` | install the pinned mise and just (checksum-verified), then `mise install --locked` | BOOT-02 |
| `mise install --locked` | install every pinned tool from `mise.lock` | FND-01 |
| `just quick` | fmt-check, architecture (from FND-07), clippy, unit tests, script tests: the inner loop | FND-02 |
| `just check` | the full blocking gate: every stage, in order, stopping at none silently | FND-02 |
| `just check-only <stage>…` | named stages through the same stage code (what each CI job runs) | FND-02 |
| `bash scripts/xtask.sh <check\|quick\|check-only <stage>…\|list [--json]\|preflight\|tool-config [ROOT]>` | the gate's composition, which the recipes above delegate to: built from outside the tree with an allowlisted environment, then run; `list --json` is the stage catalog. `cargo run --locked -q -p xtask -- <args>` runs the same binary but builds it under the working directory's cargo configuration; the `cargo xtask` alias is deferred (it needs FND-09's G2 rule to admit one pinned cargo configuration file) | XT-01 |
| `just clippy-lists` | rewrite every crate's `clippy.toml` from the root list and `[grants]` (§2.4); the `clippy` stage fails when one differs | FND-02 |
| `just doctor` | `mise doctor`, `mise ls --missing`, `just --version` against the pin, and the five security tools of FND-04 on PATH | FND-01, FND-04 |
| `adrs new "<title>"`, `just adr-index` (the `docs` stage) | create an ADR from `doc/adr/template.md` (`adrs.toml`); check the index against the files (`scripts/adr-index.sh`) | FND-03 |
| `just dco [range]` | every commit of the range is signed off by its author (`scripts/dco-check.sh`) | FND-03 |
| `just dco-event <event> <before> <base> <head>` | the same over the commits a GitHub event brought: a push's before..head, a pull request's base..head, else head~1..head (`ci.yml`'s `dco` job) | FND-05 |
| `scripts/land.sh <sha>… [--no-push]` | cherry-pick onto a fresh `origin/main` worktree, check sign-offs and signatures, `just check`, push only on green | FND-03 |
| `just coverage`, `just changed-lines` | llvm-cov vs `coverage-floor.toml`; diff-cover on the lines changed since `git merge-base HEAD origin/main` (plus the working tree) | FND-06 |
| `just mutants-diff [base]` | cargo-mutants `--in-diff` over the same diff, with a time cap; outside `check` (below) | FND-06 |
| `just secrets`, `just workflows` | gitleaks over the history; actionlint, zizmor, pinact (offline) and shellcheck | FND-04 |
| `just workflows-verify` | pinact's version-comment check; needs the GitHub API, so outside `check` (§7.3) | FND-04 |
| `just workflows-online` | zizmor with its online audits over the workflows and `dependabot.yml`; needs the GitHub API (`GH_TOKEN`), so outside `check`; `security.yml` runs it | FND-05 |
| `just ci-parity` | the cargo stages with CI's environment: the target directory inside the checkout and `ci.yml`'s workflow `env:` (`CARGO_TERM_COLOR=always`, `NEXTEST_PROFILE=ci`); outside `check` (a second build), run before landing a change to tests that start cargo, nextest or a scratch workspace | FND-05 |
| `just platform-tests` | the unit, integration and doctest stages; what `ci.yml`'s `linux-arm` and `macos` jobs run natively (§7.2 rule 4); not a stage of `check` | FND-05 |
| `just bench-corpus` | fetch the pinned corpus of `doc/bench-corpus.md` into the cache directory | BENCH-01 |
| `just bench [suite] [corpus=small]` | the benchmark suites with machine facts, JSON results; reports, never gates | BENCH-02 |
| `just bench-gate` | compare a run with the history of the same runner class; `bench.yml` only | BENCH-04 |
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

Stages of `check` in order (each a stage of xtask's catalog and a recipe that runs it):
`fmt-check`, `architecture`, `clippy`, `nostd`
(§2.3), `docs` (the ADR index matches the files), `unit-tests`, `integration-tests`, `doctests`,
`script-tests` (the offline self-tests `scripts/test-*.sh`), `e2e-tests`, `lsp-conformance`, `formal-typecheck`, `formal-test`, `formal-freshness`,
`conformance-replay`, `java-fixtures`, `coverage`, `changed-lines`, `deny`, `secrets`,
`workflows`. A stage enters `check` in the commit of the task that introduces
it, together with its `ci.yml` job entry.

Two recipes are deliberately **outside** `check`:

- `mutants-diff` reruns the tests once per mutant, which grows with the suite. It runs in the CI
  `coverage` job, and the orchestrator runs it before landing a task sized `L` (or one touching
  `jfl-domain`, `jfl-protocol` or `jfl-application`), with a time cap of 20 minutes; a run cut by
  the cap reports which mutants it did not reach and never counts as green.
- `bench-gate` compares wall times, which are only comparable on one machine class. It runs only
  in `bench.yml` on a fixed runner class against the history recorded by that class, on the
  median of N runs written by `jfl-bench`, and a regression must reproduce once in a second run
  before it fails (ver §10). In-process suites gate on instruction counts (a deterministic metric
  from `gungraun` or cachegrind) where available and report wall time. `just bench` on a laptop
  or in the sandbox reports and never gates, and neither `check` nor `land.sh` runs it.

The diff base of `changed-lines` and `mutants-diff` is
`git merge-base HEAD origin/main` plus the working tree: the same set of lines for a lane's
uncommitted tree, its commit, and `land.sh`'s cherry-picked tree.

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
cherry-picks, runs `just check` on the merged tree, and
pushes only on green, retrying once after a fetch on a non-fast-forward. Conflicts are auto-resolved only in `Cargo.lock` (take integration's
side, then `cargo update -w`) and in the ADR index (both sides' rows kept and sorted, then
`just adr-index` in the gate); anything
else aborts and names the files. One commit lands per green CI run on `main` once `ci.yml`
exists (FND-05); before that the local gate on the merged tree is the only gate, and the next
commit may land as soon as the previous one is pushed. A red `main` is fixed
immediately, by a new commit. Before FND-03 the orchestrator did the same by hand
([`sdlc.md`](sdlc.md)).

### 10.3 ADRs

Nygard format in `doc/adr/NNNN-kebab-title.md`: `# NNNN. Title`, `Date:`, `## Status`
(`Proposed`, `Accepted (owner, YYYY-MM-DD)`, `Rejected (owner, YYYY-MM-DD)`,
`Superseded by NNNN`, with annotations such as `amended by NNNN`), `## Context`, `## Decision`,
`## Consequences`, optional `## Alternatives considered`. Records are created with `adrs new`
(ADR 0030; `cargo xtask adr new` from XT-04, ADR 0035); the index `doc/adr/README.md` is checked against the files by the `docs` stage
(`just adr-index`, FND-03). ADR 0001 records the practice; the ADR number is
the decision id. Only the owner moves a decision to Accepted or decides a `DEC-` task. An ADR is
written only for an important decision whose context is worth recalling later; everything else is
recorded in the commit message and code comments (ADR 0001, amended 2026-11-17).

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
3. Updates policy first when a new dependency, edge or authority is needed, with a recorded reason
   (commit message or ADR when the context is worth keeping).
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
