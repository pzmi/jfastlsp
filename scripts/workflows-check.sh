#!/usr/bin/env bash
# The `workflows` stage (FND-04, fnd §7.3): the static checks of GitHub Actions files and shell
# scripts, offline and deterministic.
#
#   workflows [ROOT]   actionlint (with shellcheck on every `run:`), zizmor offline audits with
#                      the hash-pin policy of .github/zizmor.yml, and pinact's syntactic pin check
#                      (`--fix=false --no-api`: every `uses:` is a 40-hex SHA) over
#                      ROOT/.github/workflows and ROOT/.github/actions; zizmor also reads
#                      ROOT/.github/dependabot.yml (a cooldown, no external code). No workflow or
#                      action file passes with a notice: a repository without workflows has
#                      nothing to get wrong.
#   verify [ROOT]      pinact `--check --verify-comment`: each version comment names the pinned
#                      SHA. It asks the GitHub API (a token in GITHUB_TOKEN lifts the rate limit),
#                      so it is a separate recipe (`just workflows-verify`) for CI, outside `check`.
#   online [ROOT]      zizmor with its online audits (impostor commits, known-vulnerable actions,
#                      ref confusion), which ask the GitHub API with GH_TOKEN, so it is a separate
#                      recipe (`just workflows-online`) for security.yml, outside `check`.
#   shell [FILE...]    shellcheck over FILEs, default scripts/*.sh and .claude/hooks/*.sh
#                      (not spikes/e1-parse/*.sh: archived evidence, kept as it ran, fnd §7.3).
#
# ROOT defaults to this repository. Pinned tools come from mise, which resolves them by the
# working directory, so the script runs from the repository root and gives the tools paths.
# Exit: 0 clean, 1 a finding or a missing tool, 2 usage.
#
# Pinned by: scripts/test-workflows.sh
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$here"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "$1 is missing: run 'mise install --locked' (or bash scripts/bootstrap-tools.sh); just doctor says what else is" >&2
    exit 1
  }
}

# The files of ROOT's .github that the audits read, one per line, sorted.
collect() {
  local root="$1"
  { find "$root/.github/workflows" -maxdepth 1 -type f \( -name '*.yml' -o -name '*.yaml' \) 2>/dev/null || true
    find "$root/.github/actions" -type f \( -name 'action.yml' -o -name 'action.yaml' \) 2>/dev/null || true
  } | LC_ALL=C sort
}

# Dependabot's configuration, which zizmor audits too (cooldown, insecure external code).
dependabot() {
  local root="$1" f
  for f in "$root/.github/dependabot.yml" "$root/.github/dependabot.yaml"; do
    if [ -f "$f" ]; then echo "$f"; fi
  done
}

mode="${1:-}"
[ $# -eq 0 ] || shift
case "$mode" in
  workflows | verify | online)
    [ $# -le 1 ] || { echo "workflows: $mode takes at most one ROOT" >&2; exit 2; }
    root="${1:-$here}"
    files=()
    while IFS= read -r f; do files+=("$f"); done < <(collect "$root")
    if [ "${#files[@]}" -eq 0 ]; then
      echo "workflows: no workflow or action files under $root/.github; nothing to check" >&2
      exit 0
    fi
    workflow_files=()
    for f in "${files[@]}"; do
      case "$f" in */.github/workflows/*) workflow_files+=("$f") ;; esac
    done
    zizmor_files=("${files[@]}")
    while IFS= read -r f; do zizmor_files+=("$f"); done < <(dependabot "$root")
    if [ "$mode" = online ]; then
      need zizmor
      zizmor --no-progress --config "$here/.github/zizmor.yml" "${zizmor_files[@]}"
      exit 0
    fi
    if [ "$mode" = verify ]; then
      need pinact
      pinact run --check --verify-comment "${files[@]}"
      exit 0
    fi
    need actionlint
    need shellcheck
    need zizmor
    need pinact
    status=0
    if [ "${#workflow_files[@]}" -gt 0 ]; then
      echo "workflows: actionlint" >&2
      actionlint -no-color "${workflow_files[@]}" || status=1
    fi
    echo "workflows: zizmor (offline, hash-pin policy)" >&2
    zizmor --offline --no-progress --config "$here/.github/zizmor.yml" "${zizmor_files[@]}" || status=1
    echo "workflows: pinact (every uses: is a SHA)" >&2
    pinact run --fix=false --no-api "${files[@]}" || status=1
    exit "$status"
    ;;
  shell)
    need shellcheck
    if [ $# -eq 0 ]; then
      shopt -s nullglob
      set -- scripts/*.sh .claude/hooks/*.sh
    fi
    [ $# -gt 0 ] || { echo "workflows: no shell files to lint" >&2; exit 1; }
    echo "workflows: shellcheck $#" "file(s)" >&2
    shellcheck "$@"
    ;;
  *)
    echo "usage: workflows-check.sh workflows [ROOT] | verify [ROOT] | online [ROOT] | shell [FILE...]" >&2
    exit 2
    ;;
esac
