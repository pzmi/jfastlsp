# jfastlsp: verification strategy

**Status:** plan, 2026-11-16. Owner: Patryk Żmigrodzki.

This document owns *how jfastlsp is shown to be right*: the test levels, determinism, golden
fixtures, contract suites, the Quint models and their conformance replay, the oracles, fuzzing,
mutation, benchmarks, the agent evaluation, the lanes that run each of them, and what counts as
evidence. [`plan.md`](plan.md) owns behaviour, [`foundation.md`](foundation.md) owns the rules
that enforce structure, and [`tasks.md`](tasks.md) owns order. A citation "ver §6" points here.

---

## 1. Principles

1. **Independent oracles.** Something that checks production must not share production's
   assumptions. The Quint models are written before the code they govern (ADR 0007); `jfl-oracle`
   and `jfl-eval` import no production crate (fnd §2.7); SCIP comes from a different indexer;
   golden fixtures are files, not round trips.
2. **Fail closed.** A missing tool, an empty selection, a trace that does not decode, a fixture
   that does not compile, or a harness that crashed is never a pass. Every check has a negative
   control showing it can fail.
3. **Four verdicts** for evidence-producing runs (conformance generation, Apalache, oracle runs,
   evaluations): `PASS`, `FAIL`, `INCONCLUSIVE` (the run did not reach what it set out to test)
   or `HARNESS_ERROR`. Only `PASS` is success.
4. **Lowest level that can fail.** A test lives at the lowest level that can fail for the reason
   it exists (§2). A rule about position conversion is a unit test, not an LSP transcript.
5. **Races are found, never retried.** nextest runs with zero retries. A failure that passes on a
   rerun is a race: repeat it, read its trace, name the race, then add a test that forces the
   interleaving every time (barriers, scripted fakes, fake clock).
6. **`complete=true` is the property agents rely on most.** It has its own oracle (§7.2), its own
   mutation suite (§7.3) and its own model invariant (plan §7.1).
7. **Performance claims need a benchmark** (§10). No budget is set or loosened from a guess.

---

## 2. Test levels

| Level | Location | May use | Gate stage | A test lives here when |
|:--|:--|:--|:--|:--|
| **Unit** | `#[cfg(test)] mod tests` beside the code | the crate under test | `unit-tests` | it decides a pure rule: position conversion, key derivation, envelope ordering, completeness computation, a transition table |
| **Property** | `proptest!` in `src/` or `tests/properties.rs` | generated inputs, committed regression seeds | `unit-tests` | a law holds for all inputs: encoding round trips, generations increase, `possible` ⊇ every name-and-arity match, framing never allocates past the limit |
| **Golden** | `fixtures/`, `crates/<c>/tests/fixtures/` (§4) | files, the codec | `unit-tests`, `integration-tests` | a byte-level shape is a promise: protocol JSON, `--json` output, LSP transcripts, the Java corpus's summaries |
| **Component** | `crates/jfl-application/tests/` | real application code, testkit fakes, fake clock, paused Tokio time | `integration-tests` | a use case behaves across several steps with no OS resource: pinned generations, reconcile, diagnostics coordinator |
| **Contract** | suites in `jfl-testkit::contracts`, run from each implementation's crate (§5) | the port, a factory | `integration-tests` | a fake and a real adapter of one port must agree |
| **Integration** | `crates/jfl-adapter-*/tests/`, `crates/jfl-composition/tests/` | real files and Git repositories in tempdirs, real Unix sockets, real children through `jfl-process`, the pinned JDK | `integration-tests` | the reason is an OS or library fact: `flock`, socket modes, peer credentials, `git` behaviour, `javac` output |
| **E2E** | `crates/jfl-cli/tests/e2e_*.rs` | the built `jfl`, fixture projects copied to tempdirs, `assert_cmd` | `e2e-tests` | the promise is a process boundary: CLI output, exit codes, spawn on demand, restart |
| **LSP conformance** | `crates/jfl-cli/tests/lsp_*.rs` + `fixtures/lsp/*.jsonl` | the built `jfl lsp` over stdio, scripted transcripts | `lsp-conformance` | the promise is LSP 3.17 behaviour: framing, lifecycle, encodings, cancellation, errors, every advertised method |
| **Architecture** | `tools/xtask` + fixtures (fnd §3) | `cargo metadata`, `syn` | `architecture` | the rule is structural: edges, purity, authority, naming |
| **Conformance** | `formal/` + `crates/jfl-conformance` (§6) | Quint (pinned), ITF traces, real code behind drivers | `formal-*`, `conformance-replay` | a model step must equal what production does |
| **Formal verify** | `cargo xtask formal-verify` (§6.3) | Apalache (pinned) | weekly, by hand | an invariant must hold for every behaviour to a bound |
| **Differential and oracles** | `tools/jfl-oracle` (§7) | the built `jfl`, SCIP indexes, the golden corpus | `integration-tests` (corpus), owner-run (target repository) | an answer must agree with an independent indexer, or completeness must survive an inserted caller |
| **Fuzz** | `fuzz/` (§8) | `cargo-fuzz` on the pinned nightly | nightly | a parser must survive hostile input |
| **Mutation** | `cargo xtask mutation` (§9) | `cargo-mutants` | weekly | the question is whether the tests can fail |
| **Benchmark** | `crates/*/benches/`, `cargo xtask bench` (§10) | Criterion, recorded machine facts | by hand, release | a latency, memory or throughput claim |
| **Agent evaluation** | `tools/jfl-eval` (§11) | Claude Code transcripts, the target repository | owner-run per milestone | the claim is about agents finishing tasks |

Rules across levels:

1. **Naming and shape.** `should_<outcome>_when_<condition>` with `// given`, `// when`,
   `// then` in order (fnd §3.3).
2. **No mocks.** Real objects first, testkit fakes second; no mocking framework, no patching of
   production functions. A fake is never looser than production (§5).
3. **Isolation.** Tempdirs per test; a fresh state directory and socket per test; fixture
   projects copied, never used in place; no fixed paths, no network.
4. **nextest.** Profiles `default`, `ci` (no fail-fast, JUnit) and `serial`; `retries = 0` in
   every profile; slow-timeout 30 s, terminated after four periods. Tests that start the daemon
   or a JVM share a test group bounded to the lane's parallelism (FND-08).
5. **Nothing skips.** A test that cannot find its environment (the pinned JDK, `git`) fails. Tests
   that need the target repository or a Claude Code run live in owner-run harnesses, never in the
   gate.

---

## 3. Determinism

1. **Fake clock.** Time reaches application code only through the `Clock` port. Deadlines, idle
   eviction, backoff and freshness stamps are tested on fake time (`tokio::time::pause` where
   Tokio timers are involved).
2. **Seeded randomness.** Anything randomised (property tests, trace generation, the completeness
   oracle's choice of module) takes an explicit seed, prints it on failure, and reproduces with it.
3. **No sleeps** (`ARCH-test-sleep`). Synchronisation is by channels, barriers, scripted fakes and
   fake time.
4. **The poll helper.** `jfl_testkit::poll::wait_until(deadline, reason, condition)` is the only
   sanctioned wait, for real-OS conditions in integration and e2e tests (a daemon's socket
   appeared, a child exited, a watcher event arrived). It is bounded, carries a reason shown on
   timeout, and is never a correctness oracle.
5. **A signal a test waits on promises exactly what its documentation says.** "Ready", "fresh"
   or "published" is read only from a signal documented to mean that (`jfl status --json`
   fields, `jfl sync` returning), never from a log line or a side effect that happens to come
   first.
6. **Property seeds** are persisted under `crates/<c>/proptest-regressions/` and committed; case
   counts come from the gate stage, never from code.
7. **Race tests** are bounded by iteration count, never by time, and include a negative control
   showing the naive implementation really races.

---

## 4. Golden fixtures

A golden fixture is a committed file that production must produce or accept byte for byte (after
documented normalisation of absolute paths and timestamps). Goldens are never rewritten to get
green; a deliberate change rewrites them in a commit that says why, with the `Impact:` line naming
the schema. `fixtures.lock` holds each fixture's SHA-256 (`ARCH-fixtures-lock`, IDX-07).

| Fixture set | Location | Pins |
|:--|:--|:--|
| Protocol messages | `crates/jfl-protocol/tests/fixtures/` | every message, both directions, each protocol version |
| CLI `--json` | `crates/jfl-cli/tests/fixtures/json/` | the envelope and every subcommand's output |
| LSP transcripts | `fixtures/lsp/*.jsonl` | client and server messages in order, per scenario (§2 LSP conformance) |
| Java projects | `fixtures/java/projects/` | small synthetic projects: single module; multi-module Gradle; a main checkout with three nested `.claude/worktrees/*`; Lombok and MapStruct; generated sources present and missing; a multi-release JAR; Java 8, 17 and 21 syntax |
| Java summaries | `fixtures/java/summaries/` | the expected `FileSummary` of each corpus file |
| Resolution cases | `fixtures/java/resolution/` | 200+ cases by construct (static call, field or parameter receiver, chain, `var`, lambda, stream, method reference, overload, inheritance, generics, Lombok member, inner and anonymous classes, static imports) with the expected `exact` target or the expected `possible` reason (EVAL-07) |
| Class files and JARs | `fixtures/jars/` | built from committed Java sources by a test, never committed as opaque binaries, except malformed inputs for refusal tests |

Every Java fixture compiles with the pinned JDK (`java-fixtures` stage, EVAL-07), except files in
`fixtures/java/broken/` which exist to fail and say so in a header comment.

---

## 5. Contract suites

For each port of plan §6.4, `jfl-testkit::contracts::<port>` holds one suite, parameterised by a
factory, run against the testkit fake **and** the real adapter from the adapter's crate. Each
suite's own tests run it against a deliberately broken implementation and assert it fails. The
suites verify refusals, ordering, idempotency, cancellation, error mapping and resource cleanup.
The fake must not be looser than production: where the real adapter can fail (a stat racing a
delete, a worker crash), the fake can be told to fail the same way.

| Suite | Implementations | Task |
|:--|:--|:--|
| `source_tree` | fake tree, `jfl-adapter-fs` | IDX-06 |
| `repo_probe` | scripted repository, `jfl-adapter-git` | IDX-05 |
| `syntax_indexer` | canned summaries, `jfl-adapter-java` | IDX-07 |
| `summary_store` | in-memory store, `jfl-adapter-store` | IDX-08 |
| `process_runner` | scripted runner, `jfl-process` | IDX-04 |
| `endpoint` | in-memory duplex, `jfl-adapter-ipc` | IDX-12 |
| `library_indexer` | canned API summaries, `jfl-adapter-classfile` | PROJ-03 |
| `project_model` | canned models, `jfl-adapter-gradle` | PROJ-01 |
| `compiler` | scripted compiler, `jfl-adapter-javac` | SEM-04 |

---

## 6. Formal models and conformance

### 6.1 Models and invariants

The models of plan §7, registered in `formal/models.toml` (the `MODELS` registry, FORMAL-01):

| Model | Invariants | Model task | Driver task |
|:--|:--|:--|:--|
| `views.qnt` | `NoMixedGeneration`, `GenerationsIncrease`, `AtomicPublish`, `FreshAfterStat`, `CompleteIsHonest`, `NoCrossView` | FORMAL-03 | IDX-09 |
| `daemon.qnt` | `AtMostOneOwner`, `ServedOnlyByOwner`, `NoGhostBuffers`, `ShutdownReleases`; liveness `EventuallyServed` | FORMAL-04 | IDX-12 |
| `diagnostics.qnt` | `NeverOlderOverNewer`, `PublishedMatchesVersion`, `ClosedPublishesNothing` | FORMAL-05 | LSP-04 |

Each model has named scenarios (`run`s) for the interleavings that matter (a checkout during a
reconcile; two proxies spawning at once; results arriving in reverse order), a `val
allInvariants`, and a **broken-model control**: an action variant that must violate a named
invariant within the simulation bound, so an invariant that can never fail is caught.

### 6.2 Trace replay

1. `formal/traces/regression/` holds committed ITF traces listed in `manifest.toml` (model, main,
   init, step, max steps, seed, invariant); `formal-freshness` regenerates each and compares byte
   for byte (excluding metadata). `formal/traces/generated/` is ignored by Git.
2. `conformance-replay` generates seeded traces with `quint run --mbt` (a budget per model,
   starting at 10 traces × 25 steps, measured and ratcheted) and replays them with the committed
   ones through `jfl-conformance`.
3. A driver decodes each step's action and nondeterministic picks, applies it to the **real** code
   (never a re-implementation of the transition), observes the real state through a canonical
   observation, and compares it with the model's projected state **after every step**, the
   initial state included. A divergence reports model, seed, trace, step, action, expected,
   actual and a line diff.
4. **Negative controls:** each driver has a test that swaps in a deliberately faulty component
   (plan §7: publish before store, release the lock before removing the socket, publish every
   result) and asserts the replay fails at a named step.
5. A model change without the matching driver and behaviour change fails the gate.

### 6.3 Apalache verification lane

`cargo xtask formal-verify [--model M]` runs Apalache (pinned, SHA-256-checked) bounded
verification of each model's `allInvariants` plus a long multi-seed simulation. It runs weekly
and by hand (`formal.yml`, one job per model), not in `check`. A counterexample becomes a
regression trace.

---

## 7. Oracles

### 7.1 SCIP differential

`jfl-oracle` reads a SCIP index (produced by `scip-java`, an independent indexer) and the `--json`
answers of the built `jfl`, and reports, per construct: precision of `exact` (agrees with SCIP),
coverage (share of SCIP references `jfl` answers `exact`), and envelope recall (every SCIP
reference is in `exact ∪ possible` whenever `complete=true`). On the golden corpus it runs in the
gate (expected answers are committed); on the target repository it is owner-run (SEM-06) and only
aggregates are committed.

### 7.2 Completeness oracle

Generate or take a fixture project, choose a target declaration, insert one extra caller in a
random module (seeded), and assert the caller appears in `exact ∪ possible` or the answer says
`complete=false`. Run for every module of every multi-module fixture (IDX-11, PROJ-02, SEM-03).
A single counterexample is a HIGH defect.

### 7.3 Envelope-honesty mutation suite

Curated mutants of the resolver and the completeness computation (flip a scoping rule, drop an
import, treat an unindexed module as indexed, ignore a deadline): each must make a named test fail
with a wrong `exact` or a wrong `complete=true` caught (SEM-03). The suite runs in
`integration-tests`.

---

## 8. Fuzzing

Fuzz targets (REL-03; earlier tasks add property tests on the same parsers): JSON-RPC framing and
LSP message decoding, `jfl-protocol` frame decoding, the tree-sitter summariser on arbitrary bytes
and on mutated corpus files, class-file parsing, JAR (zip) reading, Gradle model output parsing,
`git status -z` and `diff --name-only` parsing. Each target asserts no panic, bounded memory and
bounded time per input. Crashes become regression inputs under the target's corpus and a unit
test.

---

## 9. Mutation testing

`cargo-mutants` over `jfl-domain`, `jfl-protocol`, `jfl-application` and the resolver (REL-05),
weekly and by hand; survivors are triaged (equivalent, or a missing test added). The curated
envelope mutants of §7.3 are separate and gate on every push.

---

## 10. Benchmarks and performance claims

1. A latency, memory or throughput claim (in a doc, an ADR, a commit body or a report) names the
   benchmark or measurement, the machine (CPU, RAM, OS, disk), the input (fixture or target
   repository, aggregated) and the numbers.
2. Benchmarks (REL-05): cold and warm full-worktree index; per-request latency for def, hover,
   refs at 1, 3 and 10 worktrees; `initialize` against a warm daemon; RSS per view and per
   worker; reconcile after a checkout. Criterion for in-process benches, `cargo xtask bench` for
   process-level ones, results saved with machine facts.
3. Budgets in plan §2.5 and §2.7 (`initialize` < 200 ms warm, 300 ms and 2 s deadlines) are
   promises measured by the benchmark suite; a budget is changed only by the owner, never
   loosened to make a test pass. A test that times something against a fixed bound runs in the
   `serial` nextest profile, after the others.

---

## 11. Agent evaluation

1. **Harness** (`tools/jfl-eval`, EVAL-01): a task set format (task id, starting commit, prompt,
   test command, known-good diff reference), arm definitions (plugin set, settings, environment),
   and a transcript reader that extracts per run: success (the test command after the run),
   tool calls by kind, tokens, edits that broke compilation and whether pushed diagnostics caught
   them, the LSP operation histogram, and the reaction to empty references.
2. **Runs** are owner-run (Claude Code sessions cost money and need the target repository): 3
   runs per task per arm. The harness produces an aggregated report; if the target repository
   is private, only the report enters this repository (plan §9.1).
3. **Regression use:** a fixed task subset reruns at each milestone gate (IDX-15, LSP-06,
   SEM-07, REL-06); results are stored with the `jfl` version and the Claude Code version.

---

## 12. Lanes and budgets

| Lane | Runs | Where |
|:--|:--|:--|
| `check` | every stage of fnd §9 | every push (CI jobs), before every commit (local) |
| `formal.yml` | Apalache per model, fresh-seed conformance | weekly, by hand |
| `fuzz.yml` | each target for a time budget | nightly, by hand |
| `mutation.yml` | `cargo-mutants` | weekly, by hand |
| `security.yml` | advisories, secret scan, workflow lint | weekly, by hand |
| owner-run | the agent evaluation, SCIP differential and diagnostics gate on the target repository, real Claude Code smoke tests | per milestone |

Budgets (suite durations, trace counts, fuzz seconds) start as measured values and ratchet; a
lane that outgrows its budget is split, never silently trimmed.

---

## 13. Evidence and reporting

Every run leaves: the command, tool versions (`cargo xtask doctor --json`), seeds, verdicts per
stage, and for owner-run lanes the aggregated report with the `jfl` and Claude Code versions. A
task report (fnd §11.4) maps each acceptance item to the test or command that proves it. A claim
without evidence is written as planned or as a limitation.
