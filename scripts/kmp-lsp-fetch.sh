#!/usr/bin/env bash
# Fetch and build the pinned kmp-lsp, the BENCH-03 comparison arm (ADR 0031, plan §1.7): a
# tree-sitter Java/Kotlin language server in Rust (MIT), measured beside jfastlsp, never linked
# or vendored. Clones the pinned commit shallow into a temporary directory, checks the commit,
# builds it with its own lock file and release profile, and moves the binary into place, so a
# cached install is always the pinned one.
#
#   scripts/kmp-lsp-fetch.sh [--dest DIR]
#
# --dest  where to install (default: ${XDG_CACHE_HOME:-$HOME/.cache}/jfastlsp/arms/kmp-lsp-<version>,
#         which is where `jfl-bench` looks when --kmp-lsp is not given); the binary is DIR/bin/kmp-lsp
#
# The build runs the project's own build scripts and proc macros (tree-sitter grammars), which is
# why it is a separate step and never part of the gate. It is contained: `cargo fetch --locked`
# downloads the locked crates with only the proxy and certificate variables, then the build runs
# `--offline` under `env -i` with PATH, HOME, CARGO_HOME, RUSTUP_HOME and RUSTUP_TOOLCHAIN only (no
# tokens, agent sockets or credentials), on the toolchain named here even if upstream adds a
# rust-toolchain file. `.verified` records the commit, the toolchain and the binary's sha256.
# No Java build runs.
set -euo pipefail

KMP_LSP_VERSION="0.27.0"
KMP_LSP_COMMIT="cd086c8fa363e442b88223ec8c23fa39ea41d609"
KMP_LSP_URL="https://github.com/Hessesian/kmp-lsp.git"

die() { echo "kmp-lsp-fetch: $*" >&2; exit 1; }

dest=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dest) [ "$#" -ge 2 ] || die "--dest needs a value"; dest="$2"; shift 2 ;;
    -h | --help) sed -n '2,14p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
if [ -z "$dest" ]; then
  cache="${XDG_CACHE_HOME:-}"
  [ -n "$cache" ] || cache="${HOME:?HOME is not set}/.cache"
  dest="$cache/jfastlsp/arms/kmp-lsp-$KMP_LSP_VERSION"
fi

if [ -x "$dest/bin/kmp-lsp" ] && [ "$(sed -n 's/^commit //p' "$dest/.verified" 2>/dev/null)" = "$KMP_LSP_COMMIT" ] &&
  [ "$(sed -n 's/^sha256 //p' "$dest/.verified")" = "$(sha256sum "$dest/bin/kmp-lsp" | cut -d' ' -f1)" ]; then
  echo "kmp-lsp-fetch: cached $KMP_LSP_VERSION at $dest" >&2
  exit 0
fi
command -v cargo >/dev/null 2>&1 || die "cargo is not on PATH"
command -v rustup >/dev/null 2>&1 || die "rustup is not on PATH"
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"

parent="$(dirname "$dest")"
mkdir -p "$parent"
work="$(mktemp -d "$parent/.kmp-lsp-fetch.XXXXXX")"
trap 'rm -rf "$work"' EXIT

echo "kmp-lsp-fetch: fetching $KMP_LSP_COMMIT ($KMP_LSP_VERSION)" >&2
git init -q "$work/src"
git -C "$work/src" remote add origin "$KMP_LSP_URL"
GIT_TERMINAL_PROMPT=0 git -C "$work/src" fetch -q --depth 1 --no-tags origin "$KMP_LSP_COMMIT"
git -C "$work/src" -c advice.detachedHead=false checkout -q --detach FETCH_HEAD
[ "$(git -C "$work/src" rev-parse HEAD)" = "$KMP_LSP_COMMIT" ] || die "fetched the wrong commit"
grep -q "^version = \"$KMP_LSP_VERSION\"" "$work/src/Cargo.toml" ||
  die "the commit is not version $KMP_LSP_VERSION"

# The default toolchain, named outside the clone so no upstream rust-toolchain file chooses it.
toolchain="$(cd "$work" && rustup show active-toolchain | cut -d' ' -f1)"
[ -n "$toolchain" ] || die "no active rustup toolchain"
keep=(PATH="$PATH" HOME="$HOME" CARGO_HOME="$cargo_home" RUSTUP_HOME="$rustup_home" RUSTUP_TOOLCHAIN="$toolchain")
net=()
for v in HTTPS_PROXY https_proxy HTTP_PROXY http_proxy NO_PROXY no_proxy SSL_CERT_FILE SSL_CERT_DIR CARGO_HTTP_CAINFO; do
  if [ -n "${!v:-}" ]; then net+=("$v=${!v}"); fi
done
echo "kmp-lsp-fetch: fetching the locked crates" >&2
(cd "$work/src" && env -i "${keep[@]}" "${net[@]}" cargo fetch --locked)
echo "kmp-lsp-fetch: building offline with $toolchain (cargo build --locked --offline --release)" >&2
(cd "$work/src" && env -i "${keep[@]}" CARGO_TARGET_DIR="$work/target" \
  cargo build --locked --offline --release --bin kmp-lsp)

mkdir -p "$work/out/bin"
cp "$work/target/release/kmp-lsp" "$work/out/bin/kmp-lsp"
{
  echo "commit $KMP_LSP_COMMIT"
  echo "toolchain $(env -i "${keep[@]}" rustc --version)"
  echo "sha256 $(sha256sum "$work/out/bin/kmp-lsp" | cut -d' ' -f1)"
} >"$work/out/.verified"
rm -rf "$dest"
mv "$work/out" "$dest"
echo "kmp-lsp-fetch: built $KMP_LSP_VERSION at $dest/bin/kmp-lsp" >&2
