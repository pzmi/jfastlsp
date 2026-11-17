#!/usr/bin/env bash
# Self-test of scripts/private-names-check.sh and scripts/private-name-add.sh (FND-04). Scratch git
# repositories in a tempdir and a hash file with its own salt and a synthetic name; no real private
# name is ever used or written. Needs git, perl and sha256sum or shasum.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
check="$here/private-names-check.sh"
add="$here/private-name-add.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

hashes="$tmp/hashes.sha256"
printf '%s\n' '# salt: 00112233445566778899aabbccddeeff' >"$hashes"
"$add" --file "$hashes" "Foo Bar" >/dev/null
[ "$(grep -cvE '^(#|$)' "$hashes")" -eq 1 ] || fail "the add script did not write one hash"
"$add" --file "$hashes" "foo-bar" | grep -q "already listed" || fail "foo-bar is not the same entry as Foo Bar"
"$add" --file "$hashes" "ab" 2>/dev/null && fail "a 2-character name was accepted"

repo="$tmp/repo"
git init -q "$repo"
git -C "$repo" config user.email t@example.invalid
git -C "$repo" config user.name t
git -C "$repo" config commit.gpgsign false
git -C "$repo" checkout -q -b main
commit() { git -C "$repo" add -A && git -C "$repo" commit -q -m "$1"; }

# a name that is already in the base commit is outside the diff
printf 'old: foo bar was here\n' >"$repo/old.txt"
commit "base"
git -C "$repo" update-ref refs/remotes/origin/main HEAD
base_sha="$(git -C "$repo" rev-parse HEAD)"
git -C "$repo" checkout -q -b lane

run() { (cd "$repo" && env -u CI bash "$check" --hashes "$hashes" "$@") 2>"$tmp/err" >"$tmp/out"; }
expect_clean() { run || { cat "$tmp/err" >&2; fail "$what: expected a clean run"; }; }
expect_hit() {
  # $1: the text that must be in the report; the name never may be
  if run; then fail "$what: expected a match"; fi
  grep -q "$1" "$tmp/err" || { cat "$tmp/err" >&2; fail "$what: report does not say '$1'"; }
  if grep -qiE 'foo|bar' "$tmp/err" "$tmp/out"; then fail "$what: the name was printed"; fi
}

what="a clean change"
printf 'nothing to see\nhere\n' >"$repo/new.txt"
commit "clean change"
expect_clean

what="the diff base keeps an old name out of scope"
expect_clean

what="--all reads the whole tree"
if run --all; then fail "$what: expected the old name to be found"; fi
grep -q 'old.txt:1' "$tmp/err" || fail "$what: old.txt:1 not reported"

what="Foo Bar, foo-bar and FooBar in committed lines"
printf 'ok\nthe Foo Bar team\nsee foo-bar\nclass FooBar {}\nlet fooBarService = 1;\nFOO_BAR\nfine\n' >"$repo/new.txt"
commit "add lines"
expect_hit 'new.txt:2'
for line in 2 3 4 5 6; do grep -q "new.txt:$line " "$tmp/err" || fail "$what: line $line not reported"; done
grep -q 'new.txt:1 \|new.txt:7 ' "$tmp/err" && fail "$what: a clean line was reported"
git -C "$repo" reset -q --hard HEAD~1

what="a changed line in the working tree"
printf 'ok\nFoo Bar\n' >"$repo/new.txt"
expect_hit 'new.txt:2'
git -C "$repo" checkout -q -- new.txt

what="an untracked file"
printf 'x\n\n\nfoo bar\n' >"$repo/untracked.txt"
expect_hit 'untracked.txt:4'
rm "$repo/untracked.txt"

what="a binary untracked file is skipped"
printf 'foo bar\0\n' >"$repo/blob.bin"
expect_clean
rm "$repo/blob.bin"

what="a line that starts with ++ keeps its path"
printf '++ foo bar\n' >"$repo/inc.txt"
commit "plus plus"
expect_hit 'inc.txt:1'
git -C "$repo" reset -q --hard HEAD~1

what="a commit message"
git -C "$repo" commit -q --allow-empty -m "subject" -m "the Foo Bar note"
expect_hit 'message line 3'
grep -q 'commit [0-9a-f]\{4,\}' "$tmp/err" || fail "$what: no commit named"
git -C "$repo" reset -q --hard HEAD~1

what="no origin/main falls back to everything"
git -C "$repo" update-ref -d refs/remotes/origin/main
if run; then fail "$what: expected the old name in history to be found"; fi
grep -q 'scanning every file' "$tmp/err" || fail "$what: no fallback notice"
git -C "$repo" update-ref refs/remotes/origin/main "$base_sha"

what="a push to main under CI=true (base is HEAD) scans everything"
git -C "$repo" update-ref refs/remotes/origin/main HEAD
expect_clean
if (cd "$repo" && env CI=true bash "$check" --hashes "$hashes") 2>"$tmp/err"; then fail "$what: expected the old name to be found"; fi
grep -q 'the diff base is HEAD under CI=true' "$tmp/err" || fail "$what: no fallback notice"
grep -q 'old.txt:1' "$tmp/err" || fail "$what: old.txt:1 not reported"
git -C "$repo" update-ref refs/remotes/origin/main "$base_sha"

what="an all-zero --base under CI=true scans everything; outside CI it is refused"
zero=0000000000000000000000000000000000000000
if (cd "$repo" && env CI=true bash "$check" --hashes "$hashes" --base "$zero") 2>"$tmp/err"; then fail "$what: expected the old name to be found"; fi
grep -q 'all-zero commit' "$tmp/err" || fail "$what: no fallback notice"
if run --base "$zero"; then fail "$what: accepted outside CI"; fi
grep -q 'is not a commit' "$tmp/err" || fail "$what: wrong reason outside CI"

what="a removed line is not a finding"
git -C "$repo" rm -q old.txt
commit "remove the old name"
expect_clean

what="a missing hash file under CI=true"
rm -f "$tmp/none"
if (cd "$repo" && env CI=true bash "$check" --hashes "$tmp/none") 2>"$tmp/err"; then fail "$what: passed"; fi
grep -q 'fails closed' "$tmp/err" || fail "$what: wrong reason"

what="a header-only hash file under CI=true"
printf '# salt: 00112233445566778899aabbccddeeff\n' >"$tmp/empty"
if (cd "$repo" && env CI=true bash "$check" --hashes "$tmp/empty") 2>"$tmp/err"; then fail "$what: passed"; fi

what="a missing hash file outside CI only warns"
(cd "$repo" && env -u CI bash "$check" --hashes "$tmp/none") 2>"$tmp/err" || fail "$what: failed"
grep -q 'warning' "$tmp/err" || fail "$what: no warning"

what="a malformed hash line"
printf '# salt: 00112233445566778899aabbccddeeff\nnot-a-digest\n' >"$tmp/bad"
if (cd "$repo" && env -u CI bash "$check" --hashes "$tmp/bad") 2>"$tmp/err"; then fail "$what: passed"; fi
grep -q 'not a SHA-256' "$tmp/err" || fail "$what: wrong reason"

# ---- the review's bypasses, each with an invented name and its own scratch repository ----
h2="$tmp/hashes2.sha256"
printf '%s\n' '# salt: ffeeddccbbaa99887766554433221100' >"$h2"
"$add" --file "$h2" "Quill Marrow" >/dev/null
nfc="$(printf 'Qu\xc3\xadll M\xc3\xa1rrow')"
"$add" --file "$h2" "$nfc" >/dev/null
"$add" --file "$h2" "$(printf '\xc5\x81ukasz \xc3\x98rn\xc3\xa6s')" >/dev/null
what="a name of more than 3 words"
if "$add" --file "$h2" "alpha beta gamma delta" >/dev/null 2>"$tmp/err"; then fail "$what: accepted"; fi
grep -q 'more than 3 words' "$tmp/err" || fail "$what: wrong reason"

fresh() { # NAME: a repository with a base commit as origin/main on branch lane
  local r="$tmp/$1"
  git init -q "$r"
  git -C "$r" config user.email t@example.invalid
  git -C "$r" config user.name t
  git -C "$r" config commit.gpgsign false
  git -C "$r" checkout -q -b main
  printf 'base\n' >"$r/base.txt"
  git -C "$r" add -A && git -C "$r" commit -q -m base
  git -C "$r" update-ref refs/remotes/origin/main HEAD
  git -C "$r" checkout -q -b lane
  repo2="$r"
}
run2() { (cd "$repo2" && env -u CI bash "$check" --hashes "$h2") 2>"$tmp/err" >"$tmp/out"; }
hit2() {
  if run2; then fail "$what: expected a match"; fi
  grep -q "$1" "$tmp/err" || { cat "$tmp/err" >&2; fail "$what: report does not say '$1'"; }
  if grep -qiE "quill|marrow|lukasz|rnaes" "$tmp/err" "$tmp/out"; then cat "$tmp/err" >&2; fail "$what: the name was printed"; fi
}

fresh fold
nfd="$(printf 'Qui\xcc\x81ll Ma\xcc\x81rrow')"
printf 'a\n%s\nb\n%s\nc\nQuill Marrow\n%s\nLukasz Ornaes\n' "$nfc" "$nfd" "$(printf '\xc5\x81ukasz \xc3\x98rn\xc3\xa6s')" >"$repo2/people.txt"
git -C "$repo2" add -A && git -C "$repo2" commit -q -m people
what="NFC, NFD, ASCII and mapped-letter spellings of an accented name"
hit2 'people.txt'
for line in 2 4 6 7 8; do grep -q "people.txt:$line " "$tmp/err" || fail "$what: line $line not reported"; done

fresh stray
printf 'ok \xff stray then %s\n' "$nfc" >"$repo2/stray.txt"
what="an accented name on a line with an invalid UTF-8 byte"
hit2 'stray.txt:1'

fresh paths
mkdir "$repo2/docs"
printf 'clean\n' >"$repo2/docs/quill-marrow.md"
what="a committed file path"
git -C "$repo2" add -A && git -C "$repo2" commit -q -m docs
hit2 'name of docs/<name>'
what="an untracked file path"
git -C "$repo2" reset -q --hard HEAD~1
printf 'clean\n' >"$repo2/QuillMarrow.txt"
hit2 'name of <name>'
rm "$repo2/QuillMarrow.txt"
mkdir -p "$repo2/quill/marrow"
printf 'clean\n' >"$repo2/quill/marrow/x.txt"
what="a name split across directories"
hit2 'withheld'

fresh utf16
printf 'the Quill Marrow note\n' | iconv -f UTF-8 -t UTF-16 >"$repo2/note16.txt"
what="a BOM-marked UTF-16 file"
hit2 'note16.txt:1'
git -C "$repo2" add -A && git -C "$repo2" commit -q -m utf16
what="a UTF-16 file in a commit"
hit2 'note16.txt'
git -C "$repo2" reset -q --hard HEAD~1
printf '*.dat binary\n' >"$repo2/.gitattributes"
printf 'see quill-marrow\n' >"$repo2/x.dat"
git -C "$repo2" add -A && git -C "$repo2" commit -q -m attrs
what="a file git is told to treat as binary"
hit2 'x.dat:1'

fresh ident
git -C "$repo2" -c user.name="Quill Marrow" commit -q --allow-empty -m "empty"
what="an author name"
hit2 'author or committer'
git -C "$repo2" reset -q --hard HEAD~1
git -C "$repo2" checkout -q -b quill-marrow-fix
what="a branch name"
hit2 'branch name'

# removing an entry from the committed list weakens the check; the stage refuses it. A copy of the
# scripts in the scratch repository makes its own .private-names.sha256 the default list.
fresh shrink
mkdir "$repo2/scripts"
cp "$check" "$add" "$here/private-names-fold.pl" "$repo2/scripts/"
cp "$h2" "$repo2/.private-names.sha256"
git -C "$repo2" add -A && git -C "$repo2" commit -q -m list
git -C "$repo2" update-ref refs/remotes/origin/main HEAD
what="a list with its entries intact"
(cd "$repo2" && env -u CI bash scripts/private-names-check.sh) 2>"$tmp/err" || { cat "$tmp/err" >&2; fail "$what: failed"; }
what="a list with an entry removed"
sed -i.bak '$d' "$repo2/.private-names.sha256"; rm "$repo2/.private-names.sha256.bak"
if (cd "$repo2" && env -u CI bash scripts/private-names-check.sh) 2>"$tmp/err"; then fail "$what: passed"; fi
grep -q 'removes an entry' "$tmp/err" || fail "$what: wrong reason"

what="the committed hash file is well formed and non-empty"
real="$here/../.private-names.sha256"
grep -qvE '^[[:space:]]*(#|$)' "$real" || fail "$what: no entry"
grep -vE '^[[:space:]]*(#|$)' "$real" | grep -qvE '^[0-9a-f]{64}$' && fail "$what: a bad line"
grep -qE '^# salt: [0-9a-f]{32,}$' "$real" || fail "$what: no salt header"

echo "test-private-names: ok"
