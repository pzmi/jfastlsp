#!/usr/bin/env bash
# shellcheck disable=SC2016 # backticks and $PATH in single quotes are literal on purpose
# SessionStart: make a fresh Claude Code on the web container ready for the
# gate with no manual setup. Local sessions already have their tools, so the
# hook does nothing outside a remote session. It reports and never blocks the
# session: every step that fails prints what failed and how to retry, and the
# hook exits 0. Each step is a no-op while its prerequisite does not exist yet
# (no rust-toolchain.toml, no Cargo.toml, no mise.toml), so the repository
# works before FND-01 lands, and a no-op when its tool is already installed.
# It installs only what is pinned (mise and just by scripts/bootstrap-tools.sh,
# everything else from mise.lock); it never fetches Gradle, jdtls, the bench
# corpus or a target Java repository.
set -uo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}" || exit 0

# ~/.local/bin holds mise and just; make it visible to this hook and, through
# CLAUDE_ENV_FILE, to every later command of the session.
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac
if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  printf 'export PATH="%s/.local/bin:$PATH"\n' "$HOME" >> "$CLAUDE_ENV_FILE"
fi

# The pinned toolchain. rustup >= 1.28 installs the toolchain named by
# rust-toolchain.toml (with its components and targets) when given no argument.
if [ -f rust-toolchain.toml ] || [ -f rust-toolchain ]; then
  if command -v rustup >/dev/null 2>&1; then
    if log="$(rustup toolchain install 2>&1)"; then
      echo "rust: $(rustc --version 2>/dev/null) (pinned by rust-toolchain.toml)"
    else
      printf 'rust: `rustup toolchain install` failed; retry it by hand:\n%s\n' "$(printf '%s\n' "$log" | tail -n 20)"
    fi
  else
    echo "rust: rustup missing; install it from https://rustup.rs, then run \`rustup toolchain install\`"
  fi
else
  echo "rust: $(rustc --version 2>/dev/null || echo missing) (no rust-toolchain.toml yet)"
fi

# Crates, so the first build in a worktree does not wait on the network.
if [ -f Cargo.lock ] && [ -f Cargo.toml ]; then
  if log="$(cargo fetch --locked 2>&1)"; then
    echo "crates: fetched (Cargo.lock)"
  else
    printf 'crates: `cargo fetch --locked` failed:\n%s\n' "$(printf '%s\n' "$log" | tail -n 20)"
  fi
elif [ -f Cargo.toml ]; then
  echo "crates: no Cargo.lock yet; the first build will create it"
else
  echo "crates: no Cargo.toml yet; nothing to fetch"
fi

# mise and just (pinned, checksum-verified), then everything in mise.lock once
# mise.toml exists (FND-01): scripts/bootstrap-tools.sh does both and is
# idempotent (fnd §8).
if [ -f scripts/bootstrap-tools.sh ]; then
  if log="$(bash scripts/bootstrap-tools.sh 2>&1)"; then
    printf '%s\n' "$log"
  else
    printf 'tools: scripts/bootstrap-tools.sh failed (rerun it; `just doctor` names what is missing):\n%s\n' "$(printf '%s\n' "$log" | tail -n 20)"
  fi
fi

# The JDK: the javac worker tests, the Gradle model tests and Apalache need
# one at the version mise.toml pins (FORMAL-01). Report only.
if command -v java >/dev/null 2>&1; then
  echo "java: $(java -version 2>&1 | grep -v '^Picked up' | head -n 1)"
else
  echo "java: missing; only the JVM-backed stages need it (\`just doctor\` names the pin)"
fi

if [ -f Cargo.toml ]; then
  if [ -f justfile ] && grep -qsE '^check([[:space:]]|:)' justfile; then
    echo "jfastlsp ready: run \`just quick\` while working, \`just check\` before commit (see doc/tasks.md)"
  else
    echo "jfastlsp ready: the gate is the fallback in CLAUDE.md until the justfile has it (FND-02)"
  fi
else
  echo "jfastlsp: no code yet; see doc/tasks.md for the first task (FND-01)"
fi
exit 0
