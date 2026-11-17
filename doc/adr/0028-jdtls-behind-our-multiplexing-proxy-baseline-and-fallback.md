# 0028. jdtls behind our multiplexing proxy: the baseline, for testing and comparison only

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17); amended 2026-11-17 (as built) and 2026-11-17 (owner: testing and
comparison only).

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

## Amended 2026-11-17 (orchestrator, EVAL-11 and FND-08): as built

This amendment was first written inline in the Decision section (EVAL-11) and moved here by
FND-08; its last two items are FND-08's.

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
- The socket (c2668b9): `<state>/sock` when that path fits a socket address (`MAX_SOCKET_PATH`,
  under macOS's 104 bytes and Linux's 108), else `/tmp/jfl-jdtls-<euid>/<key>.sock`, keyed on
  the canonical state directory so two state directories never share a socket. A fixed short
  base, not `$TMPDIR`, which may itself be long (a sandbox's long `TMPDIR` is what broke
  `land.sh`). The access control is the directory that holds the socket, whichever it is: created
  `0700`, and an existing one refused unless it is a real directory (not a symlink) of mode
  `0700` owned by the effective uid (`daemon::prepare_state_dir`), so the short directory under
  the world-writable `/tmp` is held to the state directory's rules. The peer-uid check
  (`SO_PEERCRED`, through `rustix`) runs on Linux only; on macOS it is the directory check only,
  as `rustix` has no `getpeereid`. Plan §5.3 now states this policy, Proposed for the product
  daemon (IDX-12).
- The daemon's idle exit and the hand-shake and reply deadlines are timed waits
  (`recv_timeout`, socket read timeouts), which fnd §2.4 now puts under the `sleep` grant
  (FND-08, the FND-05 checkpoint's N7): `jfl-jdtls` holds it.

## Amended 2026-11-17 (owner): for testing and comparison only, never a shipped fallback

The owner decided at the FND checkpoint that `jfl-jdtls` is for testing and comparison only: the
kill-criterion baseline (plan §1.3), an E0 and E4 arm and a bench arm. It is **never a shipped
fallback product**: the Decision's sentence "If DEC-02 stops the project, it is the deliverable,
promoted from tool to product by that decision" is withdrawn, and so is "fallback product" in the
title (the file keeps its name so links stay valid). The kill criterion becomes: if phase 0
measurements show jfastlsp cannot beat this baseline on speed with honest envelopes, the project
stops or is rescoped (plan §1.3, DEC-02); it does not become "ship jdtls". `jfl-jdtls` stays a
tool-only crate that imports no production crate.
