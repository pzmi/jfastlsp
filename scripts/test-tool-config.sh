#!/usr/bin/env bash
# Self-test of scripts/tool-config-check.sh and of how scripts/bootstrap-tools.sh uses it (FND-09):
# scratch trees in a tempdir, offline. Each tree that holds a configuration a tool of the gate
# would read by itself, or a mise.toml or mise.lock outside its shape, must be refused with the
# file named; the repository's own tree must pass. The bootstrap runs against a fake `mise` and
# `just` at the pinned versions (so nothing is downloaded) that log every call: with the
# reviewer's payload in mise.toml it must not call mise beyond `--version`, and it must never
# call `mise trust` other than to remove a trust.
# Needs bash, awk, git and cp only.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
repo="$(cd "$here/.." && pwd -P)"
check="$here/tool-config-check.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
fail() { echo "FAIL: $*" >&2; exit 1; }

# tree NAME: a copy of the repository's tool configuration in $tmp/NAME
tree() {
  mkdir -p "$tmp/$1/.github" "$tmp/$1/.config"
  cp "$repo/mise.toml" "$repo/mise.lock" "$tmp/$1/"
  cp "$repo/.github/zizmor.yml" "$tmp/$1/.github/"
  cp "$repo/.config/nextest.toml" "$tmp/$1/.config/"
}

# expect_refused NAME PATTERN: the check refuses $tmp/NAME and names PATTERN
expect_refused() {
  if bash "$check" "$tmp/$1" >"$tmp/out" 2>&1; then fail "$1: the check passed"; fi
  grep -qF "$2" "$tmp/out" || { cat "$tmp/out" >&2; fail "$1: output does not name '$2'"; }
}

bash "$check" "$repo" >"$tmp/out" 2>&1 || { cat "$tmp/out" >&2; fail "the repository: refused"; }
tree clean
bash "$check" "$tmp/clean" >"$tmp/out" 2>&1 || { cat "$tmp/out" >&2; fail "clean: refused"; }

# Configuration files the gate's tools read without being told (the FND-08 re-verification's F1).
for file in .shellcheckrc .yamllint .tool-versions .mise.toml mise.local.toml mise.ci.toml \
  .pinact.yaml .envrc .github/actionlint.yaml .github/actionlint.yml .github/pinact.yaml \
  .config/mise.toml; do
  name="implicit-$(tr '/.' '__' <<<"$file")"
  tree "$name"
  printf 'disable=all\n' >"$tmp/$name/$file"
  expect_refused "$name" "$file"
done
tree confdir
mkdir -p "$tmp/confdir/.config/mise/conf.d"
printf '[env]\n' >"$tmp/confdir/.config/mise/conf.d/x.toml"
expect_refused confdir ".config/mise"

# G2 (the FND-09 review): cargo and toolchain configuration, which any cargo command obeys.
for file in .cargo/config.toml .cargo/config crates/x/.cargo/config.toml crates/x/rust-toolchain.toml \
  rust-toolchain .Cargo/Config.toml crates/x/Rust-Toolchain.toml; do
  name="g2-cargo-$(tr '/.' '__' <<<"$file")"
  tree "$name"
  mkdir -p "$(dirname "$tmp/$name/$file")"
  printf '[build]\nrustc-wrapper = "./evil.sh"\n' >"$tmp/$name/$file"
  expect_refused "$name" "$file"
done
tree g2-toolchain-path
cp "$repo/rust-toolchain.toml" "$tmp/g2-toolchain-path/"
printf 'path = "/elsewhere/toolchain"\n' >>"$tmp/g2-toolchain-path/rust-toolchain.toml"
expect_refused g2-toolchain-path "rust-toolchain.toml:"
tree g2-toolchain-ok
cp "$repo/rust-toolchain.toml" "$tmp/g2-toolchain-ok/"
bash "$check" "$tmp/g2-toolchain-ok" >"$tmp/out" 2>&1 || { cat "$tmp/out" >&2; fail "g2-toolchain-ok: refused"; }

# mise.toml outside its shape: the reviewer's payload, a plugin backend, a template version.
tree payload
printf '\n[env]\n_.source = "{{config_root}}/payload.sh"\n' >>"$tmp/payload/mise.toml"
expect_refused payload "mise.toml"
tree plugin
printf '"asdf:someone/plugin" = "1.0.0"\n' >>"$tmp/plugin/mise.toml"
expect_refused plugin "mise.toml"
tree template
printf '"aqua:a/b" = "{{exec(command='"'"'true'"'"')}}"\n' >>"$tmp/template/mise.toml"
expect_refused template "mise.toml"
tree symlink
rm "$tmp/symlink/mise.toml"
ln -s "$repo/mise.toml" "$tmp/symlink/mise.toml"
expect_refused symlink "mise.toml: not a regular file"

# mise.lock outside its shape: an asset from elsewhere, a weak checksum, a platform without one.
tree url
first() { awk -v key="$1" -v to="$2" '!done && index($0, key) == 1 { done = 1; if (to == "") next; $0 = to } { print }' "$repo/mise.lock"; }
first 'url = ' 'url = "https://example.invalid/tool.tar.gz"' >"$tmp/url/mise.lock"
expect_refused url "mise.lock"
tree md5
first 'checksum = ' 'checksum = "md5:0123"' >"$tmp/md5/mise.lock"
expect_refused md5 "mise.lock"
tree nosum
first 'checksum = ' '' >"$tmp/nosum/mise.lock"
expect_refused nosum "without a sha256 checksum"

# The bootstrap: a fake mise and just at the pinned versions, each call logged.
mise_version="$(sed -n 's/^MISE_VERSION="\(.*\)"$/\1/p' "$here/bootstrap-tools.sh")"
just_version="$(sed -n 's/^JUST_VERSION="\(.*\)"$/\1/p' "$here/bootstrap-tools.sh")"
[ -n "$mise_version" ] && [ -n "$just_version" ] || fail "bootstrap: no pinned versions found"
mkdir -p "$tmp/bin"
cat >"$tmp/bin/mise" <<SH
#!/usr/bin/env bash
echo "mise \$*" >>"$tmp/calls"
case "\$1" in --version) echo "$mise_version linux-x64" ;; esac
SH
cat >"$tmp/bin/just" <<SH
#!/usr/bin/env bash
echo "just $just_version"
SH
chmod +x "$tmp/bin/mise" "$tmp/bin/just"

# bootstrap NAME: run a copy of the bootstrap in the scratch repository $tmp/NAME
bootstrap() {
  mkdir -p "$tmp/$1/scripts"
  cp "$here/bootstrap-tools.sh" "$check" "$tmp/$1/scripts/"
  : >"$tmp/calls"
  PATH="$tmp/bin:$PATH" bash "$tmp/$1/scripts/bootstrap-tools.sh" "$tmp/bin" >"$tmp/out" 2>&1
}

bootstrap clean || { cat "$tmp/out" >&2; fail "bootstrap clean: failed"; }
grep -qx 'mise install --locked' "$tmp/calls" || { cat "$tmp/calls" >&2; fail "bootstrap clean: no mise install"; }
if grep -E '^mise trust' "$tmp/calls" | grep -qv -- '--untrust'; then
  fail "bootstrap clean: it trusted the repository for mise"
fi

printf 'touch "%s/ran"\n' "$tmp" >"$tmp/payload/payload.sh"
if bootstrap payload; then fail "bootstrap payload: passed"; fi
grep -q 'mise.toml' "$tmp/out" || { cat "$tmp/out" >&2; fail "bootstrap payload: mise.toml not named"; }
if grep -v -x 'mise --version' "$tmp/calls" | grep -q .; then
  cat "$tmp/calls" >&2
  fail "bootstrap payload: mise ran on a refused tree"
fi
[ ! -e "$tmp/ran" ] || fail "bootstrap payload: the payload ran"

echo "test-tool-config: ok"
