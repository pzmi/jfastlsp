#!/usr/bin/env bash
# The DCO check (FND-03, fnd §7.4, ADR 0002): every commit in a range carries a `Signed-off-by:`
# trailer naming its own author (name and email, as `git log` prints them). A sign-off by someone
# else, or none, fails; merge commits are skipped (a merge has no author's statement to certify).
# `git interpret-trailers` reads the trailer block, so a sign-off quoted in a message body does
# not count. `ci.yml` (FND-05) and `scripts/land.sh` call it.
#
# Usage: scripts/dco-check.sh [RANGE]    RANGE is any `git rev-list` range (default origin/main..HEAD)
#
# Pinned by: tools/jfl-architecture/tests/land_scripts.rs (dco_check cases)
set -euo pipefail

range="${1:-origin/main..HEAD}"
shas="$(git rev-list --no-merges "$range")"
failed=0
checked=0
while IFS= read -r sha; do
  [ -n "$sha" ] || continue
  checked=$((checked + 1))
  author="$(git log -1 --format='%an <%ae>' "$sha")"
  if ! git log -1 --format=%B "$sha" | git interpret-trailers --parse |
    grep -Fxq "Signed-off-by: $author"; then
    echo "dco-check: $(git log -1 --format='%h %s' "$sha") has no 'Signed-off-by: $author'" >&2
    failed=1
  fi
done <<<"$shas"
[ "$failed" -eq 0 ] || exit 1
echo "dco-check: $checked commit(s) in $range signed off by their author"
