#!/usr/bin/env bash
# Build this checkout's xtask and run it with the given arguments (XT-01, ADR 0035): what the
# justfile's gate recipes, scripts/land.sh, scripts/bootstrap-tools.sh and the Stop hook call, and
# the command a person runs (`bash scripts/xtask.sh check`).
#
# The build is the one thing that runs before xtask's preflight can look, so nothing the tree, the
# caller or the machine's cargo configuration controls may steer it (fnd §3.6); a steered build
# could produce a binary that exits 0, and the gate would pass having run nothing:
#
#   - before any cargo runs, it refuses a cargo configuration file in any ancestor of the
#     directory it builds from (a launcher-owned directory under the target directory: cargo reads
#     `.cargo/config{,.toml}` there and in every ancestor, `/` included), and one in the caller's
#     CARGO_HOME outside the plain shape the preflight allows (plain_config below, the justfile's
#     `_preflight` shape before XT-01: no linker, runner, rustc, wrapper, flags, alias, source
#     replacement or profile can pass it);
#   - cargo runs with a CARGO_HOME the launcher owns (`cargo-home/` beside the build, holding only
#     links to the real registry and git caches: no configuration, credentials or binaries), a
#     cleared environment plus an allowlist (where rustup and the network are), RUSTFLAGS
#     `-D warnings`, empty wrappers, RUSTC and RUSTDOC set to the pinned toolchain's own binaries
#     (`rustup which`, beating any `build.rustc`) and the toolchain rust-toolchain.toml pins;
#   - it builds with `cargo build`, never `cargo run`, so no configured runner starts it, and then
#     runs the binary with the caller's environment unchanged, which xtask's preflight judges.
#
# It builds into xtask/<SHA-256 of this checkout's path>/ under CARGO_TARGET_DIR (made absolute)
# or ROOT/target: one directory per checkout. cargo leaves `debug/xtask` alone when its own unit
# is fresh, so two checkouts sharing a target directory (scripts/land.sh passes it to its worktree)
# would otherwise run whichever xtask was linked last, another tree's code judging this one. xtask
# runs in the caller's working directory, with the root it was built from.
#
# Not closed here (the XT-02 card): the `cargo` and `rustup` found on PATH, and a binary that
# code the gate runs could overwrite in a shared target directory (the gate is not a sandbox).
#
# Usage: scripts/xtask.sh <xtask arguments>     (`bash scripts/xtask.sh list` names the stages)
# Exit: xtask's, or 1 when the build is refused or fails.
#
# Pinned by: tools/xtask/tests/launcher.rs.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
die() { echo "xtask.sh: $*" >&2; exit 1; }

channel="$(sed -n 's/^channel[[:space:]]*=[[:space:]]*"\(.*\)"[[:space:]]*$/\1/p' "$root/rust-toolchain.toml")"
[[ "$channel" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "$root/rust-toolchain.toml pins no plain channel (x.y.z)"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum | cut -d' ' -f1
  else shasum -a 256 | cut -d' ' -f1; fi
}
target="${CARGO_TARGET_DIR:-$root/target}"
case "$target" in /*) ;; *) target="$PWD/$target" ;; esac
target="$target/xtask/$(printf '%s' "$root" | sha256_of)"
build_dir="$target/build"
home="$target/cargo-home"
mkdir -p "$build_dir" "$home"

# A cargo configuration file outside the repository passes only when every line is a comment, a
# blank, one of the headers [net], [http], [term], [build] or [registries.crates-io], or
# `key = value` with a bare key that table allows and a plain value (the shape is pinned, no bad
# key is listed; the same rule as plain_config in tools/xtask/src/preflight.rs).
plain_config() {
  awk '
    function allowed(table, key) {
      if (table == "net") return key ~ /^(retry|git-fetch-with-cli|offline)$/
      if (table == "http") return key ~ /^(proxy|timeout|cainfo|check-revoke|low-speed-limit|multiplexing|user-agent)$/
      if (table == "term") return key ~ /^(quiet|verbose|color)$/
      if (table == "build") return key ~ /^(jobs|target-dir)$/
      if (table == "registries.crates-io") return key == "protocol"
      return 0
    }
    /^[[:space:]]*(#.*)?$/ { next }
    /^\[(net|http|term|build|registries\.crates-io)\][[:space:]]*(#.*)?$/ {
      table = $0; sub(/^\[/, "", table); sub(/\].*$/, "", table); next
    }
    /^[a-z][a-z0-9-]*[[:space:]]*=[[:space:]]*("[^"\\]*"|true|false|[0-9]+)[[:space:]]*(#.*)?$/ {
      key = $0; sub(/[[:space:]]*=.*$/, "", key)
      if (allowed(table, key)) next
    }
    { bad = 1; exit }
    END { exit bad }' "$1"
}

refused=()
real_home="${CARGO_HOME:-$HOME/.cargo}"
for file in "$real_home/config" "$real_home/config.toml"; do
  if [ -e "$file" ] && ! plain_config "$file"; then refused+=("$file"); fi
done
dir="$(cd "$build_dir" && pwd -P)"
while :; do
  for file in "$dir/.cargo/config" "$dir/.cargo/config.toml"; do
    [ -e "$file" ] || continue
    # The caller's CARGO_HOME may also be an ancestor (~/.cargo); it was judged above.
    [ -e "$real_home" ] && [ "$dir/.cargo" -ef "$real_home" ] && continue
    refused+=("$file")
  done
  [ "$dir" = / ] && break
  dir="$(dirname "$dir")"
done
for entry in "$home"/* "$home"/.[!.]*; do
  [ -e "$entry" ] || [ -L "$entry" ] || continue
  case "${entry##*/}" in
    registry | git | .package-cache | .package-cache-mutate | .global-cache) ;;
    *) refused+=("$entry") ;;
  esac
done
if [ "${#refused[@]}" -gt 0 ]; then
  die "refusing to build xtask: ${refused[*]} could steer the build of the gate's own binary (fnd §3.6); remove it, or keep CARGO_HOME's configuration to the plain shape"
fi

mkdir -p "$real_home/registry" "$real_home/git"
for cache in registry git; do
  [ "$(readlink "$home/$cache" 2>/dev/null || true)" = "$real_home/$cache" ] ||
    ln -sfn "$real_home/$cache" "$home/$cache"
done

rustc="$(rustup which --toolchain "$channel" rustc)" || die "rustup has no toolchain $channel: run \`rustup toolchain install\`"
rustdoc="$(rustup which --toolchain "$channel" rustdoc)" || die "rustup has no rustdoc in toolchain $channel"

kept=()
while IFS= read -r name; do
  case "$name" in
    PATH | HOME | USER | TMPDIR | LANG | LC_ALL | TERM | RUSTUP_HOME | CARGO_INCREMENTAL | \
      CARGO_NET_* | CARGO_HTTP_* | CARGO_REGISTRIES_CRATES_IO_PROTOCOL | RUSTUP_DIST_SERVER | \
      RUSTUP_UPDATE_ROOT | RUSTUP_MAX_RETRIES | RUSTUP_IO_THREADS | HTTP_PROXY | HTTPS_PROXY | \
      NO_PROXY | http_proxy | https_proxy | no_proxy | SSL_CERT_FILE | SSL_CERT_DIR)
      kept+=("$name=${!name}") ;;
  esac
done < <(compgen -e)

if ! (cd "$build_dir" && env -i ${kept[@]+"${kept[@]}"} CARGO_HOME="$home" \
  RUSTUP_TOOLCHAIN="$channel" RUSTC="$rustc" RUSTDOC="$rustdoc" CARGO_TARGET_DIR="$target" \
  RUSTFLAGS='-D warnings' RUSTC_WRAPPER= RUSTC_WORKSPACE_WRAPPER= \
  cargo build --locked --quiet --manifest-path "$root/Cargo.toml" -p xtask); then
  die "cannot build xtask from $root (above)"
fi
exec "$target/debug/xtask" "$@"
