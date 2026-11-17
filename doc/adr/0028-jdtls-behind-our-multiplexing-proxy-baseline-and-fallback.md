# 0028. jdtls behind our multiplexing proxy: the baseline and the fallback product

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07).

## Context

Plan §1.3's kill criterion compares this design with "a tuned `jdtls-lsp`". The survey of
2026-11-16 shows the stock setup is not a fair baseline. The official `jdtls-lsp` plugin is a bare
`jdtls` command with no `-data`, shared index or heap setting
([claude-plugins-official](https://github.com/anthropics/claude-plugins-official)); jdtls's
launcher keys `-data` on the *basename* of the working directory, so worktrees with the same
basename collide ([claude-code#67182](https://github.com/anthropics/claude-code/issues/67182));
and every subagent starts its own jdtls of 2–3 GB
([claude-code#64536](https://github.com/anthropics/claude-code/issues/64536)). jdtls itself can do
better: `-data` per workspace, `-Djdt.core.sharedIndexLocation` shares library indexes across
workspaces, and `-Dsyntaxserver=true` serves syntax before the project import finishes.

## Decision

Build **`jfl-jdtls`** in phase 0 (EVAL-11), a tool-only crate (`tools/jfl-jdtls`) that imports
no production crate:

- a stdio proxy that every Claude Code LSP client starts, and a per-user daemon that runs **one
  jdtls per worktree root**, shared by every client in that worktree;
- `-data` keyed on a digest of the **full** worktree path; one shared library index directory;
  a heap cap; optionally the syntax server until the import finishes;
- multiplexing: request ids remapped per client, document sync reference-counted per URI (agents
  write to disk; the last open buffer wins), cancellation forwarded, a client's exit never stops
  the jdtls others use; `lsp-server` for framing (ADR 0020).

Amended 2026-11-17 (EVAL-11), as built:

- Framing is the tool's own bounded `Content-Length` reader, not `lsp-server`: a proxy forwards
  raw bodies and must refuse an oversized frame before allocating it, which `lsp-server`'s
  `Message::read` does not, and `deny.toml` wraps `lsp-server` to the LSP and IPC adapters.
  Messages are decoded only as far as the JSON-RPC envelope (`serde_json`, `RawValue`):
  payloads the proxy does not rewrite are forwarded byte for byte.
- jdtls is pinned at 1.61.0, the newest release that runs on Java 21 (1.62.0 and later need
  Java 25).
- Binding is per connection (the proxy's working directory), not per URI; the outer checkout's
  jdtls excludes `**/.claude/worktrees/**` from its import so nested agent worktrees are not
  imported twice.
- The syntax server is a whole-session mode (`--syntax-server`); running it until the import
  finishes and then switching to the full server is not built.
- The socket: the `0700` state directory (created so, an existing one refused unless a real
  directory of mode `0700` owned by the effective uid) is the access control everywhere; the
  peer-uid check (`SO_PEERCRED`, through `rustix`) runs on Linux only. On macOS it is the
  directory check only, as `rustix` has no `getpeereid`.

It is the **kill-criterion baseline** (plan §1.3) and an E0 and E4 arm. If DEC-02 stops the
project, it is the deliverable, promoted from tool to product by that decision.

## Consequences

E0 compares like with like: our design against jdtls with the same sharing a user could get.
jdtls (EPL-2.0) is only ever a separate process, never linked or bundled. Keeping the proxy's
backend pluggable also hedges the risk that JetBrains ships a headless, agent-licensed IntelliJ
LSP ([IDEA-393669](https://youtrack.jetbrains.com/projects/IDEA/issues/IDEA-393669)): it could
front that the same way. About a week of phase 0 work.

## Alternatives considered

- The stock plugin as the baseline: duplicated per subagent and colliding by basename; unfair.
- Serena (GPL-3.0 application; its jdtls collision fix is an open PR) and Plumb (Go, a shared
  daemon with one LSP per project root, no worktree story): ideas only.
