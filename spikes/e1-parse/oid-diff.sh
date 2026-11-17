#!/usr/bin/env bash
# Files that differ between two worktrees, by index blob OID (no file content read).
# Usage: oid-diff.sh WT1 WT2 ; prints count of changed/added/removed .java paths.
join -t $'\t' -a1 -a2 -e NONE -o 0,1.2,2.2 \
  <(git -C "$1" ls-files -s -- '*.java' | awk '{print $4"\t"$2}' | LC_ALL=C sort) \
  <(git -C "$2" ls-files -s -- '*.java' | awk '{print $4"\t"$2}' | LC_ALL=C sort) \
  | awk -F'\t' '$2!=$3' | wc -l
