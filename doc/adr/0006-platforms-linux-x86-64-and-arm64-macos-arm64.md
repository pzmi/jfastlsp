# 0006. Platforms: Linux x86-64 and arm64, macOS arm64; Windows out of scope

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-16). Revisitable when a Windows user base appears.

## Context

The consumers are Claude Code agents on developer machines and cloud sandboxes: macOS on Apple
Silicon and Linux on x86-64 and arm64. The design relies on Unix domain sockets with peer
credentials, `flock`, process groups and POSIX file modes (plan §5.2, §5.3); Windows would need a
second implementation of each.

## Decision

Supported targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
`aarch64-apple-darwin`. Windows and Intel macOS are out of scope. Platform-specific code is a
`#[cfg]` item, never a `cfg!` branch. CI runs the platform tests natively on each target
(FND-07); the architecture checker evaluates edges per target (fnd §2.2); `deny.toml` counts
these targets only; releases ship these three archives (REL-06).

## Consequences

Unix-only APIs are used freely and tested natively. A Windows port would be a new ADR with a new
IPC and process design.
