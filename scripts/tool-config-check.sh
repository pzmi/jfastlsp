#!/usr/bin/env bash
# The implicit configuration of the gate's tools, checked before any of them runs (FND-09, fnd §3,
# §3.6): mise, shellcheck, actionlint, pinact and the like read files from the repository that
# no command line names, and mise even runs code from its configuration (`[env] _.source`,
# `{{exec(...)}}` templates) as soon as it loads a trusted file, before `_preflight` can look.
# So this check reads those files itself, with bash and awk only, and nothing calls mise on a
# tree it has not passed: the justfile's top-level lines (before `mise bin-paths`), `_preflight`,
# scripts/land.sh (the landing checkout's own copy, on the merged tree),
# scripts/bootstrap-tools.sh (before `mise install`) and the Stop hook. Every check is an
# allowlist or a pinned shape (fnd §3):
#
#   - ROOT/mise.toml: only `[tools]`, each key `just` or a quoted `aqua:<owner>/<repo>[/<name>]`
#     or `github:<owner>/<repo>` (the backends that install a checksummed release asset; no
#     plugin backend, whose plugin is code), each value a plain version string;
#   - ROOT/mise.lock: only `lockfile_version = 3`, `[[tools.<key>]]` with `version`, `backend`
#     (aqua or github) and `specifiers`, and per-platform tables of linux-x64, linux-arm64 or
#     macos-arm64 with a `sha256:` checksum, a `https://github.com/` url, a
#     `https://api.github.com/repos/` url_api and an optional `github-attestations` provenance;
#   - no cargo configuration file (`.cargo/config`, `.cargo/config.toml`) anywhere in ROOT, and no
#     toolchain file but ROOT/rust-toolchain.toml, which holds only `[toolchain]` with plain
#     `channel`, `profile`, `components` and `targets` (a `path` swaps the compiler): cargo runs
#     a configured `rustc-wrapper` or runner for any command, `cargo update` included, so
#     scripts/land.sh runs this check on each cherry-picked tree before it may resolve a
#     Cargo.lock conflict (the FND-09 review's G2);
#   - the entries of ROOT whose names start with a dot, and those starting with `mise`, are an
#     allowlist (so `.mise.toml`, `mise.local.toml`, `.tool-versions`, `.shellcheckrc`,
#     `.yamllint`, `.pinact.yaml`, `.envrc` and whatever a tool reads next all fail), and so are
#     the entries of ROOT/.github (no `actionlint.yaml`, `pinact.yaml`, ...) and ROOT/.config
#     (no `mise/`, `mise.toml`, ...).
#
# The check reads the file system, so an untracked or ignored file counts as much as a committed
# one (inside a git work tree: tracked and untracked files that are not ignored; elsewhere: every
# file). ARCH-gate-config repeats these checks with a TOML parser.
#
# Usage: scripts/tool-config-check.sh [ROOT]     (default: this repository)
# Exit: 0 every file in its shape, 1 a refusal (each named on stderr), 2 usage.
#
# Pinned by: scripts/test-tool-config.sh, tools/jfl-architecture/tests/gate_negative_cases.rs
# (should_run_nothing_from_mise_toml_when_the_gate_meets_the_reviewer_s_payload) and
# tools/jfl-architecture/tests/land_scripts.rs
# (should_refuse_before_any_just_when_the_merged_tree_s_mise_toml_has_a_payload).
set -euo pipefail

[ $# -le 1 ] || { echo "usage: tool-config-check.sh [ROOT]" >&2; exit 2; }
root="${1:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)}"
[ -d "$root" ] || { echo "tool-config-check: $root is not a directory" >&2; exit 2; }

refused=()

# The entries allowed at the root among the dot-names and the mise names, under .github and under
# .config. A new one is added here, in the commit that needs it, with ARCH-gate-config's lists.
root_allowed=" .claude .config .DS_Store .editorconfig .git .gitattributes .github .gitignore .gitleaks.toml .idea .private-names.sha256 .vscode .work mise.lock mise.toml "
github_allowed=" actions CODEOWNERS dependabot.yml ISSUE_TEMPLATE pull_request_template.md workflows zizmor.yml "
config_allowed=" gate.sha256 nextest.toml "

entries() {
  local dir="$1" path
  [ -d "$dir" ] || return 0
  for path in "$dir"/.* "$dir"/*; do
    [ -e "$path" ] || [ -L "$path" ] || continue
    case "${path##*/}" in . | ..) continue ;; esac
    printf '%s\n' "${path##*/}"
  done
}

while IFS= read -r name; do
  case "$name" in
    .* | mise*) case "$root_allowed" in *" $name "*) ;; *) refused+=("$root/$name: not an allowed entry at the root (dotfiles and mise* names are an allowlist)") ;; esac ;;
  esac
done < <(entries "$root")
while IFS= read -r name; do
  case "$github_allowed" in *" $name "*) ;; *) refused+=("$root/.github/$name: not an allowed entry of .github/") ;; esac
done < <(entries "$root/.github")
while IFS= read -r name; do
  case "$config_allowed" in *" $name "*) ;; *) refused+=("$root/.config/$name: not an allowed entry of .config/") ;; esac
done < <(entries "$root/.config")

# Cargo and toolchain configuration at any depth.
if git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  listing() { git -C "$root" ls-files -z --cached --others --exclude-standard; }
else
  listing() { (cd "$root" && find . -path ./.git -prune -o -type f -print0); }
fi
while IFS= read -r -d '' file; do
  file="${file#./}"
  # Case-insensitively: on a case-insensitive file system (macOS) cargo opens `.cargo/Config.toml`
  # as `config.toml` (the FND-09 review's H6).
  case "/$file" in
    */.[cC][aA][rR][gG][oO]/[cC][oO][nN][fF][iI][gG] | */.[cC][aA][rR][gG][oO]/[cC][oO][nN][fF][iI][gG].[tT][oO][mM][lL])
      refused+=("$root/$file: a cargo configuration file; none may exist in the repository (it can set a rustc-wrapper or a runner that any cargo command runs)") ;;
    */[rR][uU][sS][tT]-[tT][oO][oO][lL][cC][hH][aA][iI][nN] | */[rR][uU][sS][tT]-[tT][oO][oO][lL][cC][hH][aA][iI][nN].[tT][oO][mM][lL])
      [ "$file" = rust-toolchain.toml ] ||
        refused+=("$root/$file: a toolchain file; only the root rust-toolchain.toml may exist") ;;
  esac
done < <(listing | LC_ALL=C sort -zu)
toolchain="$root/rust-toolchain.toml"
if [ -L "$toolchain" ] || { [ -e "$toolchain" ] && [ ! -f "$toolchain" ]; }; then
  refused+=("$toolchain: not a regular file")
elif [ -f "$toolchain" ] && ! line="$(awk '
    /^[[:space:]]*(#.*)?$/ { next }
    /^\[toolchain\][[:space:]]*(#.*)?$/ && !seen { seen = 1; next }
    seen && /^(channel|profile)[[:space:]]*=[[:space:]]*"[A-Za-z0-9._-]+"[[:space:]]*(#.*)?$/ { next }
    seen && /^(components|targets)[[:space:]]*=[[:space:]]*\[[[:space:]]*("[A-Za-z0-9._-]+"[[:space:]]*,[[:space:]]*)*("[A-Za-z0-9._-]+")?[[:space:]]*\][[:space:]]*(#.*)?$/ { next }
    { print NR ": " $0; bad = 1; exit }
    END { exit bad }' "$toolchain")"; then
  refused+=("$toolchain:$line: outside the shape (only [toolchain] with plain channel, profile, components and targets)")
fi

# A tool key of mise.toml or mise.lock: `just`, or a quoted aqua or github identifier.
key='(just|"aqua:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)?"|"github:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+")'
version='"[A-Za-z0-9._+-]+"'
tail='[[:space:]]*(#.*)?$'

mise_toml="$root/mise.toml"
if [ -L "$mise_toml" ] || { [ -e "$mise_toml" ] && [ ! -f "$mise_toml" ]; }; then
  refused+=("$mise_toml: not a regular file")
elif [ -f "$mise_toml" ]; then
  if ! line="$(awk -v key="$key" -v version="$version" -v tail="$tail" '
      /^[[:space:]]*(#.*)?$/ { next }
      $0 ~ "^\\[tools\\]" tail { tools = 1; next }
      tools && $0 ~ "^" key "[[:space:]]*=[[:space:]]*" version tail { next }
      { print NR ": " $0; bad = 1; exit }
      END { exit bad }' "$mise_toml")"; then
    refused+=("$mise_toml:$line: outside the shape (only [tools], each tool \`just\` or an aqua or github backend, pinned by a plain version)")
  fi
fi

mise_lock="$root/mise.lock"
if [ -L "$mise_lock" ] || { [ -e "$mise_lock" ] && [ ! -f "$mise_lock" ]; }; then
  refused+=("$mise_lock: not a regular file")
elif [ -f "$mise_lock" ]; then
  if ! line="$(awk -v key="$key" -v version="$version" -v tail="$tail" '
      function close_platform() {
        if (table == "platform" && !checksum) { print NR ": a platform table without a sha256 checksum"; bad = 1; exit }
      }
      /^[[:space:]]*(#.*)?$/ { next }
      $0 ~ "^lockfile_version = 3" tail && table == "" { next }
      $0 ~ "^\\[\\[tools\\." key "\\]\\]" tail { close_platform(); table = "tool"; next }
      $0 ~ "^\\[tools\\." key "\\.\"platforms\\.(linux-x64|linux-arm64|macos-arm64)\"\\]" tail {
        close_platform(); table = "platform"; checksum = 0; next
      }
      table == "tool" && $0 ~ "^version = " version tail { next }
      table == "tool" && $0 ~ "^backend = \"(aqua:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+(/[A-Za-z0-9._-]+)?|github:[A-Za-z0-9._-]+/[A-Za-z0-9._-]+)\"" tail { next }
      table == "tool" && $0 ~ "^specifiers = \\[" version "\\]" tail { next }
      table == "platform" && $0 ~ "^checksum = \"sha256:[0-9a-f]+\"" tail && length($3) == 73 { checksum = 1; next }
      table == "platform" && $0 ~ "^url = \"https://github\\.com/[A-Za-z0-9._/+-]+\"" tail { next }
      table == "platform" && $0 ~ "^url_api = \"https://api\\.github\\.com/repos/[A-Za-z0-9._/+-]+\"" tail { next }
      table == "platform" && $0 ~ "^provenance = \"github-attestations\"" tail { next }
      { print NR ": " $0; bad = 1; exit }
      END { if (!bad) close_platform(); exit bad }' "$mise_lock")"; then
    refused+=("$mise_lock:$line: outside the shape (checksummed GitHub release assets of aqua or github tools only)")
  fi
fi

if [ "${#refused[@]}" -gt 0 ]; then
  printf 'tool-config-check: %s\n' "${refused[@]}" >&2
  echo "tool-config-check: refusing to run mise or the gate's tools on this tree (fnd §3.6): their implicit configuration is an allowlist" >&2
  exit 1
fi
