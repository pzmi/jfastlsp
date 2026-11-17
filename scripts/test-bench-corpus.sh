#!/usr/bin/env bash
# Self-test of scripts/bench-corpus.sh (BENCH-01), without the network: a local source repository
# stands in for GitHub, so it checks the script's logic, not GitHub. The real fetch of the small
# repository is the owner-visible `just bench-corpus small`. Needs only git.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
script="$here/bench-corpus.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

"$script" check-doc

# a stand-in for the small repository: two commits, so the pin is not the only commit; the pin
# is rewritten into a copy of the script so the real table is untouched
src="$tmp/src/junit-framework"
mkdir -p "$src"
git -C "$src" init -q
git -C "$src" config user.email t@example.invalid
git -C "$src" config user.name t
git -C "$src" config commit.gpgsign false
echo 'class A {}' >"$src/A.java"
git -C "$src" add A.java
git -C "$src" commit -q -m one
echo 'class B {}' >"$src/B.java"
git -C "$src" add B.java
git -C "$src" commit -q -m two
sha="$(git -C "$src" rev-parse HEAD)"
git -C "$src" config uploadpack.allowAnySHA1InWant true

test_script="$tmp/repo/scripts/bench-corpus.sh"
mkdir -p "$tmp/repo/scripts"
sed -e "s#e5198f9b098c550e66d378650f78bac8883e2a72|1813#$sha|2#" "$script" >"$test_script"
chmod +x "$test_script"
cache="$tmp/cache"
co="$cache/junit-framework"
err="$tmp/err"

# run_fail DESCRIPTION PATTERN CMD...: the command must fail and its stderr must match PATTERN
run_fail() {
  local what="$1" pat="$2"
  shift 2
  if "$@" 2>"$err"; then fail "$what passed"; fi
  grep -q "$pat" "$err" || { cat "$err" >&2; fail "$what: wrong reason (wanted '$pat')"; }
}
fetch() { "$test_script" fetch small --cache "$cache" --local-source "$tmp/src"; }

# given an empty cache, when fetched, then it is verified and the tree is clean
fetch 2>"$err" || fail "fetch"
grep -q "verified at $sha" "$err" || fail "no verification message"
[ -z "$(git -C "$co" status --porcelain)" ] || fail "dirty after fetch"

# when fetched again, then it is a no-op (cached, nothing fetched)
fetch 2>"$err" || fail "second fetch"
grep -q "cached at $sha" "$err" || fail "second run was not a no-op"
if grep -q "fetching" "$err"; then fail "second run fetched"; fi

# when 10 worktrees are requested, then 10 exist at the pin, and the checkout stays clean
"$test_script" worktrees small 10 --cache "$cache" 2>"$err"
grep -q "10 linked worktrees" "$err" || fail "wrong worktree count reported"
[ "$(git -C "$co" worktree list | wc -l | tr -d ' ')" = 11 ] || fail "expected 10 linked worktrees"
[ -z "$(git -C "$co" status --porcelain)" ] || fail "worktrees dirty the checkout"
"$test_script" worktrees small 10 --cache "$cache" 2>/dev/null || fail "worktrees not idempotent"
"$test_script" clean-worktrees small --cache "$cache" 2>/dev/null
[ "$(git -C "$co" worktree list | wc -l | tr -d ' ')" = 1 ] || fail "worktrees not removed"

# given a dirty tree on a cached checkout, when fetched, then verification fails on it
echo stray >"$co/stray.txt"
run_fail "dirty tree" "dirty working tree" fetch
rm "$co/stray.txt"

# given a tracked file added to a cached checkout, when fetched, then the count check fails
echo 'class C {}' >"$co/C.java"
git -C "$co" add C.java
run_fail "wrong count on a cached checkout" "has 3 .java files" fetch
git -C "$co" rm -q -f C.java

# given a checkout at another commit with the same count, when fetched, then the pin check fails
git -C "$co" -c advice.detachedHead=false checkout -q --detach HEAD
git -C "$co" -c user.email=t@example.invalid -c user.name=t -c commit.gpgsign=false \
  commit -q --allow-empty -m other
run_fail "wrong commit" "the pin is $sha" fetch

# given a wrong expected count, when fetched into a fresh cache, then it fails and leaves nothing
bad_script="$tmp/repo/scripts/bench-corpus-bad.sh"
sed -e "s#|$sha|2|#|$sha|3|#" "$test_script" >"$bad_script"
chmod +x "$bad_script"
run_fail "wrong count" "has 2 .java files, doc/bench-corpus.md says 3" \
  "$bad_script" fetch small --cache "$tmp/cache2" --local-source "$tmp/src"
[ ! -e "$tmp/cache2/junit-framework" ] || fail "partial checkout left behind"
[ -z "$(find "$tmp/cache2" -mindepth 1 -maxdepth 1)" ] || fail "partial directory left behind"

# a cache inside the repository is refused before anything is created or fetched
run_fail "in-repo cache" "inside the repository" \
  "$script" fetch small --cache "$here/../corpus-cache" --local-source "$tmp/src"
[ ! -e "$here/../corpus-cache" ] || fail "refused cache directory was created"
echo "ok"
