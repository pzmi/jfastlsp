#!/usr/bin/env bash
# Install the two tools the gate needs before mise can install the rest: mise and just, each
# at a pinned version and verified against a SHA-256 recorded here. Idempotent: a tool already
# on PATH at the pinned version is left alone. Then, once mise.toml exists (FND-01), runs
# `mise install --locked` for everything else, once scripts/tool-config-check.sh has passed the
# tool configuration (and without ever trusting the repository for mise).
#
# Called by .claude/hooks/session-start.sh in cloud sessions, and usable as (or from) a cloud
# environment's setup script and on a fresh laptop. It never uses sudo and installs into
# ~/.local/bin (override with the first argument). Exit status: 0 when both tools are present
# at the pinned versions afterwards, 1 otherwise.
#
# Sources, in order (the first that verifies wins): the GitHub release asset, then mise's own
# mirror (mise only), then `cargo install --locked` (just only). The checksums below were
# copied from each release's SHASUMS256.txt / SHA256SUMS and checked against the downloaded
# archives on 2026-11-16. A version bump changes the version and every checksum in one commit,
# and FND-01's mise.toml pins just at the same version (`just doctor` compares them).
set -euo pipefail

MISE_VERSION="2026.10.10"
JUST_VERSION="1.59.0"

bin_dir="${1:-$HOME/.local/bin}"
mkdir -p "$bin_dir"
case ":$PATH:" in *":$bin_dir:"*) ;; *) export PATH="$bin_dir:$PATH" ;; esac

os="$(uname -s)"
arch="$(uname -m)"
case "$os/$arch" in
  Linux/x86_64)
    mise_asset="mise-v${MISE_VERSION}-linux-x64.tar.gz"
    mise_sha="58063fdda315c29ff7504daf8f2414462d6a52e4de2243c3ce23f849975a4ab5"
    just_asset="just-${JUST_VERSION}-x86_64-unknown-linux-musl.tar.gz"
    just_sha="409044617b6f06517c1035e55ca38c8c856ff8b650e41bc2aa1ef82d5903a23e" ;;
  Linux/aarch64 | Linux/arm64)
    mise_asset="mise-v${MISE_VERSION}-linux-arm64.tar.gz"
    mise_sha="b67f4ad5e35199b14362aa64d065977f8513a2046430179c2ab42262914a73b0"
    just_asset="just-${JUST_VERSION}-aarch64-unknown-linux-musl.tar.gz"
    just_sha="23871b1702bfd550fb5f901778fd4290ecf2ee034a45a889fbe96d89b188086d" ;;
  Darwin/arm64)
    mise_asset="mise-v${MISE_VERSION}-macos-arm64.tar.gz"
    mise_sha="6f99b395c8d2a4364fd5cf6a73521fb7f8444831a330db6a4c7cea9a58d69930"
    just_asset="just-${JUST_VERSION}-aarch64-apple-darwin.tar.gz"
    just_sha="6574cea774ed4cae12e24b6aa11857de33e80dab4667cf628fd5840df96a2a9d" ;;
  *)
    echo "bootstrap-tools: unsupported platform $os/$arch (ADR 0006)" >&2
    exit 1 ;;
esac

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# fetch_verified <expected sha256> <output file> <url>...: the first URL whose bytes match.
fetch_verified() {
  local want="$1" out="$2" url
  shift 2
  for url in "$@"; do
    if curl -fsSL --retry 2 -o "$out" "$url" 2>/dev/null; then
      if [ "$(sha256_of "$out")" = "$want" ]; then return 0; fi
      echo "bootstrap-tools: checksum mismatch from $url; discarded" >&2
    fi
    rm -f "$out"
  done
  return 1
}

status=0

if command -v mise >/dev/null 2>&1 && [ "$(mise --version 2>/dev/null | cut -d' ' -f1)" = "$MISE_VERSION" ]; then
  echo "mise: $MISE_VERSION already installed"
elif fetch_verified "$mise_sha" "$work/mise.tar.gz" \
    "https://github.com/jdx/mise/releases/download/v${MISE_VERSION}/${mise_asset}" \
    "https://mise.jdx.dev/v${MISE_VERSION}/${mise_asset}"; then
  tar -xzf "$work/mise.tar.gz" -C "$work"
  install -m 0755 "$work/mise/bin/mise" "$bin_dir/mise"
  echo "mise: installed $MISE_VERSION (sha256 verified) into $bin_dir"
else
  echo "bootstrap-tools: could not download a verified mise $MISE_VERSION" >&2
  status=1
fi

if command -v just >/dev/null 2>&1 && [ "$(just --version 2>/dev/null | cut -d' ' -f2)" = "$JUST_VERSION" ]; then
  echo "just: $JUST_VERSION already installed"
elif fetch_verified "$just_sha" "$work/just.tar.gz" \
    "https://github.com/casey/just/releases/download/${JUST_VERSION}/${just_asset}"; then
  mkdir -p "$work/just"
  tar -xzf "$work/just.tar.gz" -C "$work/just"
  install -m 0755 "$work/just/just" "$bin_dir/just"
  echo "just: installed $JUST_VERSION (sha256 verified) into $bin_dir"
elif command -v cargo >/dev/null 2>&1 \
    && cargo install --locked --quiet --root "$work/cargo-root" "just@${JUST_VERSION}"; then
  # crates.io verifies the crate archive against the index checksum.
  install -m 0755 "$work/cargo-root/bin/just" "$bin_dir/just"
  echo "just: built $JUST_VERSION with cargo install --locked into $bin_dir"
else
  echo "bootstrap-tools: could not install just $JUST_VERSION" >&2
  status=1
fi

# Everything else comes from mise.lock, once FND-01 has created mise.toml. The repository is left
# untrusted for mise: mise reads an untrusted mise.toml's [tools] and refuses to load an [env], a
# template or a hook from it, whereas a trusted one runs them as it loads, and a trust by path
# covers every later edit of the file (the FND-08 re-verification's F3). So the tool configuration
# passes scripts/tool-config-check.sh (bash and awk, no mise) before mise reads it, a refusal is
# reported and mise is not run, and the trust that `mise install` records by itself (mise
# 2026.10 trusts the directory it installs for; an earlier version of this script ran
# `mise trust --yes`) is removed again afterwards (FND-09).
repo_root="$(cd "$(dirname "$0")/.." && pwd)"
if [ -f "$repo_root/mise.toml" ] && command -v mise >/dev/null 2>&1; then
  if ! bash "$repo_root/scripts/tool-config-check.sh" "$repo_root"; then
    echo "bootstrap-tools: the tool configuration is outside its allowlist (above); mise was not run" >&2
    status=1
  else
    if (cd "$repo_root" && mise install --locked); then
      echo "tools: installed from mise.lock"
    else
      echo "bootstrap-tools: \`mise install --locked\` failed (run \`just doctor\`)" >&2
      status=1
    fi
    (cd "$repo_root" && mise trust --untrust >/dev/null 2>&1) || true
  fi
fi

exit "$status"
