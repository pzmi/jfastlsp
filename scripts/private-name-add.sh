#!/usr/bin/env bash
# Add the salted hash of a private name to .private-names.sha256 (fnd §7.4). The name itself is
# never written anywhere: only sha256("<salt>:<normalised name>") is appended. Normalising
# lower-cases the name and drops everything but ASCII letters and digits, the same rule the
# `private-names` check applies to the tokens it hashes, so "Foo Bar", "foo-bar" and "FooBar"
# all match one entry.
#
# Usage: scripts/private-name-add.sh NAME    (or with no argument: read one name from stdin,
# which keeps it out of the shell history). Owner only; commit the changed file.
set -euo pipefail

file="$(cd "$(dirname "$0")/.." && pwd)/.private-names.sha256"
[ -f "$file" ] || { echo "private-name-add: $file is missing" >&2; exit 1; }

if [ "$#" -ge 1 ]; then
  name="$*"
else
  IFS= read -r -s -p "private name: " name
  echo >&2
fi

norm="$(printf '%s' "$name" | tr '[:upper:]' '[:lower:]' | tr -cd 'a-z0-9')"
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
