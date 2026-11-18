# 0020. LSP stack: lsp-server, gen-lsp-types and line-index

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17); amended 2026-11-17 (LSP-01: the in-process harness runs on byte
pipes, not `Connection::memory()`; see the amendment below; accepted by the owner 2026-11-18).

## Context

`jfl lsp` is a thin proxy that maps the envelope onto LSP (plan §2.5). It must control the order
of `didChange` against requests, per-operation deadlines and cancellation, and it needs correct
LSP types. The long-standing `lsp-types` crate (0.97.0, 2024-06-04) is hand-written and stalled;
the original `tower-lsp` (0.20.0, 2023-08-11) is unmaintained. Survey of 2026-11-16, crates.io.

## Decision

- **[`lsp-server`](https://crates.io/crates/lsp-server) 0.10.0** (2026-07-16, MIT OR Apache-2.0;
  rust-analyzer's, also used by ruff and ty): a synchronous main loop that applies notifications
  in order and runs requests on a pool against an immutable snapshot, the rust-analyzer and ruff
  scheduler shape. Cancellation is ours (`$/cancelRequest` sets a token the query engine checks;
  `RequestCancelled` -32800, `ContentModified` -32801 for a stale generation).
- **[`gen-lsp-types`](https://github.com/ribru17/gen-lsp-types) 0.11.0** (2026-07-28, MIT),
  generated from the official LSP 3.18 metamodel; distinguishes `null` from absent.
  rust-analyzer moved to it on 2026-06-29 (#22115), ruff too. Pinned exactly (`=0.11.x`): every
  spec change is a breaking release. We advertise only the LSP 3.17 features plan §2.5 lists.
- **[`line-index`](https://crates.io/crates/line-index) 0.1.2** (MIT OR Apache-2.0) for UTF-8
  offset ↔ UTF-16 position. Conversions stay in `jfl-domain` (plan §2.3): it uses `line-index`
  directly if the crate builds under `#![no_std]` + `alloc`, and otherwise keeps its own
  conversion with `line-index` as the property-test oracle.
- Tests drive the server in process over `lsp_server::Connection::memory()` (rust-analyzer's
  slow-tests pattern) plus black-box transcripts against the built binary (ver §2).
- Both crates live in `jfl-adapter-lsp` only (`cargo-deny` wrappers, ADR 0030); IPC reuses
  `lsp_server::Message` too (ADR 0021).

## Consequences

LSP-01 builds on these instead of hand-written JSON-RPC types; every outgoing message can be
type-checked against the metamodel in a test. A `gen-lsp-types` bump is its own commit.

## Alternatives considered

- [`tower-lsp-server`](https://github.com/tower-lsp-community/tower-lsp-server) 0.24 (async, on
  gen-lsp-types, used by Biome and Oxc): the runner-up once 0.24 is final (0.24.0-rc.1 on
  2026-09-11); test notification ordering under load before switching.
- `async-lsp` 0.2.4: the best design to copy (ordered notifications, concurrent requests), but
  pinned to `lsp-types` 0.95.
- `lsp-types`, `tower-lsp`: stalled or unmaintained.

## Amended 2026-11-17 (LSP-01, technical correction; accepted by the owner 2026-11-18): the in-process harness runs on bytes

The in-process tests do not use `lsp_server::Connection::memory()`. It carries
`lsp_server::Message`, whose response result is a `serde_json::Value`, so a server on it could
not keep results on the typed path that plan §5.3 requires (the orchestrator's IDX-03 decision:
about 20 ms against 9 ms for a 10,000-location answer); it would also bypass `jfl-protocol`'s
frame decoder, which the harness must exercise, and its channels are unbounded (fnd §4.4 rule 5).
The harness keeps rust-analyzer's slow-tests shape (a server in process, a client that sends and
receives messages) on a bounded in-memory byte pipe (`jfl_adapter_lsp::memory`), through the same
reader and writer as stdio. Pinned by `crates/jfl-adapter-lsp/tests/server.rs`; the black-box
transcripts are unchanged.

`lsp-server` stays the message type (`Request`, `Notification`, `RequestId`; ADR 0021), but none
of its I/O is called (orchestrator, LSP-01 review): `Message::write` and `read` and the
`Connection` transports pass every message's text to `log::debug!`, so a `log` bridged into
`tracing` at DEBUG would log source text (fnd §5). The adapter parses frames in one typed pass and
writes them through `jfl-protocol` (errors as its `Response`, results on the typed path), and the
root `clippy.toml` denies those calls in every crate.
