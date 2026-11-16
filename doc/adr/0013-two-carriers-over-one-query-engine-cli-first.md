# 0013. Two carriers over one query engine: JSON CLI first, LSP second

Date: 2026-11-16

## Status

Proposed.

## Context

Claude Code's LSP tool exposes definition, references, hover, symbols, implementations and call
hierarchies, but plain LSP cannot carry a completeness statement, and in cloud sessions Claude
Code starts no plugin language server. The Bash tool, by contrast, can call a binary on `PATH`
from a plugin's `bin/` and read its JSON.

## Decision

One query engine in the daemon serves two carriers: the **`jfl` JSON CLI** (primary; carries the
full envelope; works in cloud sessions through the setup script) and **`jfl lsp`** (secondary,
lossy; maps the envelope onto LSP, with hover's status line as the completeness channel). Both
are thin proxies speaking `jfl-protocol` to the daemon; `jfl-adapter-lsp` depends only on
`jfl-protocol` and `jfl-domain`, so the carriers cannot compute different answers. Two plugin
variants ship: a CLI plugin with `bin/` and the skill, and an LSP-only plugin (plan §2.6).

## Consequences

M1 is a CLI vertical slice; M2 adds the LSP carrier. The skill text and E0 decide how agents use
each. The `referencesMode` default is DEC-06.
