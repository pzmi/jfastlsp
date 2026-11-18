# 0024. Git: the git CLI on the reconcile path, stat-only freshness, not gitoxide

Date: 2026-11-16

## Status

Accepted (owner, 2026-11-17); amended 2026-11-18 (IDX-05). Settles the open
question plan §14 gave to IDX-05; IDX-05 may amend it with measurements.

## Context

The daemon needs, per worktree: the innermost root, the common directory, the `HEAD` and index
files to stat on every request, and the files changed since a tree (plan §4.3, §4.6). Linked
worktrees under `.claude/worktrees/` are the normal case. Survey of 2026-11-16:
[gitoxide](https://github.com/GitoxideLabs/gitoxide/blob/main/crate-status.md) (`gix` 0.90,
monthly breaking releases) documents that "the interplay between `GIT_COMMON_DIR` and
`GIT_WORK_TREE` isn't implemented"; `git2` brings libgit2's C build and lags git on extensions.

## Decision

- **Hot path, no Git library:** the per-request freshness check stats `$GIT_DIR/HEAD` and
  `$GIT_DIR/index` and reads the few small files it needs directly (`.git` file's `gitdir:`,
  `commondir`, the first line of `HEAD` or a ref, `packed-refs`).
- **Reconcile path, the `git` CLI through `jfl-process`:** `git status -z --porcelain=v2`,
  `git diff --name-only -z <tree> <tree>`, `git worktree list --porcelain -z`,
  `git rev-parse --git-common-dir`; output parsed defensively and fuzzed. It is always right for
  the user's git version and configuration, at about 2–5 ms per spawn.
- Revisit `gix` (its `status`, `dirwalk` and `blob-diff` features, or the near-stable
  `gix-ref`/`gix-discover` sub-crates for the hot path) if measurements show spawn cost.

## Consequences

`jfl-adapter-git` depends on `jfl-process` (already legal, plan §6.3) and on no Git crate. Plan
§14's "git CLI or gix" row is closed. IDX-05 measures spawn cost on the fixtures.

## Alternatives considered

- `gix`: pure Rust and fast, but the linked-worktree gap hits our main case.
- `git2`: a C build for capability the CLI already gives.

## Amended 2026-11-18 (IDX-05): no user or system configuration, no configured commands; the numbers

"Always right for the user's git version and configuration" no longer holds for configuration.
The indexed repository is untrusted input (plan §11), its `.git/config` included: a repository
unpacked from an archive, copied, synced or mounted belongs to the user, so Git's ownership check
passes it, though its configuration came from elsewhere. So `jfl-adapter-git` runs `git` with:

- `GIT_CONFIG_NOSYSTEM=1`, `GIT_CONFIG_GLOBAL=/dev/null`, `core.excludesFile=/dev/null` and
  `core.attributesFile=/dev/null`: no system or user configuration, ignore or attributes file. A
  user's global `safe.directory` entries therefore no longer apply: a repository owned by another
  user that the user had allowed globally is refused (`NotAWorktree`), and jfl does not index it.
- no command the repository's configuration names: `core.fsmonitor=false`,
  `core.hooksPath=/dev/null`, every filter driver listed with `git config --get-regexp` before a
  `diff` or `status` and passed with empty `clean`, `smudge` and `process` and `required=false`
  (a `status` that rehashes a file would otherwise run its filter), `--no-ext-diff` and
  `--no-textconv`, `protocol.allow=never` and `GIT_NO_LAZY_FETCH=1`, and
  `safe.bareRepository=explicit`. No command run merges, so `merge.<driver>.driver` is not
  reachable.
- `GIT_OPTIONAL_LOCKS=0` (a status never rewrites the index the stat set compares) and
  `GIT_NO_REPLACE_OBJECTS=1` (replace refs, which move no file of the stat set, cannot change the
  tree a view diffs from).

The repository's own configuration is still read for everything else. For indexing this is the
right answer too: `status` compares bytes on disk without a user's `core.autocrlf`, and reports
untracked files no global ignore hides, the same set `jfl-adapter-fs`'s walk lists (it reads no
global ignore either). The module doc of `jfl-adapter-git::cli` lists each setting;
`tests/hardening.rs` pins them, each "is not run" test after a control showing a plain `git` runs
it.

Repositories whose refs are in the reftable format are refused (`ReftableUnsupported`) until a
pinned git lets a test pin their stat set (IDX-09, IDX-15).

Measured spawn cost (`just bench-git`, 4-vCPU VM, medians, small and medium corpus): `rev-parse`
1.7–1.8 ms, `diff --name-only` of two trees 100 files apart 2.2–3.0 ms, `ls-files -z` 3.1 and
9.3 ms, `status -z` 10.9 and 40.9 ms, the filter listing about 1.7 ms. The hot path spawns nothing:
stat set 3 µs, cached resolve 19–23 µs, and a root's confirmation (1.8–2.0 ms) happens once, not
again after a user's `git add`, `status` or commit. Within the 2–5 ms a spawn the decision assumed:
no change to it.
