#!/usr/bin/env bash
# The DCO check (FND-03, fnd §7.4, ADR 0002): every commit in a range carries a `Signed-off-by:`
# trailer naming its own author (name and email, as `git log` prints them). A sign-off by someone
# else, or none, fails; merge commits are skipped (a merge has no author's statement to certify).
# `git interpret-trailers` reads the trailer block, so a sign-off quoted in a message body does
# not count. `ci.yml` (FND-05) and `scripts/land.sh` call it.
#
# Merge commits are skipped on purpose, also in a pull request's range: an outside contribution
# is integrated by the maintainer as signed-off commits, never merged (ADR 0002), so a merge
# commit never reaches main, and refusing one would only fail a contributor's merge of main into
# their branch.
#
# Usage: scripts/dco-check.sh [RANGE]    RANGE is any `git rev-list` range (default origin/main..HEAD)
#        scripts/dco-check.sh --event EVENT BEFORE BASE HEAD
#                                         the commits a GitHub event brought (ci.yml): push
#                                         BEFORE..HEAD, pull_request BASE..HEAD, else HEAD~1..HEAD.
#                                         A push whose BEFORE is all zeros (a first push, a
#                                         recreated branch) or not a commit here (a force-push)
#                                         fails: main is neither (ADR 0002).
#
# Pinned by: tools/jfl-architecture/tests/land_scripts.rs (dco_check cases)
set -euo pipefail

if [ "${1:-}" = --event ]; then
  [ $# -eq 5 ] || { echo "usage: dco-check.sh --event EVENT BEFORE BASE HEAD" >&2; exit 2; }
  event="$2" before="$3" base="$4" head="$5"
  case "$event" in
    push)
      if ! git cat-file -e "$before^{commit}" 2>/dev/null; then
        echo "dco-check: push 'before' $before is not a commit here (a first push, a recreated branch or a force-push); check the range by hand" >&2
        exit 1
      fi
      set -- "$before..$head" ;;
    pull_request) set -- "$base..$head" ;;
    *) set -- "$head~1..$head" ;;
  esac
fi

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
