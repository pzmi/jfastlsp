#!/usr/bin/env bash
# Self-test of scripts/secrets-check.sh and scripts/gitleaks-config.sh (FND-04, FND-08): scratch
# repositories in a tempdir, the fake secret assembled at run time so that no finding is ever
# committed (and none is allowlisted).
# Needs git and gitleaks.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
check="$here/secrets-check.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

fake="ghp_""aB3dE5fG7hJ9""kL1mN3pQ5rS7""tU9vW1xY3zA5"

repo="$tmp/repo"
git init -q "$repo"
git -C "$repo" config user.email t@example.invalid
git -C "$repo" config user.name t
git -C "$repo" config commit.gpgsign false
printf 'nothing here\n' >"$repo/ok.txt"
git -C "$repo" add ok.txt
git -C "$repo" commit -q -m clean

what="a clean history"
bash "$check" "$repo" 2>"$tmp/err" || { cat "$tmp/err" >&2; fail "$what: expected a pass"; }

what="a secret in a commit"
printf 'token = "%s"\n' "$fake" >"$repo/settings.cfg"
git -C "$repo" add settings.cfg
git -C "$repo" commit -q -m "add settings"
if bash "$check" "$repo" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q 'settings.cfg' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: the file is not named"; }
grep -q 'github-pat' "$tmp/out" || fail "$what: the rule is not named"
if grep -q "$fake" "$tmp/out"; then fail "$what: the secret was printed"; fi

what="a secret removed by a later commit is still in the history"
git -C "$repo" rm -q settings.cfg
git -C "$repo" commit -q -m "remove settings"
if bash "$check" "$repo" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q 'settings.cfg' "$tmp/out" || fail "$what: the file is not named"

what="a shallow clone"
shallow="$tmp/shallow"
git clone -q --depth 1 "file://$repo" "$shallow"
if bash "$check" "$shallow" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q 'shallow' "$tmp/out" || fail "$what: wrong reason"

what="a policy with an allowlist"
printf '[extend]\nuseDefault = true\n[allowlist]\nregexes = ["ghp_"]\n' >"$tmp/allow.toml"
if bash "$check" --config "$tmp/allow.toml" "$repo" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q 'exactly the generated policy' "$tmp/out" || fail "$what: wrong reason"

what="the default ruleset with gitleaks' own global path allowlist (useDefault inherits it)"
printf '[extend]\nuseDefault = true\n' >"$tmp/extend.toml"
if bash "$check" --config "$tmp/extend.toml" "$repo" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi

what="the generated policy with one rule's regex changed"
awk '!done && /^regex = / { print "regex = \"zzz\""; done = 1; next } { print }' \
  "$here/../.gitleaks.toml" >"$tmp/edited.toml"
if bash "$check" --config "$tmp/edited.toml" "$repo" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi

what="a token in each kind of path gitleaks' default allowlist skips (FND-08, the checkpoint's N5)"
paths="$tmp/paths"
git init -q "$paths"
git -C "$paths" config user.email t@example.invalid
git -C "$paths" config user.name t
git -C "$paths" config commit.gpgsign false
skipped=(doc.pdf img.png node_modules/x/cfg.js go.sum package-lock.json vendor/github.com/x/y.go)
for file in "${skipped[@]}"; do
  mkdir -p "$paths/$(dirname "$file")"
  printf 'token = "%s"\n' "$fake" >"$paths/$file"
done
git -C "$paths" add -A
git -C "$paths" commit -q -m "tokens where the default allowlist looks away"
if bash "$check" "$paths" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
for file in "${skipped[@]}"; do
  grep -qF "$file" "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: $file is not named"; }
done

what="a policy that drops the default rules"
printf '# empty\n' >"$tmp/none.toml"
if bash "$check" --config "$tmp/none.toml" "$repo" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi

what="a policy that disables rules under [extend]"
printf '[extend]\nuseDefault = true\ndisabledRules = ["github-pat", "generic-api-key"]\n' >"$tmp/disabled.toml"
if bash "$check" --config "$tmp/disabled.toml" "$repo" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi

what="a policy with an indented rules table"
printf '[extend]\nuseDefault = true\n  [[ rules ]]\n  id = "x"\n  regex = "zzz"\n' >"$tmp/indent.toml"
if bash "$check" --config "$tmp/indent.toml" "$repo" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi

what="an inline gitleaks:allow comment"
inline="$tmp/inline"
git init -q "$inline"
git -C "$inline" config user.email t@example.invalid
git -C "$inline" config user.name t
git -C "$inline" config commit.gpgsign false
printf 'token = "%s" # gitleaks:allow\n' "$fake" >"$inline/settings.cfg"
git -C "$inline" add settings.cfg
git -C "$inline" commit -q -m "allowed inline"
if bash "$check" "$inline" >"$tmp/out" 2>&1; then fail "$what: the comment waved the secret through"; fi
grep -q 'settings.cfg' "$tmp/out" || fail "$what: the file is not named"

what="a committed .gitleaksignore"
printf 'token = "%s"\n' "$fake" >"$inline/settings.cfg"
git -C "$inline" add settings.cfg
git -C "$inline" commit -q -m "plain secret"
fp="$(git -C "$inline" rev-parse HEAD):settings.cfg:github-pat:1"
printf '%s\n' "$fp" >"$inline/.gitleaksignore"
git -C "$inline" add .gitleaksignore
git -C "$inline" commit -q -m "ignore it"
if bash "$check" "$inline" >"$tmp/out" 2>&1; then fail "$what: the ignore file waved the secret through"; fi

what="an untracked .gitleaksignore"
untracked="$tmp/untracked"
git init -q "$untracked"
git -C "$untracked" config user.email t@example.invalid
git -C "$untracked" config user.name t
git -C "$untracked" config commit.gpgsign false
printf 'ok\n' >"$untracked/ok.txt"
git -C "$untracked" add ok.txt
git -C "$untracked" commit -q -m clean
printf 'abc:settings.cfg:github-pat:1\n' >"$untracked/.gitleaksignore"
if bash "$check" "$untracked" >"$tmp/out" 2>&1; then fail "$what: passed"; fi
grep -q 'gitleaksignore' "$tmp/out" || fail "$what: wrong reason"

what="the committed policy"
bash "$here/gitleaks-config.sh" --check || fail "$what: not the generated one"
[ "$(grep -cE '^\[(allowlist|\[allowlists)\]' "$here/../.gitleaks.toml")" = 1 ] || fail "$what: not exactly one global allowlist"
grep -qF "paths = ['''^\.gitleaks\.toml\$''']" "$here/../.gitleaks.toml" || fail "$what: the global allowlist is not only the policy's own path"

echo "test-secrets: ok"
