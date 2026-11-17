#!/usr/bin/env bash
# Self-test of scripts/jdtls-fetch.sh (EVAL-11), without the network: a stand-in archive with its
# checksum rewritten into a copy of the script checks the verify-then-extract logic, not the
# download. The real fetch is `bash scripts/jdtls-fetch.sh`. Also checks that the script pins the
# version tools/jfl-jdtls/src/launch.rs expects.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
script="$here/jdtls-fetch.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

# the pinned version agrees with the crate's
pinned="$(sed -n 's/^JDTLS_VERSION="\(.*\)"$/\1/p' "$script")"
crate="$(sed -n 's/^pub const JDTLS_VERSION: &str = "\(.*\)";$/\1/p' "$here/../tools/jfl-jdtls/src/launch.rs")"
[ -n "$pinned" ] && [ "$pinned" = "$crate" ] || fail "script pins '$pinned', launch.rs '$crate'"

# a stand-in release
mkdir -p "$tmp/src/plugins" "$tmp/src/config_linux"
echo jar >"$tmp/src/plugins/org.eclipse.equinox.launcher_1.0.0.jar"
tar -czf "$tmp/good.tar.gz" -C "$tmp/src" .
good_sha="$(sha256_of "$tmp/good.tar.gz")"
mkdir -p "$tmp/bad"
echo nothing >"$tmp/bad/README"
tar -czf "$tmp/bad.tar.gz" -C "$tmp/bad" .
cp "$tmp/good.tar.gz" "$tmp/tampered.tar.gz"
echo x >>"$tmp/tampered.tar.gz"

test_script="$tmp/jdtls-fetch.sh"
sed -e "s/^JDTLS_SHA256=.*/JDTLS_SHA256=\"$good_sha\"/" "$script" >"$test_script"
dest="$tmp/install/jdtls"
err="$tmp/err"

# given a tampered archive, when fetched, then it is refused and nothing is installed
if bash "$test_script" --dest "$dest" --archive "$tmp/tampered.tar.gz" 2>"$err"; then fail "tampered archive accepted"; fi
grep -q "checksum mismatch" "$err" || fail "tampered: wrong reason"
[ ! -e "$dest" ] || fail "tampered archive left an install"

# given the pinned archive, when fetched, then it is verified and installed
bash "$test_script" --dest "$dest" --archive "$tmp/good.tar.gz" 2>"$err" || { cat "$err" >&2; fail "fetch"; }
grep -q "verified" "$err" || fail "no verification message"
[ -f "$dest/plugins/org.eclipse.equinox.launcher_1.0.0.jar" ] || fail "launcher jar not installed"

# when fetched again, then it is a cached no-op
bash "$test_script" --dest "$dest" --archive "$tmp/missing.tar.gz" 2>"$err" || fail "second fetch"
grep -q "cached" "$err" || fail "second run was not a no-op"

# given an archive whose checksum matches but which holds no launcher, then it is refused
bad_sha="$(sha256_of "$tmp/bad.tar.gz")"
sed -e "s/^JDTLS_SHA256=.*/JDTLS_SHA256=\"$bad_sha\"/" "$script" >"$tmp/bad.sh"
if bash "$tmp/bad.sh" --dest "$tmp/install/bad" --archive "$tmp/bad.tar.gz" 2>"$err"; then fail "no-launcher archive accepted"; fi
grep -q "no equinox launcher" "$err" || fail "no-launcher: wrong reason"
[ ! -e "$tmp/install/bad" ] || fail "no-launcher archive left an install"

# no work directory is left behind
[ -z "$(find "$tmp/install" -maxdepth 1 -name '.jdtls-fetch.*')" ] || fail "work directory left behind"

echo "test-jdtls-fetch: ok" >&2
