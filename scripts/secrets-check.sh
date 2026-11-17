#!/usr/bin/env bash
# The `secrets` stage (FND-04, fnd §7.4): gitleaks over the full history of a repository with the
# default ruleset and nothing allowlisted. The report is redacted: it names the rule, the file,
# the line and the commit, never the secret.
#
# Usage: scripts/secrets-check.sh [--config FILE] [REPO]    (default: this repository)
# Exit: 0 no finding, 1 a finding, a shallow history or a policy that allowlists something,
# 2 usage. A shallow clone scans only part of the history, so it fails closed: CI fetches full
# history. The working tree is not scanned until it is committed; the next gate then sees it.
#
# Pinned by: scripts/test-secrets.sh
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
config="$root/.gitleaks.toml"
repo="$root"
while [ $# -gt 0 ]; do
  case "$1" in
    --config) [ $# -ge 2 ] || { echo "secrets: --config needs a path" >&2; exit 2; }; config="$2"; shift ;;
    -h | --help) sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//' >&2; exit 2 ;;
    -*) echo "secrets: unknown option $1" >&2; exit 2 ;;
    *) repo="$1" ;;
  esac
  shift
done

command -v gitleaks >/dev/null 2>&1 || {
  echo "gitleaks is missing: run 'mise install --locked' (or bash scripts/bootstrap-tools.sh); just doctor says what else is" >&2
  exit 1
}
[ -f "$config" ] || { echo "secrets: $config is missing" >&2; exit 1; }
# The policy is exactly `[extend]` and `useDefault = true`: any other key can disable a rule, add an
# allowlist or redefine one, and a pattern-based refusal misses spellings (indented tables, keys
# under [extend]). Comments and blank lines are free.
policy="$(grep -vE '^[[:space:]]*(#|$)' "$config" | sed -E 's/[[:space:]]+/ /g; s/^ //; s/ $//')"
if [ "$policy" != "$(printf '[extend]\nuseDefault = true')" ]; then
  echo "secrets: $config must hold only '[extend]' and 'useDefault = true'; the policy allows nothing and disables nothing (fnd §7.4)" >&2
  exit 1
fi
# The other two ways to wave a finding through are refused as well: a committed .gitleaksignore
# (its fingerprints) and `gitleaks:allow` comments in the scanned text.
if git -C "$repo" log --all --name-only --format= -- '.gitleaksignore' '**/.gitleaksignore' | grep -q .; then
  echo "secrets: a .gitleaksignore exists in the history of $repo; the policy allows nothing (fnd §7.4)" >&2
  exit 1
fi
if [ -e "$repo/.gitleaksignore" ]; then
  echo "secrets: $repo/.gitleaksignore exists (tracked or not); the policy allows nothing (fnd §7.4)" >&2
  exit 1
fi
empty="$(mktemp)"
trap 'rm -f "$empty"' EXIT
if [ "$(git -C "$repo" rev-parse --is-shallow-repository)" = "true" ]; then
  echo "secrets: $repo is a shallow clone, so the history is not all there; fetch it in full" >&2
  exit 1
fi

# Run from the repository root so mise's shims resolve the pinned gitleaks.
cd "$root"
gitleaks git --no-banner --redact --verbose --ignore-gitleaks-allow --gitleaks-ignore-path "$empty" --config "$config" "$repo"
