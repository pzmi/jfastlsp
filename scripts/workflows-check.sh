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
# No linter reads an implicit configuration (FND-09; the FND-08 re-verification's F1): shellcheck
# runs with `--norc` (a `.shellcheckrc` with `disable=all` turned it off), actionlint with
# `-config-file /dev/null` (it reads `.github/actionlint.yaml` by itself; its embedded shellcheck
# already ignores rc files) and zizmor with the pinned `.github/zizmor.yml` (.config/gate.sha256).
# pinact's own configuration files are refused by scripts/tool-config-check.sh's allowlists.
# Inline suppressions are held like `#[allow]` (the FND-09 review's G3, H1, H2): every line of a
# linted file (a shell script, or a workflow's `run:` block) with a shellcheck directive comment,
# at its start or trailing a command (`{ # ...`, which shellcheck honours too), and every zizmor
# ignore comment must be one of `allowed_suppressions` below, by file and exact line, each with
# its reason, and holds once in its file; a `disable=all` never is. actionlint has no inline ignores, and pinact's pin
# check takes none.
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

# The allowed inline suppressions, one per line: `<path>|<the directive line, trimmed, without a
# leading "# " (so this list holds no directive of its own)>|<reason>`.
# A new one is added here, in the commit that needs it, with its reason (this file is pinned).
allowed_suppressions="$(cat <<'LIST'
.claude/hooks/session-start.sh|shellcheck disable=SC2016 # backticks and $PATH in single quotes are literal on purpose|the hook prints shell text for the user to copy
LIST
)"

# suppressions BASE FILE...: refuse every shellcheck directive or zizmor ignore comment in FILEs
# that is not allowlisted for its path relative to BASE.
suppressions() {
  local base="$1" file rel line status=0 seen=$'\n'
  shift
  for file in "$@"; do
    rel="${file#"$base"/}"
    while IFS= read -r line; do
      line="$(sed 's/^[[:space:]]*//; s/[[:space:]]*$//' <<<"$line")"
      case "$line" in *disable=all* | *disable=*all,* | *disable=*,all*)
        echo "workflows: $rel: \`$line\` disables every check; never allowed (fnd §7.3)" >&2
        status=1; continue ;;
      esac
      if ! printf '%s\n' "$allowed_suppressions" | awk -F'|' -v p="$rel" -v l="$line" \
          '$1 == p && ("# " $2 == l || $2 == l) && $3 != "" { found = 1 } END { exit !found }'; then
        echo "workflows: $rel: \`$line\` is an inline suppression outside the allowlist in scripts/workflows-check.sh (fnd §7.3)" >&2
        status=1
        continue
      fi
      # Each allowlisted line holds once: a copy elsewhere in the file would silence other code.
      case "$seen" in *$'\n'"$rel|$line"$'\n'*)
        echo "workflows: $rel: \`$line\` is allowlisted once; this copy is outside the allowlist (fnd §7.3)" >&2
        status=1 ;;
      esac
      seen="$seen$rel|$line"$'\n'
    done < <(grep -E '#[[:space:]]*shellcheck[[:space:]]|zizmor:[[:space:]]*ignore' "$file" || true)
  done
  return "$status"
}

# yaml_shape FILE...: the YAML of each file is held to a shape where nothing is decoded, so the
# raw text the other checks read is what actionlint and the runner read (the FND-09 review's J1:
# `"\x72un":` is a `run:` key, and `? run` a complex one). Outside block scalars, refused:
#   - a backslash anywhere on a line holding a double quote (an escape sequence);
#   - a `?` complex-key indicator;
#   - a quoted mapping key (a quote followed by `:`), anywhere on the line, flow mappings included;
#   - a tag (`!`), an anchor (`&`), an alias (`*`) where a node starts, a merge key (`<<`) and a
#     directive (`%` at the start of a line). No workflow needs one today.
# A block scalar's lines (after `|` or `>`, more indented than its key) are script text, not
# YAML, and only the inline suppression scan reads them.
yaml_shape() {
  local file status=0
  for file in "$@"; do
    awk -v file="$file" '
      function indent(s) { match(s, /^[ ]*/); return RLENGTH }
      function bad(why) { printf "workflows: %s:%d: %s (fnd §7.3)\n", file, NR, why > "/dev/stderr"; failed = 1 }
      BEGIN { block = -1 }
      block >= 0 && ($0 ~ /^[ \t]*$/ || indent($0) > block) { next }
      { block = -1 }
      /^[ \t]*(#.*)?$/ { next }
      /^%/ { bad("a YAML directive") }
      /"/ && /\\/ { bad("a backslash on a line with a double quote: no YAML escape sequences") }
      /^[ ]*(- +)*\?([ ]|$)/ || /[{,[][ ]*\?[ ]/ { bad("a complex key (?)") }
      /["\047][ \t]*:([ \t]|$)/ { bad("a quoted mapping key: keys are plain scalars") }
      /^[ ]*(- +)*[!&*]/ || /:[ ]+[!&*]/ || /- +[!&*]/ || /[\[{,][ ]*[!&*]/ { bad("a tag, an anchor or an alias") }
      /(^|[ {,])<<[ ]*:/ { bad("a merge key (<<)") }
      # A block scalar starts: remember the indentation of its key (or of its `- `).
      # Anchored, so a comment that ends in `: |` opens no block (FND-09 review, K1).
      /^[ ]*(- +)*([A-Za-z0-9_.-]+:[ ]+)?[|>][-+0-9]*[ ]*(#.*)?$/ { block = indent($0) }
      END { exit failed }' "$file" || status=1
  done
  return "$status"
}

# run_scalars FILE...: every `run:` of a workflow or action is a block scalar (`|` or `>`, with an
# optional chomping or indentation indicator) or a plain scalar on one line, or the mapping of
# `defaults: run:`; never a single- or double-quoted string, a flow form or a multi-line plain
# scalar. actionlint decodes a double-quoted string (`\x23` is `#`) before shellcheck reads it,
# so a directive there would pass the raw-text scan of `suppressions` (the FND-09 review's I1).
# Any other line that names a `run` key (quoted, spaced, inside a flow mapping) fails too.
run_scalars() {
  local file status=0
  for file in "$@"; do
    awk -v file="$file" '
      function indent(s) { match(s, /^[ ]*/); return RLENGTH }
      function bad(why) { printf "workflows: %s:%d: %s (fnd §7.3)\n", file, NR, why > "/dev/stderr"; failed = 1 }
      # Inside a block scalar: its lines are script text, not YAML.
      block >= 0 && ($0 ~ /^[ \t]*$/ || indent($0) > block) { next }
      { block = -1 }
      # The line after a plain scalar or a defaults mapping key: more indented means a continuation.
      after >= 0 {
        if ($0 !~ /^[ \t]*(#.*)?$/ && indent($0) > after) {
          if (mapping && $0 ~ /^[ ]*(shell|working-directory):[ ]+[A-Za-z0-9_.\/${}-]+[ ]*$/) { next }
          bad("a run: value continued on the next line; use a block scalar (|)")
        }
        if ($0 !~ /^[ \t]*(#.*)?$/) { after = -1; mapping = 0 }
      }
      /^[ ]*(- +)?run:([ ]|$)/ {
        line = $0
        sub(/^[ ]*(- +)?/, "", line)
        key = indent($0) + (match($0, /^[ ]*- +/) ? RLENGTH - indent($0) : 0)
        value = substr(line, 5)
        sub(/^[ ]+/, "", value)
        if (value ~ /^[|>]([+-]?[1-9]?|[1-9][+-])([ ]+#.*)?$/) { block = key; next }
        if (value == "" || value ~ /^#/) { after = key; mapping = 1; next }
        if (value ~ /^[\047"\[{!&*%@`|>?:,#-]/) { bad("a run: value that is quoted, a flow form or an indicator; use a block scalar (|) or a plain line"); next }
        after = key; mapping = 0
        next
      }
      /^[ \t]*#/ { next }
      /(^|[^A-Za-z0-9_-])["\047]?run["\047]?[ ]*:/ { bad("a run key in another form than `run:` at the start of its line") }
      BEGIN { block = -1; after = -1 }
      END { exit failed }' "$file" || status=1
  done
  return "$status"
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
    echo "workflows: inline suppressions (allowlist)" >&2
    suppressions "$root" "${zizmor_files[@]}" || status=1
    echo "workflows: YAML shape (no escapes, complex or quoted keys, tags, anchors)" >&2
    yaml_shape "${zizmor_files[@]}" || status=1
    echo "workflows: run: values (block or plain scalars only)" >&2
    run_scalars "${files[@]}" || status=1
    if [ "${#workflow_files[@]}" -gt 0 ]; then
      echo "workflows: actionlint" >&2
      actionlint -no-color -config-file /dev/null "${workflow_files[@]}" || status=1
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
    status=0
    suppressions "$here" "$@" || status=1
    shellcheck --norc "$@" || status=1
    exit "$status"
    ;;
  *)
    echo "usage: workflows-check.sh workflows [ROOT] | verify [ROOT] | online [ROOT] | shell [FILE...]" >&2
    exit 2
    ;;
esac
