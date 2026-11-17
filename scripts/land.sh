#!/usr/bin/env bash
# Land reviewed commits on origin/main (FND-03, ADR 0002 as amended, fnd §10.2). The orchestrator's
# one way to integrate: each commit is cherry-pickable, gated on the MERGED tree, and pushed only
# on green. Nothing in the calling checkout moves until the push has succeeded.
#
# Usage: scripts/land.sh [--no-push] [--allow-unsigned] [--author "Name <email>"]
#                        [--remote NAME] [--branch NAME] SHA...
#
#   1. refuse a dirty checkout (modified, staged or untracked files), fetch the remote;
#   2. make a temporary worktree at <remote>/<branch>, cherry-pick each SHA in order, keeping its
#      author, message and trailers (the committer, and the signature, are the environment's);
#   3. resolve only two conflicts: Cargo.lock (take the integration side, then `cargo update -w`)
#      and doc/adr/README.md (union of both sides' rows, sorted); any other conflict aborts and
#      names its files. The index is resolved only when every conflicting line is an index row
#      (prose in the README that both sides changed aborts), and the resulting Cargo.lock diff
#      against the lane's own is printed;
#   4. check the merged commits: each is authored by the owner identity of ADR 0003 (--author
#      overrides it, for a scratch repository), a `Signed-off-by` matching the author
#      (scripts/dco-check.sh) and a signature in the commit HEADER (a `gpgsig` line in the message
#      body does not count; --allow-unsigned waives it for a repository that does not sign);
#   5. check each cherry-picked tree's tool configuration with THIS checkout's xtask
#      (`scripts/xtask.sh tool-config`, built from this checkout, from outside the tree, into
#      a target directory of its own that no gate is given: `$XDG_CACHE_HOME/jfastlsp/land-xtask`,
#      else `~/.cache/jfastlsp/land-xtask`, refused as CARGO_TARGET_DIR; XT-02) before
#      anything there runs, a Cargo.lock resolution included
#      (mise runs code from a trusted configuration as it loads it, `just` evaluates the
#      justfile's `mise bin-paths`, and any cargo command runs a configured rustc-wrapper or a
#      toolchain `path`), never trusting the tree for mise (FND-09); a commit that widens that
#      check's allowlists lands only once this checkout has it (land it by hand, doc/sdlc.md);
#      then run `just check` on the merged tree. The gate is not a sandbox: it compiles and runs
#      the tree's build scripts, proc-macros and tests with this user's privileges, so review
#      before landing is the control for code (fnd §3.6). A gate that left the tree changed
#      aborts (what was gated must be what is pushed);
#   6. push the result to <remote> <branch>, never forced; on a non-fast-forward fetch and redo
#      everything (cherry-pick and gate included) once.
#
# --no-push stops after step 5 (a dry run: the gate on what would land). In both modes the result
# is kept as refs/land/last; after a push the local branch fast-forwards to it when it can.
# CARGO_TARGET_DIR passes through to the gate (a fresh worktree otherwise builds from scratch), never
# to the tree check: code the gate runs can write into its target directory, and cargo relinks no
# binary it considers fresh, so an xtask built there could be one a gated tree left (the XT-01
# review's LOW, XT-02).
# Exit: 0 landed (or dry run green), 1 refused, aborted or gate red, 2 usage.
#
# Pinned by: tools/jfl-architecture/tests/land_scripts.rs (land cases)
set -euo pipefail

# The same `just` as the Stop hook (.claude/hooks/stop-gate.sh): the first on PATH, with the
# bootstrap's ~/.local/bin only as the last resort. The justfile runs the stages' tools from their
# install directories and `_preflight` refuses a mise [env], so how just is found does not change
# what the gate sees (fnd §3.6, FND-08); a mise shim of just loads the tree's mise.toml, which
# xtask's tool-config has passed first (FND-09, XT-01).
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) PATH="$PATH:$HOME/.local/bin" ;; esac

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
remote=origin
branch=main
push=1
allow_unsigned=0
owner="Patryk Żmigrodzki <pzmi@users.noreply.github.com>"
requested=()

die() { echo "land: $*" >&2; exit 1; }
usage() { sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d; s/^# \{0,1\}//' >&2; exit 2; }

while [ $# -gt 0 ]; do
  case "$1" in
    --no-push) push=0 ;;
    --allow-unsigned) allow_unsigned=1 ;;
    --remote | --branch | --author)
      [ $# -ge 2 ] || usage
      case "$1" in --remote) remote="$2" ;; --branch) branch="$2" ;; *) owner="$2" ;; esac
      shift ;;
    -h | --help) usage ;;
    -*) echo "land: unknown option $1" >&2; usage ;;
    *) requested+=("$1") ;;
  esac
  shift
done
[ "${#requested[@]}" -gt 0 ] || usage

root="$(git rev-parse --show-toplevel)"
cd "$root"
[ -z "$(git status --porcelain)" ] || die "the checkout is dirty; commit, stash or remove the changes first"
cache_base="${XDG_CACHE_HOME:-}"
case "$cache_base" in /*) ;; *) cache_base="$HOME/.cache" ;; esac
land_target="$cache_base/jfastlsp/land-xtask"
mkdir -p "$land_target" || die "cannot create $land_target"
land_target="$(cd "$land_target" && pwd -P)"
if [ -n "${CARGO_TARGET_DIR:-}" ]; then
  mkdir -p "$CARGO_TARGET_DIR" || die "cannot create $CARGO_TARGET_DIR"
  gate_target="$(cd "$CARGO_TARGET_DIR" && pwd -P)"
  case "$gate_target/" in "$land_target"/*) die "CARGO_TARGET_DIR $gate_target is the tree check's own target directory ($land_target); give the gate another" ;; esac
  case "$land_target/" in "$gate_target"/*) die "CARGO_TARGET_DIR $gate_target holds the tree check's own target directory ($land_target); give the gate another" ;; esac
fi
commits=()
for ref in "${requested[@]}"; do
  sha="$(git rev-parse --verify --quiet "$ref^{commit}")" || die "$ref is not a commit"
  commits+=("$sha")
done

work=""
cleanup() {
  if [ -n "$work" ]; then
    git -C "$root" worktree remove --force "$work/tree" >/dev/null 2>&1 || true
    rm -rf "$work"
  fi
}
trap cleanup EXIT

# Resolve the two conflicts land.sh may resolve, in the worktree $1 (cherry-pick of $2); abort on
# any other. Every command is checked: this runs where `set -e` is off.
# The landing checkout's own xtask (`tool-config`, scripts/xtask.sh) on the tree $1 after the
# cherry-pick of $2, before anything there runs: mise runs code from a trusted configuration as it
# loads it, and cargo (even the `cargo update` that resolves a Cargo.lock conflict) runs a
# configured rustc-wrapper and the toolchain a rust-toolchain file names (FND-09, its review's G2).
# xtask is built from this checkout and from outside the tree, so the tree steers nothing of it,
# into $land_target, which no gate is given, so no gated code left the binary it runs (XT-02).
check_tree() {
  CARGO_TARGET_DIR="$land_target" bash "$here/xtask.sh" tool-config "$1" >&2 ||
    die "the tree after the cherry-pick of $2 has tool configuration outside its allowlist (above); nothing in it was run"
}

resolve_conflicts() {
  local tree="$1" file others=() lock=0 index=0 files
  check_tree "$tree" "$2"
  files="$(git -C "$tree" diff --name-only --diff-filter=U)" || die "cannot list the conflicts"
  [ -n "$files" ] || die "cherry-pick of $2 failed: $3"
  while IFS= read -r file; do
    case "$file" in
      Cargo.lock) lock=1 ;;
      doc/adr/README.md) index=1 ;;
      *) others+=("$file") ;;
    esac
  done <<<"$files"
  [ "${#others[@]}" -eq 0 ] || die "cherry-pick of $2 conflicts in files land.sh does not resolve: ${others[*]}"
  if [ "$index" -eq 1 ]; then
    local f="doc/adr/README.md" t
    t="$(mktemp -d)" || die "cannot make a temporary directory"
    git -C "$tree" show :1:"$f" >"$t/base" 2>/dev/null || : >"$t/base"
    git -C "$tree" show :2:"$f" >"$t/ours" 2>/dev/null || die "$f was deleted on one side; resolve it by hand"
    git -C "$tree" show :3:"$f" >"$t/theirs" 2>/dev/null || die "$f was deleted on one side; resolve it by hand"
    # Only index rows may be in conflict: the plain merge shows the hunks, which must hold rows only.
    git merge-file -p "$t/ours" "$t/base" "$t/theirs" >"$t/plain" || true
    awk '/^<<<<<<< /{in_hunk=1; next} /^>>>>>>> /{in_hunk=0; next} /^=======$/{next}
         in_hunk && $0 !~ /^\| \[[0-9][0-9][0-9][0-9]\]\(/ {bad=1}
         END{exit bad}' "$t/plain" || die "$f conflicts outside the index rows; resolve it by hand"
    git merge-file --union -p "$t/ours" "$t/base" "$t/theirs" >"$tree/$f" || true
    rm -rf "$t"
    bash "$here/adr-index.sh" sort "$tree/doc/adr" || die "cannot sort the ADR index"
    git -C "$tree" add -- "$f" || die "cannot stage $f"
  fi
  if [ "$lock" -eq 1 ]; then
    git -C "$tree" checkout --ours -- Cargo.lock || die "Cargo.lock has no integration side; resolve it by hand"
    (cd "$tree" && cargo update --workspace --quiet) || die "cargo update -w failed while resolving Cargo.lock"
    echo "land: Cargo.lock re-resolved over integration's; diff against the lane's own:" >&2
    git -C "$tree" diff "$2" -- Cargo.lock >&2 || true
    git -C "$tree" add -- Cargo.lock || die "cannot stage Cargo.lock"
  fi
  GIT_EDITOR=true git -C "$tree" cherry-pick --continue >/dev/null 2>&1 || die "could not continue the cherry-pick of $2"
}

# One try: 0 landed, 10 the push lost a race (fetch and retry), anything else aborted. Called in a
# `||` list, where `set -e` is off, so every command that can fail carries its own `|| die`.
attempt() {
  local base tree head sha n why who changed
  git fetch --quiet "$remote" || die "cannot fetch $remote"
  base="$(git rev-parse --verify --quiet "refs/remotes/$remote/$branch^{commit}")" ||
    die "$remote/$branch does not exist"
  work="$(mktemp -d "${TMPDIR:-/tmp}/land.XXXXXX")" || die "cannot make a temporary directory"
  tree="$work/tree"
  git worktree add --quiet --detach "$tree" "$base" || die "cannot create the worktree"
  for sha in "${commits[@]}"; do
    echo "land: cherry-pick $(git log -1 --format='%h %s' "$sha")" >&2
    if ! why="$(git -C "$tree" cherry-pick "$sha" 2>&1)"; then resolve_conflicts "$tree" "$sha" "$why"; fi
    check_tree "$tree" "$sha"
  done
  head="$(git -C "$tree" rev-parse HEAD)" || die "cannot read the merged head"
  [ "$head" != "$base" ] || die "nothing to land: the commits change nothing on $remote/$branch"

  (cd "$tree" && bash "$here/dco-check.sh" "$base..HEAD") >&2 || die "a commit lacks its author's sign-off"
  for sha in $(git -C "$tree" rev-list --reverse "$base..$head"); do
    who="$(git -C "$tree" log -1 --format='%an <%ae>' "$sha")"
    [ "$who" = "$owner" ] || die "$(git -C "$tree" log -1 --format=%h "$sha") is authored by $who, not the owner identity of ADR 0003 (--author overrides)"
    n="$(git -C "$tree" cat-file commit "$sha" | sed '/^$/q' | grep -c '^gpgsig' || true)"
    if [ "$n" -eq 0 ] && [ "$allow_unsigned" -eq 0 ]; then
      die "$(git -C "$tree" log -1 --format=%h "$sha") is not signed (the environment signs; --allow-unsigned waives this)"
    fi
  done
  echo "land: just check on the merged tree ($(git -C "$tree" rev-parse --short "$head"))" >&2
  (cd "$tree" && just check) || die "just check failed; nothing was pushed"
  changed="$(git -C "$tree" status --porcelain)" || die "cannot read the gated tree's status"
  [ "$(git -C "$tree" rev-parse HEAD)" = "$head" ] && [ -z "$changed" ] ||
    die "the gate changed the tree, so what was gated is not what would be pushed: $(tr '\n' ' ' <<<"$changed")"
  git update-ref refs/land/last "$head" || die "cannot record refs/land/last"
  if [ "$push" -eq 0 ]; then
    echo "land: --no-push: green on $head (kept as refs/land/last); nothing pushed"
    return 0
  fi
  if ! git push --quiet "$remote" "$head:refs/heads/$branch"; then
    git fetch --quiet "$remote" || die "cannot fetch $remote after the refused push"
    [ "$(git rev-parse "refs/remotes/$remote/$branch")" = "$base" ] || return 10
    die "the push to $remote $branch was refused"
  fi
  echo "land: landed $head on $remote/$branch"
  if git merge-base --is-ancestor "refs/heads/$branch" "$head" 2>/dev/null; then
    if [ "$(git symbolic-ref --quiet --short HEAD || true)" = "$branch" ]; then
      git merge --ff-only --quiet "$head" || true
    else
      git branch -f "$branch" "$head" >/dev/null 2>&1 || true
    fi
  fi
  return 0
}

rc=0
attempt || rc=$?
if [ "$rc" -eq 10 ]; then
  echo "land: $remote/$branch moved; redoing the cherry-pick and the gate once" >&2
  cleanup
  work=""
  rc=0
  attempt || rc=$?
  [ "$rc" -ne 10 ] || die "$remote/$branch moved again; run land.sh once more"
fi
exit "$rc"
