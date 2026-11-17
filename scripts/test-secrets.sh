#!/usr/bin/env bash
# Self-test of scripts/secrets-check.sh (FND-04): scratch repositories in a tempdir, the fake
# secret assembled at run time so that no finding is ever committed (and none is allowlisted).
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
grep -q 'only' "$tmp/out" || fail "$what: wrong reason"

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
grep -qE '^useDefault[[:space:]]*=[[:space:]]*true' "$here/../.gitleaks.toml" || fail "$what: no useDefault"
[ "$(grep -vE '^[[:space:]]*(#|$)' "$here/../.gitleaks.toml" | tr -d ' ')" = "$(printf '[extend]\nuseDefault=true')" ] || fail "$what: not exactly [extend] and useDefault"

echo "test-secrets: ok"
