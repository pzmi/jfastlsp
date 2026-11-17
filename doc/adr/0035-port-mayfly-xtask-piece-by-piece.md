# 0035. Port mayfly's xtask, piece by piece

Date: 2026-11-17

## Status

Accepted (owner, 2026-11-17). Supersedes
[0030](0030-foundation-from-maintained-tools.md); restores the direction of
[0012](0012-port-xtask-from-mayfly.md) and the tool pins of
[0011](0011-pinned-toolchains-rust-1-97-0-and-tools-versions-toml.md).

## Context

ADR 0012 proposed porting `tools/xtask` from the public repository mayfly; ADR 0030 replaced
that with maintained tools composed by a justfile, mise and shell scripts (FND-01..FND-08). At
the FND checkpoint (2026-11-17) the owner reversed 0030: the command surface becomes mayfly's
`cargo xtask`, ported here. What FND built is the gate today and keeps running until each piece
is replaced; nothing is dropped to make the move.

## Decision

- **`tools/xtask`** with a `cargo xtask` surface replaces the justfile, mise and the shell
  scripts **piece by piece**, one card of lane XT each, starting from mayfly's modules (cited per
  card) and adapted to this repository.
- **External tools stay** and xtask runs them: cargo-deny, gitleaks, zizmor, actionlint,
  shellcheck, hyperfine, cargo-nextest and Quint (with Apalache), and the others the gate uses
  today (pinact, adrs, cargo-llvm-cov, diff-cover, cargo-mutants) until a card decides otherwise.
- **Tool versions** live in `tools/versions.toml`, checked by `cargo xtask doctor` and installed
  checksum-verified, as ADR 0011 described; `rust-toolchain.toml` keeps the Rust pin, recorded here
  (1.97.0, ADR 0011's pin, until FND-10 moves it to 1.99.1 by amending this record).
- **The architecture checks move in**: `jfl-architecture`'s rules, fixtures and meta-test become
  `cargo xtask architecture`, keeping every negative case.
- **Every check the gate runs today keeps running**: each XT card lists the stages, negative
  tests and script-test cases it moves, and they are its acceptance criteria. The gate's own
  configuration stays pinned (fnd §3.6), the pin following the code into xtask. The last card
  removes just and mise.
- **Later**, the parts shared with mayfly and site-preview move into a separate project; not
  scheduled yet.

## Consequences

Lane XT in `doc/tasks.md` (XT-01: scaffold `tools/xtask` and port the gate composition, then one
card per piece, the last removing just and mise). Until a piece is ported, `just`, `mise` and the
scripts described in fnd §8 and §9 remain the gate, and their tests stay green. The
one-cargo-configuration rule of FND-08 (N1) admits exactly the `xtask` alias as a pinned shape
when XT-01 adds it. The ported code is ours to maintain: more lines than ADR 0030's glue, in
exchange for one Rust command surface across this repository, mayfly and site-preview.

## Alternatives considered

- Keep ADR 0030's maintained-tool foundation: fewer lines to own, but three surfaces (just,
  mise, shell) and nothing shared with the owner's other repositories.
- Port xtask in one go (ADR 0012): a large change that would stop the gate while it lands.
