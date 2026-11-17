#!/usr/bin/env bash
# Add the salted hash of a private name to .private-names.sha256 (fnd §7.4). The name itself is
# never written anywhere: only sha256("<salt>:<normalised name>") is appended. Normalising
# folds it to ASCII (NFKD, no combining marks, ł ø đ ß æ and the like mapped), lower-cases it and drops everything but letters and digits, the same rule the
# `private-names` check applies to the tokens it hashes, so "Foo Bar", "foo-bar" and "FooBar"
# all match one entry.
#
# Usage: scripts/private-name-add.sh [--file FILE] [NAME]    (with no NAME: read one name from
# stdin, which keeps it out of the shell history). Owner only; commit the changed file. --file
# names another hash file (the check's self-test uses one with its own salt).
set -euo pipefail

file="$(cd "$(dirname "$0")/.." && pwd)/.private-names.sha256"
if [ "${1:-}" = "--file" ]; then
  [ $# -ge 2 ] || { echo "private-name-add: --file needs a path" >&2; exit 2; }
  file="$2"
  shift 2
fi
[ -f "$file" ] || { echo "private-name-add: $file is missing" >&2; exit 1; }

if [ "$#" -ge 1 ]; then
  name="$*"
else
  IFS= read -r -s -p "private name: " name
  echo >&2
fi

fold="$(cd "$(dirname "$0")" && pwd)/private-names-fold.pl"
command -v perl >/dev/null 2>&1 || { echo "private-name-add: perl is missing" >&2; exit 1; }
# Fold to ASCII exactly as the check does, then count the tokens: the check hashes runs of at most
# 3 adjacent tokens, so a longer name could never match spaced text and is refused.
tokens="$(printf '%s' "$name" | perl -e 'require $ARGV[0]; local $/; print lc fold_ascii(<STDIN>)' "$fold" | tr -cs 'a-z0-9' '\n' | sed '/^$/d')"
count="$(printf '%s\n' "$tokens" | sed '/^$/d' | wc -l | tr -d ' ')"
[ "$count" -le 3 ] || { echo "private-name-add: a name of more than 3 words cannot be matched (the check reads runs of 3); add its distinctive part instead" >&2; exit 1; }
norm="$(printf '%s' "$tokens" | tr -d '\n')"
[ "${#norm}" -ge 3 ] || { echo "private-name-add: a name needs at least 3 letters or digits" >&2; exit 1; }

salt="$(sed -n 's/^# salt: \([0-9a-f]\{32,\}\)$/\1/p' "$file")"
[ -n "$salt" ] || { echo "private-name-add: no '# salt: <hex>' header in $file" >&2; exit 1; }

if command -v sha256sum >/dev/null 2>&1; then
  hash="$(printf '%s:%s' "$salt" "$norm" | sha256sum | cut -d' ' -f1)"
else
  hash="$(printf '%s:%s' "$salt" "$norm" | shasum -a 256 | cut -d' ' -f1)"
fi

if grep -qx "$hash" "$file"; then
  echo "private-name-add: already listed"
else
  printf '%s\n' "$hash" >> "$file"
  echo "private-name-add: added one hash; commit $file"
fi
