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

# actionlint reads the .github/actionlint.yaml of the project it runs in (the stage runs from the
# repository root) by itself; one that ignores every finding must change nothing (FND-09, the
# FND-08 re-verification's F1). A scratch repository holds a copy of the stage's script.
fixture lintconfig "$checkout
      - run: |
          dir=\$HOME/build
          rm -rf \$dir"
git init -q "$tmp/lintconfig"
mkdir -p "$tmp/lintconfig/scripts"
cp "$check" "$tmp/lintconfig/scripts/workflows-check.sh"
cp "$here/../.github/zizmor.yml" "$tmp/lintconfig/.github/zizmor.yml"
printf 'paths:\n  .github/workflows/**/*.yml:\n    ignore:\n      - ".*"\n' >"$tmp/lintconfig/.github/actionlint.yaml"
if bash "$tmp/lintconfig/scripts/workflows-check.sh" workflows >"$tmp/out" 2>&1; then
  fail "lintconfig: actionlint's own configuration silenced the stage"
fi
grep -q 'SC2086' "$tmp/out" || { cat "$tmp/out" >&2; fail "lintconfig: actionlint's own configuration silenced SC2086"; }

fixture injection "$checkout
      - run: echo \"\${{ github.event.pull_request.title }}\""
expect_fail injection 'injection.yml'
grep -q 'template-injection' "$tmp/out" || fail "injection: zizmor did not flag it"

fixture badsyntax "$checkout
      - runs: echo typo"
expect_fail badsyntax 'badsyntax.yml'

# dependabot.yml is audited too: no cooldown fails (FND-05)
fixture dependabot "$checkout
      - run: echo hello"
printf 'version: 2\nupdates:\n  - package-ecosystem: cargo\n    directory: /\n    schedule:\n      interval: weekly\n' \
  >"$tmp/dependabot/.github/dependabot.yml"
expect_fail dependabot 'dependabot-cooldown'

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

# a .shellcheckrc beside the script (or above it) must change nothing (FND-09, F1)
printf 'disable=all\n' >"$tmp/.shellcheckrc"
if bash "$check" shell "$tmp/bad.sh" >"$tmp/out" 2>&1; then fail "shell: a .shellcheckrc silenced shellcheck"; fi
grep -q 'SC2086' "$tmp/out" || fail "shell: a .shellcheckrc silenced the finding"
rm -f "$tmp/.shellcheckrc"

# G3 (the FND-09 review): inline suppressions outside the allowlist fail the stage.
what="G3 shellcheck-disable-all: a directive disabling all checks in a shell script"
# The directives are spelled through %s, so this file holds none of its own.
sc=shellcheck
printf "#!/usr/bin/env bash\n# %s disable=all\nx=\$1\nrm -rf \$x\n" "$sc" >"$tmp/g3-all.sh"
if bash "$check" shell "$tmp/g3-all.sh" >"$tmp/out" 2>&1; then fail "$what: passed"; fi
grep -q 'disables every check' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: wrong reason"; }
what="G3 shellcheck-disable-unlisted: a directive disabling SC2086, not in the allowlist"
printf "#!/usr/bin/env bash\n# %s disable=SC2086\nx=\$1\nrm -rf \$x\n" "$sc" >"$tmp/g3-one.sh"
if bash "$check" shell "$tmp/g3-one.sh" >"$tmp/out" 2>&1; then fail "$what: passed"; fi
grep -q 'outside the allowlist' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: wrong reason"; }
what="G3 shellcheck-disable-in-run: a shellcheck directive inside a workflow's run: block"
fixture g3run "$checkout
      - run: |
          # $sc disable=SC2086
          dir=\$HOME/build
          rm -rf \$dir"
expect_fail g3run 'outside the allowlist'
zi=ignore
what="G3 zizmor-ignore: a zizmor ignore comment for artipacked in a workflow"
fixture g3zizmor "      - uses: actions/checkout@$sha # v4.3.0 # zizmor: ${zi}[artipacked]
      - run: echo hello"
expect_fail g3zizmor 'outside the allowlist'

# H1: shellcheck also honours a directive trailing a command (`{ # ...`, `then # ...`).
what="G3 shellcheck-trailing-directive-script: a directive after 'then' in a shell script"
printf "#!/usr/bin/env bash\nif true; then # %s disable=all\n  rm -rf \$2/*\nfi\n" "$sc" >"$tmp/h1-then.sh"
if bash "$check" shell "$tmp/h1-then.sh" >"$tmp/out" 2>&1; then fail "$what: passed"; fi
grep -q 'disables every check' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: wrong reason"; }
what="G3 shellcheck-trailing-directive-brace: a directive after '{' in a shell script"
printf "#!/usr/bin/env bash\n{ # %s disable=SC2086,SC2115\n  rm -rf \$2/*\n}\n" "$sc" >"$tmp/h1-brace.sh"
if bash "$check" shell "$tmp/h1-brace.sh" >"$tmp/out" 2>&1; then fail "$what: passed"; fi
grep -q 'outside the allowlist' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: wrong reason"; }
what="G3 shellcheck-trailing-directive-run: a directive after 'then' in a workflow run: block"
fixture h1run "$checkout
      - run: |
          if true; then # $sc disable=all
            rm -rf \$HOME/\$X/*
          fi"
expect_fail h1run 'disables every check'

# H2: an allowlisted directive holds once in its file; a copy elsewhere is refused.
what="G3 allowlisted-directive-repeated: the allowlisted session-start.sh directive, twice"
mkdir -p "$tmp/h2/scripts" "$tmp/h2/.claude/hooks"
cp "$check" "$tmp/h2/scripts/workflows-check.sh"
cp "$here/../.claude/hooks/session-start.sh" "$tmp/h2/.claude/hooks/session-start.sh"
sed -n '2p' "$here/../.claude/hooks/session-start.sh" >>"$tmp/h2/.claude/hooks/session-start.sh"
printf "echo '\$HOME'\n" >>"$tmp/h2/.claude/hooks/session-start.sh"
if (cd "$tmp/h2" && bash scripts/workflows-check.sh shell .claude/hooks/session-start.sh) >"$tmp/out" 2>&1; then
  fail "$what: passed"
fi
grep -q 'allowlisted once' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: wrong reason"; }
what="G3 allowlisted-directive-once: the hook as committed passes the allowlist"
cp "$here/../.claude/hooks/session-start.sh" "$tmp/h2/.claude/hooks/session-start.sh"
(cd "$tmp/h2" && bash scripts/workflows-check.sh shell .claude/hooks/session-start.sh) >"$tmp/out" 2>&1 ||
  { cat "$tmp/out" >&2; fail "$what: refused"; }

# I1: actionlint decodes a double-quoted YAML string before shellcheck reads it, so `\x23` is a
# `#` the raw-text scan never sees; every run: is a block scalar or a plain line.
what="G3 shellcheck-yaml-escape: a directive spelled with \x23 in a double-quoted run:"
fixture i1x23 "$checkout
      - run: \"if true; then \\x23 $sc disable=all\\n  rm -rf \$HOME/\$X/*\\nfi\""
expect_fail i1x23 'quoted, a flow form or an indicator'
what="G3 shellcheck-yaml-escape-x20: a directive spelled #\x20 in a double-quoted run:"
fixture i1x20 "$checkout
      - run: \"#\\x20$sc disable=all\\nrm -rf \$HOME/\$X/*\""
expect_fail i1x20 'quoted, a flow form or an indicator'
what="G3 run-single-quoted: a single-quoted run:"
fixture i1single "$checkout
      - run: 'rm -rf \$HOME/\$X/*'"
expect_fail i1single 'quoted, a flow form or an indicator'
what="G3 run-flow-mapping: a step written as a flow mapping"
fixture i1flow "$checkout
      - {run: \"rm -rf \$HOME/\$X/*\"}"
expect_fail i1flow 'a run key in another form'
what="G3 run-plain-continued: a plain run: value continued on the next line"
fixture i1cont "$checkout
      - run: echo one
          two"
expect_fail i1cont 'continued on the next line'

# J1: the YAML shape is an allowlist, so no spelling of a key decodes into run: unseen.
what="G3 run-escaped-key: a run key spelled with \x72 in a double-quoted key"
fixture j1esc "$checkout
      - \"\\x72un\": \"echo \\x23 $sc disable=all\""
expect_fail j1esc 'no YAML escape sequences'
what="G3 run-complex-key: a run key behind the ? complex-key indicator"
fixture j1complex "$checkout
      - ? run
        : rm -rf \$HOME/\$X/*"
expect_fail j1complex 'a complex key'
what="G3 quoted-key: a quoted mapping key"
fixture j1quoted "$checkout
      - 'name': step
        run: echo hi"
expect_fail j1quoted 'a quoted mapping key'
what="G3 yaml-tag: a tag on a run: value"
fixture j1tag "$checkout
      - run: !!str echo hi"
expect_fail j1tag 'a tag, an anchor or an alias'
what="G3 yaml-anchor: an anchor and an alias"
fixture j1anchor "$checkout
      - run: &cmd echo hi
      - run: *cmd"
expect_fail j1anchor 'a tag, an anchor or an alias'

# K1: a comment ending in `: |` must not open a block whose lines go unchecked.
what="G3 yaml-fake-block-comment: 'steps: # x: |' before an escaped run key"
mkdir -p "$tmp/k1/.github/workflows"
printf '%s\n' "name: k1" "on: pull_request" "permissions:" "  contents: read" "jobs:" "  job:" \
  "    runs-on: ubuntu-24.04" "    steps: # x: |" \
  "      - \"\\x72un\": \"if true; then \\x23 $sc disable=all\\n  rm -rf \$HOME/\$X/*\\nfi\"" \
  >"$tmp/k1/.github/workflows/k1.yml"
expect_fail k1 'no YAML escape sequences'

echo "test-workflows: ok"
