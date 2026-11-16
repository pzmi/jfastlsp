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

## Platforms

Linux (x86-64, arm64) and macOS (arm64). Windows is out of scope for now
([ADR 0006](doc/adr/0006-platforms-linux-x86-64-and-arm64-macos-arm64.md)).

## Development

The work is planned as small tasks in [doc/tasks.md](doc/tasks.md) and carried out by coding
agents under the process in [doc/sdlc.md](doc/sdlc.md). Once the workspace exists (task FND-01),
the toolchain is pinned in `rust-toolchain.toml`, `cargo xtask check` is the gate and
`cargo xtask quick` the faster loop (task FND-02). See [CONTRIBUTING.md](CONTRIBUTING.md).

## Licence

Dual licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
