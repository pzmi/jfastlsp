#!/usr/bin/env bash
# SessionStart: make a fresh Claude Code on the web container ready for the
# gate with no manual setup. Local sessions already have their toolchain, so
# the hook does nothing outside a remote session. Every step is a no-op while
# its prerequisite does not exist yet (no rust-toolchain.toml, no Cargo.toml,
# no Cargo.lock, no mise.toml), so the repository works before FND-01 lands.
# It installs only what mise.lock pins (the JDK once FORMAL-01 pins it); it
# never fetches Gradle, jdtls, the bench corpus or a target Java repository.
set -uo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}" || exit 0

# The pinned toolchain. rustup >= 1.28 installs the toolchain named by
# rust-toolchain.toml (with its components) when given no argument.
if [ -f rust-toolchain.toml ] || [ -f rust-toolchain ]; then
  if command -v rustup >/dev/null 2>&1; then
    if ! log="$(rustup toolchain install 2>&1)"; then
      printf 'rustup could not install the pinned toolchain:\n%s\n' "$log" >&2
      exit 1
    fi
    echo "rust: $(rustc --version 2>/dev/null) (pinned by rust-toolchain.toml)"
  else
    echo "rust: rustup missing; install it from https://rustup.rs, then run \`rustup toolchain install\`"
  fi
else
  echo "rust: $(rustc --version 2>/dev/null || echo missing) (no rust-toolchain.toml yet)"
fi

# Crates, so the first build in a worktree does not wait on the network.
if [ -f Cargo.lock ] && [ -f Cargo.toml ]; then
  if ! log="$(cargo fetch --locked 2>&1)"; then
    printf 'cargo fetch --locked failed:\n%s\n' "$(printf '%s\n' "$log" | tail -n 30)" >&2
    exit 1
  fi
  echo "crates: fetched (Cargo.lock)"
elif [ -f Cargo.toml ]; then
  echo "crates: no Cargo.lock yet; the first build will create it"
else
  echo "crates: no Cargo.toml yet; nothing to fetch"
fi

# Pinned tools (ADR 0030): mise installs everything in mise.lock once mise.toml
# exists (FND-01), Quint, Apalache and the JDK included once FORMAL-01 adds
# them. A failed install is reported, never fatal: `just doctor` names what is
# missing.
if [ -f mise.toml ]; then
  if command -v mise >/dev/null 2>&1; then
    if log="$(mise install --locked 2>&1)"; then
      echo "tools: installed from mise.lock"
    else
      printf 'tools: `mise install --locked` failed (run `just doctor`):\n%s\n' "$(printf '%s\n' "$log" | tail -n 20)"
    fi
  else
    echo "tools: mise missing; install it from https://mise.jdx.dev, then run \`mise install --locked\`"
  fi
else
  echo "tools: no mise.toml yet (FND-01); nothing pinned to install"
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
