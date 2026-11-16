# 0021. Daemon IPC: LSP framing over a Unix socket, single owner by a std file lock

Date: 2026-11-16

## Status

Proposed (orchestrator default; owner confirms at the FND checkpoint, DEC-07).

## Context

Proxies (`jfl lsp`, the CLI) talk to one daemon per user over a Unix socket (ADR 0016, plan
§5.2–§5.3). BOOT-00 left the wire as "length-prefixed `jfl-protocol` messages", a second protocol
beside LSP. The survey (2026-11-16) found the pieces in std and maintained crates, and daemon
lifecycles worth copying in sccache, turborepo, buck2 and Biome.

## Decision

- **One wire protocol:** JSON-RPC 2.0 with the LSP base protocol's `Content-Length` framing over
  the Unix socket, reusing `lsp_server::Message` (ADR 0020). The CLI, the LSP proxy and the
  daemon share one message type, one fuzz target and one trace format readable by LSP tooling.
  `jfl-protocol` defines the method names, payloads, versions and limits, and a sans-io
  `Content-Length` decoder (about 60 lines, `no_std`) that checks the declared length against the
  limit **before allocating**; the IPC and LSP adapters feed it bytes rather than trusting
  `lsp-server`'s own reader with untrusted lengths.
- **Daemon:** Tokio `UnixListener`; `UnixStream::peer_cred()` rejects any uid but its own.
  **Client:** std `UnixStream`, no async runtime at start-up (it matters for `jfl refs` latency).
- **Single owner:** std `File::lock`/`try_lock` (stable since Rust 1.89) on `daemon.lock`, held
  for the daemon's life; a pid file is informational only (turborepo's failure mode is a stale
  pid file over a dead socket). A spawn lock serialises racing proxies.
- **Spawn:** `jfl-process`'s detached mode runs `jfl daemon` with `process_group(0)`; the daemon
  calls `rustix::process::setsid()` (rustix 1.1, safe API) itself, so no `pre_exec`. sccache's
  handshake: the proxy binds a one-shot notify socket before spawning, the child reports ready or
  its error, the proxy waits with a bounded budget.
- Version handshake on connect; a mismatch asks the old daemon to stop and respawns (buck2,
  rust-analyzer's proc-macro server). Biome's `lsp-proxy` is the reference for the proxy shape.

## Consequences

Plan §5.3 and IDX-03, IDX-12 and LSP-01 change: the protocol crate stays pure (its decoder is
sans-io) and the sockets live in `jfl-adapter-ipc`. Modes, peer checks and path limits are
unchanged. `daemon.qnt` is unaffected.

## Alternatives considered

- `jsonrpsee`, `tarpc`: heavy, or a second protocol no LSP tool can read.
- `fs2` (2018), `fd-lock`, `fs4`: unnecessary since Rust 1.89. `daemonize`/`fork`: `fork()` in a
  multi-threaded process is unsafe in practice. `interprocess`: only if Windows becomes a target.
