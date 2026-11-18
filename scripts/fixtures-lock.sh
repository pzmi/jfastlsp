#!/usr/bin/env bash
# Rewrites fixtures.lock from the tree (ver §4, ARCH-fixtures-lock): the SHA-256 of every file
# under fixtures/ and under each crate's tests/fixtures/, sorted by path. Run it only after
# reviewing the changed goldens: the architecture stage fails on any golden whose line here is
# stale, and review reads every moved line as a change of the promise. `just fixtures-lock`.
set -euo pipefail
cd "$(dirname "$0")/.."
{
  printf '%s\n' \
    '# The golden fixtures'"'"' SHA-256 (ver §4, ARCH-fixtures-lock in tools/jfl-architecture/src/fixtures_lock.rs):' \
    '# every file under fixtures/ and under each crate'"'"'s tests/fixtures/, sorted. A golden is' \
    '# never rewritten to get green: a deliberate change moves its line here in the same commit' \
    '# (just fixtures-lock), and review reads the line as a change of the promise.'
  { find fixtures -type f; find crates -path 'crates/*/tests/fixtures/*' -type f; } \
    | LC_ALL=C sort -u \
    | while IFS= read -r file; do sha256sum -- "$file"; done
} > fixtures.lock.new
mv fixtures.lock.new fixtures.lock
