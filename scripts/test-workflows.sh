#!/usr/bin/env bash
# Self-test of scripts/workflows-check.sh (FND-04): fixture workflow trees in a tempdir, offline.
# Each fixture that violates a rule must fail the stage with the offending file named; the clean
# one must pass. The SHAs below are syntactically valid, not real: the offline pin check is
# syntactic, and the comment check (`verify`) needs the GitHub API, so it is not run here.
# Needs actionlint, shellcheck, zizmor and pinact.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
check="$here/workflows-check.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

sha=0123456789abcdef0123456789abcdef01234567

# fixture NAME STEPS: a tree $tmp/NAME/.github/workflows/NAME.yml whose job has the given steps
fixture() {
  local name="$1" steps="$2"
  mkdir -p "$tmp/$name/.github/workflows"
  cat >"$tmp/$name/.github/workflows/$name.yml" <<YAML
name: $name
on: pull_request
permissions:
  contents: read
jobs:
  job:
    runs-on: ubuntu-24.04
    steps:
$steps
YAML
}
checkout="      - uses: actions/checkout@$sha # v4.3.0
        with:
          persist-credentials: false"

# expect_fail NAME PATTERN: the stage fails on the fixture and its output matches PATTERN
expect_fail() {
  if bash "$check" workflows "$tmp/$1" >"$tmp/out" 2>&1; then fail "$1: the stage passed"; fi
  grep -q "$2" "$tmp/out" || { cat "$tmp/out" >&2; fail "$1: output does not match '$2'"; }
}

fixture clean "$checkout
      - run: echo hello"
bash "$check" workflows "$tmp/clean" >"$tmp/out" 2>&1 || { cat "$tmp/out" >&2; fail "clean: the stage failed"; }

fixture tagpinned "      - uses: actions/checkout@v4
        with:
          persist-credentials: false"
expect_fail tagpinned 'tagpinned.yml'

fixture unpinned "      - uses: actions/checkout@main
        with:
          persist-credentials: false"
expect_fail unpinned 'unpinned.yml'

fixture shellbug "$checkout
      - run: |
          dir=\$HOME/build
          rm -rf \$dir"
expect_fail shellbug 'shellbug.yml'
grep -q 'SC2086' "$tmp/out" || fail "shellbug: shellcheck did not run on the step"

fixture injection "$checkout
      - run: echo \"\${{ github.event.pull_request.title }}\""
expect_fail injection 'injection.yml'
grep -q 'template-injection' "$tmp/out" || fail "injection: zizmor did not flag it"

fixture badsyntax "$checkout
      - runs: echo typo"
expect_fail badsyntax 'badsyntax.yml'

mkdir -p "$tmp/action/.github/actions/setup"
cat >"$tmp/action/.github/actions/setup/action.yml" <<'YAML'
name: setup
description: a composite action with an unpinned step
runs:
  using: composite
  steps:
    - uses: actions/cache@v4
      with:
        path: x
        key: y
YAML
if bash "$check" workflows "$tmp/action" >"$tmp/out" 2>&1; then fail "action: the stage passed"; fi
grep -q 'action.yml' "$tmp/out" || fail "action: the file is not named"

mkdir -p "$tmp/empty"
bash "$check" workflows "$tmp/empty" >"$tmp/out" 2>&1 || fail "empty: a tree without workflows must pass"
grep -q 'nothing to check' "$tmp/out" || fail "empty: no notice"
bash "$check" verify "$tmp/empty" >"$tmp/out" 2>&1 || fail "empty: verify must pass with no files"

bash "$check" nonsense >"$tmp/out" 2>&1 && fail "an unknown mode passed"

# the shell subcommand
printf '#!/usr/bin/env bash\nset -euo pipefail\necho "ok"\n' >"$tmp/good.sh"
cat >"$tmp/bad.sh" <<'SH'
#!/usr/bin/env bash
x=$1
rm -rf $x
SH
bash "$check" shell "$tmp/good.sh" >"$tmp/out" 2>&1 || { cat "$tmp/out" >&2; fail "shell: a clean script failed"; }
if bash "$check" shell "$tmp/bad.sh" >"$tmp/out" 2>&1; then fail "shell: a bad script passed"; fi
grep -q 'bad.sh' "$tmp/out" || fail "shell: the file is not named"

echo "test-workflows: ok"
