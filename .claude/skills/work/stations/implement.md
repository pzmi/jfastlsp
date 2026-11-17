# implement

Dispatch: subagent (general-purpose). Parallel sub-subagents only for independent, crate-scoped pieces.

## Inputs

plan.{planPath, checklist, adrs}, intake.{criteria, cites, taskId, authority, invariants, modelImpact, protocolImpact, gate}, review.findingsToFix (if looping)

## Prompt

Implement task {taskId}. Checklist: {plan.planPath}. Criteria: {criteria}. Gate: {gate}. {findingsToFix ? "Fix these review findings only: " + findingsToFix : ""}

1. Read `CLAUDE.md`; its rules bind you. Read the cited `doc/plan.md`, `doc/foundation.md` and `doc/verification.md` sections, the ADRs for the crates you touch, and the files the checklist names.
2. Work the checklist in order, ticking `- [x]` in `.work/<id>/plan.md` as each lands. Write the test with the code, not after; a failing test first where practical.
3. Model first: if the task touches views and generations, the daemon lifecycle or diagnostics publication, change the Quint model, its scenarios and the conformance driver before the Rust code (ver §6). Never remove or weaken a modelled invariant to get green.
4. Rules that fail the gate or the review if broken:
   - **Architecture law** (plan §6, fnd §2, §3): pure crates have no `std::{fs,net,process,env}`, clock, randomness, Tokio or threads; adapters do not reference sibling adapters or composition; production crates never depend on the test-only crates (`jfl-testkit`, `jfl-conformance`, `jfl-architecture`) or the tool crates (`jfl-oracle`, `jfl-eval`, `jfl-bench`, `xtask`); a new crate gets its workspace member, `architecture.toml` entry and external allowlist in the same commit. Fix an architecture failure by fixing the structure, never by loosening `architecture.toml` unless the task says so.
   - **Authority** (plan §6.5, fnd §2.4): each crate holds only the grants its row in `architecture.toml` gives it (process, socket, fs, spawn, sleep, clock, env); in production code a child process only through `jfl-process` (argv only, never `sh -c`; cleared environment plus an allowlist; deadline; own process group; bounded output); a socket only in `jfl-adapter-ipc`; no TCP listener anywhere; the request path never waits on a JVM (ADR 0015).
   - **The envelope** (plan §2.1, ADR 0014): `exact` only with evidence; `possible` a sound superset; `complete=true` only when every module that can see the target is indexed and fresh at the pinned generation. An empty answer always carries `complete` and `incomplete_because`.
   - **Views and freshness** (plan §4): every answer is computed at one pinned generation; a generation is published atomically; disk wins over a stale buffer; a request's file is resolved to the innermost worktree; nested worktrees are never scanned by their parent.
   - **LSP fidelity** (plan §2.5): positions in the negotiated encoding (UTF-16 by default), never bytes; every request answered or cancelled; framing limits enforced before allocation; `initialize` never waits on indexing.
   - **Untrusted input** (plan §11): Java sources, class files, JARs, JSON-RPC and protocol frames, Gradle output: bounded, no `unwrap`/`expect`/indexing on input-derived values, no panic.
   - `unsafe_code` is forbidden. No `todo!`, `unimplemented!`, `dbg!`. No `#[ignore]`. No `#[allow]`/`#[expect]` without an `architecture.toml` exception citing an ADR. `#[cfg]`, never `cfg!`.
   - Tests are named `should_<outcome>_when_<condition>` with `// given`, `// when`, `// then`. No sleeps: fake clock, `tokio::time::pause`, channels; a real-OS wait only via the testkit's `poll` helper (bounded, with a reason). Real objects or testkit fakes; no mocking frameworks; tempdirs; no fixed socket paths. Never the network, never a target Java repository outside a scratchpad copy.
   - Logging: `tracing`, fixed `snake_case` event names, data in fields; nothing at INFO on the healthy path; never source text in a log.
5. A doc comment's behavioural claim names the test that pins it. Comments only for non-obvious constraints and why.
6. A new dependency: `cargo add` (in the right crate, or `[workspace.dependencies]`), its `architecture.toml` external allowlist entry (and a `deny.toml` wrapper if only one crate may use it), a `Reuse considered:` note, and list it in `deviations` so ship names it in the commit message. An important decision whose context is worth recalling (ADR 0001, amended): `adrs new "<title>"` (or the template copied to the reserved number), or amend or supersede the ADR from the checklist, then `just adr-index`; any other decision goes in `deviations` with its reason, for the commit message.
7. `cargo fmt --all`, then the gate in the foreground with the lane's `CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS=2` and `CARGO_INCREMENTAL=0` until green (`just quick` while iterating once it exists). Nothing outside the task's files is staged or reverted.

## Output

```
{ filesChanged[], affectedCrates[], newDependencies[], adrsWritten[], summary, deviations[], issues[] }
```

`affectedCrates`: `git diff --name-only HEAD; git ls-files --others --exclude-standard` mapped to `crates/<name>`, `tools/<name>`, `formal/` or `fixtures/`.

## Sensors

Diff non-empty; `cargo fmt --all --check` exit 0; every checklist item ticked or listed in `deviations` with a reason.

## Gates

To test: always.
