//! jdtls behind a multiplexing proxy: the kill-criterion baseline and the fallback product
//! (ADR 0028); imports no production crate. Role: tool only, independent.
//!
//! `jfl-jdtls lsp` is a stdio proxy that every LSP client (Claude Code and each subagent)
//! starts; it hands its stream to a per-user daemon that runs **one jdtls per worktree root**
//! and multiplexes every client of that root onto it. See `doc/eval/jfl-jdtls.md`.

pub mod daemon;
pub mod fake;
pub mod framing;
pub mod json;
pub mod launch;
pub mod mux;
pub mod proxy;
pub mod session;
pub mod worktree;
