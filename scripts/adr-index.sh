#!/usr/bin/env bash
# The `docs` stage's check (FND-03, fnd §10.3): the index table of doc/adr/README.md agrees with
# the ADR files. `adrs generate toc` lists files but knows nothing of our status annotations
# ("Accepted (owner, 2026-11-16); amendment proposed (ADR 0030)"), so this is the "check of about
# 50 lines" the card allows; `adrs` still creates the records (`adrs new`, adrs.toml).
#
# Usage:
#   scripts/adr-index.sh [check] [DIR]   fail when an index row and a file disagree (DIR: doc/adr)
#   scripts/adr-index.sh sort [DIR]      sort the rows by number and drop exact duplicates
#                                        (what scripts/land.sh runs after a union merge)
#
# `check` fails on: a file without a row or a row without a file, two rows (or files) with one
# number, a row out of order, a link that is not the file's name, a title that is not the file's
# `# NNNN. Title`, a status keyword (Proposed, Accepted, Rejected, Superseded) that differs from
# the file's `## Status`, an `(owner, DATE)` that differs, an Accepted or Rejected record with no
# `(owner, DATE)` (only the owner decides), and a Superseded one whose file or row lacks
# `by NNNN` or whose successor has no file. A gap in the numbers is fine: a lane may hold a reserved number for a
# while. Plain bash 3.2 and awk, so it runs on macOS as well.
#
# Pinned by: tools/jfl-architecture/tests/land_scripts.rs (adr_index cases)
set -euo pipefail

mode="check"
case "${1:-}" in check | sort) mode="$1"; shift ;; esac
dir="${1:-doc/adr}"
readme="$dir/README.md"
[ -f "$readme" ] || { echo "adr-index: $readme not found" >&2; exit 2; }

if [ "$mode" = sort ]; then
  tmp="$(mktemp)"
  rows="$(mktemp)"
  trap 'rm -f "$tmp" "$rows"' EXIT
  # Every row, wherever it stands, sorted and de-duplicated, goes where the first row was.
  grep -E '^\| \[[0-9]{4}\]\(' "$readme" | LC_ALL=C sort -u >"$rows" || true
  awk -v rows="$rows" '
    /^\| \[[0-9][0-9][0-9][0-9]\]\(/ {
      if (!done) { while ((getline line < rows) > 0) print line; done = 1 }
      next
    }
    { print }' "$readme" >"$tmp"
  cat "$tmp" >"$readme"
  exit 0
fi

records="$(mktemp)"
trap 'rm -f "$records"' EXIT
# One tab-separated record per file: F, number, file name, title, status line.
for file in "$dir"/[0-9][0-9][0-9][0-9]-*.md; do
  [ -e "$file" ] || continue
  name="$(basename "$file")"
  title="$(sed -n '1s/^# [0-9]*\. //p' "$file")"
  status="$(awk '/^## Status/ { on = 1; next } on && NF { print; exit }' "$file")"
  printf 'F\t%s\t%s\t%s\t%s\n' "${name%%-*}" "$name" "$title" "$status" >>"$records"
done
# One per row: R, number, link target, title, status cell, line.
awk -F' [|] ' '
  /^\| \[[0-9][0-9][0-9][0-9]\]\(/ {
    num = substr($1, 4, 4); target = $1; sub(/^[^(]*\(/, "", target); sub(/\)$/, "", target)
    status = $3; sub(/ \|$/, "", status)
    printf "R\t%s\t%s\t%s\t%s\t%d\n", num, target, $2, status, NR
  }' "$readme" >>"$records"

awk -F'\t' '
  function keyword(s) { return (match(s, /^(Proposed|Accepted|Rejected|Superseded)/)) ? substr(s, 1, RLENGTH) : "" }
  function owner(s) { return (match(s, /\(owner, [0-9-]+\)/)) ? substr(s, RSTART, RLENGTH) : "" }
  function successor(s) { return (match(s, /Superseded by \[?[0-9]+/)) ? substr(s, RSTART + RLENGTH - 4, 4) : "" }
  function bad(msg) { print "adr-index: " msg > "/dev/stderr"; failed = 1 }
  $1 == "F" {
    if ($2 in file) bad("two files numbered " $2 ": " file[$2] " and " $3)
    file[$2] = $3; ftitle[$2] = $4; fstatus[$2] = $5
    if (keyword($5) == "") bad($3 ": the status \"" $5 "\" starts with none of Proposed, Accepted, Rejected, Superseded")
  }
  $1 == "R" {
    if ($2 in row) bad("two index rows numbered " $2)
    if ($2 < last) bad("index row " $2 " is out of order")
    last = $2; row[$2] = 1
    rtarget[$2] = $3; rtitle[$2] = $4; rstatus[$2] = $5
  }
  END {
    for (n in file) if (!(n in row)) bad("no index row for " file[n])
    for (n in row) {
      if (!(n in file)) { bad("index row " n " names no ADR file"); continue }
      if (rtarget[n] != file[n]) bad("index row " n " links " rtarget[n] ", not " file[n])
      if (rtitle[n] != ftitle[n]) bad("index row " n " title \"" rtitle[n] "\" differs from the file'\''s \"" ftitle[n] "\"")
      if (keyword(rstatus[n]) != keyword(fstatus[n])) bad("index row " n " status \"" rstatus[n] "\" differs from the file'\''s \"" fstatus[n] "\"")
      if (owner(rstatus[n]) != owner(fstatus[n])) bad("index row " n " owner decision \"" owner(rstatus[n]) "\" differs from the file'\''s \"" owner(fstatus[n]) "\"")
      k = keyword(fstatus[n])
      if ((k == "Accepted" || k == "Rejected") && owner(fstatus[n]) == "") bad(file[n] ": " k " without \"(owner, DATE)\": only the owner decides")
      if (k == "Superseded") {
        by = successor(fstatus[n])
        if (by == "") bad(file[n] ": Superseded without \"by NNNN\"")
        else {
          if (successor(rstatus[n]) != by) bad("index row " n " says \"" rstatus[n] "\" but the file is superseded by " by)
          if (!(by in file)) bad(file[n] ": superseded by " by ", and no such ADR exists")
        }
      }
    }
    exit failed
  }' "$records"
