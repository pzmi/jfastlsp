#!/usr/bin/env bash
# Fetch the pinned benchmark corpus of doc/bench-corpus.md (BENCH-01, ADR 0031) into a cache
# directory OUTSIDE this repository, verify it, and add linked worktrees to a checkout for
# multi-worktree runs. Nothing from the corpus is ever committed.
#
# Usage:
#   scripts/bench-corpus.sh [fetch] [SIZE] [--cache DIR]   fetch and verify (default SIZE: small)
#   scripts/bench-corpus.sh worktrees SIZE N [--cache DIR] add N linked worktrees (default 10)
#   scripts/bench-corpus.sh clean-worktrees SIZE [--cache DIR]
#   scripts/bench-corpus.sh path SIZE [--cache DIR]        print the checkout path
#   scripts/bench-corpus.sh check-doc                      pins here match doc/bench-corpus.md
#
# SIZE is small | medium | lombok | huge-files | large | large-permissive | all. `all` fetches the six;
# `large` is elasticsearch (SSPL / Elastic License 2.0 / AGPL-3.0): clone at bench time only,
# never vendor it and never derive a fixture, snippet or position file from it.
#
# The cache directory is --cache, else
# ${XDG_CACHE_HOME:-$HOME/.cache}/jfastlsp/bench-corpus. A directory inside this repository
# is refused. --local-source DIR clones from DIR/<repo-name> instead of GitHub (a test aid;
# the pinned commit is still fetched and verified).
#
# Each repository is fetched shallow (`git fetch --depth 1 <sha>`) into a partial directory,
# verified (HEAD is the pin, `git ls-files '*.java'` has the expected count, the tree is clean),
# then renamed into place, so a half-fetched checkout is never mistaken for a cached one. A
# cached checkout is re-verified (cheap) and left alone; a wrong commit or count fails and
# names the directory to delete. Two fetches into one cache at once are not supported.
#
# Pinned by: scripts/test-bench-corpus.sh (fetch, no-op, wrong commit, worktrees, doc check)
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"

# size|dir name|owner/repo|commit|expected .java files|licence note
pins() {
  cat <<'PINS'
small|junit-framework|junit-team/junit-framework|e5198f9b098c550e66d378650f78bac8883e2a72|1813|EPL-2.0
medium|spring-framework|spring-projects/spring-framework|4cb62d893e09e23791d661e8d9b35aedee7006d0|9294|Apache-2.0
lombok|fineract|apache/fineract|9dfe8dfe5e162afe09f102a9aa57d00a70c770f2|7301|Apache-2.0
huge-files|kafka|apache/kafka|7e0ebdf74f30cab1ded2a462cc9d45cefdf86c2a|6307|Apache-2.0
large|elasticsearch|elastic/elasticsearch|157ebe1325f24f34bf871997e880f0c0293b7279|33850|SSPL, Elastic License 2.0 or AGPL-3.0
large-permissive|gradle|gradle/gradle|db675891c2bcf8e3c9a7c66ac9de429170a8a9cd|11750|Apache-2.0
PINS
}

die() { echo "bench-corpus: $*" >&2; exit 1; }
say() { echo "bench-corpus: $*" >&2; }

cache=""
local_source=""
args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --cache) [ $# -ge 2 ] || die "--cache needs a directory"; cache="$2"; shift 2 ;;
    --local-source) [ $# -ge 2 ] || die "--local-source needs a directory"; local_source="$2"; shift 2 ;;
    -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d' | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) args+=("$1"); shift ;;
  esac
done
set -- "${args[@]+"${args[@]}"}"

cmd="fetch"
case "${1:-}" in fetch | worktrees | clean-worktrees | path | check-doc) cmd="$1"; shift ;; esac

resolve_cache() {
  local dir="${cache:-${XDG_CACHE_HOME:-$HOME/.cache}/jfastlsp/bench-corpus}" probe rest="" real
  case "$dir" in /*) ;; *) dir="$PWD/$dir" ;; esac
  # resolve the deepest existing ancestor (symlinks included), so the refusal comes before mkdir
  probe="$dir"
  while [ ! -d "$probe" ]; do rest="/$(basename "$probe")$rest"; probe="$(dirname "$probe")"; done
  real="$(cd "$probe" && pwd -P)$rest"
  case "$real/" in
    "$repo_root"/*) die "cache directory $real is inside the repository; corpus code is never committed" ;;
  esac
  mkdir -p "$real"
  echo "$real"
}

# select_sizes SIZE -> the matching pin lines
select_sizes() {
  local want="${1:-small}" out
  if [ "$want" = all ]; then pins; return; fi
  out="$(pins | awk -F'|' -v s="$want" '$1 == s')"
  [ -n "$out" ] || die "unknown size '$want' (small, medium, lombok, huge-files, large, large-permissive, all)"
  echo "$out"
}

# verify DIR SHA COUNT: fail (return 1, message on stderr) unless DIR is the verified pin
verify() {
  local dir="$1" sha="$2" want="$3" head n st
  head="$(git -C "$dir" rev-parse HEAD 2>/dev/null)" || { say "$dir is not a git checkout"; return 1; }
  [ "$head" = "$sha" ] || { say "$dir is at $head, the pin is $sha"; return 1; }
  n="$(git -C "$dir" ls-files '*.java' | wc -l | tr -d ' ')"
  [ "$n" = "$want" ] || { say "$dir has $n .java files, doc/bench-corpus.md says $want"; return 1; }
  st="$(git -C "$dir" status --porcelain)" || { say "$dir: git status failed"; return 1; }
  [ -z "$st" ] || { say "$dir has a dirty working tree"; return 1; }
}

fetch_one() {
  local size="$1" name="$2" slug="$3" sha="$4" count="$5" licence="$6" dest part url
  dest="$cache_dir/$name"
  if [ -e "$dest" ]; then
    verify "$dest" "$sha" "$count" || die "cached $name failed verification; delete $dest and fetch again"
    [ -f "$dest/.git/jfl-corpus-verified" ] || die "$dest was not made by this script; delete it and fetch again"
    say "$name: cached at $sha ($count .java files)"
    return 0
  fi
  [ "$size" != large ] || say "$name is $licence: bench-time clone only, never vendor or derive fixtures from it"
  if [ -n "$local_source" ]; then url="file://$(cd "$local_source/$name" && pwd -P)"; else url="https://github.com/$slug.git"; fi
  part="$cache_dir/.$name.partial.$$"
  rm -rf "$part"
  trap 'rm -rf "$part"' EXIT
  say "$name: fetching $sha"
  git init -q "$part"
  git -C "$part" remote add origin "$url"
  GIT_TERMINAL_PROMPT=0 git -C "$part" fetch -q --depth 1 --no-tags origin "$sha"
  git -C "$part" -c advice.detachedHead=false checkout -q --detach FETCH_HEAD
  # the helper's worktrees live inside the checkout; keep them out of `git status`
  printf '/.claude/worktrees/\n' >>"$part/.git/info/exclude"
  verify "$part" "$sha" "$count" || die "$name failed verification after the fetch"
  printf '%s\n' "$sha" >"$part/.git/jfl-corpus-verified"
  [ ! -e "$dest" ] || die "$dest appeared during the fetch (concurrent fetches are unsupported)"
  mv "$part" "$dest"
  trap - EXIT
  say "$name: verified at $sha ($count .java files) in $dest"
}

# the pin line of one size, as "name sha count"
one_pin() { select_sizes "$1" | awk -F'|' '{print $2, $4, $5}'; }

doc_check() {
  local f="$repo_root/doc/bench-corpus.md" bad=0 size name slug sha count n
  while IFS='|' read -r size name slug sha count _; do
    grep -q "$sha" "$f" || { say "doc/bench-corpus.md lacks the pin $sha ($name)"; bad=1; }
    n="$(awk -v s="$count" 'BEGIN { o = ""; while (length(s) > 3) { o = "," substr(s, length(s) - 2) o; s = substr(s, 1, length(s) - 3) } print s o }')"
    grep -F "$sha" "$f" | grep -q "| $n |" || { say "doc/bench-corpus.md lacks $n .java files for $name"; bad=1; }
  done < <(pins)
  return "$bad"
}

case "$cmd" in
  check-doc) doc_check && say "pins match doc/bench-corpus.md" ;;
  fetch)
    cache_dir="$(resolve_cache)"
    # a variable, not `< <(...)`: the unknown-size refusal must fail the script, not vanish
    selected="$(select_sizes "${1:-small}")"
    while IFS='|' read -r size name slug sha count licence; do
      fetch_one "$size" "$name" "$slug" "$sha" "$count" "$licence"
    done <<<"$selected"
    ;;
  path)
    cache_dir="$(resolve_cache)"
    read -r name _ _ < <(one_pin "${1:-small}")
    echo "$cache_dir/$name"
    ;;
  worktrees | clean-worktrees)
    size="${1:-}"; [ -n "$size" ] || die "$cmd needs a SIZE"
    [ "$size" != all ] || die "$cmd takes one size"
    cache_dir="$(resolve_cache)"
    read -r name sha count < <(one_pin "$size")
    dest="$cache_dir/$name"
    [ -f "$dest/.git/jfl-corpus-verified" ] || die "$name is not fetched; run: scripts/bench-corpus.sh fetch $size"
    verify "$dest" "$sha" "$count" || die "$name failed verification"
    wt_dir="$dest/.claude/worktrees"
    if [ "$cmd" = clean-worktrees ]; then
      for d in "$wt_dir"/wt-*; do
        [ -d "$d" ] || continue
        git -C "$dest" worktree remove --force "$d"
      done
      git -C "$dest" worktree prune
      say "$name: worktrees removed"
      exit 0
    fi
    n="${2:-10}"
    case "$n" in '' | *[!0-9]* | 0) die "N must be a positive integer" ;; esac
    mkdir -p "$wt_dir"
    for i in $(seq 1 "$n"); do
      d="$wt_dir/$(printf 'wt-%02d' "$i")"
      if [ -d "$d" ]; then
        [ "$(git -C "$d" rev-parse HEAD 2>/dev/null)" = "$sha" ] || die "$d is not at the pin; run clean-worktrees $size"
        continue
      fi
      git -C "$dest" worktree add -q --detach "$d" "$sha"
    done
    have="$(find "$wt_dir" -mindepth 1 -maxdepth 1 -name "wt-*" | wc -l | tr -d ' ')"
    say "$name: $have linked worktrees under $wt_dir"
    ;;
esac
