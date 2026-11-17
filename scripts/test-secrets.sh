#!/usr/bin/env bash
# Self-test of scripts/secrets-check.sh and scripts/gitleaks-config.sh (FND-04, FND-08, FND-09): scratch
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

# fresh NAME: an empty scratch repository $tmp/NAME
fresh() {
  git init -q "$tmp/$1"
  git -C "$tmp/$1" config user.email t@example.invalid
  git -C "$tmp/$1" config user.name t
  git -C "$tmp/$1" config commit.gpgsign false
}

what="a token after a NUL byte, which git log -p shows as a binary file (FND-09, F2)"
fresh nul
printf 'header\0\ntoken = "%s"\n' "$fake" >"$tmp/nul/blob.dat"
git -C "$tmp/nul" add blob.dat
git -C "$tmp/nul" commit -q -m "binary-looking file"
if bash "$check" "$tmp/nul" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q 'blob.dat' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: the file is not named"; }

what="a token in a file a local -diff attribute hides from git log -p"
fresh nodiff
printf 'f.txt -diff\n' >"$tmp/nodiff/.git/info/attributes"
printf 'token = "%s"\n' "$fake" >"$tmp/nodiff/f.txt"
git -C "$tmp/nodiff" add f.txt
git -C "$tmp/nodiff" commit -q -m "hidden diff"
if bash "$check" "$tmp/nodiff" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q 'f.txt' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: the file is not named"; }

what="a committed .gitattributes with an attribute outside the allowlist"
for attr in '-diff' 'binary' '-text' 'diff=x' 'filter=x'; do
  fresh "attr$attr"
  printf '* text=auto eol=lf\n*.dat %s\n' "$attr" >"$tmp/attr$attr/.gitattributes"
  git -C "$tmp/attr$attr" add .gitattributes
  git -C "$tmp/attr$attr" commit -q -m attributes
  if bash "$check" "$tmp/attr$attr" >"$tmp/out" 2>&1; then fail "$what: $attr passed"; fi
  grep -q 'outside the allowlist' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: $attr: wrong reason"; }
done
fresh attrok
printf '# the allowlist\n* text=auto eol=lf\n*.sh eol=lf\ndoc/** linguist-documentation\n' >"$tmp/attrok/.gitattributes"
git -C "$tmp/attrok" add .gitattributes
git -C "$tmp/attrok" commit -q -m attributes
bash "$check" "$tmp/attrok" >"$tmp/out" 2>&1 || { cat "$tmp/out" >&2; fail "allowlisted attributes: expected a pass"; }

what="a token in .gitleaks.toml in one commit, removed in the next (no allowlist covers the policy)"
fresh selfpolicy
printf 'token = "%s"\n' "$fake" >"$tmp/selfpolicy/.gitleaks.toml"
git -C "$tmp/selfpolicy" add .gitleaks.toml
git -C "$tmp/selfpolicy" commit -q -m one
git -C "$tmp/selfpolicy" rm -q .gitleaks.toml
git -C "$tmp/selfpolicy" commit -q -m two
if bash "$check" "$tmp/selfpolicy" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q '.gitleaks.toml' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: the file is not named"; }

what="the generated policy committed as .gitleaks.toml (its own rule texts) passes"
fresh policy
cp "$here/../.gitleaks.toml" "$tmp/policy/.gitleaks.toml"
git -C "$tmp/policy" add .gitleaks.toml
git -C "$tmp/policy" commit -q -m policy
bash "$check" "$tmp/policy" >"$tmp/out" 2>&1 || { cat "$tmp/out" >&2; fail "$what: expected a pass"; }

what="a token only a merge resolution added"
fresh merge
printf 'a\n' >"$tmp/merge/a.txt"
git -C "$tmp/merge" add a.txt
git -C "$tmp/merge" commit -q -m base
git -C "$tmp/merge" checkout -q -b side
printf 'b\n' >"$tmp/merge/b.txt"
git -C "$tmp/merge" add b.txt
git -C "$tmp/merge" commit -q -m side
git -C "$tmp/merge" checkout -q -
git -C "$tmp/merge" merge -q --no-commit --no-ff side
printf 'token = "%s"\n' "$fake" >"$tmp/merge/evil.txt"
git -C "$tmp/merge" add evil.txt
git -C "$tmp/merge" commit -q -m merge
if bash "$check" "$tmp/merge" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q 'evil.txt' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: the file is not named"; }

# G1 (the FND-09 review): gitleaks sniffs a file's type and skipped these as binary, plain text
# after the header included; each must be found or refused, and its file named.
# probe NAME FILE: commit FILE (written by the caller under $tmp/NAME) and expect a failure
probe() {
  git -C "$tmp/$1" add -A
  git -C "$tmp/$1" commit -q -m "$1"
  if bash "$check" "$tmp/$1" >"$tmp/out" 2>&1; then fail "$1: expected a failure"; fi
  grep -qF "$2" "$tmp/out" || { cat "$tmp/out" >&2; fail "$1: $2 is not named"; }
}
what="G1 mz-header: a token after an MZ (executable) header"
fresh g1-mz
printf 'MZ\0\0 token = "%s"\n' "$fake" >"$tmp/g1-mz/notes.txt"
probe g1-mz notes.txt
what="G1 pdf-header: a token after a %PDF- header and a NUL"
fresh g1-pdf
printf '%%PDF-1.4\n\0\ntoken = "%s"\n' "$fake" >"$tmp/g1-pdf/doc.pdf"
probe g1-pdf doc.pdf
what="G1 stored-zip: a token in plain text behind a zip local header"
fresh g1-zip
printf 'PK\003\004\024\0\0\0\0\0token = "%s"\n' "$fake" >"$tmp/g1-zip/data.zip"
probe g1-zip data.zip
what="G1 tar: a token in plain text inside an uncompressed tar"
fresh g1-tar
mkdir -p "$tmp/g1-tar-src"
printf 'token = "%s"\n' "$fake" >"$tmp/g1-tar-src/inner.txt"
tar -cf "$tmp/g1-tar/t.tar" -C "$tmp/g1-tar-src" inner.txt
probe g1-tar t.tar

# G4 and the secret-scanning rule: the policy has no allowlist of its own, so a key the bedrock
# rule matches is found in the root .gitleaks.toml too, even once a later commit removed it.
what="G4 root-policy-history: a bedrock short-lived key in .gitleaks.toml, removed in the next commit"
fresh g4-root
bedrock="bedrock-api-key-""YmVkcm9jay5hbWF6""b25hd3MuY29t"
printf 'x = "%s"\n' "$bedrock" >"$tmp/g4-root/.gitleaks.toml"
git -C "$tmp/g4-root" add .gitleaks.toml
git -C "$tmp/g4-root" commit -q -m one
git -C "$tmp/g4-root" rm -q .gitleaks.toml
probe g4-root .gitleaks.toml

# G6: commit and tag messages are pushed and public, so they are scanned too.
what="G6 commit-message: a token in a commit message"
fresh g6-commit
git -C "$tmp/g6-commit" commit -q --allow-empty -m "token = \"$fake\""
if bash "$check" "$tmp/g6-commit" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q '_messages/commits' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: not named"; }
what="G6 tag-message: a token in an annotated tag's message"
fresh g6-tag
git -C "$tmp/g6-tag" commit -q --allow-empty -m clean
git -C "$tmp/g6-tag" -c tag.gpgSign=false tag -a v1 -m "token = \"$fake\""
if bash "$check" "$tmp/g6-tag" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q '_messages/tags' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: not named"; }

# H4: the commit object is scanned whole, its author and committer included.
what="G6 commit-author: a token in a commit's author name"
fresh h4-author
git -C "$tmp/h4-author" -c user.name="token = $fake" commit -q --allow-empty -m clean
if bash "$check" "$tmp/h4-author" >"$tmp/out" 2>&1; then fail "$what: expected a failure"; fi
grep -q '_messages/commits' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: not named"; }

what="the committed policy"
bash "$here/gitleaks-config.sh" --check || fail "$what: not the generated one"
[ "$(grep -cE '^\[(allowlist|\[allowlists)\]' "$here/../.gitleaks.toml")" = 0 ] || fail "$what: a global allowlist"
if grep -q 'gitleaks\\\.toml' "$here/../.gitleaks.toml"; then fail "$what: an allowlist of the policy's own path"; fi

# The secret-scanning rule (fnd §7.4): the policy holds no secret-shaped literal, so gitleaks over
# .gitleaks.toml with the policy's own rules and none of their allowlists finds nothing; the same
# scan finds a key appended at run time (the negative case), so the check is not vacuous.
# self_scan FILE: gitleaks dir over a copy of FILE named .gitleaks.toml, rules without allowlists
self_scan() {
  rm -rf "$tmp/self" && mkdir -p "$tmp/self/tree"
  awk '/^\[\[rules\.allowlists\]\]/ { skip = 1; next } skip && /^\[/ { skip = 0 } !skip' \
    "$here/../.gitleaks.toml" >"$tmp/self/bare.toml"
  cp "$1" "$tmp/self/tree/.gitleaks.toml"
  gitleaks dir --no-banner --redact --verbose --config "$tmp/self/bare.toml" "$tmp/self/tree" >"$tmp/out" 2>&1
}
what="policy-self-scan-clean: the committed policy, scanned with its own rules and no allowlist"
self_scan "$here/../.gitleaks.toml" || { cat "$tmp/out" >&2; fail "$what: found something"; }
what="policy-self-scan-negative: the policy with a GCP API key appended (assembled at run time)"
cp "$here/../.gitleaks.toml" "$tmp/policy-plus.toml"
printf 'x = "%s"\n' "AIza""SyQ7mK2pX9vB4nR8tL1wE6yU3oI5aS0dF7g" >>"$tmp/policy-plus.toml"
if self_scan "$tmp/policy-plus.toml"; then fail "$what: expected a finding"; fi
grep -q 'gcp-api-key' "$tmp/out" || { cat "$tmp/out" >&2; fail "$what: not the gcp rule"; }

echo "test-secrets: ok"
