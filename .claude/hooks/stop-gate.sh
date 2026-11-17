#!/usr/bin/env bash
# Stop: refuse to finish a turn that leaves the fast gate red in the checkout
# the session started in (for the orchestrator, the main checkout). Lane
# worktrees are guarded by the work skill's ship station, which runs the full
# gate on the exact tree it commits and records its exit code, and by the gate
# on the merged tree when landing; this hook is a backstop, not that gate.
#
# Runs only when Rust code, manifests, the lockfile, the architecture policy,
# `deny.toml`, a `clippy.toml`, the justfile, the tool pins, a Quint model, a
# golden Java fixture or an LSP transcript changed; a clean tree or a
# docs-only change stops at once, and so does a repository with no Cargo.toml
# yet (before FND-01) or a machine without cargo or git. Once the justfile has
# `quick` (FND-02), a missing `just` is a failure with the install command,
# never a silent fall back to the cargo-only gate. `stop_hook_active` is true
# when Claude is already continuing because of this hook, so a gate that
# cannot go green never loops.
set -uo pipefail

payload="$(cat)"
if command -v jq >/dev/null 2>&1; then
  active="$(printf '%s' "$payload" | jq -r '.stop_hook_active // false' 2>/dev/null)"
elif command -v python3 >/dev/null 2>&1; then
  active="$(printf '%s' "$payload" | python3 -c 'import json,sys; print(str(json.load(sys.stdin).get("stop_hook_active", False)).lower())' 2>/dev/null)"
else
  exit 0
fi
[ "$active" = "true" ] && exit 0

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}" || exit 0
[ -f Cargo.toml ] || exit 0
command -v cargo >/dev/null 2>&1 || exit 0
command -v git >/dev/null 2>&1 || exit 0
git rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0

changed="$(git status --porcelain --untracked-files=all 2>/dev/null | awk '{print $NF}' \
  | grep -E '\.rs$|(^|/)Cargo\.toml$|^Cargo\.lock$|^architecture\.toml$|^deny\.toml$|(^|/)clippy\.toml$|^justfile$|^mise\.(toml|lock)$|\.qnt$|^rust-toolchain(\.toml)?$|^\.cargo/config\.toml$|^fixtures/|\.java$' || true)"
[ -z "$changed" ] && exit 0

# The sandbox runs at most two lanes beside this checkout (CLAUDE.md, Sandbox).
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac

if [ -f justfile ] && grep -qsE '^quick([[:space:]]|:)' justfile; then
  if ! command -v just >/dev/null 2>&1; then
    echo "the gate is \`just quick\` but \`just\` is not installed: run \`bash scripts/bootstrap-tools.sh\`, then the gate" >&2
    exit 2
  fi
  gate="just quick"
  run() { just quick; }
else
  # Before FND-02: the fallback gate of CLAUDE.md.
  gate="cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked"
  run() {
    cargo fmt --all --check \
      && cargo clippy --workspace --all-targets --locked -- -D warnings \
      && cargo test --workspace --locked
  }
fi

if ! output="$(run 2>&1)"; then
  echo "\`$gate\` is red with uncommitted changes; fix it before stopping:" >&2
  printf '%s\n' "$output" | tail -n 60 >&2
  exit 2
fi
exit 0
