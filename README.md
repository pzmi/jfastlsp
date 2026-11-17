# jfastlsp

Agent-centric Java code intelligence in Rust. One binary, `jfl`: `jfl lsp` is a stdio language
server, and every other subcommand (`jfl def`, `jfl refs`, `jfl hover`, `jfl sym`, ...) is a JSON
CLI. Both talk to one shared daemon that indexes the Java sources of many Git worktrees of a large
Gradle repository at once, so many coding agents, each in its own worktree, share one index.

Every navigation answer says how complete it is: `exact` (proved), `possible` (a sound superset
of the rest, each with the reason it could not be proved) and `complete`. When `complete` is
`true`, `exact ∪ possible` holds every reference; when it is `false`, the answer says why. An
agent can then decide between trusting the answer and searching further, instead of guessing from
an empty list.

**Speed is the point.** An answer an agent has to wait for is an answer it replaces with grep, and
a server that imports a Gradle build for minutes per worktree is one it never gets to use. The
targets, in [plan §1.7](doc/plan.md), include a cold syntax index of a 34k-file repository in at
most 13 s on 4 cores (measured 8.7 s on a 4 vCPU Xeon 2.3 GHz VM, 2026-11-16), definition and
hover answered end to end in under 50 ms at p95, and an extra worktree costing under 50 MB.
Every change is benchmarked against a pinned corpus of real open-source Gradle repositories
([doc/bench-corpus.md](doc/bench-corpus.md)) and against jdtls behind a shared proxy; from the
first milestone, a regression of more than 10 % blocks the change.

> **Status: pre-alpha, planning.** There is no code yet. Phase 0 measures real agent tasks,
> parse times, resolvers and `javac` latency before building (see the plan, §9); a go/no-go
> decision follows. There is no release.

## Design

- [doc/plan.md](doc/plan.md): what jfastlsp is and how it is structured: the agent contract and
  result envelope (§2), worktrees and freshness (§4), runtime (§5), crates (§6), formal models
  (§7), phase 0 (§9) and milestones (§10).
- [doc/foundation.md](doc/foundation.md): engineering rules; [doc/verification.md](doc/verification.md):
  how it is tested; [doc/sdlc.md](doc/sdlc.md): how a change reaches `main`.
- [doc/adr/](doc/adr/README.md): the decisions; [doc/tasks.md](doc/tasks.md): the task order.
- Reuse first: what is built and what is reused (a Gradle model, a javac worker, LSP and storage
  crates, the tooling) is in plan §16.

## Platforms

Linux (x86-64, arm64) and macOS (arm64). Windows is out of scope for now
([ADR 0006](doc/adr/0006-platforms-linux-x86-64-and-arm64-macos-arm64.md)).

## Development

The work is planned as small tasks in [doc/tasks.md](doc/tasks.md) and carried out by coding
agents under the process in [doc/sdlc.md](doc/sdlc.md). The Rust toolchain is pinned in
`rust-toolchain.toml` and every other tool in `mise.toml` (`bash scripts/bootstrap-tools.sh`
installs mise and just, then `mise install --locked` the rest); `just check` is the gate, `just
quick` the faster loop, and CI runs the same stages. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
