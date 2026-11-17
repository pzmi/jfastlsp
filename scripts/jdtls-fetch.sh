#!/usr/bin/env bash
# Fetch the pinned Eclipse JDT Language Server for the bench's jdtls arm (BENCH-03, ADR 0028
# amended 2026-11-17: stock jdtls, a comparison arm only, wrapped by nothing): download from
# the official milestone site, verify the pinned SHA-256 before extracting anything, extract into
# a temporary directory and move it into place, so a cached install is always a verified one.
# jdtls is EPL-2.0: it is run as a separate process, never vendored or linked.
#
#   scripts/jdtls-fetch.sh [--dest DIR] [--archive FILE]
#
# --dest     where to install (default: ${XDG_CACHE_HOME:-$HOME/.cache}/jfastlsp/arms/jdtls-<version>,
#            which is where `jfl-bench` looks when --jdtls-home is not given)
# --archive  verify and extract this file instead of downloading (offline use, the self-test)
#
# The version and digest below must match JDTLS_VERSION and JDTLS_SHA256 in
# tools/jfl-bench/src/jdtls.rs, which refuses an install whose .verified is another digest
# (scripts/test-jdtls-fetch.sh checks both). 1.61.0 is the newest release that runs on Java 21:
# from 1.62.0 the Eclipse platform bundles require JavaSE-25.
set -euo pipefail

JDTLS_VERSION="1.61.0"
JDTLS_BUILD="202609031315"
JDTLS_SHA256="338e7e73d61836651ba2453919a0d34fa763eb4e7c03342092309bffb8934c64"
JDTLS_FILE="jdt-language-server-${JDTLS_VERSION}-${JDTLS_BUILD}.tar.gz"
JDTLS_URL="https://download.eclipse.org/jdtls/milestones/${JDTLS_VERSION}/${JDTLS_FILE}"

die() { echo "jdtls-fetch: $*" >&2; exit 1; }

dest=""
archive=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dest) [ "$#" -ge 2 ] || die "--dest needs a value"; dest="$2"; shift 2 ;;
    --archive) [ "$#" -ge 2 ] || die "--archive needs a value"; archive="$2"; shift 2 ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done
if [ -z "$dest" ]; then
  cache="${XDG_CACHE_HOME:-}"
  [ -n "$cache" ] || cache="${HOME:?HOME is not set}/.cache"
  dest="$cache/jfastlsp/arms/jdtls-$JDTLS_VERSION"
fi

if [ -f "$dest/.verified" ] && [ "$(cat "$dest/.verified")" = "$JDTLS_SHA256" ]; then
  echo "jdtls-fetch: cached $JDTLS_VERSION at $dest" >&2
  exit 0
fi

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

parent="$(dirname "$dest")"
mkdir -p "$parent"
work="$(mktemp -d "$parent/.jdtls-fetch.XXXXXX")"
trap 'rm -rf "$work"' EXIT

if [ -z "$archive" ]; then
  echo "jdtls-fetch: fetching $JDTLS_URL" >&2
  curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
    --output "$work/$JDTLS_FILE" "$JDTLS_URL"
  archive="$work/$JDTLS_FILE"
fi
[ -f "$archive" ] || die "no archive at $archive"

actual="$(sha256_of "$archive")"
[ "$actual" = "$JDTLS_SHA256" ] || die "checksum mismatch: expected $JDTLS_SHA256, got $actual"

mkdir "$work/x"
tar --no-same-owner -xzf "$archive" -C "$work/x"
compgen -G "$work/x/plugins/org.eclipse.equinox.launcher_*.jar" >/dev/null ||
  die "the archive holds no equinox launcher jar"
echo "$JDTLS_SHA256" >"$work/x/.verified"
rm -rf "$dest"
mv "$work/x" "$dest"
echo "jdtls-fetch: verified $JDTLS_VERSION at $dest" >&2
