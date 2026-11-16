# Architecture decision records

This directory holds the decisions behind jfastlsp, one file per decision, in Michael Nygard's
lightweight format: `# NNNN. Title`, a date, then **Status**, **Context**, **Decision** and
**Consequences**, and optionally **Alternatives considered**. Keep a record short (about 30-50
lines); the design detail lives in the document the ADR points to. ADR 0001 records this
practice.

**An ADR is a decision record, not a rule.** It records a decision as it was made: the context
at the time, the options weighed and why one was chosen. When the context changes, the decision
can change, deliberately and on the record. Never cite an ADR as a reason not to do the better
thing; never drift from one silently either. Change it on purpose, say why, and change the
rules, fixtures and tests that pin it in the same commit.

**Creating.** From FND-03 on, `cargo xtask adr new "<title>"` copies [`template.md`](template.md)
with the next free number and adds the row here (`--number N` for a number the orchestrator
reserved when it dispatched the task); `cargo xtask adr index` checks the rows against the files
and `--fix` rewrites them. Before FND-03, copy the template by hand and add the row. A change that
adds or reverses an architectural decision (a crate boundary, a dependency of consequence, a
protocol, a security or lifecycle policy, a tool or CI topology, a rule exception) adds or changes
an ADR in the same commit.

**Statuses.**

- `Proposed`: a planner default. Tasks follow it as written until the owner decides.
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
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted (owner, 2026-11-16) |
| [0002](0002-integrate-on-main-one-signed-off-commit-per-task.md) | Integrate on main: one signed-off commit per task, landed by the orchestrator | Accepted (owner, 2026-11-16) |
| [0003](0003-commit-identity-subject-and-trailer-order.md) | Commit identity, subject and trailer order | Accepted (owner, 2026-11-16) |
| [0004](0004-public-repository-no-private-projects.md) | Public repository: no private projects named or alluded to | Accepted (owner, 2026-11-16) |
| [0005](0005-dual-licence-mit-or-apache-2-0.md) | Dual licence MIT OR Apache-2.0 | Accepted (owner, 2026-11-16) |
| [0006](0006-platforms-linux-x86-64-and-arm64-macos-arm64.md) | Platforms: Linux x86-64 and arm64, macOS arm64; Windows out of scope | Accepted (owner, 2026-11-16) |
| [0007](0007-formal-methods-required-quint-with-itf-conformance.md) | Formal methods required: Quint models with ITF conformance against the real code | Accepted (owner, 2026-11-16) |
| [0008](0008-no-event-sourcing-the-index-is-a-rebuildable-cache.md) | No event sourcing: the index is a rebuildable cache | Accepted (owner, 2026-11-16) |
| [0009](0009-phase-0-first-owner-run-evaluations-and-decision-tasks.md) | Phase 0 first: owner-run evaluations and a decision task per open choice | Accepted (owner, 2026-11-16) |
| [0010](0010-workspace-crate-map-and-dependency-law.md) | Workspace crate map and dependency law in architecture.toml | Proposed |
| [0011](0011-pinned-toolchains-rust-1-97-0-and-tools-versions-toml.md) | Pinned toolchains: Rust 1.97.0, edition 2024; JDK and tools in tools/versions.toml | Proposed |
| [0012](0012-port-xtask-from-mayfly.md) | Port xtask from mayfly | Proposed |
| [0013](0013-two-carriers-over-one-query-engine-cli-first.md) | Two carriers over one query engine: JSON CLI first, LSP second | Proposed |
| [0014](0014-result-envelope-exact-possible-complete.md) | Result envelope: exact, possible, complete | Proposed |
| [0015](0015-no-jvm-and-no-build-on-the-request-path.md) | No JVM and no build on the request path | Proposed |
| [0016](0016-one-daemon-per-user-with-thin-proxies.md) | One daemon per user with thin per-client proxies | Proposed |
| [0017](0017-bind-views-per-request-uri-exclude-nested-worktrees.md) | Bind views per request URI; exclude nested worktrees | Proposed |
| [0018](0018-agent-sandbox-and-fail-closed-tools.md) | Agent sandbox and fail-closed tools | Proposed |
