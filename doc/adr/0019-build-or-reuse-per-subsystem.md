# 0019. Build or reuse, per subsystem

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17); amended 2026-11-17.

## Context

BOOT-00 planned to hand-build most subsystems and to port about 16k generic lines of mayfly's
`xtask`. A build-vs-buy survey on 2026-11-16 (crates.io, the projects' repositories, release
pages) found maintained projects that cover much of it, and the owner's instruction is "don't
reinvent the wheel". It also settled one of plan §9.2's unverified facts: Claude Code starts one
language server per process subtree, so **subagents do not share a server**
([claude-code#64536](https://github.com/anthropics/claude-code/issues/64536): one jdtls of 2–3 GB
per subagent, over 50 GB in multi-agent sessions). Nobody found shares indexes across Git
worktrees: [codebase-memory-mcp](https://github.com/DeusData/codebase-memory-mcp/discussions/436)
treats worktrees as separate projects, jdtls shares library indexes only.

## Decision

Reuse by default; build only what nothing maintained does. Plan §16 holds the full table with
versions and licences; the subsystems:

| Subsystem | Verdict | What | ADR |
|---|---|---|---|
| Daemon, proxies, per-worktree views, freshness, envelope, storage layout | **Build** | the novel part; #64536 makes the shared daemon essential | 0016, 0017 |
| LSP protocol | Reuse | `lsp-server` + `gen-lsp-types` + `line-index` | 0020 |
| Daemon IPC | Reuse | LSP framing over a Unix socket, std `File::try_lock`, `rustix` | 0021 |
| Java syntax | Reuse | `tree-sitter` + `tree-sitter-java-orchard`, own queries | 0022 |
| Class files, JDK API | Reuse | `cafebabe` + `zip`, `ct.sym` | 0023 |
| Git | Reuse | the `git` CLI, stat-only freshness | 0024 |
| Storage | Reuse | `rusqlite` + `fst` + `postcard` + `blake3` (tier by DEC-03) | 0025 |
| Gradle model | **Wrap** | [microsoft/build-server-for-gradle](https://github.com/microsoft/build-server-for-gradle) (MIT) | 0026 |
| javac worker | **Fork** | [georgewfraser/java-language-server](https://github.com/georgewfraser/java-language-server)'s compiler core (MIT) + [scip-java](https://github.com/scip-code/scip-java)'s plugin | 0027 |
| Resolver | E2 decides | adds a javac-attributed arm; drops [Stack Graphs](https://github.com/github/stack-graphs) (archived 2025-09) | DEC-04 |
| SCIP | Reuse | `scip-java` as the oracle, the `scip` crate; M6 folded into M4 | 0009 |
| Baseline and fallback | **Wrap** | jdtls behind our multiplexing proxy | 0028 |
| Agent evaluation | **Wrap** | [Harbor](https://github.com/harbor-framework/harbor) + [Multi-SWE-bench](https://github.com/multi-swe-bench/multi-swe-bench) Java | 0029 |
| Engineering foundation | Reuse | just, mise, cargo-deny, `no_std`, adrs, zizmor, pinact, diff-cover, quint-connect | 0030 |

Every task card that adds a subsystem carries a **`Reuse considered:`** line naming what was
weighed. A hand-built subsystem that has a maintained alternative needs a recorded reason
(commit message or ADR when the context is worth keeping).
Licences stay permissive in the binary: GPL code (Serena's application) is never linked; EPL-2.0
jdtls runs only as a separate process (ADR 0028).

## Consequences

The plan shrinks where it was re-implementing (the Gradle helper, the javac worker, the tooling)
and gains phase 0 work that pays for itself: the jdtls proxy is both the fairest baseline and the
fallback product. New upstream risks (one-maintainer forks, 0.x crates) are listed in plan §13
and pinned exactly where an upstream breaks on minors.

## Alternatives considered

- Build everything as BOOT-00 planned: months of work duplicating maintained projects.
- Buy the whole product (jdtls per worktree): fails plan §1.1; each worktree pays a 2–3 GB JVM
  and a full Gradle import (#64536).
- Fork [kmp-lsp](https://github.com/Hessesian/kmp-lsp) (MIT, Rust) as the product: no worktree
  handling and Kotlin-first; kept as an E0 arm, an E2 candidate and the plan §13 fallback if it
  converges on this niche.

## Amended 2026-11-17 (orchestrator, recording the owner's 0028 decision): the jdtls baseline is not a fallback

The rows above name jdtls behind our proxy as the "baseline and fallback" (0028). The owner
decided on 2026-11-17 that it is for testing and comparison only, never a shipped product (ADR
0028, amended); the javac worker is now our own (ADR 0036, superseding 0027), and the
foundation's tools move to a ported `cargo xtask` (ADR 0035, superseding 0030). The table is
kept as the survey recorded it.
