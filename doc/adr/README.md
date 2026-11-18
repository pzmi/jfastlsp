# Architecture decision records

This directory holds the decisions behind jfastlsp, one file per decision, in Michael Nygard's
lightweight format: `# NNNN. Title`, a date, then **Status**, **Context**, **Decision** and
**Consequences**, and optionally **Alternatives considered**. Keep a record short (about 30-50
lines); the design detail lives in the document the ADR points to. ADR 0001 records this
practice.

**An ADR is for an important decision whose context we may want to recall later** (the owner,
2026-11-17, ADR 0001 as amended): a direction the project could plausibly reverse, where the
reasons and the alternatives weighed would otherwise be lost. Not everything needs one: commit
messages, code comments, the plan and the task cards record the rest. ADRs 0010 and 0018 are the
kind that did not need a record; they are kept, and no more of that kind are written.

**An ADR is a decision record, not a rule.** It records a decision as it was made: the context
at the time, the options weighed and why one was chosen. It is not absolute, and it can change
in time, deliberately and on the record. Never cite an ADR as a reason not to do the better
thing; never drift from one silently either. Change it on purpose, say why, and change the
rules, fixtures and tests that pin it in the same commit.

**Creating.** `adrs new "<title>"` (the `adrs` tool, pinned by mise; ADR 0030; configured by the
root `adrs.toml`) creates the next record from [`template.md`](template.md), as Proposed, and
`just adr-index` (the `docs` stage of `check`) checks the rows here against the files: every file
has one row with its title, its link and its status keyword (and `(owner, date)`). Add the row by
hand after `adrs new`. `template.md` is an `adrs` template: its first line, date and status are
placeholders (`{{ number }}`, `{{ title }}`, `{{ date }}`, `{{ status }}`). A number the
orchestrator reserved at dispatch is created by copying the template to `NNNN-kebab-title.md` and
replacing the placeholders by hand (`# NNNN. Title`, today's date, `Proposed`). `scripts/land.sh`
merges two lanes' new rows itself and sorts them. A change that reverses a recorded decision
amends or supersedes its ADR in the same commit; a new decision gets an ADR only when it is
important enough that its context is worth recalling, and otherwise its commit message says why.

**Statuses.**

- `Proposed`: a planner or orchestrator default. Tasks follow it as written until the owner
  decides; open ones go to the owner at the next ⛳ checkpoint. The FND checkpoint's batch
  (DEC-07) was decided on 2026-11-17.
- `Accepted (owner, YYYY-MM-DD)`: the owner's decision. Only the owner moves a record here.
- `Rejected (owner, YYYY-MM-DD)`: considered and declined; kept so it is not proposed again
  without new context.
- `Superseded by NNNN`: replaced as a whole. The new record says what it supersedes.

Any status may carry annotations after it: `amended YYYY-MM-DD`, `amended by NNNN`,
`partly superseded by NNNN (...)`.

**Changing a decision.** A refinement that keeps the decision is appended as
`## Amended YYYY-MM-DD (owner|planner): <what>` with the reason; the original Context and
Decision stay. A reversal is a new ADR that supersedes the old one. An amendment of an Accepted
record by anyone but the owner is marked `amendment proposed YYYY-MM-DD` until the owner accepts
it.

**Ids.** The ADR number is the decision's id; there are no separate D-ids. Numbers are reserved by
the orchestrator at dispatch; a lane that finds its number taken when it lands renumbers its
record and every citation of it in the same commit. `DEC-` tasks in `doc/tasks.md` produce ADRs
for the owner's decisions.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted (owner, 2026-11-16); amended 2026-11-16 (accepted 2026-11-17) and 2026-11-17 |
| [0002](0002-integrate-on-main-one-signed-off-commit-per-task.md) | Integrate on main: one signed-off commit per task, landed by the orchestrator | Accepted (owner, 2026-11-16); amended 2026-11-16 (accepted 2026-11-17) |
| [0003](0003-commit-identity-subject-and-trailer-order.md) | Commit identity, subject and trailer order | Accepted (owner, 2026-11-16) |
| [0004](0004-public-repository-no-private-projects.md) | Public repository: no private projects named or alluded to | Accepted (owner, 2026-11-16); amended 2026-11-17 |
| [0005](0005-dual-licence-mit-or-apache-2-0.md) | Dual licence MIT OR Apache-2.0 | Superseded by 0033 |
| [0006](0006-platforms-linux-x86-64-and-arm64-macos-arm64.md) | Platforms: Linux x86-64 and arm64, macOS arm64; Windows out of scope | Accepted (owner, 2026-11-17) |
| [0007](0007-formal-methods-required-quint-with-itf-conformance.md) | Formal methods required: Quint models with ITF conformance against the real code | Accepted (owner, 2026-11-17); amended 2026-11-16, 2026-11-17 (owner) and 2026-11-17 (FORMAL-02) |
| [0008](0008-no-event-sourcing-the-index-is-a-rebuildable-cache.md) | No event sourcing: the index is a rebuildable cache | Accepted (owner, 2026-11-17) |
| [0009](0009-phase-0-first-owner-run-evaluations-and-decision-tasks.md) | Phase 0 first: owner-run evaluations and a decision task per open choice | Accepted (owner, 2026-11-17); amended 2026-11-16 |
| [0010](0010-workspace-crate-map-and-dependency-law.md) | Workspace crate map and dependency law in architecture.toml | Accepted (owner, 2026-11-17); amended 2026-11-16 (twice) and 2026-11-17 |
| [0011](0011-pinned-toolchains-rust-1-97-0-and-tools-versions-toml.md) | Pinned toolchains: Rust 1.97.0, edition 2024; JDK and tools in tools/versions.toml | Superseded by 0030; tool pins restored by 0035 |
| [0012](0012-port-xtask-from-mayfly.md) | Port xtask from mayfly | Superseded by 0030; direction restored by 0035 |
| [0013](0013-two-carriers-over-one-query-engine-cli-first.md) | Two carriers over one query engine: JSON CLI first, LSP second | Superseded by 0034 |
| [0014](0014-result-envelope-exact-possible-complete.md) | Result envelope: exact, possible, complete | Accepted (owner, 2026-11-17) |
| [0015](0015-no-jvm-and-no-build-on-the-request-path.md) | No JVM and no build on the request path | Accepted (owner, 2026-11-17) |
| [0016](0016-one-daemon-per-user-with-thin-proxies.md) | One daemon per user with thin per-client proxies | Accepted (owner, 2026-11-17) |
| [0017](0017-bind-views-per-request-uri-exclude-nested-worktrees.md) | Bind views per request URI; exclude nested worktrees | Accepted (owner, 2026-11-17) |
| [0018](0018-agent-sandbox-and-fail-closed-tools.md) | Agent sandbox and fail-closed tools | Accepted (owner, 2026-11-17); amended 2026-11-16 (twice) and 2026-11-17 |
| [0019](0019-build-or-reuse-per-subsystem.md) | Build or reuse, per subsystem | Accepted (owner, 2026-11-17); amended 2026-11-17 (twice) |
| [0020](0020-lsp-stack-lsp-server-gen-lsp-types-line-index.md) | LSP stack: lsp-server, gen-lsp-types and line-index | Accepted (owner, 2026-11-17); amended 2026-11-17 (LSP-01) |
| [0021](0021-daemon-ipc-lsp-framing-over-a-unix-socket.md) | Daemon IPC: LSP framing over a Unix socket, single owner by a std file lock | Accepted (owner, 2026-11-17) |
| [0022](0022-java-grammar-tree-sitter-java-orchard-with-own-queries.md) | Java grammar: tree-sitter-java-orchard with our own queries | Accepted (owner, 2026-11-17); amended 2026-11-17; amended 2026-11-18 (proposed) |
| [0023](0023-class-files-cafebabe-and-ct-sym.md) | Class files: cafebabe and zip; the JDK API from ct.sym | Accepted (owner, 2026-11-17); amended 2026-11-17 |
| [0024](0024-git-cli-on-the-reconcile-path-not-gitoxide.md) | Git: the git CLI on the reconcile path, stat-only freshness, not gitoxide | Accepted (owner, 2026-11-17) |
| [0025](0025-storage-rusqlite-fst-postcard-blake3.md) | Storage crates: rusqlite, fst, postcard and blake3 | Accepted (owner, 2026-11-17) |
| [0026](0026-gradle-model-by-wrapping-build-server-for-gradle.md) | Gradle model: wrap microsoft/build-server-for-gradle | Accepted (owner, 2026-11-17) |
| [0027](0027-javac-worker-forked-from-java-language-server-with-scip-java.md) | javac worker: fork java-language-server's compiler core, load scip-java's plugin | Superseded by 0036 |
| [0028](0028-jdtls-behind-our-multiplexing-proxy-baseline-and-fallback.md) | jdtls behind our multiplexing proxy: the baseline, for testing and comparison only | Accepted (owner, 2026-11-17); amended 2026-11-17 (three times) |
| [0029](0029-e0-on-harbor-with-a-multi-swe-bench-java-control.md) | E0 on Harbor, with a Multi-SWE-bench Java control | Accepted (owner, 2026-11-17) |
| [0030](0030-foundation-from-maintained-tools.md) | Foundation from maintained tools: just, mise, cargo-deny, no_std and a thin test layer | Superseded by 0035 |
| [0031](0031-benchmark-early-on-a-pinned-real-corpus.md) | Benchmark early on a pinned real-repository corpus; regressions over 10 % block | Accepted (owner, 2026-11-17); amended 2026-11-16 and 2026-11-17 |
| [0032](0032-evaluation-target-spring-framework-at-the-bench-pin.md) | Evaluation target: spring-framework at the bench pin | Accepted (owner, 2026-11-17) |
| [0033](0033-licence-mit-only.md) | Licence: MIT only | Accepted (owner, 2026-11-17) |
| [0034](0034-two-carriers-lsp-first-cli-second.md) | Two carriers over one query engine: LSP first, CLI second | Accepted (owner, 2026-11-17) |
| [0035](0035-port-mayfly-xtask-piece-by-piece.md) | Port mayfly's xtask, piece by piece | Accepted (owner, 2026-11-17) |
| [0036](0036-javac-worker-our-own-warm-context-with-scip-java.md) | javac worker: our own small worker with one warm javac context, scip-java's plugin | Accepted (owner, 2026-11-17) |
| [0037](0037-completeness-requires-a-watched-cookie-synced-view.md) | Completeness requires a watched, cookie-synced view | Proposed |
